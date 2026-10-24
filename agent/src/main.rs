//! CouchRemote PC programı.
//!   :47800  HTTP   kurulum sayfası + sertifika profili
//!   :47801  HTTPS  web uygulaması, eşleşme, WSS girdi kanalı, Kestirmeler API'si
//!   :47802  HTTP   yönetim sayfası (sadece 127.0.0.1)
//!
//! Komut satırı: --background (Windows açılışında, sayfa açmadan), --uninstall,
//!               --print-cursor / --selftest-cursor (test araçları)
// Sürüm derlemesinde konsol penceresi açılmaz (günlük dosyaya yazılır)
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#![allow(non_snake_case)] // HTTP_PORT gibi yerel adlar biçim dizelerinde yakalanır

#[macro_export]
macro_rules! log {
    ($($t:tt)*) => {
        $crate::logging::line(format!($($t)*))
    };
}

mod app;
mod audio;
mod backup;
mod clipboard;
mod context;
mod events;
mod assets;
mod certs;
mod dpapi;
mod i18n;
mod icons;
mod input;
mod launcher;
mod logging;
mod maintenance;
mod mdns;
mod net;
mod notify;
mod media;
mod pairing;
mod screenshot;
mod power;
mod rtc;
mod state;
mod stats;
mod sync;
mod system;
mod tls;
mod tray;
mod tv;
mod upload;
mod web;
mod winlist;
mod ws;

use crate::sync::LockExt;
use std::sync::Arc;

/// Portlar: 47800 (kurulum, HTTP) / 47801 (uygulama, HTTPS) / 47802 (yönetim, yalnızca bu bilgisayar).
/// COUCH_PORT_BASE ile kaydırılabilir: testler, çalışan gerçek programı ve eşleşmeleri bozmadan ayrı bir örnekle yapılır.
fn port_base() -> u16 {
    static B: std::sync::OnceLock<u16> = std::sync::OnceLock::new();
    *B.get_or_init(|| std::env::var("COUCH_PORT_BASE").ok().and_then(|v| v.parse::<u16>().ok()).filter(|b| *b >= 1024 && *b < 65000).unwrap_or(47800))
}
pub fn http_port() -> u16 {
    port_base()
}
pub fn https_port() -> u16 {
    port_base() + 1
}
pub fn admin_port() -> u16 {
    port_base() + 2
}

/// Çalışan örneğin yönetim sayfası adresi (anahtar dosyadan okunur)
fn running_admin_url() -> String {
    let token = std::fs::read_to_string(state::data_dir().join("admin.token")).unwrap_or_default();
    format!("http://127.0.0.1:{}/?t={}", admin_port(), token.trim())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Konsolsuz sürümde bir thread paniklerse iz kalmazdı: günlüğe yaz
    std::panic::set_hook(Box::new(|info| {
        let thread = std::thread::current();
        log!("PANİK ({}): {info}", thread.name().unwrap_or("?"));
    }));
    let has = |flag: &str| std::env::args().any(|a| a == flag);

    if has("--print-cursor") {
        let (x, y) = input::cursor_pos();
        println!("{x},{y}");
        return Ok(());
    }
    if has("--print-audio") {
        audio::print_devices();
        return Ok(());
    }
    if has("--print-screenshot") {
        // test aracı: ekranı yakalayıp ilk yoldaki dosyaya yazar (varsayılan: ekran.jpg)
        let path = std::env::args().skip_while(|a| a != "--print-screenshot").nth(1).unwrap_or_else(|| "ekran.jpg".into());
        println!("{} monitör", screenshot::monitors().len());
        match screenshot::capture(None) {
            Some(s) => {
                std::fs::write(&path, &s.jpeg)?;
                println!("{}x{} (kaynak {}x{}), {} KB → {path}", s.width, s.height, s.source.0, s.source.1, s.jpeg.len() / 1024);
            }
            None => println!("yakalanamadı"),
        }
        return Ok(());
    }
    if has("--selftest-cursor") {
        let results = input::selftest();
        let bad: Vec<_> = results.iter().filter(|(want, got)| want != got).collect();
        for (want, got) in &bad {
            println!("✗ hedef {want:?} → {got:?}");
        }
        println!("{} / {} nokta doğru", results.len() - bad.len(), results.len());
        return Ok(());
    }
    if has("--uninstall") {
        let _ = system::uninstall(true);
        return Ok(());
    }
    // B2: sürüm derlemesinde konsol yok; başlangıç hatası sessizce kaybolmasın
    if let Err(e) = run(has("--background")).await {
        log!("Başlatılamadı: {e:#}");
        system::message(&friendly_error(&e), false);
    }
    Ok(())
}

/// Başlangıç hatasını kullanıcının anlayacağı dile çevirir
fn friendly_error(e: &anyhow::Error) -> String {
    let (HTTP_PORT, ADMIN_PORT) = (crate::http_port(), crate::admin_port());
    let in_use = e.chain().any(|c| c.downcast_ref::<std::io::Error>().is_some_and(|io| io.kind() == std::io::ErrorKind::AddrInUse));
    if in_use {
        if i18n::is_tr() {
            format!(
                "CouchRemote başlatılamadı: {HTTP_PORT}–{ADMIN_PORT} numaralı ağ portlarından biri başka bir program tarafından kullanılıyor.\n\n\
                 Bilgisayarı yeniden başlatmayı dene. Sorun sürerse bu portları kullanan programı kapat."
            )
        } else {
            format!(
                "CouchRemote couldn't start: one of the network ports {HTTP_PORT}–{ADMIN_PORT} is being used by another program.\n\n\
                 Try restarting your computer. If the problem continues, close the program that uses these ports."
            )
        }
    } else if i18n::is_tr() {
        format!("CouchRemote başlatılamadı:\n\n{e:#}\n\nAyrıntılar: {}", state::data_dir().join("couch.log").display())
    } else {
        format!("CouchRemote couldn't start:\n\n{e:#}\n\nDetails: {}", state::data_dir().join("couch.log").display())
    }
}

async fn run(background: bool) -> anyhow::Result<()> {
    let HTTPS_PORT = crate::https_port();
    const VERSION: &str = env!("CARGO_PKG_VERSION");
    // Türkçe karakterler konsolda doğru görünsün (geliştirme derlemesi)
    unsafe {
        let _ = windows::Win32::System::Console::SetConsoleOutputCP(65001);
    }

    let mut instance = system::single_instance();
    let installing = system::should_self_install();

    if instance.is_none() {
        // B1: indirilen yeni sürüm çalıştırıldı ama eski sürüm çalışıyor → güncelle
        let running = if installing { system::running_version() } else { None };
        match running {
            Some(old) if old != VERSION => {
                if !system::message(
                    &if i18n::is_tr() {
                        format!("CouchRemote {old} çalışıyor.\n\n{VERSION} sürümüne güncellensin mi?")
                    } else {
                        format!("CouchRemote {old} is running.\n\nUpdate to version {VERSION}?")
                    },
                    true,
                ) {
                    return Ok(());
                }
                instance = system::stop_running_instance();
                if instance.is_none() {
                    anyhow::bail!("Eski sürüm kapatılamadı. Saatin yanındaki simgeden Çıkış'ı seçip tekrar dene.");
                }
                log!("{old} → {VERSION} güncelleniyor");
            }
            // Aynı sürüm zaten çalışıyor: yeni örnek başlatma, "Telefonu bağla" sayfasını öne getir
            _ => {
                if !background {
                    system::open_admin(&running_admin_url());
                }
                return Ok(());
            }
        }
    }
    let _instance = instance;

    // İndirilen dosya çalıştırıldı: kendini kur (veya güncelle) ve kurulu kopyayı başlat
    if installing {
        match system::install() {
            Ok(installed) => {
                drop(_instance);
                let _ = std::process::Command::new(installed).spawn();
                return Ok(());
            }
            Err(e) => {
                system::message(
                    &if i18n::is_tr() {
                        format!("CouchRemote kurulamadı:\n{e:#}\n\nBu konumdan çalışmaya devam edecek.")
                    } else {
                        format!("CouchRemote couldn't be installed:\n{e:#}\n\nIt will keep running from this location.")
                    },
                    false,
                );
            }
        }
    }

    notify::set_process_id();
    let app = Arc::new(app::App::init()?);
    // Kısayolu olmayan eski kurulumlar (0.2.0 öncesi) için Başlat menüsü kısayolunu tamamla
    if system::is_installed_copy() && !notify::shortcut_path().exists() {
        let _ = notify::create_shortcut(&system::installed_exe());
    }
    let _mdns = mdns::start(&app.host, &app.lan_ip)?;

    let setup = tokio::net::TcpListener::bind(("0.0.0.0", http_port())).await?;
    let https = tokio::net::TcpListener::bind(("0.0.0.0", https_port())).await?;
    let admin = tokio::net::TcpListener::bind(("127.0.0.1", admin_port())).await?;

    tokio::spawn(net::serve(setup, None, web::setup_router(app.clone())));
    tokio::spawn(net::serve(https, Some(app.tls.clone()), web::app_router(app.clone())));
    tokio::spawn(net::serve(admin, None, web::admin_router(app.clone())));
    tokio::spawn(maintenance::run(app.clone()));
    tray::start(app.clone());

    // Ağ profili "Genel" ise telefon bağlanamaz: arka planda kontrol et, yönetim sayfasında uyar
    {
        let app = app.clone();
        tokio::task::spawn_blocking(move || {
            let public = system::network_is_public(&app.lan_ip);
            if public == Some(true) {
                log!("Uyarı: Windows bu ağı 'Genel' olarak işaretlemiş — telefon bağlanamayabilir");
            }
            *app.network_public.lk() = public;
        });
    }

    log!("CouchRemote {} çalışıyor — {} → {} (veri: {})", env!("CARGO_PKG_VERSION"), app.host, app.lan_ip, state::data_dir().display());
    println!(
        "\n  Uygulama : https://{host}:{HTTPS_PORT}/app\n  Yönetim  : {admin}   ← QR kod ve PIN burada\n",
        host = app.host,
        admin = app.admin_url()
    );

    // İlk açılış (henüz eşleşmiş telefon yok) veya kullanıcı elle açtıysa "Telefonu bağla" sayfasını aç
    let first_run = app.state.lk().devices.is_empty();
    if std::env::var_os("COUCH_NO_BROWSER").is_none() && (!background || first_run) {
        system::open_admin(&app.admin_url());
    }

    // Konsolsuz sürümde Ctrl+C gelmez; dinleme kurulamazsa program açık kalmaya devam etsin
    if tokio::signal::ctrl_c().await.is_err() {
        std::future::pending::<()>().await;
    }
    app.input.release_all();
    log!("Kapatılıyor");
    Ok(())
}
