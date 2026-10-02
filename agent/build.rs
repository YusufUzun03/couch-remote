// Exe'nin Özellikler › Ayrıntılar sekmesindeki yapımcı ve ürün bilgileri
fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set("ProductName", "CouchRemote");
        res.set("FileDescription", "CouchRemote: telefonla bilgisayar ve TV kumandası");
        res.set("CompanyName", "WOOP WOOP");
        res.set("LegalCopyright", "WOOP WOOP");
        res.set("OriginalFilename", "CouchRemote.exe");
        if let Err(e) = res.compile() {
            println!("cargo:warning=exe bilgileri eklenemedi: {e}");
        }
    }
}
