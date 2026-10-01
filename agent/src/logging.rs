//! Günlük: konsola ve %APPDATA%\CouchRemote\couch.log dosyasına yazar (konsolsuz sürümde tek kayıt yeri).
//! Dosya 1 MB'ı geçince bir öncekine (couch.old.log) taşınır. Yazılan metin ve kişisel veri günlüğe girmez.
use std::io::Write;
use std::sync::Mutex;

const MAX_BYTES: u64 = 1024 * 1024;
static LOCK: Mutex<()> = Mutex::new(());

pub fn line(text: String) {
    let stamped = format!("{} {text}", chrono::Local::now().format("%H:%M:%S"));
    println!("{stamped}");
    let _guard = LOCK.lock();
    let dir = crate::state::data_dir();
    // Kurulum sırasında veri klasörü henüz yok: günlük satırları kaybolmasın
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("couch.log");
    if std::fs::metadata(&path).map(|m| m.len() > MAX_BYTES).unwrap_or(false) {
        let _ = std::fs::rename(&path, dir.join("couch.old.log"));
    }
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(f, "{} {text}", chrono::Local::now().format("%Y-%m-%d %H:%M:%S"));
    }
}
