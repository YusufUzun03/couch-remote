//! Öndeki uygulamayı algılar (Netflix, YouTube, Spotify…) ve telefona sadece profil kimliğini bildirir;
//! telefon o uygulamaya özel düğmeleri (İntroyu atla, hız, altyazı…) gösterir.
//! Gizlilik: pencere başlığı telefona gönderilmez, sadece hangi profilin geçerli olduğu.
//!
//! Ayrıca kumanda tuşlarının DOĞRU pencereye gitmesini sağlar: son görülen medya penceresi hatırlanır;
//! kullanıcı arada başka bir pencereye geçtiyse tuş göndermeden önce o pencere öne getirilir.
use crate::sync::LockExt;
use serde_json::json;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use windows::core::{BOOL, PWSTR};
use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM, RECT};
use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST};
use windows::Win32::System::Threading::{
    AttachThreadInput, GetCurrentThreadId, OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::WindowsAndMessaging::{
    BringWindowToTop, EnumWindows, GetForegroundWindow, GetWindowRect, GetWindowTextW, GetWindowThreadProcessId, IsIconic, IsWindow,
    IsWindowVisible, SetForegroundWindow, ShowWindow, SW_RESTORE,
};

use crate::events::Events;

/// Son görülen medya penceresi (tutamaç, profil)
static MEDIA_WINDOW: Mutex<Option<(isize, &'static str)>> = Mutex::new(None);

fn title_of(hwnd: HWND) -> String {
    let mut buf = [0u16; 512];
    let n = unsafe { GetWindowTextW(hwnd, &mut buf) };
    String::from_utf16_lossy(&buf[..n.max(0) as usize])
}

fn exe_of(hwnd: HWND) -> String {
    unsafe {
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else { return String::new() };
        let mut path = [0u16; 520];
        let mut len = path.len() as u32;
        let ok = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(path.as_mut_ptr()), &mut len).is_ok();
        let _ = CloseHandle(h);
        if !ok {
            return String::new();
        }
        String::from_utf16_lossy(&path[..len as usize]).rsplit('\\').next().unwrap_or("").to_lowercase()
    }
}

/// Başlık ve program adından profil
pub fn profile(title: &str, exe: &str) -> &'static str {
    match exe {
        "spotify.exe" => return "spotify",
        "vlc.exe" => return "vlc",
        _ => {}
    }
    let t = title.to_lowercase();
    // Tarayıcı sekmeleri: başlık "… - Netflix - Google Chrome" gibi
    for (needle, id) in [
        ("netflix", "netflix"), ("youtube music", "ytmusic"), ("youtube", "youtube"), ("prime video", "primevideo"),
        ("disney+", "disneyplus"), ("twitch", "twitch"), ("tabii", "tabii"), ("exxen", "exxen"), ("spotify", "spotify"),
    ] {
        if t.contains(needle) {
            return id;
        }
    }
    "none"
}

fn window_profile(hwnd: HWND) -> &'static str {
    profile(&title_of(hwnd), &exe_of(hwnd))
}

pub fn start(events: Arc<Events>) {
    std::thread::Builder::new()
        .name("ondeki-uygulama".into())
        .spawn(move || {
            let mut last = ("", false);
            loop {
                let hwnd = unsafe { GetForegroundWindow() };
                let p = if hwnd.is_invalid() { "none" } else { window_profile(hwnd) };
                if p != "none" {
                    *MEDIA_WINDOW.lk() = Some((hwnd.0 as isize, p));
                }
                // Tam ekran durumu telefondaki "Tam ekran" düğmesinin etiketini belirler
                let fullscreen = p != "none" && foreground_fullscreen();
                if (p, fullscreen) != last {
                    last = (p, fullscreen);
                    events.publish("context", json!({ "profile": p, "fullscreen": fullscreen }));
                }
                std::thread::sleep(Duration::from_millis(700));
            }
        })
        .expect("öndeki uygulama thread'i");
}

/// Pencereyi öne getirir. Windows, arka plandaki bir programın odağı çalmasını engeller;
/// öndeki pencerenin girdi kuyruğuna geçici olarak bağlanmak bu kısıtı meşru şekilde aşar.
pub fn bring_to_front(hwnd: HWND) {
    unsafe {
        if IsIconic(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        }
        let fg = GetForegroundWindow();
        let fg_thread = GetWindowThreadProcessId(fg, None);
        let me = GetCurrentThreadId();
        let attached = fg_thread != 0 && fg_thread != me && AttachThreadInput(me, fg_thread, true).as_bool();
        let _ = SetForegroundWindow(hwnd);
        let _ = BringWindowToTop(hwnd);
        if attached {
            let _ = AttachThreadInput(me, fg_thread, false);
        }
    }
    std::thread::sleep(Duration::from_millis(80)); // odak değişimi tuşlardan önce otursun
}

/// Kumanda tuşundan önce: `want` profilindeki (veya herhangi bir) medya penceresini öne getir.
/// Öndeki pencere zaten uygunsa hiçbir şey yapmaz. Uygun pencere yoksa false.
pub fn focus_media(want: Option<&str>) -> bool {
    let fg = unsafe { GetForegroundWindow() };
    let fg_profile = if fg.is_invalid() { "none" } else { window_profile(fg) };
    if fg_profile != "none" && want.is_none_or(|w| w == fg_profile) {
        return true;
    }
    let stored = *MEDIA_WINDOW.lk();
    let Some((h, p)) = stored else { return false };
    if want.is_some_and(|w| w != p) {
        return false;
    }
    let hwnd = HWND(h as _);
    // Pencere kapanmış veya sekme değişmiş olabilir
    if !unsafe { IsWindow(Some(hwnd)) }.as_bool() || window_profile(hwnd) != p {
        return false;
    }
    bring_to_front(hwnd);
    true
}

/// Açık pencereler arasında bu profile ait görünür bir pencere (ör. zaten açık Netflix sekmesi)
pub fn find_window(want: &str) -> Option<HWND> {
    struct Search<'a> {
        want: &'a str,
        found: Option<HWND>,
    }
    unsafe extern "system" fn each(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let s = unsafe { &mut *(lparam.0 as *mut Search) };
        if unsafe { IsWindowVisible(hwnd) }.as_bool() && window_profile(hwnd) == s.want {
            s.found = Some(hwnd);
            return BOOL(0); // dur
        }
        BOOL(1)
    }
    let mut s = Search { want, found: None };
    unsafe {
        let _ = EnumWindows(Some(each), LPARAM(&mut s as *mut Search as isize));
    }
    s.found
}

pub fn foreground_profile() -> &'static str {
    let fg = unsafe { GetForegroundWindow() };
    if fg.is_invalid() { "none" } else { window_profile(fg) }
}

/// Öndeki pencere bulunduğu ekranı tamamen kaplıyor mu (tam ekran)?
pub fn foreground_fullscreen() -> bool {
    unsafe {
        let fg = GetForegroundWindow();
        let mut r = RECT::default();
        if fg.is_invalid() || GetWindowRect(fg, &mut r).is_err() {
            return false;
        }
        let mon = MonitorFromWindow(fg, MONITOR_DEFAULTTONEAREST);
        let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        if !GetMonitorInfoW(mon, &mut mi).as_bool() {
            return false;
        }
        let m = mi.rcMonitor;
        r.left <= m.left && r.top <= m.top && r.right >= m.right && r.bottom >= m.bottom
    }
}
