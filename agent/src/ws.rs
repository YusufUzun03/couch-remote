//! WSS girdi kanalı. İkili paketler (little endian):
//!   girdi (20 B): u8 tip, u8 butonlar, u16 sıra, f32 a, f32 b, f64 t   tip: 1=hareket(mm) 2=kaydırma(mm) 3=durum 4=hava faresi(piksel, ivmesiz)
//!   0x10 ping (12 B): u8, 3 boş, f64 t0           → 0x11 pong (20 B): u8, 3 boş, f64 t0, f64 sunucu zamanı
//!   0x12 saat farkı (20 B): u8, 3 boş, f64 fark, f64 rtt
//! Kimlik doğrulama: sunucu rastgele bir sayı gönderir, cihaz ECDSA P-256 anahtarıyla imzalar.
use crate::sync::LockExt;
use axum::extract::ws::{Message, WebSocket};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use p256::ecdsa::signature::Verifier;
use p256::ecdsa::{Signature, VerifyingKey};
use serde_json::{json, Value};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::app::App;
use crate::audio::AudioCmd;
use crate::input::{vk_from_name, Cmd, Mods};
use crate::media::MediaCmd;
use crate::power::{self, Action};
use crate::state::Device;

const MOVE: u8 = 1;
const SCROLL: u8 = 2;
const STATE: u8 = 3;
const AIR: u8 = 4; // hava faresi: telefon ivmeyi/süzmeyi kendisi yapar, sunucu pikseli olduğu gibi uygular
const PING: u8 = 0x10;
const PONG: u8 = 0x11;
const OFFSET: u8 = 0x12;

const WATCHDOG: Duration = Duration::from_millis(300); // bu süre paket gelmezse basılı butonlar bırakılır
const AUTH_TIMEOUT: Duration = Duration::from_secs(5);

// Hızlanma eğrisi (tek yerde uygulanır): yavaş = hassas, hızlı = uzağa
const BASE_PX_PER_MM: f64 = 6.0;
fn gain(mm_per_ms: f64) -> f64 {
    BASE_PX_PER_MM * (0.6 + mm_per_ms.min(1.2) * 2.5)
}
const WHEEL_PER_MM: f64 = 15.0; // 8 mm ≈ 1 çentik (120)

fn now_ms() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs_f64() * 1000.0).unwrap_or(0.0)
}

fn f32_at(b: &[u8], i: usize) -> f64 {
    f32::from_le_bytes(b[i..i + 4].try_into().unwrap()) as f64
}
fn f64_at(b: &[u8], i: usize) -> f64 {
    f64::from_le_bytes(b[i..i + 8].try_into().unwrap())
}

/// Bir bağlantının paket durumu. WebSocket ve WebRTC aynı örneği paylaşır: hangi yoldan gelirse gelsin
/// basılı tuş takibi, saat farkı ve istatistikler tek yerde tutulur.
pub struct Pkt {
    pub device_id: String,
    pub buttons_down: bool,
    pub packets: u32,
    pub last_packet: Instant,
    last_t: f64,
    offset: Option<f64>,
    /// Son DURUM paketinin zamanı ve butonları: sırasız (UDP) yoldan gecikmeyle gelen eski bir hareket paketi, bırakılmış tuşu yeniden basmasın
    state_t: f64,
    state_buttons: u8,
}

impl Pkt {
    pub fn new() -> Pkt {
        Pkt { device_id: String::new(), buttons_down: false, packets: 0, last_packet: Instant::now(), last_t: 0.0, offset: None, state_t: f64::MIN, state_buttons: 0 }
    }
}

/// İkili girdi paketini işler (hareket, kaydırma, durum, ping, saat farkı). Ping için yanıt paketini döner.
pub fn process_packet(app: &App, pkt: &Mutex<Pkt>, b: &[u8]) -> Option<Vec<u8>> {
    if b.len() < 12 {
        return None;
    }
    let mut p = pkt.lk();
    p.last_packet = Instant::now();
    p.packets += 1;
    match b[0] {
        PING => {
            let mut out = [0u8; 20];
            out[0] = PONG;
            out[4..12].copy_from_slice(&b[4..12]);
            out[12..20].copy_from_slice(&now_ms().to_le_bytes());
            Some(out.to_vec())
        }
        OFFSET => {
            p.offset = Some(f64_at(b, 4));
            None
        }
        kind @ (MOVE | SCROLL | STATE | AIR) if b.len() >= 20 => {
            let arrived = now_ms();
            let (a, c, t) = (f32_at(b, 4), f32_at(b, 8), f64_at(b, 12));
            // NaN / sonsuz değer imleç konumunu kalıcı bozardı
            if !(a.is_finite() && c.is_finite() && t.is_finite()) {
                return None;
            }
            let buttons = if kind == STATE {
                if t >= p.state_t {
                    p.state_t = t;
                    p.state_buttons = b[1];
                }
                b[1]
            } else if t < p.state_t {
                p.state_buttons
            } else {
                b[1]
            };
            p.buttons_down = buttons != 0;
            match kind {
                AIR => app.input.send(Cmd::Move { buttons, dx: a.clamp(-300.0, 300.0), dy: c.clamp(-300.0, 300.0) }),
                MOVE => {
                    let dt = (t - p.last_t).clamp(1.0, 50.0);
                    p.last_t = t;
                    let g = gain(a.hypot(c) / dt);
                    app.input.send(Cmd::Move { buttons, dx: a * g, dy: c * g });
                }
                // Doğal kaydırma: parmak yukarı → sayfa aşağı (iPhone gibi)
                SCROLL => app.input.send(Cmd::Scroll { buttons, v: c * WHEEL_PER_MM, h: -a * WHEEL_PER_MM }),
                _ => app.input.send(Cmd::State { buttons }),
            }
            if let (Some(off), true) = (p.offset, kind != STATE) {
                app.stats.lk().add_one_way(arrived - (t + off));
            }
            None
        }
        _ => None,
    }
}

enum AuthError {
    Ignore,
    Unknown,
    Bad(String),
}

fn authenticate(app: &App, text: &str, nonce: &[u8]) -> Result<Device, AuthError> {
    let msg: Value = serde_json::from_str(text).map_err(|_| AuthError::Ignore)?;
    if msg["type"] != "auth" {
        return Err(AuthError::Ignore);
    }
    let id = msg["deviceId"].as_str().unwrap_or("");
    let device = app.state.lk().devices.iter().find(|d| d.id == id).cloned().ok_or(AuthError::Unknown)?;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    let x = URL_SAFE_NO_PAD.decode(&device.x).map_err(|e| AuthError::Bad(e.to_string()))?;
    let y = URL_SAFE_NO_PAD.decode(&device.y).map_err(|e| AuthError::Bad(e.to_string()))?;
    let key = VerifyingKey::from_sec1_bytes(&[&[4u8][..], &x, &y].concat()).map_err(|e| AuthError::Bad(e.to_string()))?;
    let sig_bytes = B64.decode(msg["sig"].as_str().unwrap_or("")).map_err(|e| AuthError::Bad(e.to_string()))?;
    let sig = Signature::from_slice(&sig_bytes).map_err(|e| AuthError::Bad(e.to_string()))?;
    key.verify(nonce, &sig).map_err(|_| AuthError::Bad("imza geçersiz".into()))?;
    Ok(device)
}

pub async fn handle(mut socket: WebSocket, app: Arc<App>, peer: std::net::IpAddr) {
    let mut nonce = [0u8; 32];
    rand::fill(&mut nonce);
    let challenge = json!({ "type": "challenge", "nonce": B64.encode(nonce) }).to_string();
    if socket.send(Message::Text(challenge.into())).await.is_err() {
        return;
    }

    let started = Instant::now();
    let mut device: Option<Device> = None;
    let pkt = Arc::new(Mutex::new(Pkt::new()));
    let mut rtc_session: Option<u64> = None;
    let mut watchdog = tokio::time::interval(Duration::from_millis(50));
    let mut tick = tokio::time::interval(Duration::from_secs(1));
    let mut events_rx = app.events.subscribe();
    let mut revoked_rx = app.revoked.subscribe();
    let mut bucket = Bucket::new();

    loop {
        tokio::select! {
            msg = socket.recv() => {
                let Some(Ok(msg)) = msg else { break };
                match msg {
                    // Kimlik doğrulamadan önce sadece küçük bir "auth" mesajı beklenir
                    Message::Text(text) if device.is_none() && text.len() > 2048 => break,
                    Message::Text(text) if device.is_none() => match authenticate(&app, &text, &nonce) {
                        Ok(d) => {
                            log!("Bağlandı: {}", d.name);
                            app.connections.fetch_add(1, Ordering::Relaxed);
                            {
                                let mut st = app.state.lk();
                                if let Some(x) = st.devices.iter_mut().find(|x| x.id == d.id) {
                                    x.last_seen = Some(chrono::Local::now().to_rfc3339());
                                    x.last_ip = Some(peer.to_string());
                                }
                                let _ = st.save();
                            }
                            pkt.lk().device_id = d.id.clone();
                            app.connected.lk().insert(d.id.clone());
                            let host = d.origin.trim_start_matches("https://").split(':').next().unwrap_or("").to_string();
                            device = Some(d);
                            // Telefonda "Bağlı · MASAÜSTÜ-PC" gibi tanıdık ad görünsün
                            let name = std::env::var("COMPUTERNAME").unwrap_or_default();
                            let ok = json!({ "type": "ok", "host": host, "name": name }).to_string();
                            if socket.send(Message::Text(ok.into())).await.is_err() { break; }
                            // Yeni bağlanan telefona son durumları (medya + kapak, ses, zamanlayıcı, öndeki uygulama) hemen gönder
                            let mut failed = false;
                            for m in app.events.snapshot() {
                                if socket.send(Message::Text(m.into())).await.is_err() { failed = true; break; }
                            }
                            if failed { break; }
                            app.audio.send(AudioCmd::Refresh); // bağlanma anında ses durumu güncel olsun
                        }
                        Err(AuthError::Unknown) => {
                            app.fails.fail(peer);
                            if app.unknown_attempts.fetch_add(1, Ordering::Relaxed) == 0 {
                                log!("Kayıtlı olmayan bir telefon bağlanmaya çalıştı (eşleşme bilgisayarda yok)");
                            }
                            *app.unknown_last.lk() = Some(chrono::Local::now().to_rfc3339());
                            let _ = socket.send(Message::Text(json!({ "type": "unknown-device" }).to_string().into())).await;
                            break;
                        }
                        Err(AuthError::Bad(e)) => {
                            app.fails.fail(peer);
                            log!("Kimlik doğrulama başarısız: {e}");
                            break;
                        }
                        Err(AuthError::Ignore) => {}
                    },
                    Message::Text(text) if text.contains("\"rtc-offer\"") => {
                        pkt.lk().last_packet = Instant::now();
                        let sdp = serde_json::from_str::<Value>(&text).ok().and_then(|v| v["sdp"].as_str().map(str::to_owned));
                        let reply = match (&app.rtc, sdp) {
                            (None, _) => json!({ "type": "rtc-answer", "error": "WebRTC kapalı" }),
                            // Her teklif bir DTLS sertifikası üretir: hızlı art arda teklif işlemciyi yormasın
                            (Some(_), _) if !bucket.take(25.0) => json!({ "type": "rtc-answer", "error": "çok sık istendi, biraz bekle" }),
                            (Some(hub), Some(sdp)) if sdp.len() < 20_000 => {
                                if let Some(old) = rtc_session.take() {
                                    hub.close(old);
                                }
                                match hub.offer(app.clone(), pkt.clone(), peer, sdp).await {
                                    Ok((id, answer)) => {
                                        rtc_session = Some(id);
                                        json!({ "type": "rtc-answer", "sdp": answer })
                                    }
                                    Err(e) => json!({ "type": "rtc-answer", "error": e }),
                                }
                            }
                            _ => json!({ "type": "rtc-answer", "error": "geçersiz istek" }),
                        };
                        if socket.send(Message::Text(reply.to_string().into())).await.is_err() { break; }
                    }
                    Message::Text(text) => {
                        pkt.lk().last_packet = Instant::now();
                        // Başlatıcı Başlat menüsünü tarayabilir / program açabilir: çalışma zamanını bloklamadan
                        let id = device.as_ref().map(|d| d.id.clone()).unwrap_or_default();
                        // Hız sınırı: ağır komutlar (ekran görüntüsü, uygulama listesi…) pahalıdır, art arda istenemez
                        if !bucket.take(command_cost(&text)) {
                            if text.contains("\"screenshot\"") {
                                let busy = json!({ "type": "screenshot", "error": "Çok sık istendi, biraz bekle" }).to_string();
                                if socket.send(Message::Text(busy.into())).await.is_err() { break; }
                            }
                            continue;
                        }
                        let reply = tokio::task::block_in_place(|| command(&app, &id, &text));
                        if let Some(r) = reply {
                            if socket.send(Message::Text(r.into())).await.is_err() { break; }
                        }
                    }
                    Message::Binary(b) if device.is_some() && b.len() >= 12 => {
                        if let Some(out) = process_packet(&app, &pkt, &b) {
                            if socket.send(Message::Binary(out.into())).await.is_err() { break; }
                        }
                    }
                    Message::Close(_) => break,
                    _ => {}
                }
            }
            // S1: eşleşmesi kaldırılan cihazın bağlantısı hemen kapanır
            revoked = revoked_rx.recv() => {
                if let (Ok(id), Some(d)) = (&revoked, &device) {
                    if *id == d.id {
                        let _ = socket.send(Message::Text(json!({ "type": "revoked" }).to_string().into())).await;
                        log!("Eşleşme kaldırıldı, bağlantı kapatılıyor: {}", d.name);
                        break;
                    }
                }
            }
            update = events_rx.recv() => {
                match update {
                    Ok(m) if device.is_some() => {
                        if socket.send(Message::Text(m.into())).await.is_err() { break; }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    _ => {}
                }
            }
            _ = watchdog.tick() => {
                let stuck = { let p = pkt.lk(); p.buttons_down && p.last_packet.elapsed() > WATCHDOG };
                if stuck {
                    app.input.release_all();
                    pkt.lk().buttons_down = false;
                    log!("Heartbeat kesildi — basılı butonlar bırakıldı");
                }
            }
            _ = tick.tick() => {
                if device.is_none() {
                    if started.elapsed() > AUTH_TIMEOUT { break; }
                    continue;
                }
                let rate = std::mem::take(&mut pkt.lk().packets);
                let summary = {
                    let mut s = app.stats.lk();
                    s.rate = rate;
                    s.summary()
                };
                let mut m = summary;
                m["type"] = json!("stats");
                if socket.send(Message::Text(m.to_string().into())).await.is_err() { break; }
            }
        }
    }

    if let (Some(id), Some(hub)) = (rtc_session, &app.rtc) {
        hub.close(id);
    }
    if let Some(d) = device {
        app.connected.lk().remove(&d.id);
        app.connections.fetch_sub(1, Ordering::Relaxed);
        app.input.release_all();
        log!("Bağlantı kapandı: {}", d.name);
    }
}

const MAX_TEXT: usize = 4000;

/// Bağlantı başına komut kovası: dolu hâlde ~100 komut, saniyede 50 yenilenir (hızlı yazı rahat geçer).
struct Bucket {
    tokens: f64,
    last: Instant,
}

impl Bucket {
    const CAP: f64 = 100.0;
    const PER_SEC: f64 = 50.0;

    fn new() -> Bucket {
        Bucket { tokens: Self::CAP, last: Instant::now() }
    }

    fn take(&mut self, cost: f64) -> bool {
        let now = Instant::now();
        self.tokens = (self.tokens + now.duration_since(self.last).as_secs_f64() * Self::PER_SEC).min(Self::CAP);
        self.last = now;
        if self.tokens < cost {
            return false;
        }
        self.tokens -= cost;
        true
    }
}

/// Komutun göreli maliyeti (JSON'u çözmeden metinden tahmin edilir; istemci kompakt JSON gönderir)
fn command_cost(text: &str) -> f64 {
    for (needle, cost) in [("\"screenshot\"", 40.0), ("\"windows\"", 8.0), ("\"window\"", 5.0), ("\"apps\"", 10.0), ("\"power\"", 10.0), ("\"launch\"", 6.0), ("\"search\"", 6.0), ("\"clipboard-get\"", 6.0), ("\"game-art\"", 3.0)] {
        if text.contains(needle) {
            return cost;
        }
    }
    1.0
}

/// Kimliği doğrulanmış telefondan gelen JSON komutları (klavye, medya, başlatıcı). Yanıt gerekiyorsa döner.
/// Yazılan metin ve arama sorguları günlüğe yazılmaz.
fn command(app: &Arc<App>, device_id: &str, text: &str) -> Option<String> {
    let Ok(m) = serde_json::from_str::<Value>(text) else { return None };
    let mods = || Mods::from_names(m["mods"].as_array().into_iter().flatten().filter_map(Value::as_str));
    match m["type"].as_str().unwrap_or("") {
        "text" => {
            if let Some(t) = m["text"].as_str().filter(|t| !t.is_empty()) {
                app.input.send(Cmd::Text(t.chars().take(MAX_TEXT).collect()));
            }
        }
        "key" | "combo" => {
            // H2: kumanda/uygulama düğmeleri öndeki pencereye değil, medya penceresine gitmeli
            if let Some(err) = focus_for(&m) {
                return Some(err);
            }
            if m["type"] == "key" {
                if let Some(key) = m["key"].as_str().and_then(vk_from_name) {
                    app.input.send(Cmd::Key { key, mods: mods() });
                }
            } else if let Some(ch) = m["char"].as_str().and_then(|c| c.chars().next()) {
                app.input.send(Cmd::Combo { ch, mods: mods() });
            }
        }
        "zoom" => {
            if let Some(n) = m["steps"].as_i64() {
                app.input.send(Cmd::Zoom(n.clamp(-5, 5) as i32));
            }
        }
        // ---- açık pencereler: liste yanıtı hemen döner; eylem sonrası telefon listeyi yeniden ister
        "windows" => return Some(crate::winlist::to_json(&crate::winlist::list()).to_string()),
        "window" => {
            if let (Some(id), Some(action)) = (m["id"].as_i64(), m["action"].as_str()) {
                crate::winlist::act(id as isize, action);
            }
        }
        "media" => {
            let value = m["value"].as_f64().unwrap_or(0.0);
            let cmd = match m["action"].as_str().unwrap_or("") {
                "playpause" => MediaCmd::PlayPause,
                "next" => MediaCmd::Next,
                "prev" => MediaCmd::Prev,
                "seek" if value.is_finite() => MediaCmd::Seek(value),
                "seekby" if value.is_finite() => MediaCmd::SeekBy(value.clamp(-600.0, 600.0)),
                _ => return None,
            };
            app.media.send(cmd);
        }
        "apps" => return Some(app.launcher.list().to_string()),
        "launch" => {
            let id = m["id"].as_str().unwrap_or("");
            return Some(match app.launcher.launch(id) {
                Some((name, media)) => {
                    if media && m["fullscreen"] == true {
                        auto_fullscreen(app.clone(), id.to_string());
                    }
                    json!({ "type": "launched", "name": name, "media": media })
                }
                None => json!({ "type": "launch-error", "message": "Açılamadı" }),
            }.to_string());
        }
        "search" => {
            let (id, q) = (m["id"].as_str().unwrap_or(""), m["q"].as_str().unwrap_or(""));
            return Some(match app.launcher.search(id, q) {
                Some(name) => json!({ "type": "launched", "name": name, "media": true, "search": true }),
                None => json!({ "type": "launch-error", "message": "Arama açılamadı" }),
            }.to_string());
        }
        // ---- güç menüsü (yeniden başlat / kapat telefonda onaylanır)
        "power" => match (m["action"].as_str().unwrap_or(""), m["confirm"] == true) {
            ("lock", _) => power::lock(),
            ("screenoff", _) => power::screen_off(),
            ("sleep", _) => power::sleep(),
            ("restart", true) => power::shutdown(true),
            ("shutdown", true) => power::shutdown(false),
            _ => {}
        },
        // ---- uyku zamanlayıcısı
        "stats-reset" => app.stats.lk().reset(),
        "timer" => {
            if m["cancel"] == true {
                power::cancel_timer(app);
            } else if let (Some(min), Some(action)) = (m["minutes"].as_u64(), m["action"].as_str().and_then(Action::parse)) {
                if (1..=600).contains(&min) {
                    power::start_timer(app.clone(), min, action);
                }
            }
        }
        // ---- ses
        "volume" => {
            if let Some(l) = m["level"].as_f64().filter(|l| l.is_finite()) {
                app.audio.send(AudioCmd::SetLevel(l.clamp(0.0, 1.0) as f32));
            }
            if let Some(mute) = m["mute"].as_bool() {
                app.audio.send(AudioCmd::SetMute(mute));
            }
        }
        // ---- ekran görüntüsü: tüm ekranlar (monitor yok) veya tek monitör (0, 1, …)
        "screenshot" => {
            use base64::{engine::general_purpose::STANDARD as B64, Engine};
            let monitor = m["monitor"].as_u64().map(|i| i as usize);
            let count = crate::screenshot::monitors().len();
            return Some(match crate::screenshot::capture(monitor) {
                Some(s) => {
                    log!("Ekran görüntüsü alındı: {}x{}, {} KB", s.width, s.height, s.jpeg.len() / 1024);
                    json!({ "type": "screenshot", "jpeg": B64.encode(&s.jpeg), "w": s.width, "h": s.height, "monitors": count, "monitor": monitor })
                }
                None => json!({ "type": "screenshot", "error": "Ekran yakalanamadı", "monitors": count }),
            }.to_string());
        }
        // ---- oyun kapağı (Steam yerel önbelleği); yoksa art: null
        "game-art" => {
            let id = m["id"].as_str().unwrap_or("");
            return Some(json!({ "type": "game-art", "id": id, "art": crate::launcher::game_art(id) }).to_string());
        }
        "mixer" => {
            if let Some(id) = m["id"].as_str().filter(|i| !i.is_empty() && i.len() < 100) {
                let level = m["level"].as_f64().filter(|l| l.is_finite()).map(|l| l.clamp(0.0, 1.0) as f32);
                app.audio.send(AudioCmd::SetApp { id: id.to_string(), level, mute: m["mute"].as_bool() });
            }
        }
        // ---- LG TV (PC aracı): sabit komut eşlemesi, tv.rs
        "tv" => app.tv.handle(&m),
        "audio-device" => {
            if let Some(id) = m["id"].as_str().filter(|i| i.len() < 300) {
                app.audio.send(AudioCmd::SetDefault(id.to_string()));
            }
        }
        // ---- pano (içerik günlüğe yazılmaz)
        "clipboard-get" => {
            return Some(match crate::clipboard::get_text() {
                Ok(text) => json!({ "type": "clipboard", "text": text }),
                Err(e) => json!({ "type": "clipboard", "error": e.to_string() }),
            }.to_string());
        }
        "clipboard-set" => {
            let text = m["text"].as_str().unwrap_or("");
            let ok = crate::clipboard::set_text(text).is_ok();
            if ok && m["paste"] == true {
                app.input.send(Cmd::Combo { ch: 'v', mods: Mods { ctrl: true, ..Default::default() } });
            }
            return Some(json!({ "type": "clipboard-set", "ok": ok, "paste": m["paste"] == true }).to_string());
        }
        // ---- Siri / Kestirmeler kurulumu: eşleşmiş telefon jetonu ve adresi kopyalayabilsin
        "shortcut-info" => {
            let actions: Vec<Value> = crate::web::ACTIONS.iter().map(|a| json!({ "id": a.0, "params": a.1, "about": a.2 })).collect();
            return Some(json!({ "type": "shortcut-info", "base": format!("{}/api/v1/action", app.local_origin()), "token": *app.shortcut_token.lk(), "actions": actions }).to_string());
        }
        // ---- dosya gönderme
        "upload-token" => {
            let token = crate::upload::issue_token(app, device_id);
            return Some(json!({ "type": "upload-token", "token": token }).to_string());
        }
        "open-uploads" => {
            let dir = crate::upload::uploads_dir();
            let _ = std::fs::create_dir_all(&dir);
            crate::launcher::shell_open(&dir.to_string_lossy());
        }
        // ---- tanılama raporu: sadece eşleşmiş cihazdan, sadece bellekte (S2)
        "report" => {
            if text.len() <= 16 * 1024 {
                let mut v = m.clone();
                v["at"] = json!(chrono::Local::now().to_rfc3339());
                let mut r = app.reports.lk();
                r.push_front(v);
                r.truncate(20);
            }
        }
        _ => {}
    }
    None
}

const PROFILE_NAMES: &[(&str, &str)] = &[
    ("netflix", "Netflix"), ("youtube", "YouTube"), ("ytmusic", "YouTube Music"), ("primevideo", "Prime Video"),
    ("disneyplus", "Disney+"), ("twitch", "Twitch"), ("spotify", "Spotify"), ("vlc", "VLC"), ("tabii", "tabii"), ("exxen", "Exxen"),
];

/// "profile": bu uygulamanın penceresi öne gelmeli (yoksa hata döner, tuş başka yere yazılmaz).
/// "target": "media": son medya penceresi varsa öne getir; yoksa tuş öndeki pencereye gider.
fn focus_for(m: &Value) -> Option<String> {
    if let Some(p) = m["profile"].as_str() {
        if !crate::context::focus_media(Some(p)) {
            let name = PROFILE_NAMES.iter().find(|(id, _)| *id == p).map(|(_, n)| *n).unwrap_or(p);
            return Some(json!({ "type": "launch-error", "message": format!("{name} penceresi bulunamadı") }).to_string());
        }
    } else if m["target"] == "media" {
        crate::context::focus_media(None);
    }
    None
}

/// Servis açılınca sayfa öne geldiğinde bir kez tam ekrana al (zaten tam ekransa dokunma)
pub fn auto_fullscreen(app: Arc<App>, id: String) {
    std::thread::spawn(move || {
        for _ in 0..40 {
            std::thread::sleep(Duration::from_millis(250));
            if crate::context::foreground_profile() == id {
                std::thread::sleep(Duration::from_millis(600)); // sayfa yerleşsin
                if !crate::context::foreground_fullscreen() {
                    app.input.send(Cmd::Key { key: 0x7A, mods: Mods::default() }); // F11
                }
                return;
            }
        }
    });
}
