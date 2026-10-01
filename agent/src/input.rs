//! Windows girdi enjeksiyonu. Ayrı, yüksek öncelikli bir thread'de çalışır; async çalışma zamanı beklemez.
//! İmleç MUTLAK konumla hareket ettirilir: göreli harekete Windows'un "işaretçi hassasiyetini artır"
//! hızlanması da eklenir ve bizim eğrimizle çakışır (çift hızlanma). Mutlak konumda bu olmaz.
use crate::sync::LockExt;
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use windows::Win32::Foundation::POINT;
use windows::Win32::System::Threading::{GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_HIGHEST};
use windows::Win32::UI::HiDpi::{SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyboardLayout, SendInput, VkKeyScanExW, INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBDINPUT,
    KEYBD_EVENT_FLAGS, KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, KEYEVENTF_UNICODE,
    MOUSEEVENTF_ABSOLUTE, MOUSEEVENTF_HWHEEL, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MIDDLEDOWN,
    MOUSEEVENTF_MIDDLEUP, MOUSEEVENTF_MOVE, MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP, MOUSEEVENTF_VIRTUALDESK,
    MOUSEEVENTF_WHEEL, MOUSEINPUT, MOUSE_EVENT_FLAGS, VIRTUAL_KEY,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowThreadProcessId, GetCursorPos, GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN,
};

pub mod vk {
    pub const MEDIA_NEXT: u16 = 0xB0;
    pub const MEDIA_PREV: u16 = 0xB1;
    pub const MEDIA_PLAY_PAUSE: u16 = 0xB3;
    pub const VOLUME_MUTE: u16 = 0xAD;
    pub const VOLUME_DOWN: u16 = 0xAE;
    pub const VOLUME_UP: u16 = 0xAF;
}

/// İmleç konumu, fiziksel piksel (test aracı için: `couch-agent --print-cursor`)
pub fn cursor_pos() -> (i32, i32) {
    let mut p = POINT::default();
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let _ = GetCursorPos(&mut p);
    }
    (p.x, p.y)
}

pub enum Cmd {
    /// Önce buton durumu uygulanır (sürükleme başlangıcı için), sonra hareket
    Move { buttons: u8, dx: f64, dy: f64 },
    Scroll { buttons: u8, v: f64, h: f64 },
    State { buttons: u8 },
    ReleaseAll,
    TapKey(u16),
    /// Unicode metin (PC'nin klavye düzeninden bağımsız); satır sonu → Enter
    Text(String),
    /// Adlandırılmış tuş + modifiye tuşlar (ör. "enter", ["ctrl"])
    Key { key: u16, mods: Mods },
    /// Karakter tabanlı kısayol (ör. Ctrl + 'c'): tuş, öndeki pencerenin klavye düzenine göre bulunur
    Combo { ch: char, mods: Mods },
}

#[derive(Clone, Copy, Default)]
pub struct Mods {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub win: bool,
}

impl Mods {
    pub fn from_names<'a>(names: impl IntoIterator<Item = &'a str>) -> Mods {
        let mut m = Mods::default();
        for n in names {
            match n {
                "ctrl" => m.ctrl = true,
                "alt" => m.alt = true,
                "shift" => m.shift = true,
                "win" => m.win = true,
                _ => {}
            }
        }
        m
    }
}

/// Telefondan gelen tuş adı → sanal tuş kodu
pub fn vk_from_name(name: &str) -> Option<u16> {
    Some(match name {
        "enter" => 0x0D,
        "backspace" => 0x08,
        "tab" => 0x09,
        "esc" => 0x1B,
        "space" => 0x20,
        "pageup" => 0x21,
        "pagedown" => 0x22,
        "end" => 0x23,
        "home" => 0x24,
        "left" => 0x25,
        "up" => 0x26,
        "right" => 0x27,
        "down" => 0x28,
        "printscreen" => 0x2C,
        "insert" => 0x2D,
        "delete" => 0x2E,
        "win" => 0x5B,
        "menu" => 0x5D,
        "volup" => vk::VOLUME_UP,
        "voldown" => vk::VOLUME_DOWN,
        "mute" => vk::VOLUME_MUTE,
        "playpause" => vk::MEDIA_PLAY_PAUSE,
        "next" => vk::MEDIA_NEXT,
        "prev" => vk::MEDIA_PREV,
        f if f.len() <= 3 && f.starts_with('f') => {
            let n: u16 = f[1..].parse().ok()?;
            if !(1..=12).contains(&n) {
                return None;
            }
            0x70 + n - 1
        }
        _ => return None,
    })
}

const VK_SHIFT: u16 = 0x10;
const VK_CONTROL: u16 = 0x11;
const VK_MENU: u16 = 0x12;
const VK_LWIN: u16 = 0x5B;
const VK_RETURN: u16 = 0x0D;

/// Ok tuşları, Home/End, PgUp/PgDn, Ins/Del "genişletilmiş" tuşlardır; bayrak olmadan bazı uygulamalar numpad sanar
fn is_extended(k: u16) -> bool {
    matches!(k, 0x21..=0x28 | 0x2D | 0x2E | 0x5B | 0x5D | 0xA3 | 0xA5)
}

fn key_input(k: u16, up: bool) -> INPUT {
    let mut flags = if up { KEYEVENTF_KEYUP } else { KEYBD_EVENT_FLAGS(0) };
    if is_extended(k) {
        flags |= KEYEVENTF_EXTENDEDKEY;
    }
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: VIRTUAL_KEY(k), wScan: 0, dwFlags: flags, time: 0, dwExtraInfo: 0 } },
    }
}

fn unicode_input(unit: u16, up: bool) -> INPUT {
    let mut flags = KEYEVENTF_UNICODE;
    if up {
        flags |= KEYEVENTF_KEYUP;
    }
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: VIRTUAL_KEY(0), wScan: unit, dwFlags: flags, time: 0, dwExtraInfo: 0 } },
    }
}

fn send_all(inputs: &[INPUT]) {
    if !inputs.is_empty() {
        unsafe {
            SendInput(inputs, std::mem::size_of::<INPUT>() as i32);
        }
    }
}

fn mod_keys(m: Mods) -> Vec<u16> {
    let mut v = Vec::new();
    if m.ctrl {
        v.push(VK_CONTROL);
    }
    if m.alt {
        v.push(VK_MENU);
    }
    if m.shift {
        v.push(VK_SHIFT);
    }
    if m.win {
        v.push(VK_LWIN);
    }
    v
}

/// Modifiye tuşlara bas → tuşa bas/bırak → modifiyeleri ters sırada bırak (tek SendInput çağrısı, araya girilemez)
fn press_with_mods(k: u16, mods: &[u16]) {
    let mut v: Vec<INPUT> = mods.iter().map(|&m| key_input(m, false)).collect();
    v.push(key_input(k, false));
    v.push(key_input(k, true));
    v.extend(mods.iter().rev().map(|&m| key_input(m, true)));
    send_all(&v);
}

fn send_text(text: &str) {
    let mut v = Vec::with_capacity(text.len() * 2);
    for ch in text.chars() {
        if ch == '\n' {
            v.push(key_input(VK_RETURN, false));
            v.push(key_input(VK_RETURN, true));
            continue;
        }
        let mut buf = [0u16; 2];
        for &unit in ch.encode_utf16(&mut buf).iter() {
            v.push(unicode_input(unit, false));
            v.push(unicode_input(unit, true));
        }
    }
    send_all(&v);
}

/// Karakteri, öndeki pencerenin klavye düzeninde üreten tuşa çevirir (Türkçe Q / F / İngilizce fark etmez)
fn send_combo(ch: char, mods: Mods) {
    let mut buf = [0u16; 2];
    let units = ch.encode_utf16(&mut buf);
    let scan = unsafe {
        let thread = GetWindowThreadProcessId(GetForegroundWindow(), None);
        VkKeyScanExW(units[0], GetKeyboardLayout(thread))
    };
    if scan == -1 {
        return; // bu düzende karşılığı yok
    }
    let vk = (scan & 0xFF) as u16;
    let shift_state = ((scan >> 8) & 0xFF) as u8;
    let mut m = mods;
    m.shift |= shift_state & 1 != 0;
    m.ctrl |= shift_state & 2 != 0;
    m.alt |= shift_state & 4 != 0;
    press_with_mods(vk, &mod_keys(m));
}

#[derive(Clone)]
pub struct Input {
    tx: Sender<Cmd>,
}

impl Input {
    /// `stats`: SendInput işleme süreleri buraya yazılır (PC tarafı gecikme payı)
    pub fn start(stats: Arc<Mutex<crate::stats::Stats>>) -> Input {
        let (tx, rx) = channel::<Cmd>();
        std::thread::Builder::new()
            .name("girdi".into())
            .spawn(move || {
                unsafe {
                    let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
                    let _ = SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_HIGHEST);
                }
                let mut m = Mouse::default();
                while let Ok(cmd) = rx.recv() {
                    let t0 = Instant::now();
                    let measured = matches!(cmd, Cmd::Move { .. } | Cmd::Scroll { .. });
                    match cmd {
                        Cmd::Move { buttons, dx, dy } => {
                            m.set_buttons(buttons);
                            m.move_by(dx, dy);
                        }
                        Cmd::Scroll { buttons, v, h } => {
                            m.set_buttons(buttons);
                            m.scroll(v, h);
                        }
                        Cmd::State { buttons } => {
                            m.set_buttons(buttons);
                            m.wheel_acc = 0.0;
                            m.hwheel_acc = 0.0;
                        }
                        Cmd::ReleaseAll => m.set_buttons(0),
                        Cmd::TapKey(k) => {
                            send_key(k, false);
                            send_key(k, true);
                        }
                        Cmd::Text(t) => send_text(&t),
                        Cmd::Key { key, mods } => press_with_mods(key, &mod_keys(mods)),
                        Cmd::Combo { ch, mods } => send_combo(ch, mods),
                    }
                    if measured {
                        stats.lk().add_proc(t0.elapsed().as_secs_f64() * 1000.0);
                    }
                }
            })
            .expect("girdi thread'i");
        Input { tx }
    }

    pub fn send(&self, cmd: Cmd) {
        let _ = self.tx.send(cmd);
    }

    pub fn release_all(&self) {
        self.send(Cmd::ReleaseAll);
    }
}

fn send_mouse(flags: MOUSE_EVENT_FLAGS, dx: i32, dy: i32, data: i32) {
    let input = INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 { mi: MOUSEINPUT { dx, dy, mouseData: data as _, dwFlags: flags, time: 0, dwExtraInfo: 0 } },
    };
    unsafe {
        SendInput(&[input], std::mem::size_of::<INPUT>() as i32);
    }
}

fn send_key(key: u16, up: bool) {
    let input = INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(key),
                wScan: 0,
                dwFlags: if up { KEYEVENTF_KEYUP } else { KEYBD_EVENT_FLAGS(0) },
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    unsafe {
        SendInput(&[input], std::mem::size_of::<INPUT>() as i32);
    }
}

const BUTTONS: [(MOUSE_EVENT_FLAGS, MOUSE_EVENT_FLAGS); 3] = [
    (MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP),
    (MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP),
    (MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP),
];

#[derive(Default)]
struct Mouse {
    fx: f64, // küsuratlı konum: yavaş hareketler kaybolmasın
    fy: f64,
    /// Son iki gönderilen konum: SendInput asenkron olduğundan okuma bir öncekini yakalayabilir
    recent: [Option<(i32, i32)>; 2],
    buttons: u8,
    wheel_acc: f64,
    hwheel_acc: f64,
}

struct Desk {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
}

fn desktop() -> Desk {
    unsafe {
        Desk {
            x: GetSystemMetrics(SM_XVIRTUALSCREEN),
            y: GetSystemMetrics(SM_YVIRTUALSCREEN),
            w: GetSystemMetrics(SM_CXVIRTUALSCREEN).max(2),
            h: GetSystemMetrics(SM_CYVIRTUALSCREEN).max(2),
        }
    }
}

/// İmleci sanal masaüstündeki (ix, iy) pikseline taşır.
/// Normalize koordinat pikselin ORTASINI hedefler; kenarı hedeflemek yuvarlamada ±1 piksel kaydırır.
fn move_abs(ix: i32, iy: i32, d: &Desk) {
    let nx = ((2 * (ix - d.x) as i64 + 1) * 65536 / (2 * d.w as i64)) as i32;
    let ny = ((2 * (iy - d.y) as i64 + 1) * 65536 / (2 * d.h as i64)) as i32;
    send_mouse(MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK, nx, ny, 0);
}

/// Koordinat dönüşümü öz testi (`couch-agent --selftest-cursor`): her hedefe gidip anında geri okur.
/// Dışarıdan fare hareketinin etkisini azaltmak için her ölçüm birkaç milisaniye sürer.
pub fn selftest() -> Vec<((i32, i32), (i32, i32))> {
    let start = cursor_pos();
    let d = desktop();
    let mut out = Vec::new();
    let xs = [d.x, d.x + 1, d.x + d.w / 3, d.x + d.w / 2, d.x + d.w - 2, d.x + d.w - 1];
    let ys = [d.y, d.y + 1, d.y + d.h / 3, d.y + d.h / 2, d.y + d.h - 2, d.y + d.h - 1];
    for &x in &xs {
        for &y in &ys {
            // Iskalanan nokta bir kez daha denenir (test sırasında gerçek fare kıpırdamış olabilir)
            let mut got = (i32::MIN, i32::MIN);
            for _attempt in 0..2 {
                move_abs(x, y, &d);
                // SendInput asenkron: imleç hedefe ulaşana kadar kısa süre bekle (en fazla 50 ms)
                got = cursor_pos();
                for _ in 0..25 {
                    if got == (x, y) {
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(2));
                    got = cursor_pos();
                }
                if got == (x, y) {
                    break;
                }
            }
            out.push(((x, y), got));
        }
    }
    move_abs(start.0, start.1, &d);
    out
}

impl Mouse {
    fn move_by(&mut self, dx: f64, dy: f64) {
        let mut p = POINT::default();
        unsafe {
            let _ = GetCursorPos(&mut p);
        }
        // Gerçek fare kullanıldıysa veya imleç kenara takıldıysa gerçek konuma senkronize ol.
        // Okunan konum son iki gönderimimizden birine uyuyorsa dış hareket yoktur (henüz uygulanmamış olabilir).
        let ours = self.recent.iter().flatten().any(|(lx, ly)| (p.x - lx).abs() <= 1 && (p.y - ly).abs() <= 1);
        if !ours {
            self.fx = p.x as f64;
            self.fy = p.y as f64;
        }
        let d = desktop();
        self.fx = (self.fx + dx).clamp(d.x as f64, (d.x + d.w - 1) as f64);
        self.fy = (self.fy + dy).clamp(d.y as f64, (d.y + d.h - 1) as f64);
        let (ix, iy) = (self.fx.round() as i32, self.fy.round() as i32);
        if Some((ix, iy)) == self.recent[0] {
            return;
        }
        self.recent = [Some((ix, iy)), self.recent[0]];
        if ix == p.x && iy == p.y {
            return;
        }
        move_abs(ix, iy, &d);
    }

    /// Durum tabanlı: basılı butonların bit maskesi gelir, sadece fark uygulanır.
    fn set_buttons(&mut self, mask: u8) {
        let changed = mask ^ self.buttons;
        if changed == 0 {
            return;
        }
        for (bit, (down, up)) in BUTTONS.iter().enumerate() {
            if changed & (1 << bit) != 0 {
                send_mouse(if mask & (1 << bit) != 0 { *down } else { *up }, 0, 0, 0);
            }
        }
        self.buttons = mask;
    }

    /// Yüksek çözünürlüklü tekerlek (120 = bir çentik); küsuratlar biriktirilir.
    fn scroll(&mut self, v: f64, h: f64) {
        self.wheel_acc += v;
        self.hwheel_acc += h;
        let w = self.wheel_acc.trunc();
        let hw = self.hwheel_acc.trunc();
        if w != 0.0 {
            send_mouse(MOUSEEVENTF_WHEEL, 0, 0, w as i32);
            self.wheel_acc -= w;
        }
        if hw != 0.0 {
            send_mouse(MOUSEEVENTF_HWHEEL, 0, 0, hw as i32);
            self.hwheel_acc -= hw;
        }
    }
}
