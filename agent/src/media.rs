//! "Şu an çalıyor": Windows medya oturumu (SMTC) — Spotify, tarayıcılar (YouTube, Netflix), VLC, Medya Oynatıcı…
//! Ayrı bir thread her saniye durumu okur ve bağlı telefonlara yayınlar. Komutlar (oynat/duraklat, ileri, sarma)
//! doğrudan oturuma gider: pencere öne gelmeden çalışır. Oturum yoksa medya tuşlarına geri düşülür.
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use serde_json::{json, Value};
use std::sync::mpsc::{channel, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::time::Duration;
use windows::Media::Control::{
    GlobalSystemMediaTransportControlsSession as Session,
    GlobalSystemMediaTransportControlsSessionManager as Manager,
    GlobalSystemMediaTransportControlsSessionPlaybackStatus as Status,
};
use windows::Storage::Streams::DataReader;

use crate::events::Events;
use crate::input::{vk, Cmd, Input};

const TICKS_PER_SEC: f64 = 10_000_000.0;
const MAX_THUMB_BYTES: u32 = 300 * 1024;
/// DateTime (1601'den beri 100 ns) ile Unix zamanı arasındaki fark
const EPOCH_DIFF_TICKS: i64 = 116_444_736_000_000_000;

pub enum MediaCmd {
    PlayPause,
    Next,
    Prev,
    /// Mutlak konum (saniye)
    Seek(f64),
    /// Göreli sarma (saniye)
    SeekBy(f64),
}

#[derive(Clone)]
pub struct Media {
    tx: Sender<MediaCmd>,
}

impl Media {
    /// Son durum (kapak dahil) olaylarda saklanır; telefona kapak sadece parça değişince gönderilir
    pub fn start(input: Input, events: Arc<Events>) -> Media {
        let (tx, rx) = channel::<MediaCmd>();
        std::thread::Builder::new()
            .name("medya".into())
            .spawn(move || {
                let manager = match Manager::RequestAsync().and_then(|op| op.join()) {
                    Ok(m) => m,
                    Err(e) => {
                        crate::log!("Medya oturumu yöneticisi açılamadı: {e}");
                        // Yine de komutları medya tuşlarıyla karşıla
                        while let Ok(cmd) = rx.recv() {
                            fallback(&input, cmd);
                        }
                        return;
                    }
                };
                let mut last_key = String::new();
                let mut thumb: Option<String> = None;
                loop {
                    match rx.recv_timeout(Duration::from_secs(1)) {
                        Ok(cmd) => {
                            let session = manager.GetCurrentSession().ok();
                            if !session.as_ref().is_some_and(|s| command(s, &cmd).unwrap_or(false)) {
                                fallback(&input, cmd);
                            }
                            std::thread::sleep(Duration::from_millis(150)); // oturum durumu güncellensin
                        }
                        Err(RecvTimeoutError::Timeout) => {}
                        Err(RecvTimeoutError::Disconnected) => return,
                    }
                    let current = manager.GetCurrentSession().ok();
                    let (mut info, key) = match current.as_ref().and_then(|s| read(s).ok()) {
                        Some(x) => x,
                        None => (json!({ "active": false }), String::new()),
                    };
                    let session = !key.is_empty();
                    let mut send = info.clone();
                    if session && key != last_key {
                        // Parça değişti: kapak görselini bir kez oku ve gönder
                        thumb = current.as_ref().and_then(read_thumb);
                        send["thumb"] = json!(thumb);
                        last_key = key;
                    } else if !session {
                        thumb = None;
                        last_key.clear();
                    }
                    info["thumb"] = json!(thumb);
                    events.publish_split("media", info, send);
                }
            })
            .expect("medya thread'i");
        Media { tx }
    }

    pub fn send(&self, cmd: MediaCmd) {
        let _ = self.tx.send(cmd);
    }
}

fn fallback(input: &Input, cmd: MediaCmd) {
    match cmd {
        MediaCmd::PlayPause => input.send(Cmd::TapKey(vk::MEDIA_PLAY_PAUSE)),
        MediaCmd::Next => input.send(Cmd::TapKey(vk::MEDIA_NEXT)),
        MediaCmd::Prev => input.send(Cmd::TapKey(vk::MEDIA_PREV)),
        // Çoğu oynatıcıda ← / → sarar
        MediaCmd::SeekBy(d) => input.send(Cmd::TapKey(if d < 0.0 { 0x25 } else { 0x27 })),
        MediaCmd::Seek(_) => {}
    }
}

fn command(s: &Session, cmd: &MediaCmd) -> windows::core::Result<bool> {
    Ok(match cmd {
        MediaCmd::PlayPause => s.TryTogglePlayPauseAsync()?.join()?,
        MediaCmd::Next => s.TrySkipNextAsync()?.join()?,
        MediaCmd::Prev => s.TrySkipPreviousAsync()?.join()?,
        MediaCmd::Seek(sec) => s.TryChangePlaybackPositionAsync((sec.max(0.0) * TICKS_PER_SEC) as i64)?.join()?,
        MediaCmd::SeekBy(d) => {
            let (pos, end) = position(s)?;
            let target = (pos + d).clamp(0.0, if end > 0.0 { end } else { f64::MAX });
            s.TryChangePlaybackPositionAsync((target * TICKS_PER_SEC) as i64)?.join()?
        }
    })
}

fn now_ticks() -> i64 {
    let unix = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    EPOCH_DIFF_TICKS + (unix.as_nanos() / 100) as i64
}

/// Şu anki konum ve toplam süre (saniye). Oturum konumu son güncelleme anına aittir; çalıyorsa geçen süre eklenir.
fn position(s: &Session) -> windows::core::Result<(f64, f64)> {
    let t = s.GetTimelineProperties()?;
    let mut pos = t.Position()?.Duration as f64 / TICKS_PER_SEC;
    let end = t.EndTime()?.Duration as f64 / TICKS_PER_SEC;
    let playing = s.GetPlaybackInfo()?.PlaybackStatus()? == Status::Playing;
    if playing {
        let elapsed = (now_ticks() - t.LastUpdatedTime()?.UniversalTime) as f64 / TICKS_PER_SEC;
        if (0.0..86_400.0).contains(&elapsed) {
            pos += elapsed;
        }
    }
    Ok((if end > 0.0 { pos.min(end) } else { pos }, end))
}

/// Oturum bilgisi + parçayı tanımlayan anahtar (kapak görselini sadece değişince okumak için)
fn read(s: &Session) -> windows::core::Result<(Value, String)> {
    let props = s.TryGetMediaPropertiesAsync()?.join()?;
    let title = props.Title()?.to_string();
    let artist = props.Artist()?.to_string();
    let app = s.SourceAppUserModelId()?.to_string();
    let status = s.GetPlaybackInfo()?.PlaybackStatus()?;
    let (pos, end) = position(s).unwrap_or((0.0, 0.0));
    let key = format!("{app}\u{1}{title}\u{1}{artist}");
    Ok((
        json!({
            "active": true, "title": title, "artist": artist, "app": app_name(&app),
            "playing": status == Status::Playing, "position": pos, "duration": end,
        }),
        key,
    ))
}

/// "Spotify.exe", "Chrome", "308046B0AF4A39CB" gibi kimliklerden okunabilir ad
fn app_name(id: &str) -> String {
    let lower = id.to_lowercase();
    for (needle, name) in [
        ("spotify", "Spotify"), ("chrome", "Chrome"), ("msedge", "Edge"), ("firefox", "Firefox"), ("opera", "Opera"),
        ("brave", "Brave"), ("vlc", "VLC"), ("netflix", "Netflix"), ("zunemusic", "Medya Oynatıcı"), ("zunevideo", "Filmler ve TV"),
        ("308046b0af4a39cb", "Firefox"),
    ] {
        if lower.contains(needle) {
            return name.into();
        }
    }
    id.split(['!', '\\']).next_back().unwrap_or(id).trim_end_matches(".exe").into()
}

fn read_thumb(s: &Session) -> Option<String> {
    let props = s.TryGetMediaPropertiesAsync().ok()?.join().ok()?;
    let stream = props.Thumbnail().ok()?.OpenReadAsync().ok()?.join().ok()?;
    let size = stream.Size().ok()?.min(MAX_THUMB_BYTES as u64) as u32;
    if size == 0 {
        return None;
    }
    let ctype = stream.ContentType().map(|c| c.to_string()).unwrap_or_default();
    let reader = DataReader::CreateDataReader(&stream).ok()?;
    let loaded = reader.LoadAsync(size).ok()?.join().ok()?;
    let mut buf = vec![0u8; loaded as usize];
    reader.ReadBytes(&mut buf).ok()?;
    let mime = if ctype.starts_with("image/") { ctype } else { "image/png".into() };
    Some(format!("data:{mime};base64,{}", B64.encode(buf)))
}
