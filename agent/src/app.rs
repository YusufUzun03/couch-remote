//! Uygulamanın paylaşılan durumu.
use crate::sync::LockExt;
use anyhow::Result;
use serde_json::Value;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::atomic::AtomicUsize;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::broadcast;

use crate::audio::Audio;
use crate::certs;
use crate::dpapi;
use crate::events::Events;
use crate::input::Input;
use crate::launcher::Launcher;
use crate::media::Media;
use crate::pairing::Pairing;
use crate::power::Timer;
use crate::state::State;
use crate::stats::Stats;
use crate::tls::{self, SniResolver};
use crate::{HTTPS_PORT, HTTP_PORT};

/// Çok fazla hatalı PIN denemesinden sonra yeni eşleşme kodu bu süre boyunca kendiliğinden üretilmez
const PAIRING_LOCKOUT: Duration = Duration::from_secs(5 * 60);
/// Bu kadar süredir hiç görülmeyen eşleşmiş cihazlar silinir
const DEVICE_MAX_IDLE_DAYS: i64 = 90;

pub struct App {
    pub host: String,
    pub lan_ip: String,
    pub state: Mutex<State>,
    pub pairing: Mutex<Option<Pairing>>,
    /// Çok fazla hatalı PIN: bu ana kadar kendiliğinden yeni kod üretilmez (yönetim sayfasında elle üretilebilir)
    pub pairing_locked_until: Mutex<Option<Instant>>,
    pub stats: Arc<Mutex<Stats>>,
    pub events: Arc<Events>,
    pub input: Input,
    pub media: Media,
    pub audio: Audio,
    pub tv: crate::tv::Tv,
    pub timer: Timer,
    pub launcher: Launcher,
    pub resolver: Arc<SniResolver>,
    pub tls: Arc<rustls::ServerConfig>,
    pub ca_der: Vec<u8>,
    /// Sertifika doğrulama kodu (ör. 7F3A-91C2): iPhone'daki profil adında görünür, PC ekranıyla karşılaştırılır
    pub ca_code: String,
    /// Kestirmeler jetonu; kayıp telefon durumunda yenilenebilir
    pub shortcut_token: Mutex<String>,
    /// Tanılama raporları: sadece bellekte, sadece eşleşmiş cihazlardan (S2)
    pub reports: Mutex<VecDeque<Value>>,
    pub connections: AtomicUsize,
    /// Şu an bağlı cihaz kimlikleri
    pub connected: Mutex<HashSet<String>>,
    /// Eşleşmesi kaldırılan cihazın açık bağlantısını anında kapatmak için (S1)
    pub revoked: broadcast::Sender<String>,
    /// Dosya gönderme jetonları: jeton → (cihaz, bitiş)
    pub upload_tokens: Mutex<HashMap<String, (String, Instant)>>,
    pub last_shortcut: Mutex<Option<Value>>,
    /// Yönetim sayfası anahtarı: her açılışta yeni. Sayfa sadece tepsi menüsünden / programdan bu anahtarla açılır.
    pub admin_token: String,
    /// Windows ağ profili "Genel" mi? (None: henüz bilinmiyor)
    pub network_public: Mutex<Option<bool>>,
    pub icons: HashMap<u32, Vec<u8>>,
    /// Yanlış jeton / kimlik denemeleri (IP başına dakikada 10): sonrası 429
    pub fails: crate::net::FailLimiter,
}

/// Ayar dosyası bu kullanıcıda çözülemiyorsa (başka bilgisayardan/kullanıcıdan kopyalanmış) yedeklenip sıfırlanır
fn load_state_checked() -> Result<State> {
    let state = State::load()?;
    if dpapi::unprotect_str(&state.shortcut_token_enc).is_ok() {
        return Ok(state);
    }
    log!("Ayar dosyası bu Windows kullanıcısına ait değil — yedeklenip yeniden oluşturuluyor");
    State::backup_and_reset()
}

fn prune_devices(state: &mut State) {
    let now = chrono::Local::now();
    let before = state.devices.len();
    state.devices.retain(|d| {
        let seen = d.last_seen.as_deref().unwrap_or(&d.paired_at);
        chrono::DateTime::parse_from_rfc3339(seen).map(|t| (now.signed_duration_since(t)).num_days() < DEVICE_MAX_IDLE_DAYS).unwrap_or(true)
    });
    if state.devices.len() != before {
        log!("{} uzun süredir kullanılmayan cihaz kaldırıldı", before - state.devices.len());
    }
}

impl App {
    pub fn init() -> Result<App> {
        let mut state = load_state_checked()?;
        let host = format!("couch-{}.local", state.id);
        let lan_ip = crate::mdns::pick_lan_ip();

        if state.ca.as_ref().is_none_or(|c| c.host != host) {
            state.ca = Some(certs::create_ca(&host)?);
            state.leaf = None;
        }
        if state.leaf.as_ref().is_none_or(|l| l.host != host || certs::expires_soon(l)) {
            state.leaf = Some(certs::create_leaf(&host, state.ca.as_ref().unwrap())?);
        }
        prune_devices(&mut state);
        state.save()?;

        let resolver = Arc::new(SniResolver::default());
        let leaf = state.leaf.as_ref().unwrap();
        resolver.set(&host, &leaf.cert_pem, &dpapi::unprotect_str(&leaf.key_pem_enc)?)?;

        let ca_der = certs::ca_der(state.ca.as_ref().unwrap())?;
        let ca_code = certs::fingerprint_code(&ca_der);
        let shortcut_token = dpapi::unprotect_str(&state.shortcut_token_enc)?;
        let stats = Arc::new(Mutex::new(Stats::default()));
        let events = Events::new();
        let input = Input::start(stats.clone());
        let media = Media::start(input.clone(), events.clone());
        let audio = Audio::start(events.clone());
        let tv = crate::tv::Tv::start(events.clone());
        crate::context::start(events.clone());
        let icons = [180, 192, 512].into_iter().map(|s| (s, crate::icons::png(s))).collect();
        let (revoked, _) = broadcast::channel(16);

        let app = App {
            host,
            lan_ip,
            state: Mutex::new(state),
            // Eşleşme kodu açılışta ÜRETİLMEZ: sadece "Telefonu bağla" sayfası açıkken (S4)
            pairing: Mutex::new(None),
            pairing_locked_until: Mutex::new(None),
            events,
            input,
            media,
            audio,
            tv,
            timer: Timer::new(),
            launcher: Launcher::new(),
            stats,
            tls: tls::server_config(resolver.clone()),
            resolver,
            ca_der,
            ca_code,
            shortcut_token: Mutex::new(shortcut_token),
            reports: Mutex::new(VecDeque::new()),
            connections: AtomicUsize::new(0),
            connected: Mutex::new(HashSet::new()),
            revoked,
            upload_tokens: Mutex::new(HashMap::new()),
            last_shortcut: Mutex::new(None),
            admin_token: crate::state::random_token(24),
            network_public: Mutex::new(None),
            icons,
            fails: crate::net::FailLimiter::new(10, std::time::Duration::from_secs(60)),
        };
        // Program ikinci kez açılırsa (güncelleme dahil) yönetim sayfasına bu anahtarla ulaşabilsin
        let _ = std::fs::write(crate::state::data_dir().join("admin.token"), &app.admin_token);
        // Faz 0'dan kalan disk raporları artık tutulmuyor (S2)
        let _ = std::fs::remove_file(crate::state::data_dir().join("reports.jsonl"));
        Ok(app)
    }

    /// Yeni eşleşme kodu (kilidi de kaldırır: yönetim sayfasında elle istendi)
    pub fn new_pairing(&self) {
        *self.pairing_locked_until.lk() = None;
        *self.pairing.lk() = Some(Pairing::new());
        log!("Yeni eşleşme kodu oluşturuldu"); // PIN günlüğe yazılmaz (S5)
    }

    /// "Telefonu bağla" sayfası açıkken çağrılır: geçerli kod yoksa (ve kilit yoksa) yenisini üretir
    pub fn ensure_pairing(&self) {
        let active = self.pairing.lk().as_ref().is_some_and(|p| p.active());
        let locked = self.pairing_locked_until.lk().is_some_and(|t| Instant::now() < t);
        if !active && !locked {
            *self.pairing.lk() = Some(Pairing::new());
        }
    }

    /// Çok fazla hatalı PIN: oturumu kapat ve bir süre kendiliğinden yenisini üretme
    pub fn lock_pairing(&self) {
        *self.pairing.lk() = None;
        *self.pairing_locked_until.lk() = Some(Instant::now() + PAIRING_LOCKOUT);
        log!("Çok fazla hatalı PIN — eşleşme 5 dakika kilitlendi");
    }

    pub fn pairing_locked(&self) -> bool {
        self.pairing_locked_until.lk().is_some_and(|t| Instant::now() < t)
    }

    /// Eşleşmeyi kaldır ve açık bağlantısını hemen kapat (S1)
    /// Kestirmeler jetonunu yeniler: eski jetonla kurulmuş tüm kestirmeler (kayıp telefon dahil) çalışmaz olur
    pub fn rotate_shortcut_token(&self) -> anyhow::Result<()> {
        let token = crate::state::random_token(24);
        {
            let mut st = self.state.lk();
            st.shortcut_token_enc = dpapi::protect_str(&token)?;
            st.save()?;
        }
        *self.shortcut_token.lk() = token;
        *self.last_shortcut.lk() = None;
        log!("Kestirmeler jetonu yenilendi");
        Ok(())
    }

    /// Kayıp / çalınan telefon için: tüm eşleşmeleri sil, açık bağlantıları kes, Kestirmeler jetonunu yenile
    pub fn revoke_all(&self) {
        let ids: Vec<String> = self.state.lk().devices.iter().map(|d| d.id.clone()).collect();
        for id in &ids {
            self.revoke_device(id);
        }
        let _ = self.rotate_shortcut_token();
        log!("Tüm eşleşmeler kaldırıldı ({} cihaz)", ids.len());
    }

    pub fn revoke_device(&self, id: &str) {
        {
            let mut st = self.state.lk();
            st.devices.retain(|d| d.id != id);
            let _ = st.save();
        }
        self.upload_tokens.lk().retain(|_, (dev, _)| dev != id);
        let _ = self.revoked.send(id.to_string());
    }

    pub fn local_origin(&self) -> String {
        format!("https://{}:{HTTPS_PORT}", self.host)
    }

    /// Web uygulamasının geçerli adresleri (Origin kontrolü için)
    pub fn app_origins(&self) -> Vec<String> {
        vec![self.local_origin()]
    }

    /// HTTPS sunucusu için geçerli Host başlıkları (DNS rebinding koruması)
    pub fn app_hosts(&self) -> Vec<String> {
        vec![format!("{}:{HTTPS_PORT}", self.host)]
    }

    /// Yönetim sayfasının anahtarlı adresi (tepsi menüsü ve program bunu açar)
    pub fn admin_url(&self) -> String {
        format!("http://127.0.0.1:{}/?t={}", crate::ADMIN_PORT, self.admin_token)
    }

    pub fn setup_hosts(&self) -> Vec<String> {
        vec![format!("{}:{HTTP_PORT}", self.host), format!("{}:{HTTP_PORT}", self.lan_ip)]
    }

    pub fn setup_origins(&self) -> Vec<String> {
        self.setup_hosts().into_iter().map(|h| format!("http://{h}")).collect()
    }
}
