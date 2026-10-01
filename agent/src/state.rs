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

pub fn data_dir() -> PathBuf {
    let base = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    base.join("CouchRemote")
}

fn file() -> PathBuf {
    data_dir().join("state.json")
}

impl State {
    pub fn load() -> Result<State> {
        std::fs::create_dir_all(data_dir())?;
        let mut s: State = match std::fs::read_to_string(file()) {
            Ok(text) => serde_json::from_str(&text)?,
            Err(_) => State::default(),
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

    /// Çözülemeyen ayar dosyasını yedekler ve sıfırdan başlar
    pub fn backup_and_reset() -> Result<State> {
        let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
        let _ = std::fs::rename(file(), data_dir().join(format!("state.bozuk-{stamp}.json")));
        State::load()
    }

    pub fn save(&self) -> Result<()> {
        let tmp = file().with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_string_pretty(self)?)?;
        std::fs::rename(tmp, file())?;
        Ok(())
    }
}

pub fn random_token(bytes: usize) -> String {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    let mut b = vec![0u8; bytes];
    rand::fill(&mut b[..]);
    URL_SAFE_NO_PAD.encode(b)
}
