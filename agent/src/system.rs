//! Windows entegrasyonu: kendini kurma, Windows ile başlama, güvenlik duvarı, kaldırma, tek örnek, ağ profili.
//! Hedef: programı hiç tanımayan biri indirip çift tıkladığında her şey kendiliğinden hazır olsun.
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use windows::core::{w, HSTRING, PCWSTR};
use windows::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HANDLE};
use windows::Win32::System::Threading::{CreateMutexW, GetExitCodeProcess, WaitForSingleObject, INFINITE};
use windows::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW};
use windows::Win32::UI::WindowsAndMessaging::{
    MessageBoxW, IDYES, MB_ICONINFORMATION, MB_ICONQUESTION, MB_OK, MB_YESNO, MESSAGEBOX_STYLE, SW_HIDE,
};
use winreg::enums::HKEY_CURRENT_USER;
use winreg::RegKey;


const APP: &str = "CouchRemote";
const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const UNINSTALL_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\CouchRemote";
const FIREWALL_RULE: &str = "CouchRemote (yerel ağ)";

pub fn install_dir() -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    base.join("Programs").join(APP)
}

pub fn installed_exe() -> PathBuf {
    install_dir().join("CouchRemote.exe")
}

fn current_exe() -> PathBuf {
    std::env::current_exe().unwrap_or_default()
}

fn same_path(a: &Path, b: &Path) -> bool {
    let norm = |p: &Path| p.canonicalize().unwrap_or_else(|_| p.to_path_buf()).to_string_lossy().to_lowercase();
    norm(a) == norm(b)
}

pub fn is_installed_copy() -> bool {
    same_path(&current_exe(), &installed_exe())
}

/// Geliştirme derlemesinde veya COUCH_DEV ile kendini kurmaz (cargo run bozulmasın)
pub fn should_self_install() -> bool {
    !cfg!(debug_assertions) && std::env::var_os("COUCH_DEV").is_none() && !is_installed_copy()
}

pub fn message(text: &str, question: bool) -> bool {
    let style: MESSAGEBOX_STYLE = if question { MB_YESNO | MB_ICONQUESTION } else { MB_OK | MB_ICONINFORMATION };
    unsafe { MessageBoxW(None, &HSTRING::from(text), w!("CouchRemote"), style) == IDYES }
}

pub fn open_url(url: &str) {
    crate::launcher::shell_open(url);
}

// ------------------------------------------------------------------ tek örnek

pub struct InstanceGuard(HANDLE);
unsafe impl Send for InstanceGuard {}
unsafe impl Sync for InstanceGuard {}

impl Drop for InstanceGuard {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

/// İlk örnekse kilidi döner; program zaten çalışıyorsa None.
pub fn single_instance() -> Option<InstanceGuard> {
    unsafe {
        let h = CreateMutexW(None, true, w!("Local\\CouchRemoteAgent")).ok()?;
        if GetLastError() == ERROR_ALREADY_EXISTS {
            let _ = CloseHandle(h);
            return None;
        }
        Some(InstanceGuard(h))
    }
}

// ------------------------------------------------------------------ çalışan örnekle konuşma (güncelleme)

/// Çalışan örneğin yönetim API'sine istek (anahtar dosyadan okunur). Gövdeyi döner.
fn admin_request(method: &str, path: &str) -> Option<String> {
    use std::io::{Read, Write};
    let token = std::fs::read_to_string(crate::state::data_dir().join("admin.token")).ok()?;
    let addr: std::net::SocketAddr = ([127, 0, 0, 1], crate::ADMIN_PORT).into();
    let mut s = std::net::TcpStream::connect_timeout(&addr, std::time::Duration::from_secs(2)).ok()?;
    s.set_read_timeout(Some(std::time::Duration::from_secs(3))).ok()?;
    let host = format!("127.0.0.1:{}", crate::ADMIN_PORT);
    let req = format!(
        "{method} {path} HTTP/1.1\r\nHost: {host}\r\nCookie: couch_admin={}\r\nOrigin: http://{host}\r\nX-Couch-Admin: 1\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        token.trim()
    );
    s.write_all(req.as_bytes()).ok()?;
    let mut resp = String::new();
    s.read_to_string(&mut resp).ok()?;
    if !resp.starts_with("HTTP/1.1 200") {
        return None;
    }
    resp.split_once("\r\n\r\n").map(|(_, body)| body.to_string())
}

/// Çalışan CouchRemote'un sürümü (çalışmıyorsa None)
pub fn running_version() -> Option<String> {
    let body = admin_request("GET", "/api/version")?;
    serde_json::from_str::<serde_json::Value>(&body).ok()?["version"].as_str().map(str::to_owned)
}

/// Çalışan örnekten kapanmasını iste ve kapanana kadar bekle (en fazla 10 sn)
pub fn stop_running_instance() -> Option<InstanceGuard> {
    admin_request("POST", "/api/quit")?;
    for _ in 0..50 {
        std::thread::sleep(std::time::Duration::from_millis(200));
        if let Some(g) = single_instance() {
            return Some(g);
        }
    }
    None
}

// ------------------------------------------------------------------ Windows ile başlama

pub fn autostart_enabled() -> bool {
    RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(RUN_KEY)
        .and_then(|k| k.get_value::<String, _>(APP))
        .is_ok()
}

pub fn set_autostart(enabled: bool) -> Result<()> {
    let (run, _) = RegKey::predef(HKEY_CURRENT_USER).create_subkey(RUN_KEY)?;
    if enabled {
        let exe = if installed_exe().exists() { installed_exe() } else { current_exe() };
        run.set_value(APP, &format!("\"{}\" --background", exe.display()))?;
    } else {
        let _ = run.delete_value(APP);
    }
    Ok(())
}

// ------------------------------------------------------------------ yönetici olarak komut (UAC)

/// Programı yönetici izniyle çalıştırır ve bitmesini bekler. Kullanıcı UAC'yi reddederse false.
fn run_elevated(file: &str, params: &str) -> bool {
    let file = HSTRING::from(file);
    let params = HSTRING::from(params);
    let mut info = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOCLOSEPROCESS,
        lpVerb: w!("runas"),
        lpFile: PCWSTR(file.as_ptr()),
        lpParameters: PCWSTR(params.as_ptr()),
        nShow: SW_HIDE.0,
        ..Default::default()
    };
    unsafe {
        if ShellExecuteExW(&mut info).is_err() || info.hProcess.is_invalid() {
            return false;
        }
        WaitForSingleObject(info.hProcess, INFINITE);
        let mut code = 1u32;
        let _ = GetExitCodeProcess(info.hProcess, &mut code);
        let _ = CloseHandle(info.hProcess);
        code == 0
    }
}

fn firewall_rule_exists() -> bool {
    std::process::Command::new("netsh")
        .args(["advfirewall", "firewall", "show", "rule", &format!("name={FIREWALL_RULE}")])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Sadece Özel ağ profilinde, sadece bu program için gelen bağlantı izni (tek UAC onayı).
pub fn ensure_firewall_rule(exe: &Path) -> bool {
    if firewall_rule_exists() {
        return true;
    }
    message(
        concat!(
            "CouchRemote kuruluyor.\n\n",
            "Telefonunun bu bilgisayara bağlanabilmesi için Windows güvenlik duvarına bir izin eklenecek ",
            "(sadece evdeki özel ağın için).\n\n",
            "Birazdan çıkacak pencerede \"Evet\"e tıkla.",
        ),
        false,
    );
    let params = format!(
        "advfirewall firewall add rule name=\"{FIREWALL_RULE}\" dir=in action=allow program=\"{}\" profile=private enable=yes",
        exe.display()
    );
    let ok = run_elevated("netsh", &params);
    log!("Güvenlik duvarı kuralı {}", if ok { "eklendi" } else { "eklenemedi (izin verilmedi)" });
    ok
}

fn remove_firewall_rule() {
    if firewall_rule_exists() {
        run_elevated("netsh", &format!("advfirewall firewall delete rule name=\"{FIREWALL_RULE}\""));
    }
}

// ------------------------------------------------------------------ ağ profili

/// Windows bu ağı "Genel" olarak işaretlediyse güvenlik duvarı kuralımız devreye girmez → telefon bağlanamaz.
pub fn network_is_public(lan_ip: &str) -> Option<bool> {
    let script = format!(
        "$i=(Get-NetIPAddress -IPAddress '{lan_ip}' -ErrorAction SilentlyContinue).InterfaceIndex; \
         if($i){{(Get-NetConnectionProfile -InterfaceIndex $i).NetworkCategory}}"
    );
    let out = std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    match text.as_str() {
        "Public" => Some(true),
        "Private" | "DomainAuthenticated" => Some(false),
        _ => None,
    }
}

/// Ağı "Özel" yapar (yönetici izni ister).
pub fn make_network_private(lan_ip: &str) -> bool {
    let script = format!(
        "$i=(Get-NetIPAddress -IPAddress '{lan_ip}').InterfaceIndex; Set-NetConnectionProfile -InterfaceIndex $i -NetworkCategory Private"
    );
    run_elevated("powershell", &format!("-NoProfile -NonInteractive -Command \"{script}\""))
}

// ------------------------------------------------------------------ kurulum / kaldırma

/// Programı %LOCALAPPDATA%\Programs\CouchRemote'a kopyalar, Windows başlangıcına ve "Uygulamalar" listesine ekler,
/// güvenlik duvarı kuralını oluşturur. Kurulu kopyanın yolunu döner.
pub fn install() -> Result<PathBuf> {
    let dir = install_dir();
    std::fs::create_dir_all(&dir)?;
    let target = installed_exe();
    std::fs::copy(current_exe(), &target).context("Program kopyalanamadı (eski sürüm hâlâ çalışıyor olabilir)")?;

    let (key, _) = RegKey::predef(HKEY_CURRENT_USER).create_subkey(UNINSTALL_KEY)?;
    let exe = target.display().to_string();
    key.set_value("DisplayName", &APP)?;
    key.set_value("DisplayVersion", &env!("CARGO_PKG_VERSION"))?;
    key.set_value("Publisher", &APP)?;
    key.set_value("DisplayIcon", &exe)?;
    key.set_value("InstallLocation", &dir.display().to_string())?;
    key.set_value("UninstallString", &format!("\"{exe}\" --uninstall"))?;
    key.set_value("NoModify", &1u32)?;
    key.set_value("NoRepair", &1u32)?;
    let size_kb = std::fs::metadata(&target).map(|m| (m.len() / 1024) as u32).unwrap_or(0);
    key.set_value("EstimatedSize", &size_kb)?;

    set_autostart(true)?;
    if let Err(e) = crate::notify::create_shortcut(&target) {
        log!("Başlat menüsü kısayolu oluşturulamadı: {e:#}");
    }
    ensure_firewall_rule(&target);
    log!("Kuruldu: {exe}");
    Ok(target)
}

/// "Uygulamalar" listesinden (`external`) veya tepsi menüsünden kaldırma. Kullanıcı vazgeçerse false.
pub fn uninstall(external: bool) -> bool {
    if !message("CouchRemote bu bilgisayardan kaldırılsın mı?", true) {
        return false;
    }
    // H1: Windows Ayarlar'dan kaldırılırken program hâlâ çalışıyor olabilir; dosyası kilitli kalıp
    // klasör silinemesin diye önce kapat
    if external && single_instance().is_none() {
        let _ = stop_running_instance();
    }
    crate::notify::remove_shortcut();
    let wipe_data = message(
        "Eşleşmiş telefonlar ve ayarlar da silinsin mi?\n\n(Hayır dersen daha sonra yeniden kurduğunda telefonun eşleşmiş kalır.)",
        true,
    );
    let _ = set_autostart(false);
    let _ = RegKey::predef(HKEY_CURRENT_USER).delete_subkey_all(UNINSTALL_KEY);
    remove_firewall_rule();
    if wipe_data {
        let _ = std::fs::remove_dir_all(crate::state::data_dir());
    }
    // Çalışan .exe kendini silemez: kısa bir gecikmeyle klasörü silecek ayrı bir komut başlat
    let dir = install_dir();
    if dir.exists() {
        let _ = std::process::Command::new("cmd")
            .args(["/C", &format!("timeout /t 2 /nobreak >nul & rmdir /s /q \"{}\"", dir.display())])
            .spawn();
    }
    message("CouchRemote kaldırıldı.\n\nTelefondaki simgeyi silebilir, Ayarlar → Genel → VPN ve Aygıt Yönetimi'nden profili kaldırabilirsin.", false);
    true
}
