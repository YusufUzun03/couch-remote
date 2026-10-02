//! Kalıcı durum: %APPDATA%\CouchRemote\state.json
//! Gizli alanlar (`*_enc`) DPAPI ile şifrelidir.
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Clone)]
pub struct Device {
    pub id: String,
    pub name: String,
    /// P-256 açık anahtar koordinatları (base64url, JWK'deki x ve y)
    pub x: String,
    pub y: String,
    pub key_mode: String,
    pub paired_at: String,
    pub via: String,
    /// Eşleşmenin yapıldığı adres
    pub origin: String,
    #[serde(default)]
    pub last_seen: Option<String>,
    /// Son bağlandığı yerel IP: aynı telefon yeniden eşleşince eski kaydı bulmak için (B5)
    #[serde(default)]
    pub last_ip: Option<String>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct StoredCert {
    pub host: String,
    pub cert_pem: String,
    pub key_pem_enc: String,
    /// RFC 3339
    pub not_after: String,
    /// Sertifikaya yazılan LAN adresi (adres değişince sertifika yenilenir)
    #[serde(default)]
    pub ip: String,
}

#[derive(Serialize, Deserialize, Default)]
pub struct State {
    pub id: String,
    pub devices: Vec<Device>,
    pub shortcut_token_enc: String,
    pub profile_uuid_outer: String,
    pub profile_uuid_inner: String,
    pub ca: Option<StoredCert>,
    pub leaf: Option<StoredCert>,
}

/// Veri klasörü. COUCH_DATA_DIR ile değiştirilebilir (testler gerçek eşleşmelerle karışmasın diye ayrı klasör kullanır)
pub fn data_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("COUCH_DATA_DIR") {
        return PathBuf::from(d);
    }
    let base = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    base.join("CouchRemote")
}

/// Otomatik yedekler: ayar dosyası silinir/bozulursa en yenisinden kurtarılır (aynı Windows kullanıcısı için geçerli).
/// Roaming'den ayrı bir yerde (LocalAppData) tutulur ki Roaming temizlense de kalsın.
pub fn backup_dir() -> PathBuf {
    if std::env::var_os("COUCH_DATA_DIR").is_some() {
        return data_dir().join("backup");
    }
    let base = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    base.join("CouchRemote").join("backup")
}

const BACKUP_KEEP: usize = 5;

/// Yönetim sayfasında gösterilen bilgi: ayarlar yedekten kurtarıldı / sıfırlandı (telefonlar yeniden bağlanmalı olabilir)
static NOTICE: std::sync::Mutex<Option<&'static str>> = std::sync::Mutex::new(None);

pub fn notice() -> Option<&'static str> {
    *NOTICE.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn set_notice(code: &'static str) {
    *NOTICE.lock().unwrap_or_else(|e| e.into_inner()) = Some(code);
}

pub fn clear_notice() {
    *NOTICE.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

/// Saklanacak yedeklerden fazla olanlar (adlar tarihe göre sıralanır: en eskiler silinir)
fn backups_to_delete(mut names: Vec<String>, keep: usize) -> Vec<String> {
    names.sort();
    let n = names.len().saturating_sub(keep);
    names.truncate(n);
    names
}

fn backup_names(dir: &std::path::Path) -> Vec<String> {
    std::fs::read_dir(dir)
        .map(|rd| rd.filter_map(|e| e.ok()).filter_map(|e| e.file_name().into_string().ok()).filter(|n| n.starts_with("state-") && n.ends_with(".json")).collect())
        .unwrap_or_default()
}

/// En yeni geçerli yedeğin metni
fn latest_backup(dir: &std::path::Path) -> Option<String> {
    let mut names = backup_names(dir);
    names.sort();
    names.iter().rev().find_map(|n| {
        let t = std::fs::read_to_string(dir.join(n)).ok()?;
        serde_json::from_str::<State>(&t).ok().filter(|s| !s.id.is_empty() && s.ca.is_some()).map(|_| t)
    })
}

/// Ayar dosyasının anlık yedeği (içerik en yeniyle aynıysa yazmaz); en fazla 5 tane saklanır
pub fn snapshot_backup() {
    let Ok(text) = std::fs::read_to_string(file()) else { return };
    let dir = backup_dir();
    let _ = std::fs::create_dir_all(&dir);
    let mut names = backup_names(&dir);
    names.sort();
    if names.last().and_then(|n| std::fs::read_to_string(dir.join(n)).ok()).is_some_and(|t| t == text) {
        return;
    }
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    if std::fs::write(dir.join(format!("state-{stamp}.json")), &text).is_ok() {
        names.push(format!("state-{stamp}.json"));
        for old in backups_to_delete(names, BACKUP_KEEP) {
            let _ = std::fs::remove_file(dir.join(old));
        }
    }
}

fn file() -> PathBuf {
    data_dir().join("state.json")
}

impl State {
    pub fn load() -> Result<State> {
        Self::load_inner(true)
    }

    /// `allow_restore`: dosya yoksa/bozuksa otomatik yedekten kurtar (başka kullanıcıya ait dosya sıfırlanırken kapalı)
    fn load_inner(allow_restore: bool) -> Result<State> {
        std::fs::create_dir_all(data_dir())?;
        let mut s: State = match std::fs::read_to_string(file()) {
            Ok(text) => match serde_json::from_str(&text) {
                Ok(s) => s,
                Err(e) => {
                    // Bozuk dosya: kenara al, yedekten kurtarmayı dene
                    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
                    let _ = std::fs::rename(file(), data_dir().join(format!("state.bozuk-{stamp}.json")));
                    log!("Ayar dosyası okunamadı ({e}); yedekten kurtarılıyor");
                    Self::from_backup_or_default(allow_restore, "corrupt")
                }
            },
            Err(_) => Self::from_backup_or_default(allow_restore, "missing"),
        };
        if s.id.is_empty() {
            let mut b = [0u8; 2];
            rand::fill(&mut b);
            s.id = format!("{:02x}{:02x}", b[0], b[1]);
        }
        if s.shortcut_token_enc.is_empty() {
            s.shortcut_token_enc = crate::dpapi::protect_str(&random_token(24))?;
        }
        if s.profile_uuid_outer.is_empty() {
            s.profile_uuid_outer = uuid::Uuid::new_v4().to_string().to_uppercase();
            s.profile_uuid_inner = uuid::Uuid::new_v4().to_string().to_uppercase();
        }
        s.save()?;
        Ok(s)
    }

    /// Yedek varsa onu, yoksa boş durumu döner. Yedek varken dosyanın olmaması "silinmiş" demektir: bilgi bırakılır
    fn from_backup_or_default(allow_restore: bool, why: &'static str) -> State {
        if allow_restore {
            if let Some(text) = latest_backup(&backup_dir()) {
                if let Ok(s) = serde_json::from_str::<State>(&text) {
                    log!("Ayarlar otomatik yedekten geri yüklendi ({why})");
                    set_notice("restored");
                    return s;
                }
            }
        }
        State::default()
    }

    /// Çözülemeyen ayar dosyasını yedekler ve sıfırdan başlar (telefonlar yeniden bağlanmalı)
    pub fn backup_and_reset() -> Result<State> {
        let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
        let _ = std::fs::rename(file(), data_dir().join(format!("state.bozuk-{stamp}.json")));
        set_notice("reset");
        State::load_inner(false)
    }

    pub fn save(&self) -> Result<()> {
        let tmp = file().with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_string_pretty(self)?)?;
        std::fs::rename(tmp, file())?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eski_yedekler_silinir_en_yeniler_kalir() {
        let names: Vec<String> = ["state-20261003-100000.json", "state-20261001-100000.json", "state-20261002-100000.json", "state-20261004-100000.json"].iter().map(|s| s.to_string()).collect();
        assert_eq!(backups_to_delete(names.clone(), 2), vec!["state-20261001-100000.json", "state-20261002-100000.json"]);
        assert!(backups_to_delete(names, 10).is_empty());
    }

    #[test]
    fn yedekten_kurtarma_gecerli_en_yeniyi_secer() {
        let dir = std::env::temp_dir().join(format!("couch-test-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let mk = |id: &str| format!(r#"{{"id":"{id}","devices":[],"shortcut_token_enc":"x","profile_uuid_outer":"a","profile_uuid_inner":"b","ca":{{"host":"h","cert_pem":"c","key_pem_enc":"k","not_after":"n"}},"leaf":null}}"#);
        std::fs::write(dir.join("state-20261001-000000.json"), mk("eski")).unwrap();
        std::fs::write(dir.join("state-20261002-000000.json"), mk("yeni")).unwrap();
        std::fs::write(dir.join("state-20261003-000000.json"), "bozuk{{").unwrap(); // en yeni ama geçersiz: atlanır
        let t = latest_backup(&dir).unwrap();
        assert!(t.contains("\"id\":\"yeni\""));
        let _ = std::fs::remove_dir_all(&dir);
    }
}

pub fn random_token(bytes: usize) -> String {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    let mut b = vec![0u8; bytes];
    rand::fill(&mut b[..]);
    URL_SAFE_NO_PAD.encode(b)
}
