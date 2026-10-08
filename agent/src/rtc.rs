//! WebRTC veri kanalı (isteğe bağlı, deneysel): hareket/kaydırma paketleri için UDP.
//!
//! Neden: TCP (WebSocket) tek bir paket kaybolunca arkadaki tüm paketleri bekletir; zayıf Wi-Fi'da imleç takılır.
//! "Sırasız, yeniden göndermesiz" veri kanalı kaybolan paketi bekletmez; sıradaki hareket zaten yenisini taşır.
//!
//! Güvenlik (ayrıntı: SECURITY.md):
//!  - UDP soketi YALNIZCA bir telefon ayarı açıp teklif gönderince açılır, son oturum bitince kapanır; yalnızca LAN adresine bağlanır.
//!  - Oturum yalnızca kimliği doğrulanmış WSS bağlantısı üzerinden gelen SDP ile kurulur (ICE parolası ve DTLS parmak izi o kanaldan geçer).
//!  - Her oturum, kendisini kuran WSS bağlantısının IP adresine kilitlidir: başka bir adresten gelen paket ayrıştırılmadan atılır.
//!  - Kanaldan yalnızca küçük hareket/ping paketleri kabul edilir (komut yok), kütüphane hataları (panik) tek oturumu kapatır.
//!  - Kanal 15 sn içinde açılmazsa oturum silinir; en fazla 8 oturum.
//! Bağlantı kurulamazsa ya da kopsa telefon WebSocket'e döner (hiçbir şey kaybolmaz).
//!
//! Tek bir iş parçacığı tüm oturumları yürütür; oturum yokken uyur (boşta CPU kullanımı sıfır).
use crate::app::App;
use crate::sync::LockExt;
use crate::ws::{process_packet, Pkt};
use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr, UdpSocket};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use str0m::change::SdpOffer;
use str0m::channel::ChannelId;
use str0m::net::{Protocol, Receive};
use str0m::{Candidate, Event, IceConnectionState, Input, Output, Rtc};

const MAX_SESSIONS: usize = 8;
const MAX_PACKET: usize = 64;
/// Kanal bu süre içinde açılmazsa oturum silinir (telefon 8 sn sonra zaten vazgeçer)
const OPEN_DEADLINE: Duration = Duration::from_secs(15);

enum Cmd {
    Offer { sdp: String, peer: IpAddr, pkt: Arc<Mutex<Pkt>>, app: Arc<App>, reply: tokio::sync::oneshot::Sender<Result<(u64, String), String>> },
    Close(u64),
}

pub struct Hub {
    tx: mpsc::Sender<Cmd>,
    wake: UdpSocket,
    /// Soket açıkken dinlenen adres (uyandırma paketi buraya gider); kapalıyken None
    open_at: Arc<Mutex<Option<SocketAddr>>>,
}

impl Hub {
    /// İş parçacığını başlatır (boşta uyur). UDP soketi burada AÇILMAZ: ilk teklif gelince açılır.
    pub fn start() -> Option<Hub> {
        let open_at = Arc::new(Mutex::new(None));
        let (tx, rx) = mpsc::channel();
        let shared = open_at.clone();
        let adv = SocketAddr::from(([0, 0, 0, 0], crate::https_port()));
        std::thread::Builder::new().name("rtc".into()).spawn(move || Runner { adv, open_at: shared, sessions: HashMap::new(), socket: None, next_id: 1 }.run(rx)).ok()?;
        let wake = UdpSocket::bind("0.0.0.0:0").ok()?;
        Some(Hub { tx, wake, open_at })
    }

    fn send(&self, c: Cmd) -> bool {
        let ok = self.tx.send(c).is_ok();
        if let Some(to) = *self.open_at.lk() {
            let _ = self.wake.send_to(&[0], to); // soketi bekleyen iş parçacığını uyandırır
        }
        ok
    }

    /// Telefonun SDP önerisini kabul eder; (oturum no, SDP yanıtı) döner.
    pub async fn offer(&self, app: Arc<App>, pkt: Arc<Mutex<Pkt>>, peer: IpAddr, sdp: String) -> Result<(u64, String), String> {
        let (reply, rx) = tokio::sync::oneshot::channel();
        if !self.send(Cmd::Offer { sdp, peer, pkt, app, reply }) {
            return Err("WebRTC çalışmıyor".into());
        }
        rx.await.map_err(|_| "WebRTC yanıt vermedi".to_string())?
    }

    pub fn close(&self, id: u64) {
        self.send(Cmd::Close(id));
    }
}

struct Sess {
    rtc: Rtc,
    app: Arc<App>,
    pkt: Arc<Mutex<Pkt>>,
    peer: IpAddr,
    cid: Option<ChannelId>,
    name: String,
    created: Instant,
    dead: bool,
}

impl Sess {
    /// Kütüphanede bir hata (panik) olursa yalnızca bu oturum kapanır, iş parçacığı ve program etkilenmez
    fn guarded<R>(&mut self, f: impl FnOnce(&mut Sess) -> R) -> Option<R> {
        match catch_unwind(AssertUnwindSafe(|| f(self))) {
            Ok(r) => Some(r),
            Err(_) => {
                log!("WebRTC oturumu beklenmedik hata verdi, kapatıldı: {}", self.name);
                self.dead = true;
                None
            }
        }
    }

    fn alive(&self) -> bool {
        !self.dead && self.rtc.is_alive()
    }

    /// Rtc'nin üreteceği her şeyi işler; bir sonraki zaman aşımını döner
    fn poll(&mut self, socket: &UdpSocket) -> Instant {
        loop {
            if !self.alive() {
                return Instant::now();
            }
            match self.rtc.poll_output() {
                Ok(Output::Transmit(t)) => {
                    let _ = socket.send_to(&t.contents, t.destination);
                }
                Ok(Output::Timeout(t)) => return t,
                Ok(Output::Event(e)) => self.event(e),
                Err(e) => {
                    log!("WebRTC hatası: {e}");
                    self.rtc.disconnect();
                    return Instant::now();
                }
            }
        }
    }

    fn event(&mut self, e: Event) {
        match e {
            Event::IceConnectionStateChange(IceConnectionState::Disconnected) => self.rtc.disconnect(),
            Event::ChannelOpen(cid, label) => {
                log!("WebRTC veri kanalı açıldı: {} ({label})", self.name);
                self.cid = Some(cid);
            }
            Event::ChannelClose(_) => self.cid = None,
            Event::ChannelData(d) if d.binary && d.data.len() <= MAX_PACKET => {
                if let Some(reply) = process_packet(&self.app, &self.pkt, &d.data) {
                    if let Some(mut ch) = self.cid.and_then(|c| self.rtc.channel(c)) {
                        let _ = ch.write(true, &reply);
                    }
                }
            }
            _ => {}
        }
    }
}

struct Runner {
    /// Duyurulan/dinlenen adres: soket her açıldığında GÜNCEL yerel ağ adresiyle yeniden belirlenir (DHCP ya da ağ değişimi)
    adv: SocketAddr,
    open_at: Arc<Mutex<Option<SocketAddr>>>,
    sessions: HashMap<u64, Sess>,
    /// Yalnızca oturum varken açık
    socket: Option<UdpSocket>,
    next_id: u64,
}

impl Runner {
    fn command(&mut self, c: Cmd) {
        match c {
            Cmd::Close(id) => {
                if let Some(s) = self.sessions.get_mut(&id) {
                    s.rtc.disconnect();
                }
            }
            Cmd::Offer { sdp, peer, pkt, app, reply } => {
                let result = catch_unwind(AssertUnwindSafe(|| self.accept(sdp, peer, pkt, app))).unwrap_or_else(|_| Err("öneri işlenirken hata oluştu".to_string()));
                match result {
                    Ok((id, answer)) => {
                        let _ = reply.send(Ok((id, answer)));
                    }
                    Err(e) => {
                        let _ = reply.send(Err(e));
                    }
                }
            }
        }
    }

    fn accept(&mut self, sdp: String, peer: IpAddr, pkt: Arc<Mutex<Pkt>>, app: Arc<App>) -> Result<(u64, String), String> {
        if self.sessions.len() >= MAX_SESSIONS {
            return Err("çok fazla WebRTC oturumu".into());
        }
        let offer = SdpOffer::from_sdp_string(&sdp).map_err(|e| format!("SDP okunamadı: {e}"))?;
        if self.socket.is_none() {
            let ip: IpAddr = crate::mdns::pick_lan_ip().parse().map_err(|_| "yerel ağ adresi bulunamadı".to_string())?;
            let adv = SocketAddr::new(ip, self.adv.port());
            let s = UdpSocket::bind(adv).map_err(|e| format!("UDP {} açılamadı: {e}", adv.port()))?;
            self.adv = adv;
            *self.open_at.lk() = Some(adv);
            self.socket = Some(s);
        }
        let mut rtc = Rtc::builder().set_ice_lite(true).build(Instant::now());
        rtc.add_local_candidate(Candidate::host(self.adv, "udp").map_err(|e| e.to_string())?).ok_or("aday eklenemedi")?;
        let answer = rtc.sdp_api().accept_offer(offer).map_err(|e| format!("öneri kabul edilmedi: {e}"))?;
        let id = self.next_id;
        self.next_id += 1;
        let device_id = pkt.lk().device_id.clone();
        let name = app.state.lk().devices.iter().find(|d| d.id == device_id).map(|d| d.name.clone()).unwrap_or_default();
        self.sessions.insert(id, Sess { rtc, app, pkt, peer, cid: None, name, created: Instant::now(), dead: false });
        Ok((id, answer.to_sdp_string()))
    }

    fn run(mut self, rx: mpsc::Receiver<Cmd>) {
        let mut buf = vec![0u8; 2048];
        loop {
            // Oturum yokken soketi kapat ve tamamen uyu: yalnızca yeni bir komut uyandırır
            if self.sessions.is_empty() {
                self.socket = None;
                *self.open_at.lk() = None;
                match rx.recv() {
                    Ok(c) => self.command(c),
                    Err(_) => return,
                }
            }
            while let Ok(c) = rx.try_recv() {
                self.command(c);
            }
            let Some(socket) = self.socket.as_ref().and_then(|s| s.try_clone().ok()) else {
                self.sessions.clear();
                continue;
            };
            let mut next = Instant::now() + Duration::from_secs(1);
            for s in self.sessions.values_mut() {
                if let Some(t) = s.guarded(|s| s.poll(&socket)) {
                    next = next.min(t);
                }
            }
            self.sessions.retain(|_, s| s.alive());
            if self.sessions.is_empty() {
                continue;
            }
            let wait = next.saturating_duration_since(Instant::now()).max(Duration::from_millis(1));
            let _ = socket.set_read_timeout(Some(wait));
            if let Ok((n, source)) = socket.recv_from(&mut buf) {
                // Yalnızca oturumu kuran telefonun IP'sinden gelen paketler kütüphaneye verilir; gerisi ayrıştırılmadan atılır
                let adv = self.adv;
                for s in self.sessions.values_mut().filter(|s| s.peer == source.ip()) {
                    let datagram = &buf[..n];
                    let handled = s.guarded(|s| {
                        let Ok(contents) = datagram.try_into() else { return false };
                        let input = Input::Receive(Instant::now(), Receive { proto: Protocol::Udp, source, destination: adv, contents });
                        if !s.rtc.accepts(&input) {
                            return false;
                        }
                        if s.rtc.handle_input(input).is_err() {
                            s.rtc.disconnect();
                        }
                        true
                    });
                    if handled == Some(true) {
                        break;
                    }
                }
            }
            let now = Instant::now();
            for s in self.sessions.values_mut() {
                if s.cid.is_none() && s.created.elapsed() > OPEN_DEADLINE {
                    log!("WebRTC kanalı zamanında açılmadı, oturum siliniyor: {}", s.name);
                    s.dead = true;
                    continue;
                }
                s.guarded(|s| {
                    if s.alive() && s.rtc.handle_input(Input::Timeout(now)).is_err() {
                        s.rtc.disconnect();
                    }
                });
            }
            self.sessions.retain(|_, s| s.alive());
        }
    }
}
