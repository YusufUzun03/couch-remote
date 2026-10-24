//! Saatin yanındaki simge ve menüsü. Kendi Windows mesaj döngüsüyle ayrı bir thread'de çalışır.
use std::sync::Arc;
use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, MouseButton, TrayIconBuilder, TrayIconEvent};
use windows::Win32::UI::WindowsAndMessaging::{DispatchMessageW, GetMessageW, TranslateMessage, MSG};

use crate::app::App;
use crate::system;

pub fn start(app: Arc<App>) {
    std::thread::Builder::new()
        .name("tepsi".into())
        .spawn(move || {
            let connect = MenuItem::new(crate::i18n::pick("Telefonu bağla…", "Connect a phone…"), true, None);
            let autostart = CheckMenuItem::new(crate::i18n::pick("Windows ile başlat", "Start with Windows"), true, system::autostart_enabled(), None);
            let revoke_all = MenuItem::new(crate::i18n::pick("Tüm telefonların eşleşmesini kaldır…", "Remove all phone pairings…"), true, None);
            let uninstall = MenuItem::new(crate::i18n::pick("Kaldır…", "Uninstall…"), system::is_installed_copy(), None);
            let quit = MenuItem::new(crate::i18n::pick("Çıkış", "Quit"), true, None);
            let menu = Menu::new();
            let _ = menu.append(&connect);
            let _ = menu.append(&autostart);
            let _ = menu.append(&PredefinedMenuItem::separator());
            let _ = menu.append(&revoke_all);
            let _ = menu.append(&uninstall);
            let _ = menu.append(&quit);

            let icon = Icon::from_rgba(crate::icons::rgba(32, true), 32, 32).expect("simge");
            let _tray = match TrayIconBuilder::new()
                .with_tooltip(format!("CouchRemote — {}", app.host))
                .with_icon(icon)
                .with_menu(Box::new(menu))
                .with_menu_on_left_click(false)
                .build()
            {
                Ok(t) => t,
                Err(e) => {
                    crate::log!("Tepsi simgesi oluşturulamadı: {e}");
                    return;
                }
            };

            // Sol tık: yönetim sayfasını aç
            let url_click = app.admin_url();
            TrayIconEvent::set_event_handler(Some(move |e: TrayIconEvent| {
                if let TrayIconEvent::Click { button: MouseButton::Left, button_state: tray_icon::MouseButtonState::Up, .. } = e {
                    system::open_admin(&url_click);
                }
            }));

            let (connect_id, autostart_id, uninstall_id, quit_id, revoke_id) =
                (connect.id().clone(), autostart.id().clone(), uninstall.id().clone(), quit.id().clone(), revoke_all.id().clone());
            let app2 = app.clone();
            let url_menu = app.admin_url();
            MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
                if e.id == connect_id {
                    system::open_admin(&url_menu);
                } else if e.id == autostart_id {
                    // Menü öğesi thread'ler arası paylaşılamıyor: yeni durum kayıt defterinden hesaplanır
                    let on = !system::autostart_enabled();
                    if let Err(err) = system::set_autostart(on) {
                        crate::log!("Otomatik başlatma değiştirilemedi: {err}");
                    }
                } else if e.id == revoke_id {
                    // Telefon kaybolduysa / çalındıysa: onay penceresinden sonra hepsi silinir
                    let app3 = app2.clone();
                    std::thread::spawn(move || {
                        if system::message(
                            crate::i18n::pick(
                                "Bu bilgisayarla eşleşmiş TÜM telefonların bağlantısı kesilecek ve eşleşmeleri silinecek. Siri kestirmeleri de çalışmaz olacak.\n\nTelefonlarını yeniden eşleştirmen gerekir. Devam edilsin mi?",
                                "ALL phones paired with this computer will be disconnected and their pairings deleted. Siri shortcuts will stop working too.\n\nYou'll need to pair your phones again. Continue?",
                            ),
                            true,
                        ) {
                            app3.revoke_all();
                            system::message(
                                crate::i18n::pick(
                                    "Tüm eşleşmeler kaldırıldı. Telefonu yeniden bağlamak için tepsi menüsünden \"Telefonu bağla\"yı seç.",
                                    "All pairings were removed. To connect a phone again, choose \"Connect a phone\" from the tray menu.",
                                ),
                                false,
                            );
                        }
                    });
                } else if e.id == uninstall_id {
                    std::thread::spawn(|| {
                        if system::uninstall(false) {
                            std::process::exit(0);
                        }
                    });
                } else if e.id == quit_id {
                    app2.input.release_all();
                    std::thread::sleep(std::time::Duration::from_millis(50));
                    std::process::exit(0);
                }
            }));

            // Windows mesaj döngüsü (tepsi simgesi ve menü olayları buradan gelir)
            let mut msg = MSG::default();
            unsafe {
                while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
        })
        .expect("tepsi thread'i");
}

