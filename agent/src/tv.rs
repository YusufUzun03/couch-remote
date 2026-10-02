//! LG webOS televizyon köprüsü (SSAP).
//! Telefon TV'ye doğrudan bağlanamaz (TV'nin WebSocket'i şifresiz/öz imzalı), bu yüzden PC bir aracı gibi çalışır:
//! telefon `tv` komutu yollar, PC TV'ye bağlı kalır ve durumu (ses, açık uygulama, girişler) telefonlara yayınlar.
//! Telefondan serbest URI geçmez: yalnızca aşağıdaki sabit eşleme çalışır. TV adresi yalnızca yerel ağ adresi olabilir.
//! Eşleşme anahtarı (`client-key`) Windows DPAPI ile şifreli saklanır. TV sertifikası öz imzalıdır ve doğrulanmaz
//! (yerel ağ, tüm LG kütüphaneleri böyle çalışır); bu yüzden anahtar yalnızca ev ağında dolaşır.
use crate::events::Events;
use crate::sync::LockExt;
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::net::{Ipv4Addr, SocketAddr};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::{TcpStream, UdpSocket};
use tokio::sync::mpsc;
use tokio::time::timeout;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;
use tokio_tungstenite::{client_async_with_config, WebSocketStream};

mod android;
mod samsung;

trait Io: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Io for T {}
type Ws = WebSocketStream<Box<dyn Io>>;

const KEYS: &[&str] = &[
    "UP", "DOWN", "LEFT", "RIGHT", "ENTER", "BACK", "HOME", "EXIT", "MENU", "INFO", "RED", "GREEN", "YELLOW", "BLUE", "0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "DASH",
    "VOLUMEUP", "VOLUMEDOWN", "MUTE", "CHANNELUP", "CHANNELDOWN",
];

const PERMISSIONS: &[&str] = &[
    "LAUNCH", "LAUNCH_WEBAPP", "APP_TO_APP", "CLOSE", "TEST_OPEN", "TEST_PROTECTED", "CONTROL_AUDIO", "CONTROL_DISPLAY", "CONTROL_INPUT_JOYSTICK",
    "CONTROL_INPUT_MEDIA_RECORDING", "CONTROL_INPUT_MEDIA_PLAYBACK", "CONTROL_INPUT_TV", "CONTROL_INPUT_TEXT", "CONTROL_MOUSE_AND_KEYBOARD", "CONTROL_POWER",
    "READ_APP_STATUS", "READ_CURRENT_CHANNEL", "READ_INPUT_DEVICE_LIST", "READ_NETWORK_STATE", "READ_RUNNING_APPS", "READ_INSTALLED_APPS", "READ_TV_CHANNEL_LIST",
    "WRITE_NOTIFICATION_TOAST", "READ_POWER_STATE", "READ_COUNTRY_INFO", "READ_SETTINGS", "CONTROL_TV_SCREEN", "CONTROL_TV_STANDBY",
];

#[derive(Serialize, Deserialize, Clone, Default)]
struct Cfg {
    #[serde(default)]
    id: String,
    /// lg | samsung | android
    #[serde(default = "lg")]
    brand: String,
    ip: String,
    name: String,
    mac: Option<String>,
    /// DPAPI ile şifreli `client-key`
    key_enc: Option<String>,
}

impl Cfg {
    fn path() -> std::path::PathBuf {
        crate::state::data_dir().join("tvs.json")
    }
    /// tvs.json; yoksa eski tek-TV dosyasından (tv.json) taşınır
    fn load_all() -> Vec<Cfg> {
        let mut list: Vec<Cfg> = std::fs::read_to_string(Self::path()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
        if list.is_empty() {
            let old = crate::state::data_dir().join("tv.json");
            if let Some(mut c) = std::fs::read_to_string(&old).ok().and_then(|t| serde_json::from_str::<Cfg>(&t).ok()) {
                c.id = new_id();
                list.push(c);
                let _ = std::fs::write(Self::path(), serde_json::to_string_pretty(&list).unwrap_or_default());
                let _ = std::fs::remove_file(old);
            }
        }
        for c in &mut list {
            if c.id.is_empty() {
                c.id = new_id();
            }
        }
        list
    }
    fn key(&self) -> Option<String> {
        crate::dpapi::unprotect_str(self.key_enc.as_ref()?).ok()
    }
}

/// Yedek için: kayıtlı TV'ler, eşleşme anahtarları çözülmüş (parolalı dosyada saklanır). Android TV anahtarı taşınmaz (yeniden eşleşir)
pub fn export_tvs() -> Vec<Value> {
    Cfg::load_all()
        .into_iter()
        .map(|c| json!({ "id": c.id, "brand": c.brand, "ip": c.ip, "name": c.name, "mac": c.mac, "key": if c.brand == "android" { None } else { c.key() } }))
        .collect()
}

pub fn import_tvs(list: &[Value]) {
    let cfgs: Vec<Cfg> = list
        .iter()
        .filter_map(|v| {
            let ip = v["ip"].as_str().and_then(lan_ip)?.to_string();
            let brand = v["brand"].as_str().filter(|b| brand_ok(b))?.to_string();
            Some(Cfg {
                id: v["id"].as_str().filter(|i| word_ok(i)).map(str::to_string).unwrap_or_else(new_id),
                brand,
                ip,
                name: v["name"].as_str().unwrap_or("TV").chars().take(60).collect(),
                mac: v["mac"].as_str().map(str::to_string),
                key_enc: v["key"].as_str().and_then(|k| crate::dpapi::protect_str(k).ok()),
            })
        })
        .collect();
    let _ = std::fs::write(Cfg::path(), serde_json::to_string_pretty(&cfgs).unwrap_or_default());
}

fn lg() -> String {
    "lg".into()
}

fn brand_ok(b: &str) -> bool {
    matches!(b, "lg" | "samsung" | "android")
}

fn new_id() -> String {
    let mut b = [0u8; 3];
    rand::fill(&mut b);
    format!("{:02x}{:02x}{:02x}", b[0], b[1], b[2])
}

/// Tüm TV'lerin kayıtları ve telefonlara giden birleşik durum
struct Hub {
    events: Arc<Events>,
    cfgs: Mutex<Vec<Cfg>>,
    views: Mutex<Vec<(String, Value)>>,
}

impl Hub {
    fn save(&self, c: &Cfg) {
        let mut l = self.cfgs.lk();
        match l.iter_mut().find(|x| x.id == c.id) {
            Some(x) => *x = c.clone(),
            None => l.push(c.clone()),
        }
        let _ = std::fs::write(Cfg::path(), serde_json::to_string_pretty(&*l).unwrap_or_default());
    }
    /// Aynı adres yoksa ekler (iki eşzamanlı "ekle" tek kayıt üretsin diye kilit altında)
    fn insert_if_new(&self, c: &Cfg) -> bool {
        let mut l = self.cfgs.lk();
        if l.iter().any(|x| x.ip == c.ip) {
            return false;
        }
        l.push(c.clone());
        let _ = std::fs::write(Cfg::path(), serde_json::to_string_pretty(&*l).unwrap_or_default());
        true
    }
    fn delete(&self, id: &str) {
        let mut l = self.cfgs.lk();
        l.retain(|x| x.id != id);
        let _ = std::fs::write(Cfg::path(), serde_json::to_string_pretty(&*l).unwrap_or_default());
        self.views.lk().retain(|(i, _)| i != id);
        self.announce();
    }
    fn set_view(&self, id: &str, v: Value) {
        {
            let mut l = self.views.lk();
            match l.iter_mut().find(|(i, _)| i == id) {
                Some(x) => x.1 = v,
                None => l.push((id.to_string(), v)),
            }
        }
        self.announce();
    }
    fn announce(&self) {
        let tvs: Vec<Value> = self.views.lk().iter().map(|(_, v)| v.clone()).collect();
        self.events.publish("tvs", json!({ "tvs": tvs }));
    }
}

enum Cmd {
    /// Bağlantıyı (eşleşmeyi) yeniden dene
    Retry,
    Forget,
    PowerOn,
    /// Android TV eşleşme kodu (TV ekranındaki 6 karakter)
    PairCode(String),
    /// Markadan bağımsız eylem; her sürücü kendi protokolüne çevirir
    Act(Act),
    /// LG içi: SSAP isteği / işaretçi soketi satırı
    Req { uri: &'static str, payload: Value },
    Pointer(String),
}

#[derive(Clone, Debug)]
pub(crate) enum Act {
    Off,
    SetVol(i64),
    VolUp,
    VolDown,
    Mute(bool),
    ChUp,
    ChDown,
    Play,
    Pause,
    Stop,
    Rew,
    Ff,
    Launch(String),
    Input(String),
    Key(&'static str),
    Move(i64, i64),
    Click,
    Scroll(i64),
    Text(String),
    Enter,
    Backspace,
    /// Link aç: YouTube videosu (kimlik) ya da herhangi bir web adresi (TV tarayıcısı)
    OpenLink { youtube: Option<String>, url: String },
}

/// LG: eylemi SSAP isteğine / işaretçi satırına çevir
fn lg_cmd(a: Act) -> Option<Cmd> {
    let req = |uri: &'static str, payload: Value| Some(Cmd::Req { uri, payload });
    match a {
        Act::Off => req("ssap://system/turnOff", json!({})),
        Act::SetVol(l) => req("ssap://audio/setVolume", json!({ "volume": l.max(0) })),
        Act::VolUp => req("ssap://audio/volumeUp", json!({})),
        Act::VolDown => req("ssap://audio/volumeDown", json!({})),
        Act::Mute(b) => req("ssap://audio/setMute", json!({ "mute": b })),
        Act::ChUp => req("ssap://tv/channelUp", json!({})),
        Act::ChDown => req("ssap://tv/channelDown", json!({})),
        Act::Play => req("ssap://media.controls/play", json!({})),
        Act::Pause => req("ssap://media.controls/pause", json!({})),
        Act::Stop => req("ssap://media.controls/stop", json!({})),
        Act::Rew => req("ssap://media.controls/rewind", json!({})),
        Act::Ff => req("ssap://media.controls/fastForward", json!({})),
        Act::Launch(id) => req("ssap://system.launcher/launch", json!({ "id": id })),
        Act::Input(id) => req("ssap://tv/switchInput", json!({ "inputId": id })),
        Act::Key(k) => Some(Cmd::Pointer(format!("type:button\nname:{k}\n\n"))),
        Act::Move(x, y) => Some(Cmd::Pointer(format!("type:move\ndx:{x}\ndy:{y}\ndown:0\n\n"))),
        Act::Click => Some(Cmd::Pointer("type:click\n\n".into())),
        Act::Scroll(dy) => Some(Cmd::Pointer(format!("type:scroll\ndx:0\ndy:{dy}\n\n"))),
        Act::Text(t) => req("ssap://com.webos.service.ime/insertText", json!({ "text": t, "replace": 0 })),
        Act::Enter => req("ssap://com.webos.service.ime/sendEnterKey", json!({})),
        Act::Backspace => req("ssap://com.webos.service.ime/deleteCharacters", json!({ "count": 1 })),
        Act::OpenLink { youtube: Some(id), .. } => req("ssap://system.launcher/launch", json!({ "id": "youtube.leanback.v4", "contentId": id })),
        Act::OpenLink { url, .. } => req("ssap://system.launcher/open", json!({ "target": url })),
    }
}

pub struct Tv {
    hub: Arc<Hub>,
    tasks: Arc<Mutex<HashMap<String, mpsc::UnboundedSender<Cmd>>>>,
}

static SCANNING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

type Tasks = Arc<Mutex<HashMap<String, mpsc::UnboundedSender<Cmd>>>>;

fn spawn_tv(hub: &Arc<Hub>, tasks: &Tasks, cfg: Cfg) {
    let (tx, rx) = mpsc::unbounded_channel();
    tasks.lk().insert(cfg.id.clone(), tx);
    tokio::spawn(run(rx, hub.clone(), cfg));
}

fn lan_ip(s: &str) -> Option<Ipv4Addr> {
    let ip: Ipv4Addr = s.trim().parse().ok()?;
    (ip.is_private() || ip.is_link_local()).then_some(ip)
}

/// `arp -a` çıktısını (adres, MAC) çiftlerine ayır; MAC küçük harf ve ':' ayraçlı
fn parse_arp(text: &str) -> Vec<(Ipv4Addr, String)> {
    text.lines()
        .filter_map(|l| {
            let mut w = l.split_whitespace();
            let ip: Ipv4Addr = w.next()?.parse().ok()?;
            let mac = w.next()?;
            let ok = mac.len() == 17 && mac.matches('-').count() == 5 && mac.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-');
            ok.then(|| (ip, mac.to_lowercase().replace('-', ":")))
        })
        .collect()
}

/// ARP tablosu (TV aynı ağda ve yakın zamanda konuşulmuş olmalı). Windows'un kendi arp.exe'si tam yolla çağrılır
fn arp_table() -> Vec<(Ipv4Addr, String)> {
    use std::os::windows::process::CommandExt;
    let root = std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".into());
    let out = std::process::Command::new(format!("{root}\\System32\\arp.exe")).arg("-a").creation_flags(0x0800_0000).output();
    out.map(|o| parse_arp(&String::from_utf8_lossy(&o.stdout))).unwrap_or_default()
}

/// Uyandırma (Wake-on-LAN) için MAC
fn arp_mac(ip: Ipv4Addr) -> Option<String> {
    arp_table().into_iter().find(|(i, _)| *i == ip).map(|(_, m)| m)
}

/// TV'nin adresi değiştiyse (router DHCP): ağı yoklayıp MAC'inden yeni adresi bul
async fn rediscover(cfg: &Cfg, hub: &Hub) -> Option<Ipv4Addr> {
    let mac = cfg.mac.as_ref()?.to_lowercase().replace('-', ":");
    let _ = scan().await; // ağdaki cihazları konuşturur, ARP tablosu güncellenir
    let ip = tokio::task::spawn_blocking(arp_table).await.ok()?.into_iter().find(|(_, m)| *m == mac)?.0;
    let ip_s = ip.to_string();
    if ip_s == cfg.ip || lan_ip(&ip_s).is_none() || hub.cfgs.lk().iter().any(|c| c.ip == ip_s) {
        return None;
    }
    (detect(ip).await == Some(cfg.brand.as_str())).then_some(ip)
}

/// Uygulama kimliği: kimlik harfleri/rakamları, ayrıca Android TV bağlantıları (https://… veya şema:)
fn app_ok(s: &str) -> bool {
    word_ok(s) || ((s.starts_with("https://") || s.starts_with("spotify:")) && s.len() <= 200 && s.bytes().all(|b| b.is_ascii_graphic()))
}

/// YouTube video kimliği: 11 karakter (harf, rakam, - ve _)
fn youtube_id_ok(s: &str) -> bool {
    s.len() == 11 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// Yalnızca http(s) adresleri; boşluk, denetim karakteri ve tırnak yok
fn web_url_ok(s: &str) -> bool {
    (s.starts_with("https://") || s.starts_with("http://")) && s.len() <= 500 && s.bytes().all(|b| b.is_ascii_graphic() && b != b'"' && b != b'\\')
}

fn word_ok(s: &str) -> bool {
    !s.is_empty() && s.len() <= 80 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'_' || b == b'-')
}

impl Tv {
    pub fn start(events: Arc<Events>) -> Tv {
        let cfgs = Cfg::load_all();
        let hub = Arc::new(Hub { events, cfgs: Mutex::new(cfgs.clone()), views: Mutex::new(vec![]) });
        let tv = Tv { hub, tasks: Arc::new(Mutex::new(HashMap::new())) };
        for c in cfgs {
            tv.spawn(c);
        }
        tv.hub.announce();
        tv
    }

    fn spawn(&self, cfg: Cfg) {
        spawn_tv(&self.hub, &self.tasks, cfg);
    }

    /// Siri / Kestirmeler: `tv-<eylem>`; TV ad, adres ya da kimlikle seçilir (belirtilmezse ilk kayıtlı TV). Dönüş: TV adı
    pub fn shortcut(&self, action: &str, which: Option<&str>, v: &str) -> Result<String, &'static str> {
        let (id, name) = {
            let cfgs = self.hub.cfgs.lk();
            let found = match which.map(str::trim).filter(|w| !w.is_empty()) {
                Some(w) => {
                    let w = w.to_lowercase();
                    cfgs.iter().find(|c| c.id == w || c.ip == w || c.name.to_lowercase().contains(&w))
                }
                None => cfgs.first(),
            };
            match found {
                Some(c) => (c.id.clone(), c.name.clone()),
                None => return Err("TV bulunamadı: önce telefonda TV ekle"),
            }
        };
        let msg = match action {
            "on" => json!({ "op": "on" }),
            "off" => json!({ "op": "off" }),
            "volup" => json!({ "op": "vol-up" }),
            "voldown" => json!({ "op": "vol-down" }),
            "mute" => json!({ "op": "mute", "mute": true }),
            "volume" => match v.parse::<f64>() {
                Ok(l) if (0.0..=100.0).contains(&l) => json!({ "op": "vol", "level": l }),
                _ => return Err("v: 0 ile 100 arasında bir sayı olmalı"),
            },
            "launch" if app_ok(v) => json!({ "op": "launch", "app": v }),
            "launch" => return Err("v: geçerli bir uygulama kimliği olmalı"),
            "key" if KEYS.contains(&v.to_uppercase().as_str()) => json!({ "op": "key", "name": v.to_uppercase() }),
            "key" => return Err("v: geçerli bir tuş adı olmalı (HOME, BACK, ENTER, UP, DOWN, LEFT, RIGHT, MUTE…)"),
            _ => return Err("bilinmeyen TV eylemi"),
        };
        let mut msg = msg;
        msg["id"] = json!(id);
        self.handle(&msg);
        Ok(name)
    }

    /// Telefondan gelen `tv` komutu. Bilinmeyen / geçersiz her şey sessizce atılır.
    pub fn handle(&self, m: &Value) {
        let op = m["op"].as_str().unwrap_or("");
        // Genel komutlar (belirli bir TV'ye bağlı değil)
        match op {
            "scan" => {
                if SCANNING.swap(true, std::sync::atomic::Ordering::AcqRel) {
                    return; // zaten taranıyor
                }
                let events = self.hub.events.clone();
                tokio::spawn(async move {
                    let found = scan().await;
                    SCANNING.store(false, std::sync::atomic::Ordering::Release);
                    events.publish("tv-scan", json!({ "found": found }));
                });
                return;
            }
            "add" => {
                let Some(ip) = m["ip"].as_str().and_then(lan_ip) else { return };
                let given = m["brand"].as_str().filter(|b| brand_ok(b)).map(str::to_string);
                let (hub, tasks) = (self.hub.clone(), self.tasks.clone());
                tokio::spawn(async move {
                    let ip_s = ip.to_string();
                    let existing = hub.cfgs.lk().iter().find(|c| c.ip == ip_s).map(|c| c.id.clone());
                    if let Some(id) = existing {
                        if let Some(tx) = tasks.lk().get(&id) {
                            let _ = tx.send(Cmd::Retry);
                        }
                        return;
                    }
                    if hub.cfgs.lk().len() >= 8 {
                        hub.events.publish("tv-add", json!({ "ok": false, "ip": ip_s, "reason": "limit" }));
                        return;
                    }
                    let brand = match given {
                        Some(b) => b,
                        None => match detect(ip).await {
                            Some(b) => b.to_string(),
                            None => {
                                hub.events.publish("tv-add", json!({ "ok": false, "ip": ip_s, "reason": "notfound" }));
                                return;
                            }
                        },
                    };
                    let name = match brand.as_str() { "samsung" => "Samsung TV", "android" => "Android TV", _ => "LG TV" };
                    let cfg = Cfg { id: new_id(), brand, ip: ip_s, name: name.into(), mac: None, key_enc: None };
                    if hub.insert_if_new(&cfg) {
                        spawn_tv(&hub, &tasks, cfg);
                    }
                    hub.events.publish("tv-add", json!({ "ok": true, "ip": ip }));
                });
                return;
            }
            _ => {}
        }
        let Some(id) = m["id"].as_str() else { return };
        let int = |k: &str, lim: i64| m[k].as_f64().filter(|v| v.is_finite()).map(|v| (v.round() as i64).clamp(-lim, lim));
        let act = |a: Act| Some(Cmd::Act(a));
        let cmd = match op {
            "forget" => Some(Cmd::Forget),
            "on" => Some(Cmd::PowerOn),
            "retry" => Some(Cmd::Retry),
            "pair-code" => m["code"].as_str().filter(|c| c.len() == 6).map(|c| Cmd::PairCode(c.to_string())),
            "off" => act(Act::Off),
            "vol" => int("level", 100).and_then(|l| act(Act::SetVol(l))),
            "vol-up" => act(Act::VolUp),
            "vol-down" => act(Act::VolDown),
            "mute" => m["mute"].as_bool().and_then(|b| act(Act::Mute(b))),
            "ch-up" => act(Act::ChUp),
            "ch-down" => act(Act::ChDown),
            "play" => act(Act::Play),
            "pause" => act(Act::Pause),
            "stop" => act(Act::Stop),
            "rew" => act(Act::Rew),
            "ff" => act(Act::Ff),
            "launch" => m["app"].as_str().filter(|s| app_ok(s)).and_then(|id| act(Act::Launch(id.to_string()))),
            "input" => m["input"].as_str().filter(|s| word_ok(s)).and_then(|id| act(Act::Input(id.to_string()))),
            "key" => m["name"].as_str().and_then(|k| KEYS.iter().find(|x| **x == k)).and_then(|k| act(Act::Key(k))),
            "move" => act(Act::Move(int("dx", 200).unwrap_or(0), int("dy", 200).unwrap_or(0))),
            "click" => act(Act::Click),
            "scroll" => act(Act::Scroll(int("dy", 200).unwrap_or(0))),
            "text" => m["text"].as_str().filter(|t| t.len() <= 400).and_then(|t| act(Act::Text(t.to_string()))),
            "enter" => act(Act::Enter),
            "open-link" => match m["kind"].as_str() {
                Some("youtube") => m["video"].as_str().filter(|i| youtube_id_ok(i)).and_then(|i| act(Act::OpenLink { youtube: Some(i.to_string()), url: format!("https://www.youtube.com/watch?v={i}") })),
                Some("web") => m["url"].as_str().filter(|u| web_url_ok(u)).and_then(|u| act(Act::OpenLink { youtube: None, url: u.to_string() })),
                _ => None,
            },
            "backspace" => act(Act::Backspace),
            _ => None,
        };
        if let Some(c) = cmd {
            let forget = matches!(c, Cmd::Forget);
            if let Some(tx) = self.tasks.lk().get(id) {
                let _ = tx.send(c);
            }
            if forget {
                self.tasks.lk().remove(id);
            }
        }
    }
}

// ---------------------------------------------------------------- görünüm (telefonlara giden durum)

#[derive(Default, Clone)]
struct View {
    /// off | connecting | pairing | unpaired | on
    state: &'static str,
    volume: Option<i64>,
    muted: bool,
    app: String,
    inputs: Vec<Value>,
}

fn view_json(cfg: &Cfg, v: &View) -> Value {
    let (caps, apps) = match cfg.brand.as_str() {
        "samsung" => (
            json!({ "vol": false, "pointer": false, "apps": true, "inputs": false, "text": true, "link": true }),
            samsung::APPS,
        ),
        "android" => (json!({ "vol": false, "pointer": false, "apps": true, "inputs": false, "text": false, "link": true }), android::APPS),
        _ => (json!({ "vol": true, "pointer": true, "apps": true, "inputs": true, "text": true, "link": true }), LG_APPS),
    };
    let apps: Vec<Value> = apps.iter().map(|(l, id)| json!([l, id])).collect();
    json!({
        "id": cfg.id, "brand": cfg.brand, "name": cfg.name, "ip": cfg.ip, "state": v.state,
        "canWake": cfg.mac.is_some(), "caps": caps, "apps": apps,
        "volume": v.volume, "muted": v.muted, "app": v.app, "inputs": v.inputs,
    })
}

const LG_APPS: &[(&str, &str)] = &[
    ("Netflix", "netflix"),
    ("YouTube", "youtube.leanback.v4"),
    ("Prime Video", "amazon"),
    ("Disney+", "com.disney.disneyplus-prod"),
    ("Spotify", "spotify-beehive"),
    ("Tarayıcı", "com.webos.app.browser"),
];

// ---------------------------------------------------------------- bağlantı

/// Sertifikası doğrulanmayan TLS (TV öz imzalı sertifika kullanır; yalnızca yerel ağ adresleri için çağrılır)
mod insecure {
    use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerifier};
    use rustls::SignatureScheme;
    use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
    use rustls::{DigitallySignedStruct, Error};

    #[derive(Debug)]
    pub struct AcceptAny(pub std::sync::Arc<rustls::crypto::CryptoProvider>);
    impl ServerCertVerifier for AcceptAny {
        fn verify_server_cert(&self, _: &CertificateDer<'_>, _: &[CertificateDer<'_>], _: &ServerName<'_>, _: &[u8], _: UnixTime) -> Result<rustls::client::danger::ServerCertVerified, Error> {
            Ok(rustls::client::danger::ServerCertVerified::assertion())
        }
        fn verify_tls12_signature(&self, m: &[u8], c: &CertificateDer<'_>, d: &DigitallySignedStruct) -> Result<HandshakeSignatureValid, Error> {
            rustls::crypto::verify_tls12_signature(m, c, d, &self.0.signature_verification_algorithms)
        }
        fn verify_tls13_signature(&self, m: &[u8], c: &CertificateDer<'_>, d: &DigitallySignedStruct) -> Result<HandshakeSignatureValid, Error> {
            rustls::crypto::verify_tls13_signature(m, c, d, &self.0.signature_verification_algorithms)
        }
        fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
            self.0.signature_verification_algorithms.supported_schemes()
        }
    }
}

async fn open(ip: Ipv4Addr, port: u16, tls: bool, path: &str) -> anyhow::Result<Ws> {
    let tcp = timeout(Duration::from_secs(3), TcpStream::connect(SocketAddr::from((ip, port)))).await??;
    let _ = tcp.set_nodelay(true);
    let io: Box<dyn Io> = if tls {
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let cfg = rustls::ClientConfig::builder_with_provider(provider.clone())
            .with_safe_default_protocol_versions()?
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(insecure::AcceptAny(provider)))
            .with_no_client_auth();
        let name = rustls::pki_types::ServerName::IpAddress(std::net::IpAddr::V4(ip).into());
        Box::new(timeout(Duration::from_secs(4), tokio_rustls::TlsConnector::from(Arc::new(cfg)).connect(name, tcp)).await??)
    } else {
        Box::new(tcp)
    };
    let url = format!("{}://{ip}:{port}{path}", if tls { "wss" } else { "ws" });
    let (ws, _) = timeout(Duration::from_secs(4), client_async_with_config(url.into_client_request()?, io, Some(WebSocketConfig::default().max_message_size(Some(1 << 20)).max_frame_size(Some(1 << 20))))).await??;
    Ok(ws)
}

/// Yeni TV'ler 3001 (wss), eskiler 3000 (ws) kullanır
async fn connect(ip: Ipv4Addr) -> Option<(Ws, bool)> {
    if let Ok(ws) = open(ip, 3001, true, "/").await {
        return Some((ws, true));
    }
    open(ip, 3000, false, "/").await.ok().map(|ws| (ws, false))
}

async fn send(ws: &mut Ws, v: Value) -> bool {
    ws.send(Message::Text(v.to_string().into())).await.is_ok()
}

fn register_msg(key: Option<String>) -> Value {
    let mut payload = json!({
        "forcePairing": false, "pairingType": "PROMPT",
        "manifest": { "manifestVersion": 1, "appVersion": "1.1", "permissions": PERMISSIONS },
    });
    if let Some(k) = key {
        payload["client-key"] = json!(k);
    }
    json!({ "type": "register", "id": "reg", "payload": payload })
}

// ---------------------------------------------------------------- uyandırma / tarama

async fn wake(mac: &str, ip: &str) {
    let bytes: Vec<u8> = mac.split([':', '-']).filter_map(|h| u8::from_str_radix(h, 16).ok()).collect();
    if bytes.len() != 6 {
        return;
    }
    let mut pkt = vec![0xffu8; 6];
    for _ in 0..16 {
        pkt.extend_from_slice(&bytes);
    }
    let Ok(sock) = UdpSocket::bind("0.0.0.0:0").await else { return };
    let _ = sock.set_broadcast(true);
    let mut targets = vec!["255.255.255.255:9".to_string()];
    if let Some(ip) = lan_ip(ip) {
        let o = ip.octets();
        targets.push(format!("{}.{}.{}.255:9", o[0], o[1], o[2]));
    }
    for _ in 0..3 {
        for t in &targets {
            let _ = sock.send_to(&pkt, t).await;
        }
        tokio::time::sleep(Duration::from_millis(150)).await;
    }
}

async fn port_open(ip: Ipv4Addr, port: u16) -> bool {
    timeout(Duration::from_millis(900), TcpStream::connect((ip, port))).await.is_ok_and(|r| r.is_ok())
}

/// Açık kumanda portlarına bakarak markayı bul: Android TV (6466/6467), Samsung (8002/8001), LG (3001/3000)
async fn detect(ip: Ipv4Addr) -> Option<&'static str> {
    let (a, b, c, d, e) = tokio::join!(port_open(ip, 6467), port_open(ip, 8002), port_open(ip, 8001), port_open(ip, 3001), port_open(ip, 3000));
    if a {
        Some("android")
    } else if b || c {
        Some("samsung")
    } else if d || e {
        Some("lg")
    } else {
        None
    }
}

/// mDNS sorgusu (Android TV hizmeti, yanıt doğrudan bize gelsin diye "unicast" biti açık)
fn mdns_query() -> Vec<u8> {
    let mut q = vec![0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0];
    for label in ["_androidtvremote2", "_tcp", "local"] {
        q.push(label.len() as u8);
        q.extend_from_slice(label.as_bytes());
    }
    q.extend_from_slice(&[0, 0, 12, 0x80, 1]);
    q
}

/// Ağdaki TV'leri bul: SSDP + mDNS ile aday cihazları topla, her adayın markasını portlarından tanı
async fn scan() -> Vec<Value> {
    let Ok(sock) = UdpSocket::bind("0.0.0.0:0").await else { return vec![] };
    for st in ["urn:lge-com:service:webos-second-screen:1", "urn:samsung.com:device:RemoteControlReceiver:1", "urn:schemas-upnp-org:device:MediaRenderer:1", "ssdp:all"] {
        let msg = format!("M-SEARCH * HTTP/1.1\r\nHOST: 239.255.255.250:1900\r\nMAN: \"ssdp:discover\"\r\nMX: 2\r\nST: {st}\r\n\r\n");
        let _ = sock.send_to(msg.as_bytes(), "239.255.255.250:1900").await;
    }
    for _ in 0..2 {
        let _ = sock.send_to(&mdns_query(), "224.0.0.251:5353").await;
    }
    let mut candidates: Vec<Ipv4Addr> = vec![];
    let mut buf = [0u8; 2048];
    let end = tokio::time::Instant::now() + Duration::from_millis(3200);
    while let Ok(Ok((_, from))) = tokio::time::timeout_at(end, sock.recv_from(&mut buf)).await {
        if let std::net::IpAddr::V4(ip) = from.ip() {
            if lan_ip(&ip.to_string()).is_some() && !candidates.contains(&ip) && candidates.len() < 64 {
                candidates.push(ip);
            }
        }
    }
    let checks = candidates.into_iter().map(|ip| async move { detect(ip).await.map(|b| json!({ "ip": ip.to_string(), "brand": b })) });
    let mut found = vec![];
    for r in futures_util::future::join_all(checks).await {
        found.extend(r);
    }
    found
}

// ---------------------------------------------------------------- ana döngü

struct Ctx {
    hub: Arc<Hub>,
    cfg: Option<Cfg>,
    view: View,
}

impl Ctx {
    fn publish(&self) {
        if let Some(c) = &self.cfg {
            self.hub.set_view(&c.id, view_json(c, &self.view));
        }
    }

    /// Bağlantıdan bağımsız komutlar. true: bağlantı yeniden kurulmalı
    async fn common(&mut self, cmd: &Cmd) -> bool {
        match cmd {
            Cmd::Retry => {
                self.view = View::default();
                return true;
            }
            Cmd::Forget => {
                if let Some(c) = self.cfg.take() {
                    self.hub.delete(&c.id);
                }
                return true;
            }
            Cmd::PowerOn => {
                if let Some(c) = &self.cfg {
                    if let Some(mac) = &c.mac {
                        crate::log!("TV uyandırma paketi gönderiliyor");
                        wake(mac, &c.ip).await;
                    }
                }
                return true;
            }
            _ => {}
        }
        false
    }
}

async fn run(mut rx: mpsc::UnboundedReceiver<Cmd>, hub: Arc<Hub>, cfg: Cfg) {
    let mut cx = Ctx { hub, cfg: Some(cfg), view: View::default() };
    cx.view.state = "off";
    cx.publish();
    let mut retry = true;
    let mut fails = 0u32;
    loop {
        let Some(cfg) = cx.cfg.clone() else { return };
        let ip: Ipv4Addr = cfg.ip.parse().unwrap_or(Ipv4Addr::UNSPECIFIED);
        let mut rejoin = true;
        if retry {
            cx.view.state = "connecting";
            cx.publish();
            rejoin = match cfg.brand.as_str() {
                "samsung" => samsung::session(ip, &mut cx, &mut rx).await,
                "android" => android::session(ip, &mut cx, &mut rx).await,
                _ => match connect(ip).await {
                    Some((ws, _)) => session(ws, ip, &mut cx, &mut rx).await,
                    None => true,
                },
            };
        }
        if cx.view.state == "on" {
            fails = 0;
        } else if retry {
            fails += 1;
        }
        cx.view = View { state: if cx.cfg.as_ref().is_some_and(|c| c.key_enc.is_some()) { "off" } else { "unpaired" }, ..Default::default() };
        cx.publish();
        if !rejoin {
            continue;
        }
        // Üst üste başarısız: adres değişmiş olabilir (DHCP). MAC biliniyorsa yeni adresi bul (ilk kez 3. denemede, sonra her 4 denemede)
        if fails >= 3 && fails % 4 == 3 {
            if let Some(c) = cx.cfg.clone() {
                if let Some(nip) = rediscover(&c, &cx.hub).await {
                    if let Some(m) = cx.cfg.as_mut() {
                        m.ip = nip.to_string();
                        cx.hub.save(m);
                    }
                    crate::log!("TV adresi değişmiş, yeni adres bulundu: {nip}");
                    fails = 0;
                    retry = true;
                    continue;
                }
            }
        }
        // TV kapalı: anahtar varsa 12 sn'de bir dene; anahtar yoksa (eşleşme yapılmadı) komut bekle
        let wait = if cx.cfg.as_ref().is_some_and(|c| c.key_enc.is_some()) { Duration::from_secs(12) } else { Duration::from_secs(86400) };
        retry = match timeout(wait, rx.recv()).await {
            Ok(Some(cmd)) => {
                cx.common(&cmd).await;
                cx.cfg.is_some()
            }
            Ok(None) => return,
            Err(_) => true,
        };
    }
}

/// Bir bağlantı oturumu. Dönüş: false = oturum dışarıdan kapatıldı (yeni ayar), hemen yeniden başla
async fn session(mut ws: Ws, ip: Ipv4Addr, cx: &mut Ctx, rx: &mut mpsc::UnboundedReceiver<Cmd>) -> bool {
    let key = cx.cfg.as_ref().and_then(Cfg::key);
    if !send(&mut ws, register_msg(key.clone())).await {
        return true;
    }
    let mut ptr: Option<Ws> = None;
    let mut ptr_wait = false;
    let mut ptr_queue: Vec<String> = vec![];
    let mut counter = 0u32;
    let mut ping = tokio::time::interval(Duration::from_secs(25));
    ping.tick().await;
    loop {
        tokio::select! {
            cmd = rx.recv() => {
                let Some(cmd) = cmd else { return false };
                let cmd = match cmd {
                    Cmd::Act(a) => match lg_cmd(a) { Some(c) => c, None => continue },
                    c => c,
                };
                if matches!(cmd, Cmd::Retry | Cmd::Forget) {
                    cx.common(&cmd).await;
                    return false;
                }
                match cmd {
                    Cmd::Req { uri, payload } => {
                        counter += 1;
                        if !send(&mut ws, json!({ "type": "request", "id": format!("c{counter}"), "uri": uri, "payload": payload })).await { return true; }
                    }
                    Cmd::Pointer(text) => {
                        if let Some(p) = ptr.as_mut() {
                            if p.send(Message::Text(text.into())).await.is_err() { ptr = None; }
                        } else if !ptr_wait {
                            ptr_wait = true;
                            ptr_queue.push(text);
                            if !send(&mut ws, json!({ "type": "request", "id": "ptr", "uri": "ssap://com.webos.service.networkinput/getPointerInputSocket" })).await { return true; }
                        } else if ptr_queue.len() < 20 {
                            ptr_queue.push(text);
                        }
                    }
                    other => { cx.common(&other).await; }
                }
            }
            _ = ping.tick() => {
                if ws.send(Message::Ping(Vec::new().into())).await.is_err() { return true; }
            }
            msg = ws.next() => {
                let Some(Ok(msg)) = msg else { return true };
                let Message::Text(text) = msg else { continue };
                let Ok(v) = serde_json::from_str::<Value>(text.as_str()) else { continue };
                let (kind, id, p) = (v["type"].as_str().unwrap_or(""), v["id"].as_str().unwrap_or(""), &v["payload"]);
                match (kind, id) {
                    ("registered", _) => {
                        if let Some(k) = p["client-key"].as_str() {
                            if let Some(c) = cx.cfg.as_mut() {
                                c.key_enc = crate::dpapi::protect_str(k).ok();
                                if c.name == "LG TV" || c.name.is_empty() { c.name = "LG TV".into(); }
                                cx.hub.save(c);
                            }
                        }
                        cx.view.state = "on";
                        crate::log!("TV bağlandı");
                        cx.publish();
                        for (id, uri, sub) in [
                            ("vol", "ssap://audio/getVolume", true),
                            ("fg", "ssap://com.webos.applicationManager/getForegroundAppInfo", true),
                            ("inputs", "ssap://tv/getExternalInputList", false),
                            ("net", "ssap://com.webos.service.connectionmanager/getinfo", false),
                            ("sw", "ssap://com.webos.service.update/getCurrentSWInformation", false),
                        ] {
                            let m = json!({ "type": if sub { "subscribe" } else { "request" }, "id": id, "uri": uri });
                            if !send(&mut ws, m).await { return true; }
                        }
                    }
                    ("response", "reg") => {
                        if p["pairingType"] == "PROMPT" {
                            cx.view.state = "pairing";
                            cx.publish();
                        }
                    }
                    ("error", "reg") => {
                        // Eski anahtar geçersiz (TV sıfırlanmış olabilir): anahtarı bırak, bir sonraki denemede yeniden eşleş
                        if let Some(c) = cx.cfg.as_mut() { c.key_enc = None; cx.hub.save(c); }
                        return true;
                    }
                    ("response", "vol") => {
                        let vs = if p["volumeStatus"].is_object() { &p["volumeStatus"] } else { p };
                        if let Some(l) = vs["volume"].as_i64() { cx.view.volume = Some(l); }
                        if let Some(m) = vs["muteStatus"].as_bool().or(vs["muted"].as_bool()) { cx.view.muted = m; }
                        cx.publish();
                    }
                    ("response", "fg") => {
                        cx.view.app = p["appId"].as_str().unwrap_or("").to_string();
                        cx.publish();
                    }
                    ("response", "inputs") => {
                        cx.view.inputs = p["devices"].as_array().map(|a| a.iter()
                            .map(|d| json!({ "id": d["id"], "label": d["label"], "connected": d["connected"] })).collect()).unwrap_or_default();
                        cx.publish();
                    }
                    ("response", "net") => {
                        let pick = |side: &str| p[format!("{side}Info")]["macAddress"].as_str().map(str::to_string);
                        let mac = if p["wired"]["state"] == "connected" { pick("wired").or(pick("wifi")) } else { pick("wifi").or(pick("wired")) };
                        if let (Some(mac), Some(c)) = (mac, cx.cfg.as_mut()) {
                            if c.mac.as_deref() != Some(&mac) { c.mac = Some(mac); cx.hub.save(c); cx.publish(); }
                        }
                    }
                    ("response", "sw") => {
                        if let (Some(model), Some(c)) = (p["model_name"].as_str(), cx.cfg.as_mut()) {
                            let name = format!("LG {model}");
                            if c.name != name { c.name = name; cx.hub.save(c); cx.publish(); }
                        }
                    }
                    ("response", "ptr") => {
                        ptr_wait = false;
                        if let Some(url) = p["socketPath"].as_str() {
                            let path = url.splitn(4, '/').nth(3).map(|s| format!("/{s}")).unwrap_or_default();
                            let sec = url.starts_with("wss");
                            let pp = url.split("//").nth(1).and_then(|h| h.split('/').next()).and_then(|h| h.rsplit(':').next()).and_then(|x| x.parse().ok()).unwrap_or(if sec { 3001 } else { 3000 });
                            if let Ok(mut s) = open(ip, pp, sec, &path).await {
                                for q in ptr_queue.drain(..) {
                                    if s.send(Message::Text(q.into())).await.is_err() { break; }
                                }
                                ptr = Some(s);
                            }
                        }
                        ptr_queue.clear();
                    }
                    _ => {}
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arp_tablosu_ayristirilir() {
        let t = "\r\nInterface: 192.168.1.20 --- 0x7\r\n  Internet Address      Physical Address      Type\r\n  192.168.1.1           aa-bb-cc-00-11-22     dynamic\r\n  192.168.1.50          A8-23-FE-12-34-56     dynamic\r\n  224.0.0.22            01-00-5e-00-00-16     static\r\n";
        let r = parse_arp(t);
        assert_eq!(r.len(), 3);
        assert_eq!(r[1], ("192.168.1.50".parse().unwrap(), "a8:23:fe:12:34:56".to_string()));
        assert!(parse_arp("Interface: 1.2.3.4 --- 0x7").is_empty());
    }

    #[test]
    fn yalnizca_yerel_ag_adresi_kabul_edilir() {
        assert!(lan_ip("192.168.1.40").is_some());
        assert!(lan_ip("10.0.0.7").is_some());
        assert!(lan_ip("8.8.8.8").is_none());
        assert!(lan_ip("127.0.0.1").is_none());
        assert!(lan_ip("tv.example.com").is_none());
    }

    #[test]
    fn uygulama_kimligi_denetlenir() {
        assert!(word_ok("youtube.leanback.v4"));
        assert!(!word_ok("a b"));
        assert!(!word_ok("../x"));
        assert!(!word_ok(""));
    }
}
