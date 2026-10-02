//! Sistem ses seviyesi (Windows Core Audio). Kendi COM thread'inde çalışır; seviye dışarıdan
//! (klavye tuşları, başka program) değişirse de telefona yansır. Varsayılan çıkış cihazı değişirse
//! (ör. kulaklık takıldı) her okumada yeniden alınır.
//!
//! Çıkış cihazı listesi telefona yayınlanır ve telefondan değiştirilebilir. Windows'un bunun için
//! belgelenmiş bir API'si yok; SoundSwitch gibi araçların da kullandığı IPolicyConfig arayüzü kullanılır.
#![allow(non_snake_case)] // IPolicyConfig yöntem adları Windows'taki gibi
use serde_json::json;
use std::sync::mpsc::{channel, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::time::Duration;
use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
use windows::core::{interface, Interface, IUnknown, IUnknown_Vtbl, GUID, HRESULT, PCWSTR, PWSTR};
use windows::Win32::Foundation::{CloseHandle, S_OK};
use windows::Win32::System::Threading::{OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION};
use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
use windows::Win32::Media::Audio::{
    eCommunications, eConsole, eMultimedia, eRender, AudioSessionStateActive, AudioSessionStateExpired, ERole, IAudioSessionControl2,
    IAudioSessionManager2, IMMDevice, IMMDeviceEnumerator, ISimpleAudioVolume, MMDeviceEnumerator, DEVICE_STATE_ACTIVE,
};
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CoTaskMemFree, CLSCTX_ALL, COINIT_MULTITHREADED, STGM_READ};
use windows::Win32::System::Com::StructuredStorage::PropVariantToStringAlloc;

use crate::events::Events;

pub enum AudioCmd {
    /// 0.0 – 1.0
    SetLevel(f32),
    SetMute(bool),
    /// Varsayılan çıkış cihazını değiştir (cihaz kimliği, listedekilerden biri olmalı)
    SetDefault(String),
    /// Uygulama sesi (karıştırıcı): uygulama kimliği + seviye ve/veya sessiz
    SetApp { id: String, level: Option<f32>, mute: Option<bool> },
    /// Anlık seviyeyi döndür (zamanlayıcının sesi kısıp geri alması için)
    Get(Sender<Option<(f32, bool)>>),
    /// Telefon bağlandı: tüm durumları (ses, cihazlar, karıştırıcı) hemen yeniden tara ve yayınla
    Refresh,
}

#[derive(Clone)]
pub struct Audio {
    tx: Sender<AudioCmd>,
}

/// Belgelenmemiş IPolicyConfig (Windows 7+). Sadece SetDefaultEndpoint kullanılır; diğer yuvalar sıra için.
#[interface("f8679f50-850a-41cf-9c72-430f290290c8")]
unsafe trait IPolicyConfig: IUnknown {
    fn GetMixFormat(&self) -> HRESULT;
    fn GetDeviceFormat(&self) -> HRESULT;
    fn ResetDeviceFormat(&self) -> HRESULT;
    fn SetDeviceFormat(&self) -> HRESULT;
    fn GetProcessingPeriod(&self) -> HRESULT;
    fn SetProcessingPeriod(&self) -> HRESULT;
    fn GetShareMode(&self) -> HRESULT;
    fn SetShareMode(&self) -> HRESULT;
    fn GetPropertyValue(&self) -> HRESULT;
    fn SetPropertyValue(&self) -> HRESULT;
    fn SetDefaultEndpoint(&self, device_id: PCWSTR, role: ERole) -> HRESULT;
    fn SetEndpointVisibility(&self) -> HRESULT;
}
const CLSID_POLICY_CONFIG_CLIENT: GUID = GUID::from_u128(0x870af99c_171d_4f9e_af0d_e63df40c2bc9);

fn take_string(p: windows::core::PWSTR) -> String {
    let s = unsafe { p.to_string().unwrap_or_default() };
    unsafe { CoTaskMemFree(Some(p.0 as _)) };
    s
}

fn device_id(d: &IMMDevice) -> Option<String> {
    unsafe { d.GetId().ok().map(take_string) }
}

fn device_name(d: &IMMDevice) -> String {
    unsafe {
        let Ok(store) = d.OpenPropertyStore(STGM_READ) else { return String::new() };
        let Ok(pv) = store.GetValue(&PKEY_Device_FriendlyName) else { return String::new() };
        PropVariantToStringAlloc(&pv).map(take_string).unwrap_or_default()
    }
}

fn active_devices(en: &IMMDeviceEnumerator) -> Vec<IMMDevice> {
    unsafe {
        let Ok(col) = en.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE) else { return vec![] };
        let n = col.GetCount().unwrap_or(0);
        (0..n).filter_map(|i| col.Item(i).ok()).collect()
    }
}

fn default_device_id(en: &IMMDeviceEnumerator) -> Option<String> {
    unsafe { en.GetDefaultAudioEndpoint(eRender, eConsole).ok().and_then(|d| device_id(&d)) }
}

/// Değişince (cihaz takıldı/çıkarıldı, varsayılan değişti) liste yayınlanır
fn publish_devices(en: &IMMDeviceEnumerator, events: &Events, sig: &mut Option<(Vec<String>, Option<String>)>) {
    let devices = active_devices(en);
    let ids: Vec<String> = devices.iter().filter_map(device_id).collect();
    let default = default_device_id(en);
    if sig.as_ref().is_some_and(|(i, d)| *i == ids && *d == default) {
        return;
    }
    let list: Vec<_> = devices
        .iter()
        .filter_map(|d| {
            let id = device_id(d)?;
            Some(json!({ "id": id, "name": device_name(d), "default": Some(&id) == default.as_ref() }))
        })
        .collect();
    *sig = Some((ids, default));
    events.publish("audio-devices", json!({ "devices": list }));
}

/// Sadece şu an etkin cihazlar arasından seçilebilir (telefon rastgele kimlik gönderemez)
fn switch_default(en: &IMMDeviceEnumerator, id: &str) -> bool {
    if !active_devices(en).iter().filter_map(device_id).any(|d| d == id) {
        return false;
    }
    unsafe {
        let Ok(policy) = CoCreateInstance::<_, IPolicyConfig>(&CLSID_POLICY_CONFIG_CLIENT, None, CLSCTX_ALL) else {
            crate::log!("Ses cihazı değiştirilemedi: IPolicyConfig açılamadı");
            return false;
        };
        let wide: Vec<u16> = id.encode_utf16().chain(std::iter::once(0)).collect();
        let mut ok = true;
        for role in [eConsole, eMultimedia, eCommunications] {
            ok &= policy.SetDefaultEndpoint(PCWSTR(wide.as_ptr()), role).is_ok();
        }
        ok
    }
}

// ------------------------------------------------------------------ uygulama başına ses (karıştırıcı)

struct Sess {
    id: String,
    name: String,
    active: bool,
    vol: ISimpleAudioVolume,
}

/// İşlem kimliğinden program adı ("chrome" gibi, .exe'siz)
/// Program adı önbelleği: aynı işlem kimliği için her taramada OpenProcess çağrılmasın (30 sn'de bir tazelenir)
static STEM_CACHE: std::sync::Mutex<Option<(std::time::Instant, std::collections::HashMap<u32, String>)>> = std::sync::Mutex::new(None);

fn exe_stem(pid: u32) -> String {
    if pid != 0 {
        let mut g = STEM_CACHE.lock().unwrap_or_else(|e| e.into_inner());
        let fresh = g.as_ref().is_some_and(|(t, _)| t.elapsed() < Duration::from_secs(30));
        if !fresh {
            *g = Some((std::time::Instant::now(), std::collections::HashMap::new()));
        }
        if let Some(s) = g.as_ref().and_then(|(_, m)| m.get(&pid)) {
            return s.clone();
        }
        drop(g);
        let s = exe_stem_uncached(pid);
        if let Some((_, m)) = STEM_CACHE.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
            m.insert(pid, s.clone());
        }
        return s;
    }
    exe_stem_uncached(pid)
}

fn exe_stem_uncached(pid: u32) -> String {
    unsafe {
        let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else { return String::new() };
        let mut path = [0u16; 520];
        let mut len = path.len() as u32;
        let ok = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(path.as_mut_ptr()), &mut len).is_ok();
        let _ = CloseHandle(h);
        if !ok {
            return String::new();
        }
        let full = String::from_utf16_lossy(&path[..len as usize]);
        let file = full.rsplit('\\').next().unwrap_or("");
        file.strip_suffix(".exe").or_else(|| file.strip_suffix(".EXE")).unwrap_or(file).to_string()
    }
}

pub(crate) fn pretty_name(stem: &str) -> String {
    match stem.to_lowercase().as_str() {
        "chrome" => "Google Chrome".into(),
        "msedge" => "Microsoft Edge".into(),
        "firefox" => "Firefox".into(),
        "spotify" => "Spotify".into(),
        "discord" => "Discord".into(),
        "vlc" => "VLC".into(),
        "steam" | "steamwebhelper" => "Steam".into(),
        "teams" | "ms-teams" => "Microsoft Teams".into(),
        other => {
            let mut c = other.chars();
            c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
        }
    }
}

/// Varsayılan çıkış cihazındaki ses oturumları (süresi dolanlar hariç)
fn collect_sessions(en: &IMMDeviceEnumerator) -> Vec<Sess> {
    unsafe {
        let Ok(dev) = en.GetDefaultAudioEndpoint(eRender, eConsole) else { return vec![] };
        let Ok(mgr) = dev.Activate::<IAudioSessionManager2>(CLSCTX_ALL, None) else { return vec![] };
        let Ok(list) = mgr.GetSessionEnumerator() else { return vec![] };
        let n = list.GetCount().unwrap_or(0);
        let mut out = Vec::new();
        for i in 0..n {
            let Ok(ctrl) = list.GetSession(i) else { continue };
            let Ok(state) = ctrl.GetState() else { continue };
            if state == AudioSessionStateExpired {
                continue;
            }
            let (Ok(c2), Ok(vol)) = (ctrl.cast::<IAudioSessionControl2>(), ctrl.cast::<ISimpleAudioVolume>()) else { continue };
            let (id, name) = if c2.IsSystemSoundsSession() == S_OK {
                ("system".to_string(), "Sistem sesleri".to_string())
            } else {
                let stem = exe_stem(c2.GetProcessId().unwrap_or(0));
                let low = stem.to_lowercase();
                // Windows'un ses motoru ve arka plan yardımcıları kullanıcıya gösterilmez (Steam'in web yardımcısı Steam sayılır)
                if stem.is_empty() || low == "audiodg" || (low.contains("helper") && low != "steamwebhelper") {
                    continue;
                }
                // Aynı uygulamanın farklı programları (steam + steamwebhelper) tek satırda toplansın: kimlik görünen addan
                let name = pretty_name(&stem);
                (name.to_lowercase(), name)
            };
            out.push(Sess { id, name, active: state == AudioSessionStateActive, vol });
        }
        out
    }
}

/// Aynı programın birden çok oturumu (ör. Chrome) tek satırda toplanır; çalanlar üstte
fn mixer_state(sessions: &[Sess]) -> serde_json::Value {
    let mut apps: Vec<(String, String, f32, bool, bool)> = Vec::new(); // id, ad, seviye, sessiz, çalıyor
    for s in sessions {
        let (level, muted) = unsafe { (s.vol.GetMasterVolume().unwrap_or(1.0), s.vol.GetMute().map(|m| m.as_bool()).unwrap_or(false)) };
        match apps.iter_mut().find(|a| a.0 == s.id) {
            Some(a) => {
                a.2 = a.2.max(level);
                a.3 &= muted;
                a.4 |= s.active;
            }
            None => apps.push((s.id.clone(), s.name.clone(), level, muted, s.active)),
        }
    }
    apps.sort_by(|a, b| b.4.cmp(&a.4).then(a.1.to_lowercase().cmp(&b.1.to_lowercase())));
    json!({ "apps": apps.iter().map(|a| json!({ "id": a.0, "name": a.1, "level": (a.2 * 100.0).round() / 100.0, "muted": a.3, "active": a.4 })).collect::<Vec<_>>() })
}

fn set_app(en: &IMMDeviceEnumerator, id: &str, level: Option<f32>, mute: Option<bool>) {
    for s in collect_sessions(en).iter().filter(|s| s.id == id) {
        unsafe {
            if let Some(l) = level {
                let _ = s.vol.SetMasterVolume(l.clamp(0.0, 1.0), std::ptr::null());
            }
            if let Some(m) = mute {
                let _ = s.vol.SetMute(m, std::ptr::null());
            }
        }
    }
}

fn endpoint(en: &IMMDeviceEnumerator) -> windows::core::Result<IAudioEndpointVolume> {
    unsafe { en.GetDefaultAudioEndpoint(eRender, eConsole)?.Activate(CLSCTX_ALL, None) }
}

fn read(v: &IAudioEndpointVolume) -> windows::core::Result<(f32, bool)> {
    unsafe { Ok((v.GetMasterVolumeLevelScalar()?, v.GetMute()?.as_bool())) }
}

impl Audio {
    pub fn start(events: Arc<Events>) -> Audio {
        let (tx, rx) = channel::<AudioCmd>();
        std::thread::Builder::new()
            .name("ses".into())
            .spawn(move || {
                let en: IMMDeviceEnumerator = unsafe {
                    let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
                    match CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) {
                        Ok(e) => e,
                        Err(e) => {
                            crate::log!("Ses denetimi açılamadı: {e}");
                            return;
                        }
                    }
                };
                let mut last: Option<(f32, bool)> = None;
                let mut sig = None;
                let mut mixer_sig = String::new();
                let mut tick = 0u32;
                loop {
                    // Dinleyen telefon yoksa 5 sn'de bir uyanır ve hiçbir şey taramaz; komut gelirse hemen uyanır
                    let watched = events.subscribers() > 0;
                    let cmd = rx.recv_timeout(Duration::from_millis(if watched { 700 } else { 5000 }));
                    if matches!(cmd, Err(RecvTimeoutError::Disconnected)) {
                        return;
                    }
                    if !watched && matches!(cmd, Err(RecvTimeoutError::Timeout)) {
                        last = None; // biri bağlanınca güncel seviye gitsin
                        continue;
                    }
                    tick = tick.wrapping_add(1);
                    if matches!(cmd, Ok(AudioCmd::Refresh)) {
                        last = None;
                        sig = None;
                        mixer_sig.clear();
                    }
                    if let Ok(AudioCmd::SetDefault(id)) = &cmd {
                        let ok = switch_default(&en, id);
                        crate::log!("Ses çıkışı değiştirildi: {}", if ok { "ok" } else { "başarısız" });
                        last = None; // yeni cihazın seviyesi telefona gitsin
                    }
                    if let Ok(AudioCmd::SetApp { id, level, mute }) = &cmd {
                        set_app(&en, id, *level, *mute);
                        mixer_sig.clear(); // hemen yansısın
                    }
                    // Ses seviyesi her turda (ucuz); cihaz listesi ve uygulama karıştırıcısı ağır: ~2 sn'de bir ya da komut/bağlanma sonrası
                    let heavy = tick % 3 == 0 || matches!(cmd, Ok(AudioCmd::SetDefault(_) | AudioCmd::SetApp { .. } | AudioCmd::Refresh));
                    if heavy {
                        publish_devices(&en, &events, &mut sig);
                        let state = mixer_state(&collect_sessions(&en));
                        let text = state.to_string();
                        if text != mixer_sig {
                            mixer_sig = text;
                            events.publish("mixer", state);
                        }
                    }
                    let Ok(vol) = endpoint(&en) else {
                        continue; // ses cihazı yok (ör. çıkarıldı)
                    };
                    match cmd {
                        Ok(AudioCmd::SetLevel(l)) => unsafe {
                            let _ = vol.SetMasterVolumeLevelScalar(l.clamp(0.0, 1.0), std::ptr::null());
                            if l > 0.0 {
                                let _ = vol.SetMute(false, std::ptr::null());
                            }
                        },
                        Ok(AudioCmd::SetMute(m)) => unsafe {
                            let _ = vol.SetMute(m, std::ptr::null());
                        },
                        Ok(AudioCmd::SetDefault(_)) | Ok(AudioCmd::SetApp { .. }) | Ok(AudioCmd::Refresh) => {}
                        Ok(AudioCmd::Get(reply)) => {
                            let _ = reply.send(read(&vol).ok());
                        }
                        Err(RecvTimeoutError::Timeout) => {}
                        Err(RecvTimeoutError::Disconnected) => return,
                    }
                    if let Ok(now) = read(&vol) {
                        let changed = last.is_none_or(|(l, m)| (l - now.0).abs() > 0.004 || m != now.1);
                        if changed {
                            last = Some(now);
                            events.publish("volume", json!({ "level": now.0, "muted": now.1 }));
                        }
                    }
                }
            })
            .expect("ses thread'i");
        Audio { tx }
    }

    pub fn send(&self, cmd: AudioCmd) {
        let _ = self.tx.send(cmd);
    }

    pub fn get(&self) -> Option<(f32, bool)> {
        let (tx, rx) = channel();
        self.send(AudioCmd::Get(tx));
        rx.recv_timeout(Duration::from_secs(2)).ok().flatten()
    }
}

/// `--print-audio`: etkin çıkış cihazlarını yazdırır. Değiştirme çağrısını da dener ama zararsız biçimde:
/// şu anki varsayılanı yeniden varsayılan yapar (hiçbir şey değişmez).
pub fn print_devices() {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let Ok(en) = CoCreateInstance::<_, IMMDeviceEnumerator>(&MMDeviceEnumerator, None, CLSCTX_ALL) else { return };
        let default = default_device_id(&en);
        for d in active_devices(&en) {
            let id = device_id(&d).unwrap_or_default();
            println!("{} {} [{}]", if Some(&id) == default.as_ref() { "*" } else { " " }, device_name(&d), id);
        }
        for a in mixer_state(&collect_sessions(&en))["apps"].as_array().into_iter().flatten() {
            println!("  uygulama: {} (%{}){}{}", a["name"], (a["level"].as_f64().unwrap_or(0.0) * 100.0).round(), if a["muted"] == true { " sessiz" } else { "" }, if a["active"] == true { " çalıyor" } else { "" });
        }
        if let Some(d) = &default {
            println!("SetDefaultEndpoint(şu anki varsayılan) → {}", if switch_default(&en, d) { "ok" } else { "BAŞARISIZ" });
        }
    }
}
