//! HTTP yönlendirmeleri: kurulum (:47800), uygulama (:47801), yönetim (:47802).
//! Güvenlik: her istekte Host kontrolü (DNS rebinding), durum değiştiren isteklerde Origin kontrolü.
use crate::sync::LockExt;
use axum::body::{Body, Bytes};
use axum::extract::{Path, Query, Request, State, WebSocketUpgrade};
use axum::http::{header, HeaderMap, StatusCode};
use axum::middleware::{self, Next};
use axum::response::Response;
use axum::routing::{get, post};
use axum::Router;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use crate::app::App;
use crate::assets;
use crate::audio::AudioCmd;
use crate::input::{vk, Cmd};
use crate::media::MediaCmd;
use crate::pairing::MAX_PIN_ATTEMPTS;
use crate::state::Device;
use crate::{ADMIN_PORT, HTTP_PORT};

type S = Arc<App>;

// ------------------------------------------------------------------ yardımcılar

/// Sabit süreli karşılaştırma (uzunluk farkı da sızmasın diye özetler karşılaştırılır)
pub fn safe_eq(a: &str, b: &str) -> bool {
    let (x, y) = (Sha256::digest(a.as_bytes()), Sha256::digest(b.as_bytes()));
    x.iter().zip(y.iter()).fold(0u8, |acc, (p, q)| acc | (p ^ q)) == 0
}

fn resp(status: StatusCode, ctype: &str, body: impl Into<Body>) -> Response {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, ctype)
        .header(header::CACHE_CONTROL, "no-cache")
        .header("X-Content-Type-Options", "nosniff")
        .body(body.into())
        .unwrap()
}

fn json_resp(status: StatusCode, v: Value) -> Response {
    resp(status, "application/json", v.to_string())
}

fn err(status: StatusCode, code: &str, message: &str) -> Response {
    json_resp(status, json!({ "error": code, "message": message }))
}

fn not_found() -> Response {
    resp(StatusCode::NOT_FOUND, "text/plain; charset=utf-8", "Bulunamadı")
}

fn asset(name: &str) -> Response {
    match assets::bytes(name) {
        Some(b) => {
            let mime = mime_guess::from_path(name).first_or_octet_stream();
            let ctype = if mime.type_() == "text" || name.ends_with(".js") {
                format!("{mime}; charset=utf-8")
            } else {
                mime.to_string()
            };
            resp(StatusCode::OK, &ctype, b)
        }
        None => not_found(),
    }
}

fn page(name: &str, repl: &[(&str, &str)]) -> Response {
    let mut text = assets::text(name);
    for (k, v) in repl {
        text = text.replace(k, v);
    }
    resp(StatusCode::OK, "text/html; charset=utf-8", text)
}

fn png(app: &App, size: u32) -> Response {
    resp(StatusCode::OK, "image/png", app.icons[&size].clone())
}

fn host_of(h: &HeaderMap) -> String {
    h.get(header::HOST).and_then(|v| v.to_str().ok()).unwrap_or("").to_ascii_lowercase()
}

fn origin_of(h: &HeaderMap) -> Option<&str> {
    h.get(header::ORIGIN).and_then(|v| v.to_str().ok())
}

fn origin_ok(h: &HeaderMap, allowed: &[String], allow_missing: bool) -> bool {
    match origin_of(h) {
        None => allow_missing,
        Some(o) => allowed.iter().any(|a| a == o),
    }
}

fn parse_json(body: &Bytes) -> Option<Value> {
    serde_json::from_slice(body).ok()
}

const CODE_MAX: usize = 64;
fn valid_code(c: &str) -> bool {
    (8..=CODE_MAX).contains(&c.len()) && c.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

// Güvenlik başlıkları: tarayıcıya "bu sayfayı çerçeveleme, dışarıdan betik yükleme, yönlendiren adresi gönderme" der.
// Uygulama sayfası çok katı (sadece kendi betikleri); kurulum ve yönetim sayfalarının kendi küçük satır içi betiği var.
const CSP_APP: &str = "default-src 'none'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data: blob:; \
    connect-src 'self' wss:; manifest-src 'self'; worker-src 'self'; media-src 'self' blob:; base-uri 'none'; form-action 'self'; frame-ancestors 'none'";
const CSP_PAGE: &str = "default-src 'none'; script-src 'self' 'unsafe-inline'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; \
    connect-src 'self' https:; base-uri 'none'; form-action 'self'; frame-ancestors 'none'";

async fn sec_headers(State(csp): State<&'static str>, req: Request, next: Next) -> Response {
    // Yönetim sunucusundaki arayüz önizlemesi uygulama sayfasıdır: uygulamanın katı politikasıyla denenir
    let csp = if req.uri().path() == "/preview" { CSP_APP } else { csp };
    let mut r = next.run(req).await;
    let h = r.headers_mut();
    for (k, v) in [
        ("content-security-policy", csp),
        ("x-frame-options", "DENY"),
        ("referrer-policy", "no-referrer"),
        ("cross-origin-opener-policy", "same-origin"),
        ("cross-origin-resource-policy", "same-origin"),
        ("permissions-policy", "camera=(), microphone=(), geolocation=(), payment=(), usb=()"),
    ] {
        h.insert(k, v.parse().unwrap());
    }
    r
}

// Host kontrol katmanları (DNS rebinding koruması)
async fn guard_setup(State(app): State<S>, req: Request, next: Next) -> Response {
    if !app.setup_hosts().contains(&host_of(req.headers())) {
        return resp(StatusCode::MISDIRECTED_REQUEST, "text/plain; charset=utf-8", "Yanlış adres");
    }
    next.run(req).await
}
async fn guard_app(State(app): State<S>, req: Request, next: Next) -> Response {
    if !app.app_hosts().contains(&host_of(req.headers())) {
        return resp(StatusCode::MISDIRECTED_REQUEST, "text/plain; charset=utf-8", "Yanlış adres");
    }
    next.run(req).await
}
fn admin_hosts() -> Vec<String> {
    vec![format!("127.0.0.1:{ADMIN_PORT}"), format!("localhost:{ADMIN_PORT}")]
}
const ADMIN_COOKIE: &str = "couch_admin";

fn cookie_of<'a>(h: &'a HeaderMap, name: &str) -> Option<&'a str> {
    h.get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .find_map(|kv| kv.trim().strip_prefix(name)?.strip_prefix('='))
}

const ADMIN_LOCKED: &str = r#"<!doctype html><html lang="tr"><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><title>CouchRemote</title>
<body style="margin:0;display:grid;place-items:center;min-height:100vh;background:#0b0d12;color:#e8ecf4;font:16px/1.5 system-ui,sans-serif;padding:16px">
<div style="max-width:420px;text-align:center"><h1 style="font-size:22px">CouchRemote</h1>
<p style="color:#8a93a6">Güvenlik için bu sayfa sadece programın kendisinden açılır.<br>Saatin yanındaki <b>CouchRemote</b> simgesine tıkla veya programı yeniden çalıştır.</p></div></body></html>"#;

async fn guard_admin(State(app): State<S>, req: Request, next: Next) -> Response {
    let hosts = admin_hosts();
    if !hosts.contains(&host_of(req.headers())) {
        return resp(StatusCode::MISDIRECTED_REQUEST, "text/plain; charset=utf-8", "Yanlış adres");
    }
    // Anahtar kontrolü: program sayfayı ?t=<anahtar> ile açar; anahtar çereze yazılır ve adresten silinir.
    // Böylece bu bilgisayardaki başka bir kullanıcı veya bilinen adresi açan bir site yönetim sayfasına giremez.
    let query_token = req.uri().query().and_then(|q| q.split('&').find_map(|kv| kv.strip_prefix("t=")));
    if let Some(t) = query_token {
        if safe_eq(t, &app.admin_token) {
            return Response::builder()
                .status(StatusCode::SEE_OTHER)
                .header(header::LOCATION, req.uri().path())
                .header(header::SET_COOKIE, format!("{ADMIN_COOKIE}={}; HttpOnly; SameSite=Strict; Path=/", app.admin_token))
                .header(header::CACHE_CONTROL, "no-store")
                .body(Body::empty())
                .unwrap();
        }
    }
    if !cookie_of(req.headers(), ADMIN_COOKIE).is_some_and(|c| safe_eq(c, &app.admin_token)) {
        return if req.uri().path() == "/" {
            resp(StatusCode::UNAUTHORIZED, "text/html; charset=utf-8", ADMIN_LOCKED)
        } else {
            err(StatusCode::UNAUTHORIZED, "admin", "Yönetim anahtarı gerekli")
        };
    }
    // Durum değiştiren istekler: Origin bizim olmalı + özel başlık (başka siteler ön kontrol olmadan gönderemez)
    if req.method() == axum::http::Method::POST {
        let origins: Vec<String> = hosts.iter().map(|h| format!("http://{h}")).collect();
        let custom = req.headers().get("x-couch-admin").and_then(|v| v.to_str().ok()) == Some("1");
        if !origin_ok(req.headers(), &origins, false) || !custom {
            return err(StatusCode::FORBIDDEN, "origin", "İzin verilmeyen kaynak");
        }
    }
    next.run(req).await
}

// ------------------------------------------------------------------ 1) kurulum sunucusu (HTTP)

pub fn setup_router(app: S) -> Router {
    Router::new()
        .route("/", get(setup_page))
        .route("/setup", get(setup_page))
        .route("/ca.mobileconfig", get(mobileconfig))
        .route("/icon-180.png", get(|State(a): State<S>| async move { png(&a, 180) }))
        .route("/logo.svg", get(|| async { asset("logo.svg") }))
        .route("/api/pair/touch", post(setup_touch))
        .route("/i18n.js", get(|| async { asset("i18n.js") }))
        .route("/i18n-en.js", get(|| async { asset("i18n-en.js") }))
        .layer(middleware::from_fn_with_state(app.clone(), guard_setup))
        .layer(middleware::from_fn_with_state(CSP_PAGE, sec_headers))
        .with_state(app)
}

async fn setup_page(State(app): State<S>) -> Response {
    page("setup.html", &[("%%HOST%%", &app.host), ("%%APP_ORIGIN%%", &app.local_origin()), ("%%CA_CODE%%", &app.ca_code)])
}

async fn mobileconfig(State(app): State<S>) -> Response {
    log!("Sertifika profili indirildi");
    if let Some(p) = app.pairing.lk().as_mut() {
        p.profile_seen = true;
    }
    let st = app.state.lk();
    let body = crate::certs::mobile_config(&app.host, &st.id, &st.profile_uuid_outer, &st.profile_uuid_inner, &app.ca_der);
    Response::builder()
        .header(header::CONTENT_TYPE, "application/x-apple-aspen-config")
        .header(header::CONTENT_DISPOSITION, format!("attachment; filename=\"CouchRemote-{}.mobileconfig\"", st.id))
        .body(Body::from(body))
        .unwrap()
}

fn touch(app: &App, body: &Bytes) -> Response {
    let code = parse_json(body).and_then(|v| v["code"].as_str().map(str::to_owned)).unwrap_or_default();
    let ok = app.pairing.lk().as_mut().is_some_and(|p| {
        let valid = p.touch(&code);
        if valid {
            p.setup_seen = true; // yönetim sayfasında "QR okutuldu" adımı tamamlanır
        }
        valid
    });
    json_resp(StatusCode::OK, json!({ "ok": ok }))
}

async fn setup_touch(State(app): State<S>, headers: HeaderMap, body: Bytes) -> Response {
    if !origin_ok(&headers, &app.setup_origins(), false) {
        return err(StatusCode::FORBIDDEN, "origin", "İzin verilmeyen kaynak");
    }
    touch(&app, &body)
}

// ------------------------------------------------------------------ 2) uygulama sunucusu (HTTPS)

pub fn app_router(app: S) -> Router {
    Router::new()
        .route("/", get(|| async { Response::builder().status(StatusCode::FOUND).header(header::LOCATION, "/app").body(Body::empty()).unwrap() }))
        .route("/app", get(app_page))
        .route("/pair", get(app_page))
        .route("/app.js", get(|| async { asset("app.js") }))
        .route("/air.js", get(|| async { asset("air.js") }))
        .route("/tv.js", get(|| async { asset("tv.js") }))
        .route("/brand-icons.js", get(|| async { asset("brand-icons.js") }))
        .route("/app.css", get(|| async { asset("app.css") }))
        .route("/ui.css", get(|| async { asset("ui.css") }))
        .route("/design.css", get(|| async { asset("design.css") }))
        .route("/i18n.js", get(|| async { asset("i18n.js") }))
        .route("/i18n-en.js", get(|| async { asset("i18n-en.js") }))
        .route("/sw.js", get(|| async { asset("sw.js") }))
        .route("/apple-touch-icon.png", get(|State(a): State<S>| async move { png(&a, 180) }))
        .route("/icon-180.png", get(|State(a): State<S>| async move { png(&a, 180) }))
        .route("/icon-192.png", get(|State(a): State<S>| async move { png(&a, 192) }))
        .route("/icon-512.png", get(|State(a): State<S>| async move { png(&a, 512) }))
        .route("/logo.svg", get(|| async { asset("logo.svg") }))
        .route("/manifest.webmanifest", get(manifest))
        .route("/api/hello", get(hello))
        .route("/api/pair", post(api_pair))
        .route("/api/pair/touch", post(app_touch))
        .route("/api/v1/action/{id}", post(api_action))
        .route("/api/upload", post(crate::upload::api_upload).layer(axum::extract::DefaultBodyLimit::disable()))
        .route("/ws", get(ws_upgrade))
        .layer(middleware::from_fn_with_state(app.clone(), guard_app))
        .layer(middleware::from_fn_with_state(CSP_APP, sec_headers))
        .with_state(app)
}

async fn app_page(Query(q): Query<HashMap<String, String>>) -> Response {
    let href = match q.get("c") {
        Some(c) if valid_code(c) => format!("/manifest.webmanifest?c={c}"),
        _ => "/manifest.webmanifest".into(),
    };
    page("app.html", &[("%%MANIFEST_HREF%%", &href)])
}

/// Eşleşme kodu start_url'e gömülür: ana ekran uygulamasının depolaması Safari'den ayrıdır.
async fn manifest(Query(q): Query<HashMap<String, String>>) -> Response {
    let start = match q.get("c") {
        Some(c) if valid_code(c) => format!("/app?c={c}"),
        _ => "/app".into(),
    };
    let m = json!({
        "name": "CouchRemote", "short_name": "Couch", "id": "/app", "start_url": start, "scope": "/",
        "display": "standalone", "background_color": "#000000", "theme_color": "#000000",
        "icons": [
            { "src": "/icon-192.png", "sizes": "192x192", "type": "image/png" },
            { "src": "/icon-512.png", "sizes": "512x512", "type": "image/png" }
        ]
    });
    resp(StatusCode::OK, "application/manifest+json", m.to_string())
}

/// Kurulum sayfası (HTTP) bununla sertifikaya güvenilip güvenilmediğini anlar
async fn hello(State(app): State<S>, headers: HeaderMap) -> Response {
    let mut r = json_resp(StatusCode::OK, json!({ "ok": true, "host": host_of(&headers) }));
    if let Some(o) = origin_of(&headers) {
        if app.setup_origins().iter().any(|s| s == o) {
            r.headers_mut().insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, o.parse().unwrap());
        }
    }
    r
}

async fn app_touch(State(app): State<S>, headers: HeaderMap, body: Bytes) -> Response {
    if !origin_ok(&headers, &app.app_origins(), false) {
        return err(StatusCode::FORBIDDEN, "origin", "İzin verilmeyen kaynak");
    }
    touch(&app, &body)
}

/// Telefonun bildirdiği ad günlüğe ve yönetim sayfasına girer: kontrol karakterleri atılır, uzunluk sınırlanır
fn clean_device_name(raw: &str) -> String {
    let name: String = raw.chars().filter(|c| !c.is_control()).take(40).collect::<String>().trim().to_string();
    if name.is_empty() { "iPhone".into() } else { name }
}

fn decode_coord(s: &str) -> Option<Vec<u8>> {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    URL_SAFE_NO_PAD.decode(s).ok().filter(|b| b.len() == 32)
}

async fn api_pair(State(app): State<S>, axum::Extension(peer): axum::Extension<crate::tls::Peer>, headers: HeaderMap, body: Bytes) -> Response {
    if !origin_ok(&headers, &app.app_origins(), false) {
        return err(StatusCode::FORBIDDEN, "origin", "İzin verilmeyen kaynak");
    }
    let Some(b) = parse_json(&body) else { return err(StatusCode::BAD_REQUEST, "json", "Geçersiz istek") };

    let via = {
        let mut guard = app.pairing.lk();
        let Some(p) = guard.as_mut().filter(|p| p.active()) else {
            return err(StatusCode::GONE, "expired", "Eşleşme süresi doldu. PC ekranından yeni kod al.");
        };
        if let Some(code) = b["code"].as_str().filter(|c| !c.is_empty()) {
            if !safe_eq(code, &p.code) {
                return err(StatusCode::FORBIDDEN, "invalid", "Kod veya PIN hatalı.");
            }
            "code"
        } else if let Some(pin) = b["pin"].as_str() {
            if !safe_eq(pin, &p.pin) {
                p.pin_attempts += 1;
                if p.pin_attempts >= MAX_PIN_ATTEMPTS {
                    drop(guard);
                    app.lock_pairing();
                    return err(StatusCode::TOO_MANY_REQUESTS, "locked", "Çok fazla hatalı deneme. PC ekranından yeni kod al.");
                }
                return err(StatusCode::FORBIDDEN, "invalid", "Kod veya PIN hatalı.");
            }
            "pin"
        } else {
            return err(StatusCode::BAD_REQUEST, "missing", "Kod veya PIN gerekli.");
        }
    };

    let jwk = &b["publicKeyJwk"];
    let (Some(x), Some(y)) = (jwk["x"].as_str(), jwk["y"].as_str()) else {
        return err(StatusCode::BAD_REQUEST, "key", "Anahtar eksik");
    };
    let (Some(xb), Some(yb)) = (decode_coord(x), decode_coord(y)) else {
        return err(StatusCode::BAD_REQUEST, "key", "Anahtar geçersiz");
    };
    let sec1 = [&[4u8][..], &xb, &yb].concat();
    if jwk["crv"] != "P-256" || p256::ecdsa::VerifyingKey::from_sec1_bytes(&sec1).is_err() {
        return err(StatusCode::BAD_REQUEST, "key", "Anahtar geçersiz");
    }

    let device = Device {
        id: uuid::Uuid::new_v4().to_string(),
        name: clean_device_name(b["name"].as_str().unwrap_or("")),
        x: x.into(),
        y: y.into(),
        key_mode: b["keyMode"].as_str().unwrap_or("?").chars().take(30).collect(),
        paired_at: chrono::Local::now().to_rfc3339(),
        via: via.into(),
        origin: origin_of(&headers).unwrap_or("").into(),
        last_seen: None,
        last_ip: Some(peer.0.to_string()),
    };
    let id = device.id.clone();
    log!("✓ Cihaz eşleşti: {} ({via}, anahtar: {})", device.name, device.key_mode);
    // B5: aynı adlı, aynı IP'den bağlanmış ve şu an bağlı olmayan eski kayıt aynı telefondur
    // (ana ekran uygulaması silinip yeniden eklenince iOS eski anahtarı da siler)
    let stale: Vec<String> = {
        let connected = app.connected.lk();
        let st = app.state.lk();
        st.devices
            .iter()
            .filter(|d| d.name == device.name && d.last_ip == device.last_ip && !connected.contains(&d.id))
            .map(|d| d.id.clone())
            .collect()
    };
    for old in &stale {
        app.revoke_device(old);
    }
    if !stale.is_empty() {
        log!("Aynı telefonun {} eski kaydı kaldırıldı", stale.len());
    }
    {
        let mut st = app.state.lk();
        st.devices.push(device);
        let _ = st.save();
    }
    *app.pairing.lk() = None; // kod tek kullanımlık; yenisi sadece sayfa açıkken üretilir
    json_resp(StatusCode::OK, json!({ "deviceId": id }))
}

/// Tuş basan basit eylemler
const KEY_ACTIONS: &[(&str, u16)] = &[
    ("playpause", vk::MEDIA_PLAY_PAUSE),
    ("next", vk::MEDIA_NEXT),
    ("prev", vk::MEDIA_PREV),
    ("volup", vk::VOLUME_UP),
    ("voldown", vk::VOLUME_DOWN),
    ("mute", vk::VOLUME_MUTE),
];

/// Kestirmeler API'sindeki tüm eylemler: (kimlik, parametre, açıklama). Telefondaki rehber de bunu gösterir.
/// Yeniden başlat / kapat bilerek YOK: jeton telefonda saklanır, sızarsa en kötü ihtimalle uyutulabilir.
pub const ACTIONS: &[(&str, &str, &str)] = &[
    ("playpause", "", "Oynat / duraklat"),
    ("next", "", "Sonraki parça / bölüm"),
    ("prev", "", "Önceki parça"),
    ("seekback", "", "10 saniye geri sar"),
    ("seekfwd", "", "10 saniye ileri sar"),
    ("volup", "", "Sesi aç"),
    ("voldown", "", "Sesi kıs"),
    ("mute", "", "Sesi kapat / aç"),
    ("volume", "v = 0–100", "Ses seviyesini ayarla (ör. ?v=30)"),
    ("launch", "v = uygulama kimliği", "Uygulama / servis aç (ör. ?v=netflix)"),
    ("search", "app = servis, v = arama", "Serviste ara (ör. ?app=youtube&v=lofi)"),
    ("lock", "", "Bilgisayarı kilitle"),
    ("screenoff", "", "Ekranı kapat"),
    ("sleep", "", "Bilgisayarı uyut"),
    ("timer", "v = dakika (1–600)", "Uyku zamanlayıcısı: medyayı durdurur, ekranı kapatır (ör. ?v=30)"),
    ("timer-cancel", "", "Zamanlayıcıyı iptal et"),
    ("screenshot", "monitor = 0, 1… (yoksa tüm ekranlar)", "Ekran görüntüsünü JPEG olarak döndürür (Kestirmeler'de görüntü olarak gelir)"),
    ("tv-on", "tv = TV adı (yoksa ilk TV)", "TV'yi aç (Wake-on-LAN)"),
    ("tv-off", "tv = TV adı", "TV'yi kapat"),
    ("tv-volup", "tv = TV adı", "TV sesini aç"),
    ("tv-voldown", "tv = TV adı", "TV sesini kıs"),
    ("tv-mute", "tv = TV adı", "TV sesini kapat / aç (Samsung ve Android TV'de aç-kapa tuşu)"),
    ("tv-volume", "v = 0–100, tv = TV adı", "TV ses seviyesini ayarla (yalnızca LG)"),
    ("tv-launch", "v = uygulama kimliği, tv = TV adı", "TV'de uygulama aç (ör. v=netflix, Samsung'da v=11101200001)"),
    ("tv-key", "v = tuş (HOME, BACK, ENTER, UP…), tv = TV adı", "TV kumandasından bir tuşa bas"),
    ("ping", "", "Bağlantı testi"),
];

fn api_json(status: StatusCode, v: Value) -> Response {
    json_resp(status, v)
}

/// Kestirmeler (iOS Shortcuts) API'si: POST /api/v1/action/{eylem}?v=değer
/// Kestirmeler Origin göndermez; tarayıcıdan gelirse bizim olmalı.
async fn api_action(
    State(app): State<S>,
    axum::Extension(peer): axum::Extension<crate::tls::Peer>,
    Path(id): Path<String>,
    Query(q): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> Response {
    if !origin_ok(&headers, &app.app_origins(), true) {
        return err(StatusCode::FORBIDDEN, "origin", "İzin verilmeyen kaynak");
    }
    if app.fails.blocked(peer.0) {
        return err(StatusCode::TOO_MANY_REQUESTS, "rate", "Çok fazla hatalı deneme. Bir dakika sonra tekrar dene.");
    }
    let token = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or("");
    if !safe_eq(token, &app.shortcut_token.lk()) {
        app.fails.fail(peer.0);
        return err(StatusCode::UNAUTHORIZED, "token", "Jeton geçersiz");
    }
    let v = q.get("v").map(String::as_str).unwrap_or("");
    // Ekran görüntüsü JSON değil görüntü döndürür: Kestirmeler'de doğrudan "Görüntüyü Kaydet / Göster" eylemine bağlanır
    if id == "screenshot" {
        let monitor = q.get("monitor").and_then(|m| m.parse::<usize>().ok());
        let shot = tokio::task::spawn_blocking(move || crate::screenshot::capture(monitor)).await.ok().flatten();
        let ua = headers.get(header::USER_AGENT).and_then(|v| v.to_str().ok()).unwrap_or("").to_string();
        log!("Kestirme eylemi: screenshot ({ua})");
        *app.last_shortcut.lk() = Some(json!({ "id": id, "at": chrono::Local::now().to_rfc3339(), "ua": ua }));
        return match shot {
            Some(s) => resp(StatusCode::OK, "image/jpeg", s.jpeg),
            None => api_json(StatusCode::INTERNAL_SERVER_ERROR, json!({ "error": "yakalama", "message": "Ekran yakalanamadı" })),
        };
    }
    let bad = |msg: &str| api_json(StatusCode::BAD_REQUEST, json!({ "error": "parametre", "message": msg }));
    let mut detail = Value::Null;

    match id.as_str() {
        "ping" => {}
        "seekback" => app.media.send(MediaCmd::SeekBy(-10.0)),
        "seekfwd" => app.media.send(MediaCmd::SeekBy(10.0)),
        "volume" => match v.parse::<f32>() {
            Ok(p) if (0.0..=100.0).contains(&p) => app.audio.send(AudioCmd::SetLevel(p / 100.0)),
            _ => return bad("v: 0 ile 100 arasında bir sayı olmalı"),
        },
        "launch" => match app.launcher.launch(v) {
            Some((name, media)) => {
                if media && q.get("fullscreen").is_some_and(|f| f == "1") {
                    crate::ws::auto_fullscreen(app.clone(), v.to_string());
                }
                detail = json!(name);
            }
            None => return api_json(StatusCode::NOT_FOUND, json!({ "error": "uygulama", "message": "Açılamadı veya tanınmıyor" })),
        },
        "search" => match app.launcher.search(q.get("app").map(String::as_str).unwrap_or(""), v) {
            Some(name) => detail = json!(name),
            None => return bad("app: servis kimliği, v: arama metni (en çok 200 karakter) olmalı"),
        },
        "lock" => crate::power::lock(),
        "screenoff" => crate::power::screen_off(),
        // Uyku çağrısı dönene kadar (uyanana kadar) bloklar; yanıt hemen gitsin
        "sleep" => {
            tokio::task::spawn_blocking(crate::power::sleep);
        }
        "timer" => match v.parse::<u64>() {
            Ok(min) if (1..=600).contains(&min) => crate::power::start_timer(app.clone(), min, crate::power::Action::Stop),
            _ => return bad("v: 1 ile 600 arasında dakika olmalı"),
        },
        "timer-cancel" => crate::power::cancel_timer(&app),
        tvid if tvid.starts_with("tv-") => match app.tv.shortcut(&tvid[3..], q.get("tv").map(String::as_str), v) {
            Ok(name) => detail = json!(name),
            Err(msg) => return api_json(StatusCode::BAD_REQUEST, json!({ "error": "tv", "message": msg })),
        },
        other => match KEY_ACTIONS.iter().find(|(name, _)| *name == other) {
            Some((_, key)) => app.input.send(Cmd::TapKey(*key)),
            None => {
                return api_json(
                    StatusCode::NOT_FOUND,
                    json!({ "error": "bilinmeyen eylem", "actions": ACTIONS.iter().map(|a| a.0).collect::<Vec<_>>() }),
                )
            }
        },
    }
    let ua = headers.get(header::USER_AGENT).and_then(|v| v.to_str().ok()).unwrap_or("").to_string();
    log!("Kestirme eylemi: {id}{} ({ua})", if v.is_empty() { String::new() } else { format!(" {v}") });
    *app.last_shortcut.lk() = Some(json!({ "id": id, "at": chrono::Local::now().to_rfc3339(), "ua": ua }));
    api_json(StatusCode::OK, json!({ "ok": true, "action": id, "detail": detail }))
}

async fn ws_upgrade(
    State(app): State<S>,
    axum::Extension(peer): axum::Extension<crate::tls::Peer>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Response {
    if !origin_ok(&headers, &app.app_origins(), false) {
        return err(StatusCode::FORBIDDEN, "origin", "İzin verilmeyen kaynak");
    }
    if app.fails.blocked(peer.0) {
        return err(StatusCode::TOO_MANY_REQUESTS, "rate", "Çok fazla hatalı deneme");
    }
    // Pano paylaşımı için metin mesajları büyük olabilir (100 bin karakter)
    ws.max_message_size(512 * 1024).on_upgrade(move |socket| crate::ws::handle(socket, app, peer.0))
}

// ------------------------------------------------------------------ 3) yönetim sunucusu (sadece bu bilgisayar)

pub fn admin_router(app: S) -> Router {
    Router::new()
        .route("/", get(|| async { page("admin.html", &[]) }))
        .route("/api/status", get(status))
        // Arayüz tasarım önizlemesi (eşleşme/bağlantı çalışmaz)
        .route("/preview", get(|| async { page("app.html", &[("%%MANIFEST_HREF%%", "")]) }))
        .route("/app.css", get(|| async { asset("app.css") }))
        .route("/app.js", get(|| async { asset("app.js") }))
        .route("/air.js", get(|| async { asset("air.js") }))
        .route("/tv.js", get(|| async { asset("tv.js") }))
        .route("/brand-icons.js", get(|| async { asset("brand-icons.js") }))
        .route("/ui.css", get(|| async { asset("ui.css") }))
        .route("/design.css", get(|| async { asset("design.css") }))
        .route("/i18n.js", get(|| async { asset("i18n.js") }))
        .route("/i18n-en.js", get(|| async { asset("i18n-en.js") }))
        .route("/logo.svg", get(|| async { asset("logo.svg") }))
        .route("/apple-touch-icon.png", get(|State(a): State<S>| async move { png(&a, 180) }))
        .route("/api/new-pairing", post(|State(a): State<S>| async move {
            a.new_pairing();
            json_resp(StatusCode::OK, json!({ "ok": true }))
        }))
        .route("/api/remove-device", post(remove_device))
        .route("/api/rotate-shortcut-token", post(|State(a): State<S>| async move {
            json_resp(StatusCode::OK, json!({ "ok": a.rotate_shortcut_token().is_ok() }))
        }))
        .route("/api/remove-all-devices", post(|State(a): State<S>| async move {
            a.revoke_all();
            json_resp(StatusCode::OK, json!({ "ok": true }))
        }))
        // Güncelleme: yeni sürüm çalıştırılınca eskisinin sürümünü sorar ve kapanmasını ister (B1)
        .route("/api/version", get(|| async { json_resp(StatusCode::OK, json!({ "version": env!("CARGO_PKG_VERSION") })) }))
        .route("/api/quit", post(|State(a): State<S>| async move {
            log!("Güncelleme için kapatılıyor");
            a.input.release_all();
            tokio::spawn(async {
                tokio::time::sleep(std::time::Duration::from_millis(300)).await;
                std::process::exit(0);
            });
            json_resp(StatusCode::OK, json!({ "ok": true }))
        }))
        .route("/api/network-private", post(|State(a): State<S>| async move {
            let ip = a.lan_ip.clone();
            let ok = tokio::task::spawn_blocking(move || crate::system::make_network_private(&ip)).await.unwrap_or(false);
            if ok {
                *a.network_public.lk() = Some(false);
            }
            json_resp(StatusCode::OK, json!({ "ok": ok }))
        }))
        .layer(middleware::from_fn_with_state(app.clone(), guard_admin))
        .layer(middleware::from_fn_with_state(CSP_PAGE, sec_headers))
        .with_state(app)
}

async fn remove_device(State(app): State<S>, body: Bytes) -> Response {
    let id = parse_json(&body).and_then(|v| v["id"].as_str().map(str::to_owned)).unwrap_or_default();
    app.revoke_device(&id);
    json_resp(StatusCode::OK, json!({ "ok": true }))
}

async fn status(State(app): State<S>) -> Response {
    app.ensure_pairing();
    let pairing = {
        let guard = app.pairing.lk();
        guard.as_ref().filter(|p| p.active()).map(|p| {
            let url = format!("http://{}:{HTTP_PORT}/setup?c={}", app.host, p.code);
            let qr = qrcode::QrCode::new(url.as_bytes())
                .map(|c| c.render::<qrcode::render::svg::Color>().min_dimensions(240, 240).quiet_zone(true).build())
                .unwrap_or_default();
            json!({
                "pin": p.pin, "expiresIn": p.expires_in_secs(), "setupUrl": url, "qr": qr,
                "ipUrl": format!("http://{}:{HTTP_PORT}/setup?c={}", app.lan_ip, p.code),
                "setupSeen": p.setup_seen, "profileSeen": p.profile_seen,
            })
        })
    };
    let devices: Vec<Value> = app
        .state
        .lock()
        .unwrap()
        .devices
        .iter()
        .map(|d| json!({ "id": d.id, "name": d.name, "pairedAt": d.paired_at, "lastSeen": d.last_seen, "via": d.via, "keyMode": d.key_mode, "origin": d.origin }))
        .collect();
    let origin = app.local_origin();
    json_resp(
        StatusCode::OK,
        json!({
            "host": app.host, "lanIp": app.lan_ip, "pairing": pairing, "version": env!("CARGO_PKG_VERSION"),
            "pairingLocked": app.pairing_locked(), "caCode": app.ca_code,
            "networkPublic": *app.network_public.lk(), "devices": devices,
            "connected": app.connections.load(Ordering::Relaxed),
            "shortcut": {
                "url": format!("{origin}/api/v1/action/playpause"), "token": *app.shortcut_token.lk(),
                "actions": ACTIONS.iter().map(|a| json!({ "id": a.0, "params": a.1, "about": a.2 })).collect::<Vec<_>>(),
                "last": *app.last_shortcut.lk(),
            },
            "latency": app.stats.lk().summary(),
            "reports": app.reports.lk().iter().cloned().collect::<Vec<_>>(),
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cihaz_adi_temizlenir() {
        assert_eq!(clean_device_name("iPhone"), "iPhone");
        // satır sonu ve kontrol karakterleri atılır: günlüğe sahte satır eklenemez
        assert_eq!(clean_device_name("  iPhone\n[sahte satır]\r"), "iPhone[sahte satır]");
        assert_eq!(clean_device_name(""), "iPhone");
        assert_eq!(clean_device_name("\u{7}\u{1b}"), "iPhone");
        assert_eq!(clean_device_name(&"a".repeat(100)).chars().count(), 40);
    }
}
