//! PC programının kendi pencere, menü ve bildirim metinleri: Windows'un görüntü diline göre Türkçe veya İngilizce.
//! (Telefondaki arayüz ve yönetim sayfası kendi dil seçimini yapar; bu sadece programın yerel pencereleri içindir.)
//! COUCH_LANG=tr|en ortam değişkeni seçimi zorlar (deneme için). Günlük satırları her zaman Türkçe kalır.
use std::sync::OnceLock;
use windows::Win32::Globalization::GetUserDefaultUILanguage;

const LANG_TURKISH: u16 = 0x1F;

pub fn is_tr() -> bool {
    static TR: OnceLock<bool> = OnceLock::new();
    *TR.get_or_init(|| match std::env::var("COUCH_LANG").as_deref() {
        Ok("tr") => true,
        Ok("en") => false,
        _ => (unsafe { GetUserDefaultUILanguage() } & 0x3FF) == LANG_TURKISH,
    })
}

/// İki dilden uygun olanı seçer
pub fn pick<'a>(tr: &'a str, en: &'a str) -> &'a str {
    if is_tr() { tr } else { en }
}
