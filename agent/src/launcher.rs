//! Uygulama başlatıcı: hızlı başlat kataloğu (Netflix, YouTube, Spotify…), servis içi arama ve
//! Başlat menüsündeki programlar, Mağaza (UWP) uygulamaları ve Steam / Epic oyunları.
//! Güvenlik: telefon asla dosya yolu veya URL göndermez — sadece bu listedeki bir kimliği seçer.
//! Adresleri (arama dahil) PC kurar; sadece http(s) ve bilinen uygulama protokolleri açılır.
use crate::sync::LockExt;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use windows::core::{w, HSTRING};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
use winreg::enums::{HKEY_CLASSES_ROOT, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
use winreg::RegKey;


/// Nasıl açılır
enum Target {
    /// Web sitesi (Chrome varsa Chrome'da)
    Web(&'static str),
    /// Kurulu uygulamanın protokolü; yoksa web adresine düşülür
    App { protocol: &'static str, uri: &'static str, fallback: Option<&'static str> },
}

struct Entry {
    id: &'static str,
    name: &'static str,
    /// Kutucuk rengi ve kısa işaret (resmi logo değil)
    color: &'static str,
    mark: &'static str,
    target: Target,
    /// Arama adresi kalıbı ({q} = kodlanmış sorgu); uygulama protokolü için ayrı kalıp
    search_web: Option<&'static str>,
    search_app: Option<&'static str>,
    /// Açılınca telefon kumanda ekranına geçsin mi
    media: bool,
}

const CATALOG: &[Entry] = &[
    Entry { id: "netflix", name: "Netflix", color: "#E50914", mark: "N", target: Target::Web("https://www.netflix.com/browse"),
        search_web: Some("https://www.netflix.com/search?q={q}"), search_app: None, media: true },
    Entry { id: "youtube", name: "YouTube", color: "#FF0033", mark: "▶", target: Target::Web("https://www.youtube.com/"),
        search_web: Some("https://www.youtube.com/results?search_query={q}"), search_app: None, media: true },
    Entry { id: "spotify", name: "Spotify", color: "#1DB954", mark: "S",
        target: Target::App { protocol: "spotify", uri: "spotify:", fallback: Some("https://open.spotify.com/") },
        search_web: Some("https://open.spotify.com/search/{q}"), search_app: Some("spotify:search:{q}"), media: true },
    Entry { id: "primevideo", name: "Prime Video", color: "#00A8E1", mark: "P", target: Target::Web("https://www.primevideo.com/"),
        search_web: Some("https://www.primevideo.com/search/?phrase={q}"), search_app: None, media: true },
    Entry { id: "disneyplus", name: "Disney+", color: "#113CCF", mark: "D+", target: Target::Web("https://www.disneyplus.com/"),
        search_web: None, search_app: None, media: true },
    Entry { id: "twitch", name: "Twitch", color: "#9146FF", mark: "T", target: Target::Web("https://www.twitch.tv/"),
        search_web: Some("https://www.twitch.tv/search?term={q}"), search_app: None, media: true },
    Entry { id: "ytmusic", name: "YouTube Music", color: "#FF0033", mark: "♪", target: Target::Web("https://music.youtube.com/"),
        search_web: Some("https://music.youtube.com/search?q={q}"), search_app: None, media: true },
    Entry { id: "tabii", name: "tabii", color: "#5B2EE5", mark: "t", target: Target::Web("https://www.tabii.com/"),
        search_web: None, search_app: None, media: true },
    Entry { id: "exxen", name: "Exxen", color: "#F2A900", mark: "E", target: Target::Web("https://www.exxen.com/"),
        search_web: None, search_app: None, media: true },
    Entry { id: "steambp", name: "Steam Big Picture", color: "#1B2838", mark: "◉",
        target: Target::App { protocol: "steam", uri: "steam://open/bigpicture", fallback: None },
        search_web: None, search_app: None, media: false },
];

// ------------------------------------------------------------------ yardımcılar

/// URL için yüzde kodlama (UTF-8, RFC 3986 ayrılmamış karakterler hariç)
fn encode(q: &str) -> String {
    q.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

fn protocol_installed(protocol: &str) -> bool {
    RegKey::predef(HKEY_CLASSES_ROOT)
        .open_subkey(protocol)
        .and_then(|k| k.get_value::<String, _>("URL Protocol"))
        .is_ok()
}

/// Chrome'un yolu (kayıt defterindeki "App Paths" veya bilinen kurulum yerleri)
pub fn chrome_path() -> Option<PathBuf> {
    for root in [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE] {
        if let Ok(k) = RegKey::predef(root).open_subkey(r"Software\Microsoft\Windows\CurrentVersion\App Paths\chrome.exe") {
            if let Ok(p) = k.get_value::<String, _>("") {
                let p = PathBuf::from(p.trim_matches('"'));
                if p.exists() {
                    return Some(p);
                }
            }
        }
    }
    let candidates = [
        std::env::var_os("ProgramFiles").map(|p| PathBuf::from(p).join(r"Google\Chrome\Application\chrome.exe")),
        std::env::var_os("ProgramFiles(x86)").map(|p| PathBuf::from(p).join(r"Google\Chrome\Application\chrome.exe")),
        std::env::var_os("LOCALAPPDATA").map(|p| PathBuf::from(p).join(r"Google\Chrome\Application\chrome.exe")),
    ];
    candidates.into_iter().flatten().find(|p| p.exists())
}

/// Belge veya adresi Windows kabuğuyla açar (varsayılan program / tarayıcı / protokol).
/// `cmd /c start` kullanılmaz: adresteki '&' gibi karakterler komut satırında bozulur.
pub fn shell_open(target: &str) -> bool {
    let r = unsafe { ShellExecuteW(None, w!("open"), &HSTRING::from(target), None, None, SW_SHOWNORMAL) };
    r.0 as isize > 32
}

fn open_web(url: &str) -> bool {
    debug_assert!(url.starts_with("https://"));
    if let Some(chrome) = chrome_path() {
        // Yeni pencere değil, açık Chrome penceresinde yeni sekme (pencere yığını olmasın)
        if std::process::Command::new(chrome).arg(url).spawn().is_ok() {
            return true;
        }
    }
    shell_open(url)
}

// ------------------------------------------------------------------ Başlat menüsü

#[derive(Clone)]
struct InstalledApp {
    id: String,
    name: String,
    /// ShellExecute'a verilecek hedef: .lnk yolu veya "shell:AppsFolder\\<AppID>" (Mağaza uygulaması)
    target: String,
    uwp: bool,
}

fn short_id(prefix: &str, text: &str) -> String {
    let digest = Sha256::digest(text.to_lowercase().as_bytes());
    format!("{prefix}{}", digest[..8].iter().map(|b| format!("{b:02x}")).collect::<String>())
}

/// Başlat menüsünde yardımcı kısayollar (kaldırıcı, belgeler, geliştirici araçları) listelenmez
fn is_noise(name: &str) -> bool {
    let n = name.to_lowercase();
    n.starts_with("about ")
        || [
            "uninstall", "kaldır", "readme", "read me", "help", "yardım", "manual", "website", "web site", "release notes",
            "license", "lisans", "documentation", "sürüm notları", "verifier", "debug", "sdk ", "developer command prompt",
            "recovery", "diagnostic",
        ]
        .iter()
        .any(|w| n.contains(w))
}

/// Sistem/yönetim araçlarının klasörleri taranmaz (Not Defteri, Paint gibi Donatılar kalır)
fn is_noise_dir(name: &str) -> bool {
    let n = name.to_lowercase();
    ["administrative tools", "windows tools", "system tools", "accessibility", "erişilebilirlik", "windows kits", "maintenance", "startup", "başlangıç", "windows administrative tools", "sistem araçları", "yönetimsel araçlar"]
        .iter()
        .any(|w| n == *w)
}

fn scan_dir(dir: &Path, out: &mut Vec<InstalledApp>, depth: usize) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            let dir_name = p.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            if depth < 4 && !is_noise_dir(&dir_name) {
                scan_dir(&p, out, depth + 1);
            }
        } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("lnk")) {
            let name = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            if name.is_empty() || is_noise(&name) {
                continue;
            }
            out.push(InstalledApp { id: short_id("", &p.to_string_lossy()), name, target: p.to_string_lossy().into_owned(), uwp: false });
        }
    }
}

fn scan_start_menu() -> Vec<InstalledApp> {
    let mut apps = Vec::new();
    let roots = [
        std::env::var_os("ProgramData").map(|p| PathBuf::from(p).join(r"Microsoft\Windows\Start Menu\Programs")),
        std::env::var_os("APPDATA").map(|p| PathBuf::from(p).join(r"Microsoft\Windows\Start Menu\Programs")),
    ];
    for root in roots.into_iter().flatten() {
        scan_dir(&root, &mut apps, 0);
    }
    apps.extend(uwp_apps());
    // Aynı ad hem kısayol hem Mağaza uygulaması olarak varsa kısayol kalır
    apps.sort_by_key(|a| (a.name.to_lowercase(), a.uwp));
    apps.dedup_by(|a, b| a.name.eq_ignore_ascii_case(&b.name));
    apps
}

// ------------------------------------------------------------------ Mağaza (UWP) uygulamaları

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const UWP_CACHE_FOR: Duration = Duration::from_secs(600);
static UWP: Mutex<Option<(Instant, Vec<InstalledApp>)>> = Mutex::new(None);

/// Mağaza uygulamaları: Başlat menüsünün kendi listesi (Get-StartApps), AppID'sinde "!" olanlar.
/// PowerShell ~0,5 sn sürer; bu yüzden 10 dakika önbelleğe alınır.
fn uwp_apps() -> Vec<InstalledApp> {
    let mut cache = UWP.lk();
    if let Some((t, apps)) = cache.as_ref() {
        if t.elapsed() < UWP_CACHE_FOR {
            return apps.clone();
        }
    }
    let apps = query_uwp().unwrap_or_default();
    *cache = Some((Instant::now(), apps.clone()));
    apps
}

fn query_uwp() -> Option<Vec<InstalledApp>> {
    let out = std::process::Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "[Console]::OutputEncoding=[Text.Encoding]::UTF8; ConvertTo-Json -Compress -InputObject @(Get-StartApps | Where-Object { $_.AppID -match '!' })",
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .ok()?;
    parse_uwp(&String::from_utf8_lossy(&out.stdout))
}

fn parse_uwp(json: &str) -> Option<Vec<InstalledApp>> {
    let v: Value = serde_json::from_str(json.trim().trim_start_matches('\u{feff}')).ok()?;
    Some(
        v.as_array()?
            .iter()
            .filter_map(|e| {
                let name = e["Name"].as_str()?.trim().to_string();
                let appid = e["AppID"].as_str()?;
                // AppID komut satırına gitmez, sadece shell:AppsFolder yoluna eklenir; yine de garip karakterler elenir
                let safe = appid.chars().all(|c| c.is_alphanumeric() || "._-!".contains(c));
                (!name.is_empty() && safe && !is_noise(&name) && !is_uwp_noise(&name)).then(|| InstalledApp {
                    id: short_id("u", appid),
                    name,
                    target: format!("shell:AppsFolder\\{appid}"),
                    uwp: true,
                })
            })
            .collect(),
    )
}

/// Tanıtım / ilk kullanım girdileri
fn is_uwp_noise(name: &str) -> bool {
    let n = name.to_lowercase();
    ["promotion", "başlarken", "get started", "tips", "ipuçları", "energy star"].iter().any(|w| n.contains(w))
}

// ------------------------------------------------------------------ oyunlar (Steam, Epic)

#[derive(Clone)]
struct Game {
    id: String,
    name: String,
    store: &'static str,
    /// Açma adresi (steam://rungameid/…, com.epicgames.launcher://…); sadece bu dosyada kurulur
    uri: String,
}

const GAMES_CACHE_FOR: Duration = Duration::from_secs(120);
static GAMES: Mutex<Option<(Instant, Vec<Game>)>> = Mutex::new(None);

fn games() -> Vec<Game> {
    let mut cache = GAMES.lk();
    if let Some((t, g)) = cache.as_ref() {
        if t.elapsed() < GAMES_CACHE_FOR {
            return g.clone();
        }
    }
    let mut all = steam_games();
    all.extend(epic_games());
    all.sort_by_key(|g| g.name.to_lowercase());
    *cache = Some((Instant::now(), all.clone()));
    all
}

/// Oyun kapağı: Steam'in yerel kapak önbelleğinden (library_600x900.jpg) küçük bir JPEG, data: adresi olarak.
/// Sadece listedeki bir Steam oyunu için ve sadece sayısal appid'den kurulan yoldan okunur.
pub fn game_art(id: &str) -> Option<String> {
    use base64::{engine::general_purpose::STANDARD as B64, Engine};
    use image::codecs::jpeg::JpegEncoder;
    use image::imageops::{resize, FilterType};
    let appid = id.strip_prefix("steam:")?;
    if !games().iter().any(|g| g.id == id) {
        return None;
    }
    let path = steam_root()?.join(r"appcache\librarycache").join(appid).join("library_600x900.jpg");
    let img = image::open(path).ok()?.to_rgb8();
    let small = resize(&img, 120, 180, FilterType::Triangle);
    let mut jpeg = Vec::with_capacity(8 * 1024);
    JpegEncoder::new_with_quality(&mut jpeg, 78).encode_image(&small).ok()?;
    Some(format!("data:image/jpeg;base64,{}", B64.encode(jpeg)))
}

/// Valve'ın .vdf/.acf biçimi: satırlar "anahtar"  "değer" çiftleri. Basit satır ayrıştırması yeterli.
fn vdf_pairs(text: &str) -> Vec<(String, String)> {
    text.lines()
        .filter_map(|l| {
            let mut q = l.split('"');
            q.next()?;
            let k = q.next()?;
            q.next()?;
            let v = q.next()?;
            Some((k.to_string(), v.replace("\\\\", "\\")))
        })
        .collect()
}

fn steam_root() -> Option<PathBuf> {
    let steam = RegKey::predef(HKEY_CURRENT_USER).open_subkey(r"Software\Valve\Steam").and_then(|k| k.get_value::<String, _>("SteamPath")).ok()?;
    Some(PathBuf::from(steam.replace('/', "\\")))
}

fn steam_games() -> Vec<Game> {
    let Some(root) = steam_root() else { return vec![] };
    let mut libs = vec![root.clone()];
    if let Ok(text) = std::fs::read_to_string(root.join(r"steamapps\libraryfolders.vdf")) {
        for (k, v) in vdf_pairs(&text) {
            let p = PathBuf::from(v);
            if k == "path" && !libs.contains(&p) {
                libs.push(p);
            }
        }
    }
    let mut out = Vec::new();
    for lib in libs {
        let Ok(dir) = std::fs::read_dir(lib.join("steamapps")) else { continue };
        for e in dir.flatten() {
            let fname = e.file_name().to_string_lossy().into_owned();
            if !(fname.starts_with("appmanifest_") && fname.ends_with(".acf")) {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(e.path()) else { continue };
            if let Some(g) = steam_game_from_manifest(&text) {
                if !out.iter().any(|x: &Game| x.id == g.id) {
                    out.push(g);
                }
            }
        }
    }
    out
}

fn steam_game_from_manifest(text: &str) -> Option<Game> {
    let pairs = vdf_pairs(text);
    let get = |key: &str| pairs.iter().find(|(k, _)| k.eq_ignore_ascii_case(key)).map(|(_, v)| v.as_str());
    let appid = get("appid").filter(|a| !a.is_empty() && a.chars().all(|c| c.is_ascii_digit()))?;
    let name = get("name")?.trim();
    // StateFlags'te 4 biti = tam kurulu (güncelleme bekliyorsa 6 gibi başka bitler de olabilir)
    let flags: u32 = get("StateFlags").and_then(|f| f.parse().ok()).unwrap_or(0);
    let tool = ["steamworks common redistributables", "proton", "steam linux runtime", "steamvr"].iter().any(|t| name.to_lowercase().starts_with(t));
    (flags & 4 != 0 && !name.is_empty() && !tool).then(|| Game {
        id: format!("steam:{appid}"),
        name: name.to_string(),
        store: "steam",
        uri: format!("steam://rungameid/{appid}"),
    })
}

fn epic_games() -> Vec<Game> {
    let Some(pd) = std::env::var_os("ProgramData") else { return vec![] };
    let Ok(dir) = std::fs::read_dir(PathBuf::from(pd).join(r"Epic\EpicGamesLauncher\Data\Manifests")) else { return vec![] };
    dir.flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x.eq_ignore_ascii_case("item")))
        .filter_map(|e| epic_game_from_manifest(&std::fs::read_to_string(e.path()).ok()?))
        .collect()
}

fn epic_game_from_manifest(json: &str) -> Option<Game> {
    let v: Value = serde_json::from_str(json.trim_start_matches('\u{feff}')).ok()?;
    let is_game = v["AppCategories"].as_array()?.iter().any(|c| c == "games");
    if !is_game || v["bIsIncompleteInstall"] == true {
        return None;
    }
    let (name, app, ns, item) = (v["DisplayName"].as_str()?, v["AppName"].as_str()?, v["CatalogNamespace"].as_str()?, v["CatalogItemId"].as_str()?);
    // Adrese gömüleceği için sadece güvenli karakterler
    let safe = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    (safe(app) && safe(ns) && safe(item)).then(|| Game {
        id: format!("epic:{app}"),
        name: name.to_string(),
        store: "epic",
        uri: format!("com.epicgames.launcher://apps/{ns}%3A{item}%3A{app}?action=launch&silent=true"),
    })
}

pub struct Launcher {
    cache: Mutex<Option<(Instant, Vec<InstalledApp>)>>,
}

const CACHE_FOR: Duration = Duration::from_secs(60);

impl Launcher {
    pub fn new() -> Launcher {
        Launcher { cache: Mutex::new(None) }
    }

    fn with_apps<T>(&self, f: impl FnOnce(&[InstalledApp]) -> T) -> T {
        let mut cache = self.cache.lk();
        if cache.as_ref().is_none_or(|(t, _)| t.elapsed() > CACHE_FOR) {
            *cache = Some((Instant::now(), scan_start_menu()));
        }
        f(&cache.as_ref().unwrap().1)
    }

    /// Telefona gönderilecek liste: hızlı başlat + bu bilgisayardaki uygulamalar
    pub fn list(&self) -> Value {
        let catalog: Vec<Value> = CATALOG
            .iter()
            .filter(|e| match &e.target {
                Target::App { protocol, fallback, .. } => fallback.is_some() || protocol_installed(protocol),
                Target::Web(_) => true,
            })
            .map(|e| {
                let search = match &e.target {
                    Target::App { protocol, .. } if protocol_installed(protocol) => e.search_app.or(e.search_web).is_some(),
                    _ => e.search_web.is_some(),
                };
                json!({ "id": e.id, "name": e.name, "color": e.color, "mark": e.mark, "search": search, "media": e.media })
            })
            .collect();
        let installed = self.with_apps(|apps| apps.iter().map(|a| json!({ "id": a.id, "name": a.name })).collect::<Vec<_>>());
        let games: Vec<Value> = games().iter().map(|g| json!({ "id": g.id, "name": g.name, "store": g.store })).collect();
        json!({ "type": "apps", "catalog": catalog, "installed": installed, "games": games, "browser": if chrome_path().is_some() { "Chrome'da" } else { "varsayılan tarayıcıda" } })
    }

    /// Hızlı başlat öğesi veya Başlat menüsü uygulaması. Dönen ad ve "medya mı" telefonda kullanılır.
    pub fn launch(&self, id: &str) -> Option<(String, bool)> {
        if let Some(e) = CATALOG.iter().find(|e| e.id == id) {
            let ok = match &e.target {
                // Zaten açıksa (ör. Netflix sekmesi) yenisini açma, o pencereyi öne getir
                Target::Web(url) => match crate::context::find_window(e.id) {
                    Some(w) => {
                        crate::context::bring_to_front(w);
                        true
                    }
                    None => open_web(url),
                },
                Target::App { protocol, uri, fallback } => {
                    if protocol_installed(protocol) {
                        shell_open(uri)
                    } else {
                        fallback.is_some_and(|u| open_web(u))
                    }
                }
            };
            log!("Başlatıldı: {} ({})", e.name, if ok { "ok" } else { "başarısız" });
            return ok.then(|| (e.name.to_string(), e.media));
        }
        if id.starts_with("steam:") || id.starts_with("epic:") {
            let g = games().into_iter().find(|g| g.id == id)?;
            let ok = shell_open(&g.uri);
            log!("Oyun başlatıldı: {} ({})", g.name, if ok { "ok" } else { "başarısız" });
            return ok.then_some((g.name, false));
        }
        let found = self.with_apps(|apps| apps.iter().find(|a| a.id == id).map(|a| (a.name.clone(), a.target.clone())));
        let (name, path) = found?;
        let ok = shell_open(&path);
        log!("Başlatıldı: {name} ({})", if ok { "ok" } else { "başarısız" });
        ok.then_some((name, false))
    }

    /// Servis içinde arama (sorgu PC'de kodlanır; telefon adres gönderemez)
    pub fn search(&self, id: &str, query: &str) -> Option<String> {
        let q = query.trim();
        if q.is_empty() || q.chars().count() > 200 {
            return None;
        }
        let e = CATALOG.iter().find(|e| e.id == id)?;
        let app = matches!(&e.target, Target::App { protocol, .. } if protocol_installed(protocol));
        let ok = match (app, e.search_app, e.search_web) {
            (true, Some(tpl), _) => shell_open(&tpl.replace("{q}", &encode(q))),
            (_, _, Some(tpl)) => open_web(&tpl.replace("{q}", &encode(q))),
            _ => false,
        };
        log!("Arama: {} ({})", e.name, if ok { "ok" } else { "başarısız" }); // sorgunun kendisi günlüğe yazılmaz
        ok.then(|| e.name.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arama_sorgusu_kodlanir() {
        assert_eq!(encode("lofi hip hop"), "lofi%20hip%20hop");
        assert_eq!(encode("ığüşöç"), "%C4%B1%C4%9F%C3%BC%C5%9F%C3%B6%C3%A7");
        // Adres enjeksiyonu denemesi tek bir sorgu parametresi olarak kalır
        assert_eq!(encode("a&b=c#x/../"), "a%26b%3Dc%23x%2F..%2F");
    }

    #[test]
    fn steam_manifesti_okunur() {
        let acf = "\"AppState\"\n{\n\t\"appid\"\t\t\"730\"\n\t\"name\"\t\t\"Counter-Strike 2\"\n\t\"StateFlags\"\t\t\"6\"\n}";
        let g = steam_game_from_manifest(acf).unwrap();
        assert_eq!((g.id.as_str(), g.name.as_str(), g.uri.as_str()), ("steam:730", "Counter-Strike 2", "steam://rungameid/730"));
        // yarım kurulu ve araçlar listelenmez; sayısal olmayan appid reddedilir
        assert!(steam_game_from_manifest(&acf.replace("\"6\"", "\"1026\"")).is_none());
        assert!(steam_game_from_manifest(&acf.replace("Counter-Strike 2", "Steamworks Common Redistributables")).is_none());
        assert!(steam_game_from_manifest(&acf.replace("\"730\"", "\"7 30&x\"")).is_none());
    }

    #[test]
    fn epic_manifesti_okunur() {
        let item = r#"{"DisplayName":"Fall Guys","AppName":"Fortnite_Cat","CatalogNamespace":"ns1","CatalogItemId":"it2","AppCategories":["public","games","applications"],"bIsIncompleteInstall":false}"#;
        let g = epic_game_from_manifest(item).unwrap();
        assert_eq!(g.uri, "com.epicgames.launcher://apps/ns1%3Ait2%3AFortnite_Cat?action=launch&silent=true");
        assert!(epic_game_from_manifest(&item.replace("\"games\",", "")).is_none()); // oyun değil (DLC / araç)
        assert!(epic_game_from_manifest(&item.replace("false", "true")).is_none());
        assert!(epic_game_from_manifest(&item.replace("ns1", "a&b")).is_none()); // adres enjeksiyonu
    }

    #[test]
    fn magaza_uygulamalari_okunur() {
        let json = r#"[{"Name":"Hesap Makinesi","AppID":"Microsoft.WindowsCalculator_8wekyb3d8bbwe!App"},{"Name":"Dropbox promotion","AppID":"X!App"},{"Name":"Kötü","AppID":"a b&c!App"}]"#;
        let apps = parse_uwp(json).unwrap();
        assert_eq!(apps.len(), 1);
        assert_eq!(apps[0].target, "shell:AppsFolder\\Microsoft.WindowsCalculator_8wekyb3d8bbwe!App");
        assert!(parse_uwp("").is_none());
    }

    #[test]
    fn katalog_sadece_guvenli_adresler_icerir() {
        for e in CATALOG {
            match &e.target {
                Target::Web(u) => assert!(u.starts_with("https://"), "{}", e.id),
                Target::App { uri, fallback, .. } => {
                    assert!(uri.starts_with("spotify:") || uri.starts_with("steam://"), "{}", e.id);
                    assert!(fallback.is_none_or(|f| f.starts_with("https://")), "{}", e.id);
                }
            }
            for tpl in [e.search_web, e.search_app].into_iter().flatten() {
                assert!(tpl.contains("{q}"), "{}", e.id);
            }
        }
    }
}
