//! Android TV / Google TV sürücüsü (Android TV Remote protokolü v2: 6467 eşleşme, 6466 kumanda; TLS + protobuf).
//! Eşleşme: TV ekranda 6 karakterlik kod gösterir, kullanıcı telefona yazar; sertifika (RSA) + kod ile gizli özet hesaplanır.
//! Bu sürücü gerçek bir TV ile henüz denenmedi (protokol belgelerine göre yazıldı, sahte TV ile sınandı).
use super::{insecure::AcceptAny, Act, Cmd, Ctx};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::net::Ipv4Addr;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt, ReadHalf, WriteHalf};
use tokio::net::TcpStream;
use tokio::sync::mpsc::{self, UnboundedReceiver};
use tokio::time::timeout;
use tokio_rustls::client::TlsStream;

pub(super) const APPS: &[(&str, &str)] = &[
    ("Netflix", "https://www.netflix.com/browse"),
    ("YouTube", "https://www.youtube.com"),
    ("Prime Video", "https://app.primevideo.com"),
    ("Disney+", "https://www.disneyplus.com"),
    ("Spotify", "spotify:"),
];

// ---------------------------------------------------------------- protobuf (yalnızca gereken kadar)

fn varint(mut v: u64, out: &mut Vec<u8>) {
    loop {
        let b = (v & 0x7f) as u8;
        v >>= 7;
        if v == 0 {
            out.push(b);
            return;
        }
        out.push(b | 0x80);
    }
}
fn f_varint(field: u32, v: u64) -> Vec<u8> {
    let mut o = vec![];
    varint(u64::from(field) << 3, &mut o);
    varint(v, &mut o);
    o
}
fn f_bytes(field: u32, b: &[u8]) -> Vec<u8> {
    let mut o = vec![];
    varint((u64::from(field) << 3) | 2, &mut o);
    varint(b.len() as u64, &mut o);
    o.extend_from_slice(b);
    o
}
fn cat(parts: &[Vec<u8>]) -> Vec<u8> {
    parts.concat()
}

#[derive(Clone, Debug, PartialEq)]
enum Val {
    V(u64),
    B(Vec<u8>),
}
fn read_varint(buf: &[u8], pos: &mut usize) -> Option<u64> {
    let (mut v, mut shift) = (0u64, 0);
    loop {
        let b = *buf.get(*pos)?;
        *pos += 1;
        v |= u64::from(b & 0x7f) << shift;
        if b & 0x80 == 0 {
            return Some(v);
        }
        shift += 7;
        if shift > 63 {
            return None;
        }
    }
}
fn parse(buf: &[u8]) -> Vec<(u32, Val)> {
    let (mut pos, mut out) = (0, vec![]);
    while pos < buf.len() {
        let Some(tag) = read_varint(buf, &mut pos) else { break };
        let field = (tag >> 3) as u32;
        match tag & 7 {
            0 => match read_varint(buf, &mut pos) {
                Some(v) => out.push((field, Val::V(v))),
                None => break,
            },
            2 => {
                let Some(len) = read_varint(buf, &mut pos) else { break };
                let end = pos.saturating_add(len as usize);
                if end > buf.len() {
                    break;
                }
                out.push((field, Val::B(buf[pos..end].to_vec())));
                pos = end;
            }
            1 => pos += 8,
            5 => pos += 4,
            _ => break,
        }
    }
    out
}
fn get_b(f: &[(u32, Val)], field: u32) -> Option<Vec<u8>> {
    f.iter().find_map(|(k, v)| match v {
        Val::B(b) if *k == field => Some(b.clone()),
        _ => None,
    })
}
fn get_v(f: &[(u32, Val)], field: u32) -> Option<u64> {
    f.iter().find_map(|(k, v)| match v {
        Val::V(n) if *k == field => Some(*n),
        _ => None,
    })
}

type Rd = ReadHalf<TlsStream<TcpStream>>;
type Wr = WriteHalf<TlsStream<TcpStream>>;

async fn read_frame(r: &mut Rd) -> Option<Vec<u8>> {
    let (mut len, mut shift) = (0usize, 0);
    loop {
        let b = r.read_u8().await.ok()?;
        len |= usize::from(b & 0x7f) << shift;
        if b & 0x80 == 0 {
            break;
        }
        shift += 7;
        if shift > 28 {
            return None;
        }
    }
    if len > 64 * 1024 {
        return None;
    }
    let mut buf = vec![0u8; len];
    r.read_exact(&mut buf).await.ok()?;
    Some(buf)
}
async fn write_frame(w: &mut Wr, msg: &[u8]) -> bool {
    let mut out = vec![];
    varint(msg.len() as u64, &mut out);
    out.extend_from_slice(msg);
    w.write_all(&out).await.is_ok() && w.flush().await.is_ok()
}

// ---------------------------------------------------------------- istemci kimliği (RSA sertifika)

#[derive(Serialize, Deserialize)]
struct Stored {
    cert_b64: String,
    key_enc: String,
}

pub(super) struct Ident {
    cert: Vec<u8>,
    key: Vec<u8>,
    modulus: Vec<u8>,
    exponent: Vec<u8>,
}

fn strip0(b: &[u8]) -> Vec<u8> {
    let i = b.iter().position(|x| *x != 0).unwrap_or(b.len().saturating_sub(1));
    b[i..].to_vec()
}

fn ident_from(cert: Vec<u8>, key: Vec<u8>) -> anyhow::Result<Ident> {
    use rsa::pkcs8::DecodePrivateKey;
    use rsa::traits::PublicKeyParts;
    let k = rsa::RsaPrivateKey::from_pkcs8_der(&key)?;
    Ok(Ident { cert, key, modulus: strip0(&k.n().to_bytes_be()), exponent: strip0(&k.e().to_bytes_be()) })
}

fn load_or_create_ident() -> anyhow::Result<Ident> {
    let path = crate::state::data_dir().join("atv-client.json");
    if let Some(st) = std::fs::read_to_string(&path).ok().and_then(|t| serde_json::from_str::<Stored>(&t).ok()) {
        if let (Ok(cert), Some(key)) = (B64.decode(&st.cert_b64), crate::dpapi::unprotect_str(&st.key_enc).ok().and_then(|k| B64.decode(k).ok())) {
            if let Ok(id) = ident_from(cert, key) {
                return Ok(id);
            }
        }
    }
    use rsa::pkcs8::EncodePrivateKey;
    let key = rsa::RsaPrivateKey::new(&mut rand_core06::OsRng, 2048)?;
    let der = key.to_pkcs8_der()?.as_bytes().to_vec();
    let pki = rustls::pki_types::PrivatePkcs8KeyDer::from(der.clone());
    let kp = rcgen::KeyPair::from_pkcs8_der_and_sign_algo(&pki, &rcgen::PKCS_RSA_SHA256)?;
    let mut params = rcgen::CertificateParams::new(Vec::<String>::new())?;
    params.distinguished_name.push(rcgen::DnType::CommonName, "CouchRemote");
    let cert = params.self_signed(&kp)?.der().to_vec();
    let st = Stored { cert_b64: B64.encode(&cert), key_enc: crate::dpapi::protect_str(&B64.encode(&der))? };
    std::fs::write(&path, serde_json::to_string(&st)?)?;
    ident_from(cert, der)
}

static IDENT: tokio::sync::OnceCell<Arc<Ident>> = tokio::sync::OnceCell::const_new();
async fn ident() -> Option<Arc<Ident>> {
    IDENT
        .get_or_try_init(|| async {
            tokio::task::spawn_blocking(load_or_create_ident).await.map_err(anyhow::Error::from)?.map(Arc::new)
        })
        .await
        .ok()
        .cloned()
}

/// TLS bağlantısı (istemci sertifikalı, TV sertifikası doğrulanmaz). Dönüş: akış + TV sertifikası (DER)
async fn tls(ip: Ipv4Addr, port: u16, id: &Ident) -> Option<(TlsStream<TcpStream>, Vec<u8>)> {
    let tcp = timeout(Duration::from_secs(3), TcpStream::connect((ip, port))).await.ok()?.ok()?;
    let _ = tcp.set_nodelay(true);
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let cfg = rustls::ClientConfig::builder_with_provider(provider.clone())
        .with_safe_default_protocol_versions()
        .ok()?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(AcceptAny(provider)))
        .with_client_auth_cert(
            vec![rustls::pki_types::CertificateDer::from(id.cert.clone())],
            rustls::pki_types::PrivateKeyDer::Pkcs8(rustls::pki_types::PrivatePkcs8KeyDer::from(id.key.clone())),
        )
        .ok()?;
    let name = rustls::pki_types::ServerName::IpAddress(std::net::IpAddr::V4(ip).into());
    let s = timeout(Duration::from_secs(4), tokio_rustls::TlsConnector::from(Arc::new(cfg)).connect(name, tcp)).await.ok()?.ok()?;
    let server = s.get_ref().1.peer_certificates()?.first()?.as_ref().to_vec();
    Some((s, server))
}

fn server_rsa(cert: &[u8]) -> Option<(Vec<u8>, Vec<u8>)> {
    use x509_parser::public_key::PublicKey;
    let (_, c) = x509_parser::parse_x509_certificate(cert).ok()?;
    match c.public_key().parsed().ok()? {
        PublicKey::RSA(r) => Some((strip0(r.modulus), strip0(r.exponent))),
        _ => None,
    }
}

/// Eşleşme sırrı: SHA256(istemci modül + üs + sunucu modül + üs + kodun ilk bayttan sonrası)
fn secret(id: &Ident, server: &(Vec<u8>, Vec<u8>), code: &[u8]) -> Vec<u8> {
    let mut h = Sha256::new();
    h.update(&id.modulus);
    h.update(&id.exponent);
    h.update(&server.0);
    h.update(&server.1);
    h.update(&code[1..]);
    h.finalize().to_vec()
}

fn hex_code(s: &str) -> Option<Vec<u8>> {
    if s.len() != 6 || !s.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    (0..3).map(|i| u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).ok()).collect()
}

// ---------------------------------------------------------------- eşleşme (6467)

fn pm(inner_field: u32, inner: &[u8]) -> Vec<u8> {
    cat(&[f_varint(1, 2), f_varint(2, 200), f_bytes(inner_field, inner)])
}

enum Pair {
    Done,
    /// Kullanıcı vazgeçti / yeni komut geldi
    Abort,
    Failed,
}

async fn pair(ip: Ipv4Addr, cx: &mut Ctx, rx: &mut UnboundedReceiver<Cmd>, id: &Ident) -> Pair {
    let Some((s, server_cert)) = tls(ip, 6467, id).await else { return Pair::Failed };
    let Some(srv) = server_rsa(&server_cert) else { return Pair::Failed };
    let (mut r, mut w) = tokio::io::split(s);
    let enc = cat(&[f_varint(1, 3), f_varint(2, 6)]);
    let step = |x: Option<Vec<u8>>, field: u32| x.map(|b| parse(&b)).filter(|f| get_b(f, field).is_some());
    if !write_frame(&mut w, &pm(10, &cat(&[f_bytes(1, b"atvremote"), f_bytes(2, b"CouchRemote")]))).await { return Pair::Failed; }
    if step(timeout(Duration::from_secs(5), read_frame(&mut r)).await.ok().flatten(), 11).is_none() { return Pair::Failed; }
    if !write_frame(&mut w, &pm(20, &cat(&[f_bytes(1, &enc), f_varint(3, 1)]))).await { return Pair::Failed; }
    if step(timeout(Duration::from_secs(5), read_frame(&mut r)).await.ok().flatten(), 20).is_none() { return Pair::Failed; }
    if !write_frame(&mut w, &pm(30, &cat(&[f_bytes(1, &enc), f_varint(2, 1)]))).await { return Pair::Failed; }
    if step(timeout(Duration::from_secs(5), read_frame(&mut r)).await.ok().flatten(), 31).is_none() { return Pair::Failed; }

    // TV ekranda kodu gösteriyor: telefondan girilmesini bekle
    cx.view.state = "code";
    cx.publish();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(150);
    loop {
        let cmd = match tokio::time::timeout_at(deadline, rx.recv()).await {
            Ok(Some(c)) => c,
            Ok(None) | Err(_) => return Pair::Failed,
        };
        match cmd {
            Cmd::PairCode(code) => {
                let Some(bytes) = hex_code(&code) else { continue };
                let sec = secret(id, &srv, &bytes);
                if sec[0] != bytes[0] {
                    continue; // yanlış kod: yeni kod beklenir
                }
                if !write_frame(&mut w, &pm(40, &f_bytes(1, &sec))).await { return Pair::Failed; }
                return match timeout(Duration::from_secs(8), read_frame(&mut r)).await.ok().flatten().map(|b| parse(&b)) {
                    Some(f) if get_b(&f, 41).is_some() && get_v(&f, 2).unwrap_or(200) == 200 => Pair::Done,
                    _ => Pair::Failed,
                };
            }
            Cmd::Retry | Cmd::Forget => {
                cx.common(&cmd).await;
                return Pair::Abort;
            }
            _ => {}
        }
    }
}

// ---------------------------------------------------------------- kumanda (6466)

fn key_code(name: &str) -> Option<u64> {
    Some(match name {
        "UP" => 19, "DOWN" => 20, "LEFT" => 21, "RIGHT" => 22, "ENTER" => 23, "BACK" | "EXIT" => 4, "HOME" => 3, "MENU" => 82, "INFO" => 165,
        "RED" => 183, "GREEN" => 184, "YELLOW" => 185, "BLUE" => 186,
        "MUTE" => 164, "VOLUMEUP" => 24, "VOLUMEDOWN" => 25, "CHANNELUP" => 166, "CHANNELDOWN" => 167,
        d if d.len() == 1 && d.as_bytes()[0].is_ascii_digit() => 7 + u64::from(d.as_bytes()[0] - b'0'),
        _ => return None,
    })
}

fn act_msg(a: &Act) -> Option<Vec<u8>> {
    let code = match a {
        Act::Off => 26,
        Act::VolUp => 24,
        Act::VolDown => 25,
        Act::Mute(_) => 164,
        Act::ChUp => 166,
        Act::ChDown => 167,
        Act::Play => 126,
        Act::Pause => 127,
        Act::Stop => 86,
        Act::Rew => 89,
        Act::Ff => 90,
        Act::Enter => 66,
        Act::Backspace => 67,
        Act::Key(n) => key_code(n)?,
        Act::Launch(link) => return Some(f_bytes(90, &f_bytes(1, link.as_bytes()))),
        Act::OpenLink { url, .. } => return Some(f_bytes(90, &f_bytes(1, url.as_bytes()))),
        _ => return None,
    };
    Some(f_bytes(10, &cat(&[f_varint(1, code), f_varint(2, 3)])))
}

async fn remote(ip: Ipv4Addr, cx: &mut Ctx, rx: &mut UnboundedReceiver<Cmd>, id: &Ident) -> bool {
    let Some((s, _)) = tls(ip, 6466, id).await else { return true };
    let (mut r, mut w) = tokio::io::split(s);
    let (ftx, mut frx) = mpsc::unbounded_channel::<Vec<u8>>();
    let reader = tokio::spawn(async move {
        while let Some(f) = read_frame(&mut r).await {
            if ftx.send(f).is_err() {
                break;
            }
        }
    });
    let mut got_any = false;
    let first = tokio::time::Instant::now() + Duration::from_secs(6);
    let result = loop {
        tokio::select! {
            cmd = rx.recv() => {
                let Some(cmd) = cmd else { break false };
                match cmd {
                    Cmd::Retry | Cmd::Forget => { cx.common(&cmd).await; break false; }
                    Cmd::Act(a) => {
                        if let Some(m) = act_msg(&a) {
                            if !write_frame(&mut w, &m).await { break true; }
                        }
                    }
                    other => { cx.common(&other).await; }
                }
            }
            frame = frx.recv() => {
                let Some(frame) = frame else { break true };
                got_any = true;
                let f = parse(&frame);
                if get_b(&f, 1).is_some() {
                    // TV yapılandırma yolladı: cihaz bilgisiyle yanıtla
                    let info = cat(&[f_bytes(1, b"Couch"), f_bytes(2, b"CouchRemote"), f_varint(3, 1), f_bytes(4, b"1"), f_bytes(5, b"couch.remote"), f_bytes(6, b"1.0.0")]);
                    if !write_frame(&mut w, &f_bytes(1, &cat(&[f_varint(1, 622), f_bytes(2, &info)]))).await { break true; }
                } else if get_b(&f, 2).is_some() {
                    if !write_frame(&mut w, &f_bytes(2, &f_varint(1, 622))).await { break true; }
                    cx.view.state = "on";
                    crate::log!("Android TV bağlandı");
                    cx.publish();
                } else if let Some(p) = get_b(&f, 8) {
                    let v = get_v(&parse(&p), 1).unwrap_or(0);
                    if !write_frame(&mut w, &f_bytes(9, &f_varint(1, v))).await { break true; }
                } else if let Some(p) = get_b(&f, 50) {
                    let vf = parse(&p);
                    if let (Some(max), Some(level)) = (get_v(&vf, 6), get_v(&vf, 7)) {
                        if max > 0 { cx.view.volume = Some((level * 100 / max) as i64); }
                    }
                    cx.view.muted = get_v(&vf, 8).unwrap_or(0) != 0;
                    cx.publish();
                }
            }
            _ = tokio::time::sleep_until(first), if !got_any => {
                // TLS açıldı ama TV hiçbir şey yollamadı: eşleşme silinmiş (TV sıfırlanmış) olabilir
                if let Some(c) = cx.cfg.as_mut() { c.key_enc = None; cx.hub.save(c); }
                break true;
            }
        }
    };
    reader.abort();
    result
}

pub(super) async fn session(ip: Ipv4Addr, cx: &mut Ctx, rx: &mut UnboundedReceiver<Cmd>) -> bool {
    let Some(id) = ident().await else { return true };
    if cx.cfg.as_ref().and_then(|c| c.key()).is_none() {
        cx.view.state = "pairing";
        cx.publish();
        match pair(ip, cx, rx, &id).await {
            Pair::Done => {
                if let Some(c) = cx.cfg.as_mut() {
                    c.key_enc = crate::dpapi::protect_str("paired").ok();
                    cx.hub.save(c);
                }
            }
            Pair::Abort => return false,
            Pair::Failed => return true,
        }
    }
    if cx.cfg.as_ref().is_some_and(|c| c.mac.is_none()) {
        if let Some(mac) = super::arp_mac(ip) {
            if let Some(c) = cx.cfg.as_mut() {
                c.mac = Some(mac);
                cx.hub.save(c);
            }
        }
    }
    remote(ip, cx, rx, &id).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protobuf_gidis_donus() {
        let m = cat(&[f_varint(1, 300), f_bytes(2, b"merhaba"), f_varint(3, 0)]);
        let f = parse(&m);
        assert_eq!(get_v(&f, 1), Some(300));
        assert_eq!(get_b(&f, 2).as_deref(), Some(&b"merhaba"[..]));
        assert_eq!(get_v(&f, 3), Some(0));
    }

    #[test]
    fn kod_ve_sir() {
        assert_eq!(hex_code("1A2b3C"), Some(vec![0x1a, 0x2b, 0x3c]));
        assert!(hex_code("12345").is_none());
        assert!(hex_code("12345Z").is_none());
        let id = Ident { cert: vec![], key: vec![], modulus: vec![1, 2], exponent: vec![1, 0, 1] };
        let s = secret(&id, &(vec![3, 4], vec![1, 0, 1]), &[0x1a, 0x2b, 0x3c]);
        let mut h = Sha256::new();
        h.update([1u8, 2, 1, 0, 1, 3, 4, 1, 0, 1, 0x2b, 0x3c]);
        assert_eq!(s, h.finalize().to_vec());
    }

    #[test]
    fn tus_kodlari() {
        assert_eq!(key_code("UP"), Some(19));
        assert_eq!(key_code("5"), Some(12));
        assert_eq!(key_code("POWER"), None);
    }
}
