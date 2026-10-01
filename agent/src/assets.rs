//! Web arayüzü dosyaları programın içine gömülür (tek .exe). Geliştirme derlemesinde diskten okunur.
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "../web/"]
pub struct Web;

pub fn text(name: &str) -> String {
    Web::get(name).map(|f| String::from_utf8_lossy(&f.data).into_owned()).unwrap_or_default()
}

pub fn bytes(name: &str) -> Option<Vec<u8>> {
    Web::get(name).map(|f| f.data.into_owned())
}
