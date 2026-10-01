//! Eşleşme oturumu: QR'daki tek kullanımlık kod + yedek 6 haneli PIN.
//! Faz 0 dersi: profil kurulumu uzun sürebildiği için süre "kayan"dır — kurulum/eşleşme sayfası
//! her görüntülendiğinde 10 dk uzar (en fazla 60 dk).
use std::time::{Duration, Instant};

const TTL: Duration = Duration::from_secs(10 * 60);
const MAX_LIFETIME: Duration = Duration::from_secs(60 * 60);
pub const MAX_PIN_ATTEMPTS: u32 = 5;

pub struct Pairing {
    pub code: String,
    pub pin: String,
    created: Instant,
    expires: Instant,
    pub pin_attempts: u32,
    /// Telefon kurulum sayfasını açtı (QR okutuldu)
    pub setup_seen: bool,
    /// Telefon güvenlik profilini indirdi
    pub profile_seen: bool,
}

impl Pairing {
    pub fn new() -> Pairing {
        let now = Instant::now();
        Pairing {
            code: crate::state::random_token(18),
            pin: format!("{:06}", rand::random_range(0..1_000_000u32)),
            created: now,
            expires: now + TTL,
            pin_attempts: 0,
            setup_seen: false,
            profile_seen: false,
        }
    }

    pub fn active(&self) -> bool {
        Instant::now() < self.expires
    }

    pub fn expires_in_secs(&self) -> u64 {
        self.expires.saturating_duration_since(Instant::now()).as_secs()
    }

    /// Kod doğruysa süreyi uzatır. Süresi dolmuş ama toplam ömrü aşmamış kod da canlanır.
    pub fn touch(&mut self, code: &str) -> bool {
        let now = Instant::now();
        if !crate::web::safe_eq(code, &self.code) || now > self.created + MAX_LIFETIME {
            return false;
        }
        self.expires = (now + TTL).min(self.created + MAX_LIFETIME).max(self.expires);
        true
    }
}
