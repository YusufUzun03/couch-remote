//! Windows bildirimleri ve Başlat menüsü kısayolu.
//! Paketlenmemiş bir programın bildirim gösterebilmesi için Başlat menüsünde, uygulama kimliği (AUMID)
//! taşıyan bir kısayolu olmalıdır; kısayol aynı zamanda programa geri dönüş yoludur.
use anyhow::Result;
use std::path::{Path, PathBuf};
use windows::core::{Interface, HSTRING, PWSTR};
use windows::Data::Xml::Dom::XmlDocument;
use windows::Win32::Storage::EnhancedStorage::PKEY_AppUserModel_ID;
use windows::Win32::System::Com::StructuredStorage::PROPVARIANT;
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, IPersistFile, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, COINIT_MULTITHREADED,
};
use windows::Win32::System::Variant::VT_LPWSTR;
use windows::Win32::UI::Shell::PropertiesSystem::IPropertyStore;
use windows::Win32::UI::Shell::{IShellLinkW, SetCurrentProcessExplicitAppUserModelID, ShellLink};
use windows::UI::Notifications::{ToastNotification, ToastNotificationManager};

pub const AUMID: &str = "CouchRemote.Agent";

pub fn shortcut_path() -> PathBuf {
    let base = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    base.join(r"Microsoft\Windows\Start Menu\Programs\CouchRemote.lnk")
}

/// Bu işlemin bildirimleri Başlat menüsü kısayoluyla eşleşsin
pub fn set_process_id() {
    unsafe {
        let _ = SetCurrentProcessExplicitAppUserModelID(&HSTRING::from(AUMID));
    }
}

/// Başlat menüsüne "CouchRemote" kısayolu (uygulama kimliğiyle). COM kendi thread'inde başlatılır.
pub fn create_shortcut(exe: &Path) -> Result<()> {
    let exe = exe.to_path_buf();
    std::thread::spawn(move || -> Result<()> {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
            link.SetPath(&HSTRING::from(exe.as_os_str()))?;
            link.SetDescription(&HSTRING::from("iPhone'unla bilgisayarını kontrol et"))?;
            // Uygulama kimliği (bildirimler için)
            let store: IPropertyStore = link.cast()?;
            let mut wide: Vec<u16> = AUMID.encode_utf16().chain(std::iter::once(0)).collect();
            let mut pv = PROPVARIANT::default();
            (*pv.Anonymous.Anonymous).vt = VT_LPWSTR;
            (*pv.Anonymous.Anonymous).Anonymous.pwszVal = PWSTR(wide.as_mut_ptr());
            store.SetValue(&PKEY_AppUserModel_ID, &pv)?;
            store.Commit()?;
            // Dize bizim belleğimizde: PROPVARIANT serbest bırakılmaz (PropVariantClear çağrılmaz)
            std::mem::forget(pv);
            let file: IPersistFile = link.cast()?;
            file.Save(&HSTRING::from(shortcut_path().as_os_str()), true)?;
        }
        Ok(())
    })
    .join()
    .map_err(|_| anyhow::anyhow!("kısayol thread'i çöktü"))?
}

pub fn remove_shortcut() {
    let _ = std::fs::remove_file(shortcut_path());
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&apos;")
}

/// Windows bildirimi. `open`: tıklanınca açılacak klasör/dosya (isteğe bağlı).
pub fn toast(title: &str, body: &str, open: Option<&Path>) {
    let launch = open
        .map(|p| format!(" activationType=\"protocol\" launch=\"{}\"", xml_escape(&format!("file:///{}", p.display().to_string().replace('\\', "/")))))
        .unwrap_or_default();
    let xml = format!(
        "<toast{launch}><visual><binding template=\"ToastGeneric\"><text>{}</text><text>{}</text></binding></visual></toast>",
        xml_escape(title),
        xml_escape(body)
    );
    std::thread::spawn(move || {
        let result = (|| -> windows::core::Result<()> {
            unsafe {
                let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            }
            let doc = XmlDocument::new()?;
            doc.LoadXml(&HSTRING::from(xml))?;
            let toast = ToastNotification::CreateToastNotification(&doc)?;
            ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(AUMID))?.Show(&toast)
        })();
        if let Err(e) = result {
            crate::log!("Bildirim gösterilemedi: {e}");
        }
    });
}
