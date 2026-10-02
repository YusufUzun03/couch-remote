//! Parolalı yedek: eşleşmiş telefonlar + yerel güvenlik sertifikası (CA) + Kestirmeler jetonu + kayıtlı TV'ler.
//! Başka bir Windows kullanıcısına/bilgisayara taşımak ya da yeniden kurulumdan sonra telefonların yeniden kurulum yapmadan
//! çalışması için. Dosya AES-256-GCM ile şifrelenir; anahtar parolandan PBKDF2-HMAC-SHA256 (600 000 tur) ile türetilir.
//! İçinde özel anahtarlar olduğu için parolayı ve dosyayı güvende tut.
use crate::state::{self, Device, State, StoredCert};
use anyhow::{anyhow, bail, Result};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use ring::{aead, pbkdf2};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::num::NonZeroU32;

const ITER: u32 = 600_000;
const AAD: &[u8] = b"couchremote-backup-v1";
pub const MIN_PASSWORD: usize = 8;

#[derive(Serialize, Deserialize)]
struct Plain {
    id: String,
    ca_host: String,
    ca_cert_pem: String,
    ca_key_pem: String,
    ca_not_after: String,
    devices: Vec<Device>,
    shortcut_token: String,
    profile_uuid_outer: String,
    profile_uuid_inner: String,
    tvs: Vec<Value>,
}

fn derive(password: &str, salt: &[u8], iter: u32) -> aead::LessSafeKey {
    let mut key = [0u8; 32];
    pbkdf2::derive(pbkdf2::PBKDF2_HMAC_SHA256, NonZeroU32::new(iter).unwrap(), salt, password.as_bytes(), &mut key);
    aead::LessSafeKey::new(aead::UnboundKey::new(&aead::AES_256_GCM, &key).unwrap())
}

fn seal(plain: &[u8], password: &str) -> String {
    let mut salt = [0u8; 16];
    let mut nonce = [0u8; 12];
    rand::fill(&mut salt);
    rand::fill(&mut nonce);
    let key = derive(password, &salt, ITER);
    let mut buf = plain.to_vec();
    key.seal_in_place_append_tag(aead::Nonce::assume_unique_for_key(nonce), aead::Aad::from(AAD), &mut buf).expect("şifreleme");
    serde_json::json!({ "v": 1, "app": "CouchRemote", "iter": ITER, "salt": B64.encode(salt), "nonce": B64.encode(nonce), "ct": B64.encode(buf) }).to_string()
}

fn open(blob: &str, password: &str) -> Result<Vec<u8>> {
    let v: Value = serde_json::from_str(blob).map_err(|_| anyhow!("Dosya bir CouchRemote yedeği değil"))?;
    if v["app"] != "CouchRemote" || v["v"] != 1 {
        bail!("Dosya bir CouchRemote yedeği değil");
    }
    let iter = v["iter"].as_u64().filter(|i| (100_000..=5_000_000).contains(i)).ok_or_else(|| anyhow!("Yedek dosyası bozuk"))? as u32;
    let dec = |k: &str| B64.decode(v[k].as_str().unwrap_or("")).map_err(|_| anyhow!("Yedek dosyası bozuk"));
    let (salt, nonce, mut ct) = (dec("salt")?, dec("nonce")?, dec("ct")?);
    let nonce: [u8; 12] = nonce.try_into().map_err(|_| anyhow!("Yedek dosyası bozuk"))?;
    let key = derive(password, &salt, iter);
    let plain = key.open_in_place(aead::Nonce::assume_unique_for_key(nonce), aead::Aad::from(AAD), &mut ct).map_err(|_| anyhow!("Parola yanlış ya da dosya bozuk"))?;
    Ok(plain.to_vec())
}

/// Yedek dosyasının metni
pub fn export(password: &str) -> Result<String> {
    if password.chars().count() < MIN_PASSWORD {
        bail!("Parola en az {MIN_PASSWORD} karakter olmalı");
    }
    let st = State::load()?;
    let ca = st.ca.as_ref().ok_or_else(|| anyhow!("Sertifika yok"))?;
    let plain = Plain {
        id: st.id.clone(),
        ca_host: ca.host.clone(),
        ca_cert_pem: ca.cert_pem.clone(),
        ca_key_pem: crate::dpapi::unprotect_str(&ca.key_pem_enc)?,
        ca_not_after: ca.not_after.clone(),
        devices: st.devices.clone(),
        shortcut_token: crate::dpapi::unprotect_str(&st.shortcut_token_enc)?,
        profile_uuid_outer: st.profile_uuid_outer.clone(),
        profile_uuid_inner: st.profile_uuid_inner.clone(),
        tvs: crate::tv::export_tvs(),
    };
    Ok(seal(&serde_json::to_vec(&plain)?, password))
}

/// Yedeği geri yükler (mevcut ayarlar kenara alınır). Program sonra yeniden başlatılmalı.
pub fn import(password: &str, blob: &str) -> Result<usize> {
    let plain: Plain = serde_json::from_slice(&open(blob, password)?).map_err(|_| anyhow!("Yedek dosyası bozuk"))?;
    if plain.id.len() != 4 || !plain.id.bytes().all(|b| b.is_ascii_hexdigit()) || plain.ca_cert_pem.is_empty() || plain.ca_key_pem.is_empty() {
        bail!("Yedek dosyası bozuk");
    }
    // Mevcut ayarlar kenara
    let dir = state::data_dir();
    std::fs::create_dir_all(&dir)?;
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    let _ = std::fs::rename(dir.join("state.json"), dir.join(format!("state.onceki-{stamp}.json")));
    let n = plain.devices.len();
    let st = State {
        id: plain.id,
        devices: plain.devices,
        shortcut_token_enc: crate::dpapi::protect_str(&plain.shortcut_token)?,
        profile_uuid_outer: plain.profile_uuid_outer,
        profile_uuid_inner: plain.profile_uuid_inner,
        ca: Some(StoredCert { host: plain.ca_host, cert_pem: plain.ca_cert_pem, key_pem_enc: crate::dpapi::protect_str(&plain.ca_key_pem)?, not_after: plain.ca_not_after, ip: String::new() }),
        leaf: None, // açılışta yeniden üretilir
    };
    st.save()?;
    crate::tv::import_tvs(&plain.tvs);
    state::clear_notice();
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sifreleme_gidis_donus_ve_yanlis_parola() {
        let blob = seal(b"gizli veri", "dogru-parola-123");
        assert_eq!(open(&blob, "dogru-parola-123").unwrap(), b"gizli veri");
        assert!(open(&blob, "yanlis-parola-123").is_err());
        // bozulan şifreli metin
        let mut v: Value = serde_json::from_str(&blob).unwrap();
        let ct = v["ct"].as_str().unwrap().to_string();
        let mut raw = B64.decode(ct).unwrap();
        raw[0] ^= 1;
        v["ct"] = Value::String(B64.encode(raw));
        assert!(open(&v.to_string(), "dogru-parola-123").is_err());
        assert!(open("{\"app\":\"baska\"}", "x").is_err());
    }

    #[test]
    fn kisa_parola_reddedilir() {
        assert!(export("kisa").is_err());
    }
}
