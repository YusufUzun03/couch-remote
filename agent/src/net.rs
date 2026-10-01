//! Ağ sertleştirme. Sunucular yerel ağa açık olduğundan ağdaki herhangi bir cihaz bağlanabilir:
//!   - eşzamanlı bağlantı sınırı (toplam ve IP başına) → bağlantı doldurma saldırısı programı kilitleyemez
//!   - TLS el sıkışması ve HTTP başlık zaman aşımı → yarım bırakılan ("slowloris") bağlantılar kapatılır
//!   - hatalı deneme sınırlayıcı → jeton / kimlik tahmini denemeleri yavaşlatılır
use hyper_util::rt::{TokioExecutor, TokioIo, TokioTimer};
use hyper_util::server::conn::auto::Builder;
use hyper_util::service::TowerToHyperService;
use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_rustls::TlsAcceptor;

use crate::sync::LockExt;
use crate::tls::Peer;

const MAX_CONNS: usize = 256;
const MAX_PER_IP: usize = 48;
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);
const HEADER_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Default)]
struct ConnLimiter {
    total: AtomicUsize,
    per_ip: Mutex<HashMap<IpAddr, usize>>,
}

/// Bağlantı kapanınca sayaçları geri verir
struct ConnGuard {
    lim: Arc<ConnLimiter>,
    ip: IpAddr,
}

impl ConnLimiter {
    fn admit(self: &Arc<Self>, ip: IpAddr) -> Option<ConnGuard> {
        if self.total.load(Ordering::Relaxed) >= MAX_CONNS {
            return None;
        }
        let mut map = self.per_ip.lk();
        let n = map.entry(ip).or_insert(0);
        if *n >= MAX_PER_IP {
            return None;
        }
        *n += 1;
        self.total.fetch_add(1, Ordering::Relaxed);
        Some(ConnGuard { lim: self.clone(), ip })
    }
}

impl Drop for ConnGuard {
    fn drop(&mut self) {
        self.lim.total.fetch_sub(1, Ordering::Relaxed);
        let mut map = self.lim.per_ip.lk();
        if let Some(n) = map.get_mut(&self.ip) {
            *n = n.saturating_sub(1);
            if *n == 0 {
                map.remove(&self.ip);
            }
        }
    }
}

/// Düz HTTP (`tls` yok) veya HTTPS sunucusu: sınırlar ve zaman aşımlarıyla.
pub async fn serve(listener: tokio::net::TcpListener, tls: Option<Arc<rustls::ServerConfig>>, router: axum::Router) {
    let acceptor = tls.map(TlsAcceptor::from);
    let limiter = Arc::new(ConnLimiter::default());
    loop {
        let (tcp, peer) = match listener.accept().await {
            Ok(c) => c,
            Err(e) => {
                crate::log!("Bağlantı kabul hatası: {e}");
                // Tutamaç tükenmesi gibi geçici hatalarda sıkı döngüye girme
                tokio::time::sleep(Duration::from_millis(100)).await;
                continue;
            }
        };
        let Some(guard) = limiter.admit(peer.ip()) else {
            continue; // sınır aşıldı: bağlantı hemen kapanır
        };
        let _ = tcp.set_nodelay(true); // küçük girdi paketleri beklemeden gitsin
        let acceptor = acceptor.clone();
        let router = router.clone().layer(axum::Extension(Peer(peer.ip())));
        tokio::spawn(async move {
            let _guard = guard;
            match acceptor {
                Some(a) => match tokio::time::timeout(HANDSHAKE_TIMEOUT, a.accept(tcp)).await {
                    Ok(Ok(stream)) => run(TokioIo::new(stream), router).await,
                    _ => {}
                },
                None => run(TokioIo::new(tcp), router).await,
            }
        });
    }
}

async fn run<I>(io: TokioIo<I>, router: axum::Router)
where
    I: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let mut b = Builder::new(TokioExecutor::new());
    b.http1().timer(TokioTimer::new()).header_read_timeout(HEADER_TIMEOUT);
    let _ = b.serve_connection_with_upgrades(io, TowerToHyperService::new(router)).await;
}

/// Hatalı deneme sınırlayıcı: pencere içinde `max` hatadan sonra o IP bir süre engellenir.
pub struct FailLimiter {
    map: Mutex<HashMap<IpAddr, (u32, Instant)>>,
    max: u32,
    window: Duration,
}

impl FailLimiter {
    pub fn new(max: u32, window: Duration) -> FailLimiter {
        FailLimiter { map: Mutex::new(HashMap::new()), max, window }
    }

    pub fn blocked(&self, ip: IpAddr) -> bool {
        self.map.lk().get(&ip).is_some_and(|(n, start)| *n >= self.max && start.elapsed() < self.window)
    }

    pub fn fail(&self, ip: IpAddr) {
        let mut map = self.map.lk();
        if map.len() > 1024 {
            map.retain(|_, (_, start)| start.elapsed() < self.window); // bellek şişmesin
        }
        let e = map.entry(ip).or_insert((0, Instant::now()));
        if e.1.elapsed() >= self.window {
            *e = (0, Instant::now());
        }
        e.0 += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn baglanti_siniri_ip_basina_uygulanir() {
        let lim = Arc::new(ConnLimiter::default());
        let ip: IpAddr = "192.168.1.50".parse().unwrap();
        let other: IpAddr = "192.168.1.51".parse().unwrap();
        let guards: Vec<_> = (0..MAX_PER_IP).map(|_| lim.admit(ip).expect("sınır altında")).collect();
        assert!(lim.admit(ip).is_none(), "IP başına sınır aşıldı");
        assert!(lim.admit(other).is_some(), "başka IP etkilenmemeli");
        drop(guards);
        assert!(lim.admit(ip).is_some(), "bağlantılar kapanınca yer açılmalı");
    }

    #[test]
    fn hatali_deneme_sinirlayici() {
        let l = FailLimiter::new(3, Duration::from_secs(60));
        let ip: IpAddr = "10.0.0.2".parse().unwrap();
        for _ in 0..2 {
            l.fail(ip);
        }
        assert!(!l.blocked(ip));
        l.fail(ip);
        assert!(l.blocked(ip));
        assert!(!l.blocked("10.0.0.3".parse().unwrap()));
    }
}
