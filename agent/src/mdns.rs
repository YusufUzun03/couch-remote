//! mDNS: programın kendi sabit adını (couch-xxxx.local) yerel ağa duyurur.
//! Windows bilgisayar adına güvenmiyoruz: ad değişirse web uygulamasının adresi (origin) değişir ve eşleşme kaybolur.
//!
//! Küçük, kendi yanıtlayıcımız (RFC 6762): hazır kütüphaneler Windows'un "legacy unicast" sorgularını
//! (5353 dışı porttan gelen, yanıtı doğrudan kaynağa bekleyen) yanıtlamıyor.
use anyhow::Result;
use socket2::{Domain, Protocol, Socket, Type};
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket};

const MDNS_ADDR: Ipv4Addr = Ipv4Addr::new(224, 0, 0, 251);
const MDNS_PORT: u16 = 5353;
const TYPE_A: u16 = 1;
const TYPE_AAAA: u16 = 28;
const TYPE_NSEC: u16 = 47;
const TYPE_ANY: u16 = 255;
const CLASS_IN: u16 = 1;
const CACHE_FLUSH: u16 = 0x8000;
const TTL: u32 = 120;
const LEGACY_TTL: u32 = 10; // RFC 6762 §6.7

/// Yerel ağ IPv4 adresini seçer. COUCH_IP ortam değişkeniyle zorlanabilir.
pub fn pick_lan_ip() -> String {
    if let Ok(ip) = std::env::var("COUCH_IP") {
        return ip;
    }
    let mut best: Option<(i32, String)> = None;
    for iface in if_addrs::get_if_addrs().unwrap_or_default() {
        let std::net::IpAddr::V4(ip) = iface.ip() else { continue };
        if iface.is_loopback() || !ip.is_private() {
            continue;
        }
        let name = iface.name.to_lowercase();
        // Sanal adaptörleri (Hyper-V, VirtualBox, WSL, VPN) geri plana at
        let virtual_ = ["vethernet", "virtualbox", "vmware", "wsl", "hyper-v", "tailscale", "zerotier", "vpn"]
            .iter()
            .any(|v| name.contains(v));
        let wired = name.contains("ethernet") && !virtual_;
        let score = if virtual_ { 0 } else { 10 } + if wired { 5 } else { 0 };
        if best.as_ref().is_none_or(|(s, _)| score > *s) {
            best = Some((score, ip.to_string()));
        }
    }
    best.map(|(_, ip)| ip).unwrap_or_else(|| "127.0.0.1".into())
}

fn encode_name(out: &mut Vec<u8>, name: &str) {
    for label in name.trim_end_matches('.').split('.') {
        out.push(label.len() as u8);
        out.extend_from_slice(label.as_bytes());
    }
    out.push(0);
}

/// Sıkıştırma işaretçilerini izleyerek adı okur; `pos` adın bittiği yere ilerletilir.
fn read_name(msg: &[u8], pos: &mut usize) -> Option<String> {
    let mut labels = Vec::new();
    let mut p = *pos;
    let mut jumped = false;
    for _ in 0..64 {
        let len = *msg.get(p)? as usize;
        if len == 0 {
            if !jumped {
                *pos = p + 1;
            }
            return Some(labels.join("."));
        }
        if len & 0xC0 == 0xC0 {
            let ptr = ((len & 0x3F) << 8) | *msg.get(p + 1)? as usize;
            if !jumped {
                *pos = p + 2;
            }
            jumped = true;
            p = ptr;
            continue;
        }
        labels.push(String::from_utf8_lossy(msg.get(p + 1..p + 1 + len)?).into_owned());
        p += 1 + len;
    }
    None
}

struct Question {
    raw: Vec<u8>, // yanıtta aynen geri gönderilir (legacy unicast)
    qtype: u16,
    /// QU biti: soran taraf yanıtın doğrudan kendisine gönderilmesini istiyor
    unicast: bool,
}

/// Sorgu bizim adımızın A kaydını istiyor mu? İstiyorsa ilgili soruları döner.
fn matching_questions(msg: &[u8], host: &str) -> Option<(u16, Vec<Question>)> {
    if msg.len() < 12 || msg[2] & 0x80 != 0 {
        return None; // yanıt paketi
    }
    let id = u16::from_be_bytes([msg[0], msg[1]]);
    let qd = u16::from_be_bytes([msg[4], msg[5]]);
    let mut pos = 12;
    let mut found = Vec::new();
    for _ in 0..qd {
        let start = pos;
        let name = read_name(msg, &mut pos)?;
        let qtype = u16::from_be_bytes([*msg.get(pos)?, *msg.get(pos + 1)?]);
        let qclass = u16::from_be_bytes([*msg.get(pos + 2)?, *msg.get(pos + 3)?]);
        pos += 4;
        if name.eq_ignore_ascii_case(host) && matches!(qtype, TYPE_A | TYPE_ANY | TYPE_AAAA) {
            let mut raw = Vec::new();
            encode_name(&mut raw, host);
            raw.extend_from_slice(&qtype.to_be_bytes());
            raw.extend_from_slice(&CLASS_IN.to_be_bytes());
            let _ = start;
            found.push(Question { raw, qtype, unicast: qclass & 0x8000 != 0 });
        }
    }
    (!found.is_empty()).then_some((id, found))
}

fn record(out: &mut Vec<u8>, host: &str, rtype: u16, legacy: bool, rdata: &[u8]) {
    encode_name(out, host);
    out.extend_from_slice(&rtype.to_be_bytes());
    let class = if legacy { CLASS_IN } else { CLASS_IN | CACHE_FLUSH };
    out.extend_from_slice(&class.to_be_bytes());
    out.extend_from_slice(&(if legacy { LEGACY_TTL } else { TTL }).to_be_bytes());
    out.extend_from_slice(&(rdata.len() as u16).to_be_bytes());
    out.extend_from_slice(rdata);
}

/// NSEC (RFC 6762 §6.1): "bu adın sadece A kaydı var" — IPv6 (AAAA) soran istemci boşuna beklemez.
fn nsec_rdata(host: &str) -> Vec<u8> {
    let mut rd = Vec::new();
    encode_name(&mut rd, host); // sonraki ad = kendisi
    rd.extend_from_slice(&[0, 1, 0b0100_0000]); // pencere 0, 1 baytlık bit haritası, sadece tip 1 (A)
    rd
}

/// `echo` boşsa kendiliğinden duyuru; AAAA sorulduysa cevap NSEC, A ek bölümde; aksi halde tersi.
fn response(host: &str, ip: Ipv4Addr, id: u16, echo: &[Question], legacy: bool) -> Vec<u8> {
    let wants_a = echo.is_empty() || echo.iter().any(|q| q.qtype != TYPE_AAAA);
    let mut out = Vec::with_capacity(96);
    out.extend_from_slice(&id.to_be_bytes());
    out.extend_from_slice(&0x8400u16.to_be_bytes()); // yanıt + yetkili
    let qd = if legacy { echo.len() as u16 } else { 0 };
    out.extend_from_slice(&qd.to_be_bytes());
    out.extend_from_slice(&1u16.to_be_bytes()); // 1 cevap
    out.extend_from_slice(&0u16.to_be_bytes()); // yetki bölümü yok
    out.extend_from_slice(&1u16.to_be_bytes()); // 1 ek kayıt
    if legacy {
        for q in echo {
            out.extend_from_slice(&q.raw);
        }
    }
    let nsec = nsec_rdata(host);
    if wants_a {
        record(&mut out, host, TYPE_A, legacy, &ip.octets());
        record(&mut out, host, TYPE_NSEC, legacy, &nsec);
    } else {
        record(&mut out, host, TYPE_NSEC, legacy, &nsec);
        record(&mut out, host, TYPE_A, legacy, &ip.octets());
    }
    out
}

pub struct Responder;

const WATCH_EVERY: std::time::Duration = std::time::Duration::from_secs(5);
const REANNOUNCE_EVERY: std::time::Duration = std::time::Duration::from_secs(60);

fn open_socket() -> Result<UdpSocket> {
    let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
    socket.set_reuse_address(true)?; // Windows'un kendi mDNS istemcisiyle portu paylaş
    socket.bind(&SocketAddr::from((Ipv4Addr::UNSPECIFIED, MDNS_PORT)).into())?;
    socket.set_multicast_loop_v4(true)?; // bu bilgisayardaki çözümleyici de duysun
    socket.set_multicast_ttl_v4(255)?;
    let socket: UdpSocket = socket.into();
    // recv_from düzenli aralıkla uyansın: IP / ağ değişimini izleyebilmek için
    socket.set_read_timeout(Some(WATCH_EVERY))?;
    Ok(socket)
}

/// Çoklu yayın grubuna `ip` arayüzünden (yeniden) katılır. Zaten üyeyse hata döner, sorun değil.
/// Dönüş: üyelik şimdi (yeniden) kuruldu mu? Uyku / ağ yenilenmesi üyeliği sessizce düşürür.
fn ensure_membership(socket: &UdpSocket, ip: Ipv4Addr) -> bool {
    let joined = socket2::SockRef::from(socket).join_multicast_v4(&MDNS_ADDR, &ip).is_ok();
    let _ = socket2::SockRef::from(socket).set_multicast_if_v4(&ip);
    joined
}

/// `host` adının A kaydını yanıtlayan arka plan thread'ini başlatır.
/// Bilgisayar uykudan uyanınca, ağ değişince veya program ağdan önce açılınca da çalışmaya devam eder:
/// IP adresi ve grup üyeliği sürekli denetlenir, değişirse ad yeniden duyurulur.
pub fn start(host: &str, _ip: &str) -> Result<Responder> {
    let host = host.trim_end_matches('.').to_string();
    let group = SocketAddr::V4(SocketAddrV4::new(MDNS_ADDR, MDNS_PORT));
    let debug = std::env::var_os("COUCH_MDNS_DEBUG").is_some();
    let mut socket = open_socket()?;

    std::thread::Builder::new().name("mdns".into()).spawn(move || {
        let mut buf = [0u8; 1500];
        let mut ip: Option<Ipv4Addr> = None;
        let mut last_announce = std::time::Instant::now() - REANNOUNCE_EVERY;
        let mut last_watch = std::time::Instant::now() - WATCH_EVERY;
        loop {
            // 1) ağ durumu: IP değişti mi, üyelik düştü mü? (yoğun trafikte her pakette değil, 5 sn'de bir)
            if last_watch.elapsed() >= WATCH_EVERY {
            last_watch = std::time::Instant::now();
            let now_ip = pick_lan_ip().parse::<Ipv4Addr>().ok().filter(|i| !i.is_loopback());
            if let Some(cur) = now_ip {
                let changed = ip != Some(cur);
                let rejoined = ensure_membership(&socket, cur);
                if changed {
                    crate::log!("mDNS: {host} → {cur}");
                }
                if (changed || rejoined || last_announce.elapsed() >= REANNOUNCE_EVERY) && !host.is_empty() {
                    let _ = socket.send_to(&response(&host, cur, 0, &[], false), group);
                    last_announce = std::time::Instant::now();
                }
                ip = Some(cur);
            } else if ip.take().is_some() {
                crate::log!("mDNS: ağ bağlantısı yok, bekleniyor");
            }
            }

            // 2) sorgu bekle (en çok WATCH_EVERY kadar)
            let (n, src) = match socket.recv_from(&mut buf) {
                Ok(v) => v,
                Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => continue,
                Err(_) => {
                    // Soket bozuldu (ağ kartı kapandı vb.): yenisini aç
                    std::thread::sleep(std::time::Duration::from_secs(2));
                    if let Ok(s) = open_socket() {
                        socket = s;
                        ip = None;
                    }
                    continue;
                }
            };
            let Some(ip) = ip else { continue };
            let matched = matching_questions(&buf[..n], &host);
            if debug && buf[2] & 0x80 == 0 {
                let mut pos = 12;
                let name = read_name(&buf[..n], &mut pos).unwrap_or_default();
                let types: Vec<u16> = matched.as_ref().map(|(_, q)| q.iter().map(|q| q.qtype).collect()).unwrap_or_default();
                crate::log!("mDNS sorgu {src}: {name} → {}", if matched.is_some() { format!("yanıtlandı {types:?}") } else { "bizim değil".into() });
            }
            let Some((id, questions)) = matched else { continue };
            if src.port() != MDNS_PORT {
                // legacy unicast: yanıt doğrudan kaynağa, soru geri gönderilir, kimlik korunur
                let _ = socket.send_to(&response(&host, ip, id, &questions, true), src);
            } else {
                let answer = response(&host, ip, 0, &questions, false);
                if questions.iter().any(|q| q.unicast) {
                    let _ = socket.send_to(&answer, src);
                }
                let _ = socket.send_to(&answer, group);
            }
        }
    })?;
    Ok(Responder)
}
