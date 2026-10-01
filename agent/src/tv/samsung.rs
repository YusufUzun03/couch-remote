//! Samsung Tizen TV sürücüsü (2016+ modeller): `samsung.remote.control` WebSocket kanalı.
//! TV ilk bağlanışta ekranda izin sorar; verilen jeton (token) DPAPI ile saklanır, sonraki bağlanışlar sorusuzdur.
//! Samsung ses seviyesi / açık uygulama gibi durum bildirmez; yalnızca tuş ve uygulama komutları vardır.
use super::{open, Act, Cmd, Ctx};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::net::Ipv4Addr;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::time::timeout;
use tokio_tungstenite::tungstenite::Message;

pub(super) const APPS: &[(&str, &str)] = &[
    ("Netflix", "11101200001"),
    ("YouTube", "111299001912"),
    ("Prime Video", "3201910019365"),
    ("Disney+", "3201901017640"),
    ("Spotify", "3201606009684"),
    ("Tarayıcı", "org.tizen.browser"),
];

/// Ortak tuş adı → Samsung tuş kodu
fn key_code(name: &str) -> Option<String> {
    Some(match name {
        "UP" | "DOWN" | "LEFT" | "RIGHT" | "ENTER" | "MENU" | "INFO" | "HOME" | "EXIT" | "RED" | "GREEN" | "YELLOW" | "BLUE" => format!("KEY_{name}"),
        "BACK" => "KEY_RETURN".into(),
        "VOLUMEUP" => "KEY_VOLUP".into(),
        "VOLUMEDOWN" => "KEY_VOLDOWN".into(),
        "MUTE" => "KEY_MUTE".into(),
        "CHANNELUP" => "KEY_CHUP".into(),
        "CHANNELDOWN" => "KEY_CHDOWN".into(),
        d if d.len() == 1 && d.as_bytes()[0].is_ascii_digit() => format!("KEY_{d}"),
        _ => return None,
    })
}

fn key(code: &str) -> Value {
    json!({ "method": "ms.remote.control", "params": { "Cmd": "Click", "DataOfCmd": code, "Option": "false", "TypeOfRemote": "SendRemoteKey" } })
}

/// Eylem → Samsung mesajı (desteklenmeyenler None)
fn message(a: &Act) -> Option<Value> {
    let k = |c: &str| Some(key(c));
    match a {
        Act::Off => k("KEY_POWER"),
        Act::VolUp => k("KEY_VOLUP"),
        Act::VolDown => k("KEY_VOLDOWN"),
        Act::Mute(_) => k("KEY_MUTE"), // Samsung yalnızca aç/kapa tuşu verir
        Act::ChUp => k("KEY_CHUP"),
        Act::ChDown => k("KEY_CHDOWN"),
        Act::Play => k("KEY_PLAY"),
        Act::Pause => k("KEY_PAUSE"),
        Act::Stop => k("KEY_STOP"),
        Act::Rew => k("KEY_REWIND"),
        Act::Ff => k("KEY_FF"),
        Act::Enter => k("KEY_ENTER"),
        Act::Key(n) => key_code(n).map(|c| key(&c)),
        Act::Launch(id) => Some(json!({
            "method": "ms.channel.emit",
            "params": { "event": "ed.apps.launch", "to": "host", "data": { "action_type": "DEEP_LINK", "appId": id, "metaTag": "" } },
        })),
        Act::Text(t) => Some(json!({
            "method": "ms.remote.control",
            "params": { "Cmd": B64.encode(t.as_bytes()), "DataOfCmd": "base64", "TypeOfRemote": "SendInputString" },
        })),
        // SetVol / Input / Move / Click / Scroll / Backspace: Samsung API'sinde karşılığı yok
        _ => None,
    }
}

/// Basit HTTP GET (yalnızca yerel ağ TV'sine, küçük JSON)
async fn http_json(ip: Ipv4Addr, port: u16, path: &str) -> Option<Value> {
    let mut s = timeout(Duration::from_secs(3), TcpStream::connect((ip, port))).await.ok()?.ok()?;
    s.write_all(format!("GET {path} HTTP/1.0\r\nHost: {ip}\r\nConnection: close\r\n\r\n").as_bytes()).await.ok()?;
    let mut buf = Vec::new();
    timeout(Duration::from_secs(3), s.take(64 * 1024).read_to_end(&mut buf)).await.ok()?.ok()?;
    let text = String::from_utf8_lossy(&buf);
    serde_json::from_str(text.split("\r\n\r\n").nth(1)?).ok()
}

pub(super) async fn session(ip: Ipv4Addr, cx: &mut Ctx, rx: &mut UnboundedReceiver<Cmd>) -> bool {
    // Model adı ve MAC (uyandırma için) TV'nin açık REST arayüzünden
    if let Some(info) = http_json(ip, 8001, "/api/v2/").await {
        let dev = &info["device"];
        if let Some(c) = cx.cfg.as_mut() {
            let mut changed = false;
            if let Some(m) = dev["modelName"].as_str().or(dev["name"].as_str()).filter(|m| !m.is_empty()) {
                let name = format!("Samsung {}", m.trim_start_matches("Samsung").trim());
                if c.name != name { c.name = name; changed = true; }
            }
            if let Some(mac) = dev["wifiMac"].as_str().filter(|m| m.len() == 17) {
                if c.mac.as_deref() != Some(mac) { c.mac = Some(mac.to_string()); changed = true; }
            }
            if changed { cx.hub.save(c); }
        }
    }

    let token = cx.cfg.as_ref().and_then(|c| c.key()).filter(|t| t.bytes().all(|b| b.is_ascii_alphanumeric()));
    let mut path = format!("/api/v2/channels/samsung.remote.control?name={}", B64.encode("CouchRemote"));
    if let Some(t) = &token {
        path.push_str(&format!("&token={t}"));
    }
    let mut ws = match open(ip, 8002, true, &path).await {
        Ok(w) => w,
        Err(_) => match open(ip, 8001, false, &path).await {
            Ok(w) => w,
            Err(_) => return true,
        },
    };
    if token.is_none() {
        cx.view.state = "pairing";
        cx.publish();
    }
    let mut ping = tokio::time::interval(Duration::from_secs(25));
    ping.tick().await;
    loop {
        tokio::select! {
            cmd = rx.recv() => {
                let Some(cmd) = cmd else { return false };
                match cmd {
                    Cmd::Retry | Cmd::Forget => { cx.common(&cmd).await; return false; }
                    Cmd::Act(a) => {
                        if let Some(m) = message(&a) {
                            if ws.send(Message::Text(m.to_string().into())).await.is_err() { return true; }
                        }
                    }
                    other => { cx.common(&other).await; }
                }
            }
            _ = ping.tick() => {
                if ws.send(Message::Ping(Vec::new().into())).await.is_err() { return true; }
            }
            msg = ws.next() => {
                let Some(Ok(msg)) = msg else { return true };
                let Message::Text(text) = msg else { continue };
                let Ok(v) = serde_json::from_str::<Value>(text.as_str()) else { continue };
                match v["event"].as_str().unwrap_or("") {
                    "ms.channel.connect" => {
                        let tok = match &v["data"]["token"] {
                            Value::String(s) => Some(s.clone()),
                            Value::Number(n) => Some(n.to_string()),
                            _ => None,
                        };
                        if let (Some(t), Some(c)) = (tok, cx.cfg.as_mut()) {
                            if t.bytes().all(|b| b.is_ascii_alphanumeric()) && !t.is_empty() {
                                c.key_enc = crate::dpapi::protect_str(&t).ok();
                                cx.hub.save(c);
                            }
                        }
                        cx.view.state = "on";
                        crate::log!("Samsung TV bağlandı");
                        cx.publish();
                    }
                    "ms.channel.unauthorized" | "ms.channel.timeOut" | "ms.error" if cx.view.state != "on" => {
                        // Reddedildi, zaman aşımı ya da eski jeton geçersiz: jetonu bırak, yeniden eşleşme gerekir
                        if let Some(c) = cx.cfg.as_mut() { c.key_enc = None; cx.hub.save(c); }
                        return true;
                    }
                    _ => {}
                }
            }
        }
    }
}
