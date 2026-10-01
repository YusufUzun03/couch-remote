//! Güç menüsü (kilitle, ekranı kapat, uyut, yeniden başlat, kapat) ve uyku zamanlayıcısı.
//! Zamanlayıcı son 60 saniyede sesi yavaşça kısar, medyayı durdurur, ses seviyesini eski haline getirir
//! (bir sonraki açılışta sessiz kalmasın) ve seçilen işlemi yapar.
use serde_json::{json, Value};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use windows::core::w;
use windows::Win32::Foundation::{CloseHandle, HANDLE, LPARAM, LUID, WPARAM};
use windows::Win32::Security::{
    AdjustTokenPrivileges, LookupPrivilegeValueW, LUID_AND_ATTRIBUTES, SE_PRIVILEGE_ENABLED, TOKEN_ADJUST_PRIVILEGES,
    TOKEN_PRIVILEGES, TOKEN_QUERY,
};
use windows::Win32::System::Power::SetSuspendState;
use windows::Win32::System::Shutdown::LockWorkStation;
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, HWND_BROADCAST, SC_MONITORPOWER, WM_SYSCOMMAND};

use crate::app::App;
use crate::audio::AudioCmd;
use crate::media::MediaCmd;

const FADE_SECS: u64 = 60;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Action {
    /// Medyayı durdur ve ekranı kapat
    Stop,
    Sleep,
    Shutdown,
}

impl Action {
    pub fn parse(s: &str) -> Option<Action> {
        match s {
            "stop" => Some(Action::Stop),
            "sleep" => Some(Action::Sleep),
            "shutdown" => Some(Action::Shutdown),
            _ => None,
        }
    }
    fn id(self) -> &'static str {
        match self {
            Action::Stop => "stop",
            Action::Sleep => "sleep",
            Action::Shutdown => "shutdown",
        }
    }
}

// ------------------------------------------------------------------ anlık işlemler

pub fn lock() {
    unsafe {
        let _ = LockWorkStation();
    }
}

/// Ekranı kapatır; fare/klavye hareketi (touchpad dahil) geri açar
pub fn screen_off() {
    unsafe {
        let _ = PostMessageW(Some(HWND_BROADCAST), WM_SYSCOMMAND, WPARAM(SC_MONITORPOWER as usize), LPARAM(2));
    }
}

/// Uyku için SE_SHUTDOWN ayrıcalığı işlem jetonunda etkinleştirilmelidir
fn enable_shutdown_privilege() -> windows::core::Result<()> {
    unsafe {
        let mut token = HANDLE::default();
        OpenProcessToken(GetCurrentProcess(), TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY, &mut token)?;
        let mut luid = LUID::default();
        let r = LookupPrivilegeValueW(None, w!("SeShutdownPrivilege"), &mut luid).and_then(|_| {
            let tp = TOKEN_PRIVILEGES { PrivilegeCount: 1, Privileges: [LUID_AND_ATTRIBUTES { Luid: luid, Attributes: SE_PRIVILEGE_ENABLED }] };
            AdjustTokenPrivileges(token, false, Some(&tp), 0, None, None)
        });
        let _ = CloseHandle(token);
        r
    }
}

pub fn sleep() {
    let _ = enable_shutdown_privilege();
    // Hazırda bekletme değil uyku (hibernate=false); uyandırma olaylarına izin ver
    let ok = unsafe { SetSuspendState(false, false, false) };
    log!("Uyku {}", if ok { "başlatıldı" } else { "başarısız" });
}

pub fn shutdown(restart: bool) {
    log!("{}", if restart { "Yeniden başlatılıyor" } else { "Kapatılıyor" });
    let _ = std::process::Command::new("shutdown").args([if restart { "/r" } else { "/s" }, "/t", "0"]).spawn();
}

// ------------------------------------------------------------------ uyku zamanlayıcısı

pub struct Timer {
    /// Her yeni kurulum/iptal nesli artırır; eski görevler kendiliğinden biter
    generation: AtomicU64,
}

impl Timer {
    pub fn new() -> Timer {
        Timer { generation: AtomicU64::new(0) }
    }
}

fn publish(app: &App, v: Value) {
    app.events.publish("timer", v);
}

pub fn cancel_timer(app: &App) {
    app.timer.generation.fetch_add(1, Ordering::SeqCst);
    publish(app, json!({ "active": false }));
    log!("Uyku zamanlayıcısı iptal edildi");
}

pub fn start_timer(app: Arc<App>, minutes: u64, action: Action) {
    let my_gen = app.timer.generation.fetch_add(1, Ordering::SeqCst) + 1;
    let total = Duration::from_secs(minutes * 60);
    let end = Instant::now() + total;
    log!("Uyku zamanlayıcısı: {minutes} dk sonra {action:?}");
    tokio::spawn(async move {
        let mut original: Option<(f32, bool)> = None;
        let mut last_publish = Instant::now() - Duration::from_secs(60);
        loop {
            if app.timer.generation.load(Ordering::SeqCst) != my_gen {
                // İptal edildi / yenisi kuruldu: kısılan sesi geri getir
                if let Some((l, _)) = original {
                    app.audio.send(AudioCmd::SetLevel(l));
                }
                return;
            }
            let remaining = end.saturating_duration_since(Instant::now());
            if last_publish.elapsed() >= Duration::from_secs(5) {
                publish(&app, json!({ "active": true, "remaining": remaining.as_secs(), "action": action.id() }));
                last_publish = Instant::now();
            }
            // Son dakikada sesi yavaşça kıs
            if remaining.as_secs() <= FADE_SECS {
                if original.is_none() {
                    original = tokio::task::block_in_place(|| app.audio.get());
                }
                if let Some((l, _)) = original {
                    let f = remaining.as_secs_f32() / FADE_SECS as f32;
                    app.audio.send(AudioCmd::SetLevel(l * f));
                }
            }
            if remaining.is_zero() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        // Süre doldu: medyayı durdur, sesi eski haline getir, işlemi yap
        let playing = app.events.latest("media").is_some_and(|m| m["playing"] == true);
        if playing {
            app.media.send(MediaCmd::PlayPause);
        }
        tokio::time::sleep(Duration::from_millis(800)).await;
        if let Some((l, _)) = original {
            app.audio.send(AudioCmd::SetLevel(l));
        }
        publish(&app, json!({ "active": false, "fired": action.id() }));
        tokio::time::sleep(Duration::from_millis(500)).await;
        match action {
            Action::Stop => screen_off(),
            Action::Sleep => tokio::task::block_in_place(sleep),
            Action::Shutdown => shutdown(false),
        }
    });
}
