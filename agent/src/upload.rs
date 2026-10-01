//! Telefondan PC'ye dosya gönderme. Dosyalar sadece İndirilenler\CouchRemote klasörüne yazılır.
//! Güvenlik: WebSocket üzerinden eşleşmiş cihaza verilen kısa ömürlü jeton gerekir; dosya adı temizlenir
//! (yol, "..", Windows'un yasak karakter ve adları), var olan dosyanın üzerine yazılmaz, boyut sınırlıdır.
use crate::sync::LockExt;
use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use http_body_util::BodyExt;
use serde_json::json;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::AsyncWriteExt;

use crate::app::App;

const MAX_BYTES: u64 = 2 * 1024 * 1024 * 1024; // 2 GB
const MIN_FREE_AFTER: u64 = 1024 * 1024 * 1024; // yükleme sonrası diskte en az 1 GB boş kalmalı
const MAX_CONCURRENT: usize = 3;
static ACTIVE: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Aynı anda en çok 3 yükleme; kapanınca (hata dahil) yer açılır
struct UploadSlot;
impl UploadSlot {
    fn take() -> Option<UploadSlot> {
        use std::sync::atomic::Ordering::SeqCst;
        (ACTIVE.fetch_add(1, SeqCst) < MAX_CONCURRENT).then_some(UploadSlot).or_else(|| {
            ACTIVE.fetch_sub(1, SeqCst);
            None
        })
    }
}
impl Drop for UploadSlot {
    fn drop(&mut self) {
        ACTIVE.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    }
}

/// Dosyanın bulunacağı diskteki boş alan (bayt); okunamazsa None
fn free_space(dir: &std::path::Path) -> Option<u64> {
    use windows::core::HSTRING;
    use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    let mut avail = 0u64;
    unsafe { GetDiskFreeSpaceExW(&HSTRING::from(dir.as_os_str()), Some(&mut avail), None, None).ok()? };
    Some(avail)
}

/// Mark-of-the-Web: telefondan gelen dosya "internetten indirilmiş" sayılır; Windows çalıştırılabilir dosyalarda uyarır (SmartScreen)
fn mark_of_the_web(path: &std::path::Path) {
    let _ = std::fs::write(format!("{}:Zone.Identifier", path.display()), "[ZoneTransfer]\r\nZoneId=3\r\n");
}
const TOKEN_TTL: Duration = Duration::from_secs(10 * 60);

/// Windows'un bildiği gerçek İndirilenler klasörü (OneDrive'a / başka diske taşınmış olabilir — H6)
pub fn uploads_dir() -> PathBuf {
    use windows::Win32::System::Com::CoTaskMemFree;
    use windows::Win32::UI::Shell::{SHGetKnownFolderPath, FOLDERID_Downloads, KNOWN_FOLDER_FLAG};
    let known = unsafe {
        SHGetKnownFolderPath(&FOLDERID_Downloads, KNOWN_FOLDER_FLAG(0), None).ok().map(|p| {
            let s = p.to_string().unwrap_or_default();
            CoTaskMemFree(Some(p.0 as _));
            s
        })
    };
    let base = known.filter(|s| !s.is_empty()).map(PathBuf::from).unwrap_or_else(|| {
        std::env::var_os("USERPROFILE").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(".")).join("Downloads")
    });
    base.join("CouchRemote")
}

/// Eşleşmiş cihaza (WebSocket üzerinden) yükleme jetonu ver
pub fn issue_token(app: &App, device_id: &str) -> String {
    let token = crate::state::random_token(24);
    let mut tokens = app.upload_tokens.lk();
    tokens.retain(|_, (_, exp)| Instant::now() < *exp);
    tokens.insert(token.clone(), (device_id.to_string(), Instant::now() + TOKEN_TTL));
    token
}

/// Güvenli dosya adı: sadece son bileşen, yasak karakterler "_", ayrılmış adlar önekli, uzunluk sınırlı
pub fn sanitize(name: &str) -> String {
    let last = name.rsplit(['/', '\\']).next().unwrap_or("");
    let mut clean: String = last
        .chars()
        .map(|c| if c.is_control() || matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') { '_' } else { c })
        .collect();
    clean = clean.trim().trim_end_matches(['.', ' ']).to_string();
    if clean.is_empty() || clean.chars().all(|c| c == '.') {
        clean = "dosya".into();
    }
    let stem = clean.split('.').next().unwrap_or("").to_ascii_uppercase();
    const RESERVED: &[&str] = &["CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8", "COM9",
        "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9"];
    if RESERVED.contains(&stem.as_str()) {
        clean = format!("_{clean}");
    }
    if clean.chars().count() > 120 {
        let (base, ext) = match clean.rfind('.') {
            Some(i) if clean.len() - i <= 10 => (clean[..i].to_string(), clean[i..].to_string()),
            _ => (clean.clone(), String::new()),
        };
        clean = base.chars().take(120 - ext.chars().count()).collect::<String>() + &ext;
    }
    clean
}

/// Aynı adda dosya varsa "ad (1).uzantı" gibi yeni ad
fn unique_path(dir: &std::path::Path, name: &str) -> PathBuf {
    let p = dir.join(name);
    if !p.exists() {
        return p;
    }
    let (base, ext) = match name.rfind('.') {
        Some(i) if i > 0 => (&name[..i], &name[i..]),
        _ => (name, ""),
    };
    (1..10_000).map(|n| dir.join(format!("{base} ({n}){ext}"))).find(|p| !p.exists()).unwrap_or_else(|| dir.join(format!("{base}-{}{ext}", uuid::Uuid::new_v4())))
}

fn reply(status: StatusCode, v: serde_json::Value) -> Response {
    Response::builder().status(status).header("Content-Type", "application/json").body(v.to_string().into()).unwrap()
}

pub async fn api_upload(State(app): State<Arc<App>>, headers: HeaderMap, body: Body) -> Response {
    let origin_ok = headers.get("origin").and_then(|v| v.to_str().ok()).is_some_and(|o| app.app_origins().iter().any(|a| a == o));
    if !origin_ok {
        return reply(StatusCode::FORBIDDEN, json!({ "error": "origin" }));
    }
    let token = headers.get("x-upload-token").and_then(|v| v.to_str().ok()).unwrap_or("");
    let valid = app.upload_tokens.lk().get(token).is_some_and(|(_, exp)| Instant::now() < *exp);
    if !valid {
        return reply(StatusCode::UNAUTHORIZED, json!({ "error": "token" }));
    }
    let Some(_slot) = UploadSlot::take() else {
        return reply(StatusCode::TOO_MANY_REQUESTS, json!({ "error": "busy" }));
    };
    let declared = headers.get("content-length").and_then(|v| v.to_str().ok()).and_then(|v| v.parse::<u64>().ok());
    if declared.is_some_and(|n| n > MAX_BYTES) {
        return reply(StatusCode::PAYLOAD_TOO_LARGE, json!({ "error": "too-large" }));
    }
    let raw_name = headers.get("x-file-name").and_then(|v| v.to_str().ok()).unwrap_or("dosya");
    let decoded = percent_decode(raw_name);
    let name = sanitize(&decoded);

    let dir = uploads_dir();
    if let Err(e) = tokio::fs::create_dir_all(&dir).await {
        return reply(StatusCode::INTERNAL_SERVER_ERROR, json!({ "error": e.to_string() }));
    }
    // Disk dolmasın: boyutu bildirilmişse sığıp sığmadığına bak (bildirilmemişse 1 GB pay)
    if free_space(&dir).is_some_and(|free| free < declared.unwrap_or(0) + MIN_FREE_AFTER) {
        return reply(StatusCode::INSUFFICIENT_STORAGE, json!({ "error": "disk" }));
    }
    let path = unique_path(&dir, &name);
    let partial = path.with_extension(format!("{}.part", path.extension().and_then(|e| e.to_str()).unwrap_or("")));
    let mut file = match tokio::fs::File::create(&partial).await {
        Ok(f) => f,
        Err(e) => return reply(StatusCode::INTERNAL_SERVER_ERROR, json!({ "error": e.to_string() })),
    };
    let mut written = 0u64;
    let mut body = body;
    let result: Result<(), String> = async {
        while let Some(frame) = body.frame().await {
            let frame = frame.map_err(|e| e.to_string())?;
            if let Some(chunk) = frame.data_ref() {
                written += chunk.len() as u64;
                if written > MAX_BYTES {
                    return Err("too-large".into());
                }
                file.write_all(chunk).await.map_err(|e| e.to_string())?;
            }
        }
        file.flush().await.map_err(|e| e.to_string())
    }
    .await;
    drop(file);
    if let Err(e) = result {
        let _ = tokio::fs::remove_file(&partial).await;
        return reply(StatusCode::BAD_REQUEST, json!({ "error": e }));
    }
    if let Err(e) = tokio::fs::rename(&partial, &path).await {
        let _ = tokio::fs::remove_file(&partial).await;
        return reply(StatusCode::INTERNAL_SERVER_ERROR, json!({ "error": e.to_string() }));
    }
    mark_of_the_web(&path);
    let saved = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    log!("Dosya alındı ({} KB)", written / 1024); // dosya adı günlüğe yazılmaz
    crate::notify::toast("Telefondan dosya geldi", &format!("{saved} · İndirilenler › CouchRemote"), Some(&dir));
    reply(StatusCode::OK, json!({ "ok": true, "name": saved, "bytes": written }))
}

/// Bayt düzeyinde çözülür: "%" sonrası çok baytlı karakter gelse de metin dilimleme hatası (panik) olmaz
fn percent_decode(s: &str) -> String {
    let hex = |c: u8| (c as char).to_digit(16).map(|d| d as u8);
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let (Some(h), Some(l)) = (hex(b[i + 1]), hex(b[i + 2])) {
                out.push(h * 16 + l);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dosya_adi_temizlenir() {
        assert_eq!(sanitize("..\\..\\Windows\\system32\\evil.dll"), "evil.dll");
        assert_eq!(sanitize("../../etc/passwd"), "passwd");
        assert_eq!(sanitize("a<b>c:d|e?f*.txt"), "a_b_c_d_e_f_.txt");
        assert_eq!(sanitize("CON.txt"), "_CON.txt");
        assert_eq!(sanitize("nul"), "_nul");
        assert_eq!(sanitize("..."), "dosya");
        assert_eq!(sanitize("  foto.jpg.  "), "foto.jpg");
        assert_eq!(sanitize("Fotoğraf 1.HEIC"), "Fotoğraf 1.HEIC");
        assert!(sanitize(&"a".repeat(500)).chars().count() <= 120);
    }

    #[test]
    fn yuzde_kodu_cozulur() {
        assert_eq!(percent_decode("Foto%C4%9Fraf%201.jpg"), "Fotoğraf 1.jpg");
        assert_eq!(percent_decode("bozuk%zz%"), "bozuk%zz%");
        assert_eq!(percent_decode("%ğü"), "%ğü"); // çok baytlı karakterde panik yok
    }
}
