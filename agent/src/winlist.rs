//! Telefondaki "Açık pencereler": görünür üst düzey pencerelerin listesi ve eylemleri (öne getir, küçült, kapat).
//! Pencere kimliğini telefon gönderir ama her eylemde güncel listede aranır; listede olmayan kimlik hiçbir şey yapmaz.
//! Pencere başlıkları yalnızca eşleşmiş telefona gider ve günlüğe yazılmaz.
use serde_json::{json, Value};
use windows::core::BOOL;
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED};
use windows::Win32::System::Threading::GetCurrentProcessId;
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetClassNameW, GetForegroundWindow, GetWindow, GetWindowLongW, GetWindowThreadProcessId, IsIconic, IsWindowVisible,
    PostMessageW, ShowWindow, GWL_EXSTYLE, GW_OWNER, SW_MINIMIZE, WM_CLOSE, WS_EX_TOOLWINDOW,
};

const MAX_WINDOWS: usize = 60;

pub struct Win {
    pub id: isize,
    pub app: String,
    pub title: String,
    pub active: bool,
    pub minimized: bool,
}

fn class_of(hwnd: HWND) -> String {
    let mut buf = [0u16; 128];
    let n = unsafe { GetClassNameW(hwnd, &mut buf) };
    String::from_utf16_lossy(&buf[..n.max(0) as usize])
}

/// Kullanıcının "pencere" dediği şey mi? (görünür, sahipsiz, araç penceresi değil, gizlenmiş/UWP-askıda değil, başlığı var)
fn eligible(hwnd: HWND, own_pid: u32) -> Option<(String, String)> {
    unsafe {
        if !IsWindowVisible(hwnd).as_bool() {
            return None;
        }
        if GetWindow(hwnd, GW_OWNER).is_ok_and(|o| !o.is_invalid()) {
            return None;
        }
        if GetWindowLongW(hwnd, GWL_EXSTYLE) as u32 & WS_EX_TOOLWINDOW.0 != 0 {
            return None;
        }
        let mut cloaked = 0u32;
        if DwmGetWindowAttribute(hwnd, DWMWA_CLOAKED, &mut cloaked as *mut u32 as *mut _, 4).is_ok() && cloaked != 0 {
            return None;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == own_pid {
            return None;
        }
    }
    let title = crate::context::title_of(hwnd);
    if title.trim().is_empty() {
        return None;
    }
    if matches!(class_of(hwnd).as_str(), "Progman" | "WorkerW" | "Shell_TrayWnd" | "Shell_SecondaryTrayWnd") {
        return None;
    }
    let exe = crate::context::exe_of(hwnd);
    let stem = exe.strip_suffix(".exe").unwrap_or(&exe);
    let app = if stem.is_empty() { "Windows".to_string() } else { crate::audio::pretty_name(stem) };
    Some((app, title))
}

fn handles() -> Vec<HWND> {
    unsafe extern "system" fn each(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let v = unsafe { &mut *(lparam.0 as *mut Vec<HWND>) };
        v.push(hwnd);
        BOOL(1)
    }
    let mut v: Vec<HWND> = Vec::new();
    unsafe {
        let _ = EnumWindows(Some(each), LPARAM(&mut v as *mut Vec<HWND> as isize));
    }
    v
}

/// Üstten alta (Z sırası) açık pencereler
pub fn list() -> Vec<Win> {
    let own = unsafe { GetCurrentProcessId() };
    let fg = unsafe { GetForegroundWindow() };
    handles()
        .into_iter()
        .filter_map(|h| {
            let (app, title) = eligible(h, own)?;
            Some(Win { id: h.0 as isize, app, title: title.chars().take(160).collect(), active: h == fg, minimized: unsafe { IsIconic(h) }.as_bool() })
        })
        .take(MAX_WINDOWS)
        .collect()
}

pub fn to_json(list: &[Win]) -> Value {
    json!({ "type": "windows", "windows": list.iter().map(|w| json!({ "id": w.id, "app": w.app, "title": w.title, "active": w.active, "minimized": w.minimized })).collect::<Vec<_>>() })
}

/// focus | minimize | close. Kimlik güncel listede yoksa false
pub fn act(id: isize, action: &str) -> bool {
    let own = unsafe { GetCurrentProcessId() };
    let Some(hwnd) = handles().into_iter().find(|h| h.0 as isize == id && eligible(*h, own).is_some()) else { return false };
    unsafe {
        match action {
            "focus" => crate::context::bring_to_front(hwnd),
            "minimize" => {
                let _ = ShowWindow(hwnd, SW_MINIMIZE);
            }
            "close" => {
                // Uygulamaya nazikçe "kapan" denir; kaydedilmemiş iş varsa uygulama kendi sorusunu sorar
                let _ = PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
            }
            _ => return false,
        }
    }
    true
}
