# CouchRemote — Proje Planı (v3)

> iPhone'u **touchpad + klavye + TV kumandası + oyun kolu** olarak kullanıp Windows bilgisayarı yataktan kontrol etmeyi sağlayan, düşük gecikmeli bir sistem.
> Çalışma adı: **CouchRemote** (sonra değiştirilebilir).

### Sürüm geçmişi
- **v1:** Android + yerel uygulama + Bluetooth HID.
- **v2:** iPhone öncelikli, web uygulaması (PWA). Kullanıcının Mac'i yok.
- **v3:** Mantık hataları düzeltildi (Bölüm 13'te özet). HTTPS ilk faza çekildi. Güvenlik modeli yeniden yazıldı. Kestirmeler entegrasyonu, yatak/TV özellikleri, kalite ve optimizasyon maddeleri eklendi.

---

## 0. Bütçe Kuralı: Şimdilik 0 TL

**Faz 0'dan Faz 5'e kadar her şey ücretsiz araçlarla yapılır.** Para gerektiren her madde ya ertelenir ya da ücretsiz alternatifiyle değiştirilir.

| Kalem | Ücretsiz çözüm (şimdi) | Ücretli seçenek (ileride, isteğe bağlı) |
|---|---|---|
| Geliştirme araçları | Rust, Node.js, Vite, Svelte, VS Code, Playwright: hepsi ücretsiz | — |
| HTTPS sertifikası | **Yerel CA + iPhone profili** (merkezi servis yok, internet gerekmez) | — |
| Sanal gamepad | ViGEmBus (ücretsiz, açık kaynak) | — |
| Test cihazı | Kendi iPhone'un + kendi PC'n | — |
| 240 fps gecikme testi | iPhone kamerasının **Ağır Çekim** modu | — |
| Kestirmeler / Siri | iOS'ta yerleşik, ücretsiz | — |
| Kod imzalama | Kişisel kullanımda gerekmez: SmartScreen'de bir kez "Ek bilgi → Yine de çalıştır". Açık kaynak yapılırsa **SignPath Foundation** ücretsiz imzalar | Azure Trusted Signing (aylık ~10 $) |
| Kod barındırma ve CI | GitHub (ücretsiz), GitHub Actions (açık kaynak için ücretsiz) | — |
| Faz 6 iOS uygulaması | — (ertelendi) | Mac + Apple Developer (yıllık 99 $) |
| Live Activity push sunucusu | Ücretsiz katmanlar (Cloudflare Workers vb.) muhtemelen yeter | — |
| Faz 7 USB dongle | — (ertelendi) | ESP32-S3 kartı (~200–300 TL) |

**Sonuç:** Faz 0–5 ve Kestirmeler entegrasyonu **tamamen ücretsiz**. İlk para harcama noktası Faz 5.5'te genel kullanıcılara dağıtım (isteğe bağlı) ve Faz 6'daki Mac ile Apple hesabıdır.

---

## 1. Hedef ve Rakiplerden Farkımız

| Rakip | Eksik yanı | Bizim çözüm |
|---|---|---|
| Unified Remote | Eski arayüz, gamepad yok, iyi özellikler ücretli | Modern arayüz, sanal Xbox kolu, Siri/Kestirmeler |
| Remote Mouse | Gecikme ve "kaygan" imleç (çift hızlanma) | Mutlak konumlandırma, milimetre bazlı hassasiyet |
| KDE Connect | Touchpad hissi zayıf, iPhone desteği kısıtlı | 120 Hz touchpad, iPhone öncelikli tasarım |
| PC Remote (Monect) | Reklam dolu, karmaşık | Reklamsız, sade, yatak kullanımı için tasarlanmış |

**Temel ilkeler**
1. **Gecikme her şeyden önce gelir.**
2. **Bir kez eşleş, hep bağlan.** Simgeye dokun, hazırdır.
3. **Doğru ekran otomatik gelir.** Netflix → kumanda, oyun → gamepad, masaüstü → touchpad.
4. **Her şey atanabilir.** Tuşlar, hareketler, makrolar, Siri komutları.
5. **Güvenlik varsayılan olarak açık.** Bu program klavye ve fare kontrol ediyor; açık kalan bir kapı tüm bilgisayar demek.

---

## 2. Sistem Mimarisi

```
┌───────────────────────────────┐                         ┌───────────────────────────────────────┐
│ iPHONE                        │                         │ PC PROGRAMI (Windows, Rust, tek .exe)  │
│                               │  HTTPS + WSS             │                                       │
│ Web uygulaması (Ana Ekran)    │ ── girdi durumu ──────▶ │ • HTTPS sunucu (yerel sertifika)       │
│  • Touchpad / klavye          │                          │ • WSS sunucu (+ Faz 5: WebRTC)         │
│  • Kumanda / medya            │ ◀─ "şu an çalıyor", ─── │ • Girdi: SendInput (mutlak konum)      │
│  • Gamepad                    │    profil, önizleme      │ • Sanal Xbox kolu (ViGEmBus, opsiyonel)│
│  • Başlatıcı, makrolar        │                          │ • Medya (SMTC), ses (Core Audio)       │
│  • Service Worker: çevrimdışı │                          │ • Aktif pencere → otomatik profil      │
│    arayüz                     │                          │ • mDNS: couch-xxxx.local               │
│                               │  HTTPS (Kestirmeler API) │ • Kestirmeler API (kapsamlı jetonlar)  │
│ iOS Kestirmeler uygulaması    │ ──────────────────────▶ │ • Tepsi simgesi, Windows ile açılış    │
│  • Siri, Eylem Düğmesi        │                          │                                       │
│  • Kontrol Merkezi, otomasyon │                          │ Ayar sayfası: aynı web arayüzü          │
└───────────────────────────────┘                         │ (parola korumalı, localhost)            │
                                                          └───────────────────────────────────────┘
```

- Arayüz PC'den sunulur. PC programı güncellenince telefon arayüzü de güncellenir.
- Aynı arayüz Android'de de çalışır.
- **Kestirmeler entegrasyonu** sayesinde Siri, Eylem Düğmesi ve Kontrol Merkezi desteği **Mac olmadan** web aşamasında gelir.

---

## 3. Teknoloji Seçimi

| Katman | Seçim | Not |
|---|---|---|
| PC programı | **Rust** (`tokio`, `axum`, `rustls`, `windows` crate) | Tek `.exe`, GC duraklaması yok |
| Arayüzün gömülmesi | `rust-embed` | Dosya adları sürüm özetli (hash) |
| Sertifikalar | `rcgen` | Yerel sertifika otoritesi (CA) + sunucu sertifikası üretimi |
| mDNS | `mdns-sd` | Programın kendi sabit adı: `couch-<id>.local` |
| Tepsi simgesi | `tray-icon` | |
| Web arayüzü | **TypeScript + Svelte + Vite** | Dokunma işleme saf TS; paket < 100 KB (gzip) |
| Hızlı kanal | **WSS** (`TCP_NODELAY`) → Faz 5: **WebRTC DataChannel** (`str0m`) | DataChannel sırasız ve tekrarsız (UDP benzeri) |
| Paket formatı | Kendi ikili, **durum tabanlı** formatımız | Bölüm 6.1 |
| Kimlik doğrulama | WebCrypto ECDSA P-256 (dışa aktarılamaz anahtar, IndexedDB) | HTTPS sayesinde mümkün |
| Sanal gamepad | ViGEmBus (Xbox 360), **isteğe bağlı sürücü** | Yönetici izni gerekir |
| Testler | Rust birim testleri, Playwright (**WebKit**), ortak protokol test verileri | |
| Faz 6 | Swift + SwiftUI | Mac + Apple Developer hesabı |

---

## 4. Kurulum, Eşleşme ve Otomatik Bağlantı

### 4.1 Neden ilk günden HTTPS?
- Wake Lock, Service Worker (çevrimdışı ekran), pano, WebCrypto, jiroskop ve kamera **sadece güvenli bağlantıda** çalışır.
- Tarayıcı depolaması **şema + ad + port** (origin) ile eşleştirilir. HTTP'den HTTPS'e sonradan geçmek tüm eşleşmeleri ve ana ekran simgelerini bozar.
- Bu yüzden ilk günden HTTPS kullanılır ve adres hiç değişmez: `https://couch-<id>.local:<port>`.

### 4.2 Yerel sertifika yaklaşımı
- PC programı ilk açılışta kendi **kök sertifikasını (CA)** üretir.
  - Bu sertifikaya **ad kısıtlaması** (name constraint) konur: sadece `couch-<id>.local` için geçerlidir. Anahtar çalınsa bile başka siteleri taklit etmekte kullanılamaz.
- Sunucu sertifikası 1 yıldan kısa süreli olur (iOS'un 398 gün kuralı) ve **otomatik yenilenir**.
- iPhone'a bir kez yapılandırma profili (`.mobileconfig`) yüklenir:
  1. Profili indir.
  2. Ayarlar → Profil İndirildi → Yükle.
  3. Genel → Hakkında → Sertifika Güven Ayarları → aç.
  - Kurulum sayfası her adımı ekran görüntüleriyle gösterir.
- **Nihai karar (Faz 1): Ortak/merkezi hiçbir servis yok. Varsayılan yöntem yerel sertifika profilidir.**
  - Gerekçe: Her kurulum tamamen bağımsızdır. İnternet gerekmez, hiçbir veri dışarı çıkmaz, bakım gerektiren sunucu yoktur.
  - Kullanıcıya düşen: ilk kurulumda bir kez ~4 dokunuş (profili indir → yükle → güven). Kurulum sayfası adımları gösterir, güveni kendiliğinden algılar; "Doğrulanmadı" uyarısının neden normal olduğunu açıklar.
  - DuckDNS seçeneği de kaldırıldı: tek yöntem profil.
- **Değerlendirilip reddedilen: CouchRemote adres servisi** (Plex'in `plex.direct` yaklaşımı). Kullanıcı sıfır adım atardı ama merkezi bir bağımlılık getirirdi:
  1. PC programı ilk açılışta servisimize kendi rastgele kimliğiyle kaydolur, örn. `a7f3k2.<ana-ad>`. Kayıt anahtarı PC'de DPAPI ile saklanır.
  2. Servis bu adın A kaydını PC'nin **yerel IP'sine** yönlendirir ve Let's Encrypt DNS-01 doğrulaması için `_acme-challenge` TXT kaydını ekleyip siler.
  3. PC programı sertifikayı kendi anahtarıyla alır. **Gizli anahtar PC'den hiç çıkmaz**, trafik servisten geçmez; servis sadece DNS kaydı tutar.
  4. Servis her kurulumun sadece kendi adına dokunabilmesine izin verir; hız sınırı uygulanır.
  - **Maliyet 0:** Ana ad olarak deSEC'in ücretsiz `<isim>.dedyn.io` adı, servis olarak Cloudflare Workers ücretsiz katmanı kullanılır. Hesapları **sadece geliştirici bir kez** açar.
  - **Sınır:** Let's Encrypt aynı kayıtlı ad altında haftada 50 yeni sertifika verir (≈ haftada 50 yeni kullanıcı). Aşılırsa kendi alan adı alınır (yıllık ~10 $) ve Public Suffix List'e başvurulur (DuckDNS'in sınırsız çalışabilmesinin nedeni bu listede olması).
  - **İleri düzey seçenekler** (yönetim sayfasında gizli bölüm): kullanıcının kendi DuckDNS adı veya yerel sertifika profili. İnternetsiz kullanım ve servise güvenmek istemeyenler için.
- **Önceki karar (Faz 0 sonrası, yerini yukarıdakine bıraktı):** Faz 0'da yerel sertifika profili kullanılır (tek kullanıcı, hesap gerekmez). **Faz 1'den itibaren varsayılan yöntem ücretsiz alan adı + Let's Encrypt** olur, profil yedek olarak kalır:
  - **DuckDNS** (veya yedek olarak **deSEC.io**) ücretsiz alt alan adı, örn. `isim.duckdns.org` → PC'nin **yerel IP'si**. IP değişirse PC programı adresi günceller.
  - PC programı Let's Encrypt'ten **DNS-01** doğrulamasıyla gerçek bir sertifika alır ve her 90 günde bir otomatik yeniler. Port açmaya gerek yoktur; dışarıdan bilgisayara erişim olmaz.
  - Kullanıcı profil yüklemez: QR → Ana Ekrana Ekle → bitti. Kullanıcıdan tek istenen, bir kez DuckDNS'e giriş yapıp token'ı PC programına girmek.
  - Kısıtlar:
    - İlk ad çözümlemesi için internet gerekir.
    - Ad, sertifika şeffaflık kayıtlarında herkese açık görünür (yerel IP'yi gösterdiği için risk oluşturmaz).
    - Bazı modemlerin "DNS rebinding koruması" yerel IP'ye çözümlenen adları engeller. Bu durum kurulumda test edilir; engelleniyorsa profile geri düşülür. *Geliştiricinin modeminde test edildi: engel yok.*
    - iCloud Özel Geçiş açıkken ad çözümlemesi Faz 1'de test edilecek.
  - Adres (origin) Faz 0'dan farklı olacağı için Faz 1'de bir kez yeniden eşleşme gerekir. Faz 1'de PC programı zaten yeniden yazılacak.

### 4.3 İlk kurulum akışı (bir kez, ~2 dakika)
1. `CouchRemote.exe` çalıştırılır. Program şunları yapar:
   - Windows başlangıcına eklenir.
   - Güvenlik duvarı kuralını ekler.
   - **Windows ağ profilini kontrol eder.** Ağ "Genel" ise uyarır ve tek tıkla "Özel"e çevirmeyi önerir.
   - Ekranda bir QR kod gösterir.
2. QR → Safari'de **HTTP kurulum sayfası** açılır. Bu sayfa sadece profili ve talimatları sunar; başka hiçbir işlem yapmaz.
3. Profil yüklenip güvenilir yapıldıktan sonra sayfa `https://couch-<id>.local:<port>/pair?c=<kod>` adresine geçer.
4. **"Paylaş → Ana Ekrana Ekle"** animasyonlu olarak gösterilir. Manifest dosyası her eşleşme için dinamik üretilir ve `start_url` eşleşme kodunu taşır.
5. Ana ekrandaki simgeden ilk açılış:
   - Uygulama **sadece `navigator.standalone === true` iken** kodu kullanır. Safari'de açıkken kod asla harcanmaz.
   - Cihaz kendi anahtar çiftini üretir ve açık anahtarını PC'ye kaydeder.
   - PC'de "iPhone eşleşti ✓" bildirimi çıkar.
6. **Kodun ömrü 10 dakikadır.** Süre dolmuşsa veya uygulama silinip yeniden eklenmişse **6 haneli PIN** ile eşleşilir (PIN PC ekranında görünür).
7. Uygulama her açılışta `start_url`'deki eski kodu görür. Zaten eşleşmişse bu kod yok sayılır.

### 4.4 Sonraki her kullanım
- Simgeye dokun → Service Worker'dan önbellekli arayüz anında açılır. Bağlantı arayüz çizilirken paralel kurulur. **Hedef: ~300 ms'de kullanılabilir.**
- Kimlik doğrulama: PC rastgele bir sayı (challenge) gönderir, cihaz bunu kendi anahtarıyla imzalar. Parola veya jeton ağda hiç gitmez.
- Kopan bağlantı üstel beklemeyle (backoff) sessizce yeniden kurulur. Sayfa görünür olduğunda (`visibilitychange`) anında denenir.
- **PC kapalı veya uykudaysa:** Çevrimdışı arayüz "Bilgisayara ulaşılamıyor" ekranını gösterir. Ekranda nedenler listelenir (kapalı, uyku, farklı Wi-Fi, ağ profili) ve PC açılınca otomatik bağlanılır.
  - Web uygulaması uyandırma paketi (Wake-on-LAN) **gönderemez**; bu özellik Faz 6 / Faz 7'dedir (Bölüm 7.9).

### 4.5 iOS'a özgü tuzaklar
- Ana ekran uygulamasının depolaması Safari'den **ayrıdır**. Eşleşme bu yüzden ana ekran uygulamasının içinde yapılır.
- Ana ekran uygulamaları Safari'nin 7 günlük veri silme kuralından muaftır.
- `start_url` davranışı iOS sürümüne göre değişebilir. **Faz 0'da gerçek cihazda doğrulanacak.**
- Ekran yönü kilitlenemez (Bölüm 7.5).

---

## 5. Güvenlik Modeli

| Tehdit | Önlem |
|---|---|
| **PC'de açılan zararlı site** `wss://localhost` veya ayar API'sine bağlanır ("drive-by" saldırı) | Her HTTP/WS isteğinde **`Origin` ve `Host` başlığı kontrolü** (izin listesi: sadece kendi adresimiz). Ayar sayfası **parola korumalı**, oturum çerezi `SameSite=Strict`. Durum değiştiren her istekte CSRF jetonu |
| DNS rebinding | `Host` başlığı sadece `couch-<id>.local`, `localhost`, `127.0.0.1` olabilir |
| Aynı Wi-Fi'deki kötü niyetli kişi | TLS şifreleme + cihaz anahtarıyla challenge imzası. Eşleşmemiş bağlantı hiçbir komut çalıştıramaz |
| Eşleşme kodunun ele geçirilmesi | Tek kullanımlık, 10 dakika ömür, **eşleşme penceresi sadece PC'de "Cihaz ekle" açıkken** aktif |
| "URL aç" butonuyla komut çalıştırma | Sadece `http:` / `https:` şemalarına izin verilir. `file:`, `ms-*:`, `shell:` gibi protokoller engellenir |
| Komut/betik butonları | Varsayılan **kapalı**; PC'de beyaz liste + ilk çalıştırmada onay |
| Kestirmeler API jetonu sızması | **Kapsamlı jetonlar:** her jeton sadece belirli eylemlere izin verir (örn. sadece medya). Tek tek iptal edilebilir, PC'de son kullanım zamanı görünür |
| Sertifika otoritesi anahtarının çalınması | Ad kısıtlaması (sadece kendi `.local` adımız) + Windows DPAPI ile şifreli saklama |
| Programın internete açılması | Varsayılan dinleme sadece yerel ağ arayüzleri. "Özel adres" kontrolü ek bir katmandır, ana savunma değildir. Faz 7'de Tailscale aralığı (`100.64.0.0/10`) açıkça izin listesine eklenir |

---

## 6. Protokol ve Düşük Gecikme

### 6.1 Durum tabanlı protokol
- Butonlar ve tuşlar **olay olarak değil, anlık durum olarak** gönderilir. Her paket basılı butonların bit maskesini ve bir sıra numarasını içerir.
  - Paket kaybolsa bile bir sonraki paket durumu düzeltir.
  - Takılı tuş sorunu kökten çözülür.
- Heartbeat (300 ms) sadece son güvenlik ağıdır. Ağ gelmezse tüm tuşlar bırakılır; ağ dönünce durum paketi durumu yeniden kurar.
- Gamepad paketi değişiklik olduğunda + saniyede 60 kez yaşam sinyali olarak gönderilir.
- Klavye metni (Unicode) ayrı, sıralı bir kanal üzerinden gider. Metin kaybolmamalıdır.

### 6.2 İmleç hareketi (çift hızlanma sorunu)
- Windows, göreli `SendInput` hareketine kendi **"işaretçi hassasiyetini artır"** hızlanmasını da uygular. Biz de kendi eğrimizi uygularsak imleç tutarsız hissedilir.
- **Çözüm:**
  - PC programı imleç konumunu kendisi hesaplar ve `MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK` ile **mutlak konum** gönderir.
  - Hızlanma eğrisi tek bir yerde (bizde) uygulanır.
  - Küsuratlı hareketler biriktirilir (sub-pixel accumulation). Yavaş hareketler kaybolmaz.
- Telefon hareketi **milimetre** olarak gönderir (CSS pikseli × cihaz ölçeği). Böylece iPhone mini ile Pro Max aynı hassasiyeti verir.
- Kaydırma yüksek çözünürlüklüdür: 120 birimin altında adımlar gönderilir, Chrome ve Edge'de akıcı kaydırma sağlar. Desteklemeyen uygulamalar için biriktirilip 120'lik adımlar halinde gönderilir.

### 6.3 Gecikme bütçesi

**Hedef:** Parmaktan imlece **p95 < 20 ms** (WSS), **p95 < 15 ms** (WebRTC / iOS uygulaması).

| Aşama | Bütçe | Teknik |
|---|---|---|
| Dokunma algılama | 4–8 ms | `pointermove` doğrudan işlenir, `requestAnimationFrame` beklenmez. Destekleniyorsa `getCoalescedEvents()` |
| Tarayıcıda işleme | < 1 ms | Önceden ayrılmış `ArrayBuffer`. Dinleyiciler `passive:false`. Dokunma koduna Svelte dokunmaz |
| Ağ | 2–8 ms | Küçük ikili paket, `TCP_NODELAY`. Ağ yoğunsa (`bufferedAmount > 0`) deltalar birleştirilir |
| PC'de işleme | < 1 ms | Giriş thread'i MMCSS ile yüksek öncelikte |

### 6.4 Gecikmenin doğru ölçülmesi
- Tek yönlü süre, cihaz saatleri senkron olmadığı için **doğrudan ölçülemez**.
  - Ölçüm yöntemi: gidiş-dönüş süresi (RTT) + NTP benzeri saat farkı tahmini (en düşük RTT'li örneklerden).
  - Rapor: ortalama / p95 / p99 / paket kaybı.
- **Gerçek parmaktan ekrana gecikme:** Telefon kamerasının 240 fps ağır çekim modu ile parmak ve imleç aynı karede çekilir, kare farkı sayılır. Bu test her fazın sonunda tekrarlanır.

### 6.5 Kullanıcıya gösterilen ipuçları
- Canlı ping göstergesi.
- 2.4 GHz ağ uyarısı.
- **PC kabloyla (Ethernet) bağlıysa** havadaki yol yarıya iner. Kurulumda önerilir.
- **iPhone Düşük Güç Modu** ekranı 60 Hz'e kilitler. Tespit edilemezse tanılama sayfasında not düşülür.

---

## 7. Özellikler

### 7.1 Touchpad
- Tek parmak imleç; dokun = sol tık; iki parmakla dokun = sağ tık
- İki parmak ivmeli kaydırma; üç parmak ile sanal masaüstü ve görev görünümü
- Çift dokun + basılı tut = sürükle-bırak; iki parmakla yakınlaştırma (Ctrl + tekerlek)
- Kenar kaydırma çubukları
- **Çoklu monitör:** üç parmakla sağ/sol kaydırınca imleç bir sonraki monitörün ortasına atlar
- **Mutlak (tablet) mod** (Faz 5): Telefon yüzeyi ekranın tamamına karşılık gelir. Ekran önizlemesiyle birlikte kullanılır
- Hassasiyet, hızlanma eğrisi ve kaydırma yönü ayarları

### 7.2 Klavye
- iPhone klavyesi, Türkçe dahil. Metin Unicode olarak gönderilir
- Ek tuş satırı: Esc, Tab, Ctrl, Alt, Win, F1–F12, ok tuşları; kalıcı modifiye tuşlar
- Dikte (iPhone klavyesinin mikrofonu)
- **Kısayollar karakter tabanlıdır:** Profillerde `Shift+.` değil `>` yazılır. PC programı o anki klavye düzenine göre (`VkKeyScanEx`, öndeki pencerenin düzeni) doğru tuş kombinasyonunu üretir. Türkçe Q, F ve İngilizce düzenlerde aynı profil çalışır
- *Not:* Unicode metin bazı oyunlarda çalışmaz (oyunlar tarama kodu okur). Oyun profilinde klavye tarama kodu moduna geçer

### 7.3 TV Kumandası / Medya
- Büyük D-pad, Tamam, Geri, Oynat/Duraklat, ±10 sn, ses, sessiz, tam ekran
- **"Şu an çalıyor" kartı** (SMTC): başlık, kapak, ilerleme
  - **Sürüklenebilir ilerleme çubuğu** (`TryChangePlaybackPositionAsync`, uygulama destekliyorsa)
  - Kapak görselinin renklerine göre arayüz rengi
- **Arama kutusu:** Telefonda yaz → PC'de doğrudan YouTube/Netflix/Spotify araması açılır (URL kalıbı)
- Uygulama bazında ses mikseri, ses çıkış cihazı değiştirme

### 7.4 Uygulamaya Özel Profiller
Öndeki pencere algılanınca otomatik açılır; tam ekran video algılanırsa kumanda ekranı öne gelir.

| Profil | Tuşlar (karakter tabanlı) |
|---|---|
| YouTube | Oynat/Duraklat `k`, ±10 sn `j`/`l`, altyazı `c`, hız `<` `>`, tam ekran `f`, sonraki `N` |
| Netflix | Oynat `Space`, ±10 sn `←`/`→`, tam ekran `f`, intro atla `s` |
| Prime / Disney+ / Twitch | Hazır düzenler |
| VLC / MPC-HC | Altyazı/ses kanalı, hız |
| Spotify | Karışık, tekrar, beğen |

Profiller JSON'dur ve programla birlikte güncellenir. Faz 5'te tarayıcı eklentisi ile derin kontrol gelir (intro atla, bölüm seç).

### 7.5 Gamepad
- **Sanal Xbox 360 kolu (ViGEmBus).**
  - Bu **isteğe bağlı bir sürücü kurulumudur** ve yönetici izni ister. "Tek dosya, kurulumsuz" sözü bu özellik için geçerli değildir.
  - Kurulum sihirbazı ilk gamepad kullanımında açılır.
- 2 analog çubuk, D-pad, ABXY, LB/RB, LT/RT, Start/Select, çoklu dokunma
- **Yatay düzen sorunu:** iPhone'da web uygulaması ekranı yatay kilitleyemez.
  - Telefon dikey tutuluyorsa arayüz CSS ile 90° döndürülerek çizilir. Koordinatlar da buna göre çevrilir.
  - Kullanıcı döndürme kilidini açmak zorunda kalmaz.
- **Güvenli bölgeler:** Butonlar ekranın üst ve alt kenarından uzak tutulur. iOS'un Kontrol Merkezi ve ana ekran hareketleri oyun girdisini yutmaz.
- **Sağ çubuk = touchpad modu** (Steam Deck tarzı): Fareyle nişan alınan oyunlar için sağ bölge göreli fare hareketi gönderir
- Düzen editörü; 2–4 telefon = 2–4 oyuncu
- *Web kısıtı:* titreşim yok. Gerçek titreşim Faz 6'da

### 7.6 Başlatıcı, Butonlar ve Makrolar
- Yüklü uygulamalar ve **Steam/Epic oyunları** simgeleriyle
- Buton tipleri:
  - Tuş kombinasyonu
  - Metin yazma
  - Uygulama / URL açma (sadece `http(s)`)
  - Komut (beyaz liste)
  - Makro (çok adımlı + bekleme)
- **Makro kaydedici:** PC ayar sayfasında "Kaydet"e basılır, yapılan tuş dizisi kaydedilip bir butona atanır
- **"TV'ye geç" hazır makrosu:** Ekranı TV'ye yansıt (Win+P veya doğrudan ekran API'si) → ses çıkışını HDMI'ya al → Steam Big Picture'ı aç. Tersi: "Masaya dön"
- Sürükle-bırak ızgara editörü, profil dışa/içe aktarma (JSON)

### 7.7 iOS Kestirmeler Entegrasyonu (Mac'siz Siri, Eylem Düğmesi, Kontrol Merkezi)
- PC programı küçük bir **HTTPS API** sunar: `POST /api/v1/action/<eylem-id>`, kapsamlı jeton ile.
- Kestirmeler uygulamasının **"URL içeriğini al"** eylemi bu API'yi çağırır.
  - Profille yüklenen sertifika sistem genelinde güvenilir olduğu için bu istek HTTPS ile çalışır.
- **Hazır kestirme paketi:** iCloud bağlantısıyla paylaşılır. İçe aktarırken PC adresini ve jetonu sorar ("içe aktarma soruları"). Hazırlamak için Mac gerekmez; iPhone'da yapılır.
  - PC ayar sayfası jetonu ve adresi kopyalamaya hazır gösterir.
- Bununla mümkün olanlar:
  - "Hey Siri, bilgisayarda Netflix'i aç", "Hey Siri, bilgisayarın sesini kıs"
  - **Eylem Düğmesi** (iPhone 15 Pro+) → oynat/duraklat
  - **Kontrol Merkezi ve kilit ekranı düğmeleri** (iOS 18+: kestirmeler buraya eklenebilir)
  - Ana ekran kestirme widget'ı
  - **Otomasyonlar:** "Saat 01:00'de PC'yi uyut", "Eve gelince Film modunu hazırla", "Alarm çalınca PC'de müziği başlat"
- Her kullanıcı makrosu API'de bir eylem olarak otomatik görünür.

### 7.8 Yatak ve Gece Kullanımı
- **Yatak modu:**
  - Kırmızı/kehribar gece teması, saf siyah arka plan
  - Arayüz parlaklığı minimum
  - Touchpad'de parlak dokunma izi yok, animasyonlar kısık
- **Uyku zamanlayıcısı:** "Film bitince" veya "X dakika sonra". Son 2 dakikada ses **yavaşça kısılır**, sonra medya durur ve PC uyku/kapanma moduna geçer
- Ekranın kararmaması (Wake Lock, HTTPS sayesinde); ayarlanabilir süre

### 7.9 Sistem Kontrolleri
- Uyku, kapat, yeniden başlat, kilitle, ekranı kapat
- Pano paylaşımı (telefon ↔ PC; HTTPS sayesinde Clipboard API)
- **Dosya gönderme:** telefondan fotoğraf/dosya → PC'deki "CouchRemote Gelenler" klasörü
- Ekran görüntüsü alıp telefona gönderme
- **Wake-on-LAN (PC'yi uzaktan açma):** Web uygulaması ve Kestirmeler UDP paketi gönderemez. Bu yüzden:
  - **Faz 6:** iOS uygulaması doğrudan gönderir.
  - **Faz 7:** USB dongle / ağdaki başka bir cihaz aracı olarak gönderir.
  - Geçici çözüm: Modemin kendi "Wake-on-LAN" özelliği (varsa) kurulum rehberinde anlatılır.
  - *Not:* Wi-Fi'ye bağlı PC'lerin çoğu WoL desteklemez; genelde Ethernet gerekir. Uyku modundan uyandırma daha olasıdır.

### 7.10 Sesli Komut
- Web uygulamasında mikrofon butonu, Safari konuşma tanıma API'si ile
- Komutlar: "Netflix aç", "sesi kıs", "30 dakika sonra kapat", "YouTube'da lofi ara"
- Basit komut eşleştirme (kural tabanlı). İleride serbest metin için yerel model düşünülebilir

### 7.11 Ekran Önizleme (Faz 5)
- Küçük canlı önizleme: Windows Graphics Capture + donanım H.264 kodlama, WebRTC video kanalıyla gönderilir
- Düşük çözünürlük ve kare hızı, dokununca büyür
- Mutlak (tablet) modla birlikte: önizlemede dokunulan yere imleç gider

### 7.12 Tanılama Sayfası
- Ping, p95, paket kaybı, ağ tipi (2.4/5 GHz tahmini), Windows ağ profili, sertifika durumu, son hatalar
- Tek tuşla "tanılama raporunu kopyala"

---

## 8. Kalite

- **Kod imzalama:** İmzasız, tuş gönderen ve Windows ile açılan bir program **SmartScreen ve antivirüs uyarısı** alır. Kolay kurulum hedefini en çok bu bozar.
  - Kişisel kullanımda sorun değil: bir kez "Yine de çalıştır" denir.
  - Dağıtım için ücretsiz çözüm: proje açık kaynak olursa SignPath Foundation. Ücretli alternatif: Azure Trusted Signing (aylık ~10 $).
  - Ayrıca yayın öncesi Microsoft'a "yanlış pozitif" bildirimi yapılır.
- **Güvenli otomatik güncelleme:** PC programı kendini günceller. İndirilen dosyanın **imzası doğrulanır**. Güncellemeler kademeli dağıtılır ve geri alınabilir.
- **Önbellek yönetimi:**
  - Arayüz dosyaları sürüm özetli adlarla sunulur.
  - `index.html` ve manifest `no-cache` ile sunulur.
  - Service Worker yeni sürümü bulunca bir sonraki açılışta etkinleşir. Kullanıcı kullanırken arayüz değişmez.
- **Testler:**
  - Protokol: Rust ve TypeScript'in birlikte kullandığı test verileri (ikili paketler birebir aynı mı?)
  - PC programı: birim testleri (hızlanma eğrisi, klavye düzeni çevirisi, güvenlik başlığı kontrolleri)
  - Arayüz: Playwright + **WebKit** ile dokunma senaryoları
  - Güvenlik: kötü niyetli `Origin`/`Host` ile istek testleri, eşleşme kodu yeniden kullanım testleri
  - Her iOS güncellemesinde gerçek cihazda **elle kontrol listesi** (ana ekrana ekleme, `start_url`, sertifika, dokunma hareketleri)
- **Erişilebilirlik:** VoiceOver etiketleri, minimum 44×44 pt dokunma alanları, yüksek kontrast seçeneği
- **Dil desteği:** Türkçe ve İngilizce (tüm metinler çeviri dosyasında)
- **Günlük kaydı:** PC'de dönen (rotating) log dosyası; kişisel veri ve yazılan metin **loglanmaz**

---

## 9. Optimizasyon Özeti

| Alan | Optimizasyon |
|---|---|
| Protokol | Durum tabanlı bit maskesi, sıra numarası, ikili format, değişiklik bazlı gamepad paketi |
| İmleç | Mutlak konum, tek hızlanma eğrisi, küsurat biriktirme, milimetre bazlı delta |
| Kaydırma | Yüksek çözünürlüklü tekerlek + eski uygulamalar için biriktirme |
| Web | Dokunma yolunda çerçeve yok, `passive:false`, bellek ayırmasız, paket < 100 KB |
| Açılış | Service Worker önbelleği + paralel bağlantı → ~300 ms |
| PC | MMCSS öncelikli giriş thread'i, `TCP_NODELAY`, Faz 5'te WebRTC DataChannel |
| Pil | Kumanda ekranında dokunma yokken seyrek heartbeat; önizleme sadece görünürken |
| Ağ | Ethernet önerisi, 2.4 GHz uyarısı, Düşük Güç Modu notu |

---

## 10. Proje Klasör Yapısı

```
couch-remote/
├── PLAN.md
├── protocol/               ← paket formatı + ortak test verileri
├── agent/                  ← Rust PC programı
│   ├── src/net/            ← HTTPS, WSS, (Faz 5) WebRTC, mDNS
│   ├── src/tls/            ← yerel CA, sertifika üretimi/yenileme, .mobileconfig
│   ├── src/auth/           ← eşleşme, cihaz anahtarları, jetonlar, Origin/Host kontrolü
│   ├── src/input/          ← mutlak imleç, klavye düzeni çevirisi, Unicode
│   ├── src/gamepad/        ← ViGEm istemcisi
│   ├── src/media/          ← SMTC, Core Audio, uyku zamanlayıcısı
│   ├── src/launcher/       ← uygulama / Steam / Epic
│   ├── src/profiles/       ← profil motoru, pencere algılama, makro kaydedici
│   ├── src/api/            ← Kestirmeler API
│   ├── src/capture/        ← (Faz 5) ekran önizleme
│   └── src/tray/           ← tepsi, başlangıç, ağ profili kontrolü, güncelleme
├── web/                    ← Svelte + TS arayüz (PC programına gömülür)
│   ├── src/input/          ← touchpad, gamepad (saf TS)
│   ├── src/net/            ← bağlantı, challenge imzası, yeniden bağlanma
│   ├── src/sw/             ← Service Worker
│   ├── src/i18n/           ← tr / en
│   └── src/screens/        ← touchpad, kumanda, gamepad, başlatıcı, editörler, tanılama, kurulum
├── shortcuts/              ← hazır Kestirme tanımları ve iCloud bağlantıları
├── ios/                    ← (Faz 6) Swift uygulaması, widget'lar, Watch
├── relay/                  ← (Faz 6) Live Activity push aracı sunucusu
├── browser-extension/      ← (Faz 5) YouTube/Netflix derin kontrol
└── tools/latency-bench/    ← RTT + saat farkı ölçümü, 240 fps test talimatı
```

---

## 11. Yol Haritası

### Faz 0 — Prototip ve Doğrulama Deneyleri (1–2 hafta)
Amaç: Riskli varsayımları kod yazmaya başlamadan önce **gerçek iPhone'da** doğrulamak.
- [ ] Rust: HTTPS + WSS sunucu, mutlak konumla imleç hareketi, küsurat biriktirme
- [ ] Web: tek ekranlık touchpad (milimetre bazlı delta)
- [ ] **Deney 1:** Yerel CA profili iPhone'a yüklenip `https://couch-xxxx.local` Safari'de ve ana ekran uygulamasında sorunsuz açılıyor mu?
- [ ] **Deney 2:** Dinamik manifest `start_url`'i ana ekrana eklerken korunuyor mu? (iOS sürümü not edilir)
- [ ] **Deney 3:** Ana ekran uygulamasında Wake Lock, Service Worker, Clipboard ve `getCoalescedEvents` çalışıyor mu?
- [ ] **Deney 4:** Kestirmeler "URL içeriğini al" yerel HTTPS adresine istek atabiliyor mu?
- [ ] Gecikme ölçüm aracı (RTT + saat farkı) + 240 fps kamera testi
- **Çıkış kriteri:** p95 < 20 ms, imleç akıcı, 4 deneyin sonucu belli. Olumsuz sonuç çıkarsa plan revize edilir.

#### Faz 0 sonuçları (2026-09-30, iPhone · iOS 26.5, PC Ethernet'te, Node.js prototipi)
| Ölçüm / deney | Sonuç |
|---|---|
| Tek yön gecikme (dokunma → PC) | **p50 8,6 ms · p95 9,5 ms · p99 17,3 ms** (hedef p95 < 20 ✅) |
| PC işleme süresi | p99 2 ms |
| Deney 1: yerel CA profili + `.local` | ✅ |
| Deney 2: `start_url` kodu ana ekran uygulamasına geçiyor | ✅, ama 10 dk ömür profil kurulumuna yetmedi → PIN yedeği devreye girdi |
| Deney 3: Wake Lock, Service Worker, Clipboard, getCoalescedEvents, dışa aktarılamaz anahtar (IndexedDB), DeviceMotion izni, konuşma tanıma, `input switch` | ✅ hepsi mevcut |
| Deney 4: Kestirmeler | ⏳ bekliyor |
| Yeniden bağlanma (uygulamadan çıkıp dönme) | ✅ |
| Ekran yenileme (rAF) | 59 Hz: Safari varsayılan 60 Hz sınırı. Dokunma verisi toplu okumayla alındığı için gecikmeye etkisi yok |

**Faz 1'e çıkan dersler:**
- Eşleşme kodu Safari'deki eşleşme sayfası her açıldığında yenilenmeli (dinamik manifest yeni kodu taşır). Ömür "son görüntülenmeden itibaren 10 dk" olmalı.
- iOS 26 kullanıcı ajanında işletim sistemi sürümü `18_7` olarak dondurulmuş. Sürüm tespiti `Version/26.x` alanından yapılmalı.

### Faz 1 — Güvenli ve Kolay Bağlantı + Temel Kullanım / MVP (4–5 hafta)
- ~~İleri düzey: kullanıcının kendi DuckDNS adı~~ — **kaldırıldı** (kullanıcı kararı: gereksiz karmaşa)
- [x] **Varsayılan:** Yerel CA (ad kısıtlamalı, anahtar DPAPI ile şifreli) + otomatik yenilenen sunucu sertifikası + `.mobileconfig` kurulum sayfası
- [x] Programın kendi mDNS adı (`couch-<id>.local`) — kendi yanıtlayıcımız (legacy unicast + QU + AAAA için NSEC)
- [x] Windows ağ profili kontrolü (yönetim sayfasında uyarı + "Ağı Özel yap") + güvenlik duvarı kuralı (tek UAC, sadece Özel ağ)
- [x] QR → (profil) → ana ekrana ekleme akışı; kod sadece ana ekran uygulamasında; kayan 10 dk ömür (en fazla 60 dk); PIN yedeği
- [x] Cihaz anahtar çifti + challenge imzası; eşleşmiş cihaz listesi ve iptal
- [x] Güvenlik: Origin/Host kontrolü, yönetim API'sinde Origin + özel başlık (CSRF), DPAPI, **yönetim sayfası anahtarı** (her açılışta yeni; sadece tepsi/programdan açılır, HttpOnly SameSite=Strict çerez). URL şema filtresi → Faz 3 (URL butonlarıyla birlikte)
- [x] Service Worker (anında açılış + "Bilgisayara ulaşılamıyor" ekranı), Wake Lock
- [x] Durum tabanlı protokol, heartbeat, otomatik yeniden bağlanma; mutlak imleç (öz test 36/36 piksel), SendInput yarış durumu düzeltildi
- [x] Touchpad (tüm hareketler: ivmeli kaydırma, 3 parmak ← → masaüstü / ↑ görev görünümü / ↓ masaüstü, 3 parmak dokunma = orta tık), klavye ✓ (Unicode, fark tabanlı: otomatik düzeltme/dikte uyumlu; Ctrl/Alt/Shift/Win, F1–F12; karakter tabanlı kısayollar `VkKeyScanEx`)
- [x] Medya kumandası + "şu an çalıyor" (SMTC: başlık, sanatçı, uygulama, kapak) + sürüklenebilir ilerleme çubuğu + ±10 sn (SMTC, yoksa ←/→)
- [x] Tepsi simgesi (Telefonu bağla / Windows ile başlat / Kaldır / Çıkış), Windows ile açılış, tanılama (yönetim sayfası → Gelişmiş)
- [x] **Kurulum ve dağıtım (bilmeyen biri 2–3 dakikada kurabilmeli):**
  - [x] Tek `CouchRemote.exe` (release: 6,6 MB, konsol penceresi yok, günlük `%APPDATA%CouchRemotecouch.log`), GitHub Releases'ten ücretsiz dağıtım + indirme sayfası (`site/index.html`, GitHub Pages'e hazır; `tools/release.ps1` → `dist/CouchRemote.exe`) — *yayın için GitHub deposu gerekiyor*
  - [x] İlk çalıştırmada kendini `%LOCALAPPDATA%ProgramsCouchRemote`'a kopyalar; Windows başlangıcına ekler (HKCU Run); "Uygulamalar" listesine kaldırma kaydı
  - [x] Güvenlik duvarı kuralı: tek seferlik UAC, sadece Özel ağ; ağ "Genel" ise uyarı + tek tıkla düzeltme
  - [x] Tek örnek: ikinci kez açılırsa yenisi başlamaz, "Telefonunu bağla" ekranı öne gelir
  - [x] İlk açılışta büyük "Telefonunu bağla" QR ekranı; tepsi menüsü: Telefon bağla / Windows ile başlat / Kaldır / Çıkış
  - İmzasız dosya uyarısı (SmartScreen): indirme sayfasında ekran görüntülü açıklama; açık kaynak olursa SignPath Foundation ile ücretsiz imza
- **Çıkış kriteri:** Kurulum < 2 dk. Ertesi gün simgeye dokunup < 1 sn'de bağlanmak. Yataktan film açıp izleyip PC'yi kapatmak.

### Tasarım turu (Faz 1 sonu) — tamamlandı
**Karar:** Apple tarzı, vurgu rengi iOS mavisi (#0A84FF), tüm ekranlar.
- [x] Tasarım tokenları (iOS koyu mod renkleri, cam paneller, sistem fontu); "Hareketi azalt" desteği
- [x] SF Symbols tarzı gömülü SVG ikon seti (uygulama içinde, harici dosya yok)
- [x] Telefon: cam sekme çubuğu (ikon + etiket), üst bilgide bilgisayar adı (COMPUTERNAME) + canlı gecikme, Apple Music tarzı "şu an çalıyor" kartı, Apple TV tarzı yuvarlak yön tuşu, iOS klavye tuşları
- [x] Oyun kolu: cam düğmeler, Xbox renkli ABXY, yaylı çubuk topuzu
- [x] **Yatak modu:** kehribar/kırmızı gece paleti, görseller kısık (tek dokunuşla, hatırlanır)
- [x] Animasyonlar: sekme geçişi, basma geri bildirimi, bağlanırken nabız, açılır pencereler
- [x] **Yeni logo:** mavi→indigo gradyan üzerinde beyaz kanepe (ana ekran simgesi, tepsi simgesi, web sayfaları)
- [x] Kurulum sayfası (iOS Ayarlar tarzı, menü yolu kutucukları, durum kartı), PC yönetim sayfası ve indirme sayfası (açık/koyu)

### Faz 2 — Gamepad — **KALDIRILDI (kullanıcı kararı, 2026-10-01)**
Oyun kolu özelliği (klavye/fare modu + isteğe bağlı ViGEmBus) tamamen çıkarıldı; kod, arayüz ve bağımlılıklar silindi.

### Sürüm 0.2.0 — özellikler ve güvenlik turu (2026-10-01)
**Yeni özellikler:**
- [x] Güç menüsü: ekranı kapat, kilitle, uyut, yeniden başlat / kapat (telefonda onaylı)
- [x] Uyku zamanlayıcısı: 15 dk – 2 sa; son dakikada ses yavaşça kısılır, medya durur, ses seviyesi eski haline döner; sonra durdur+ekranı kapat / uyut / kapat. Üst çubukta geri sayım rozeti
- [x] Ses kaydırıcısı (Core Audio): gerçek sistem seviyesi, dışarıdan değişince de güncellenir, sessiz düğmesi
- [x] Uygulamaya özel kumanda: öndeki pencere algılanır (Netflix, YouTube, YouTube Music, Prime, Disney+, Twitch, Spotify, VLC); "İntroyu atla", hız, altyazı gibi düğmeler. Pencere başlığı telefona gönderilmez, sadece profil
- [x] Ayarlar: imleç ve kaydırma hızı, doğal kaydırma, sol el modu, yatak modu + akşamları otomatik (22–07), titreşim, gecikme ayrıntıları
- [x] Aktar sekmesi: pano (PC → telefon, telefon → PC, "gönder ve yapıştır") ve dosya gönderme (İndirilenlerCouchRemote)

**Güvenlik düzeltmeleri (hepsi uçtan uca testle doğrulandı):**
- [x] S1: eşleşmesi kaldırılan cihazın açık bağlantısı anında kapanır; telefon eşleşme ekranına döner
- [x] S2: kimliksiz `/api/report` kaldırıldı; raporlar sadece doğrulanmış WebSocket'ten, sadece bellekte
- [x] S3: sertifika doğrulama kodu (SHA-256'dan, ör. 9A4C-93A2) profil adında ve PC ekranında; kurulum sayfası karşılaştırmayı söyler (kodu HTTP sayfası göstermez)
- [x] S4: eşleşme kodu açılışta üretilmez, sadece "Telefonu bağla" sayfası açıkken; 5 hatalı PIN → 5 dk kilit + uyarı
- [x] S5: PIN ve eşleşme bilgisi günlüğe yazılmaz
- [x] Dosya yükleme: kısa ömürlü cihaz jetonu, dosya adı temizleme (yol, "..", yasak ad/karakter), üzerine yazmama, 2 GB sınırı (birim testleri)

**Hata düzeltmeleri:**
- [x] B1: güncelleme — yeni sürüm çalıştırılınca eskisinin sürümünü sorar, onayla eskisini kapatıp kendini kurar
- [x] B2: başlangıç hataları uyarı penceresiyle gösterilir (port dolu vb.); başka kullanıcıdan kopyalanmış ayar dosyası yedeklenip sıfırlanır; konsolsuz sürümde Ctrl+C dinleyicisi programı kapatmaz
- [x] B5: aynı telefon aynı IP'den yeniden eşleşince eski kaydı silinir; 90 gündür görülmeyen cihazlar temizlenir

### Sürüm 0.2.1 — kullanıcı gözünden düzeltmeler (2026-10-01)
Kullanıcı gözünden inceleme sonrası, gerçek güncelleme akışıyla (0.2.0 → 0.2.1) kuruldu ve test edildi.
- [x] H1: Windows Ayarlar'dan kaldırılırken önce çalışan program kapatılır (dosya kilidi kalmaz)
- [x] H2: kumanda ve uygulamaya özel düğmeler doğru pencereye gider — son medya penceresi hatırlanır, gerekirse öne getirilir; "İntroyu atla" gibi düğmeler pencere yoksa hiçbir yere yazılmaz, telefonda uyarı çıkar
- [x] H3: "Ekran açık kalsın" ayarı (5/10/30 dk/Her zaman; varsayılan 10 dk) — süre dolunca iPhone kendi kilidine bırakılır
- [x] H4: servis zaten açıksa yeni pencere açılmaz, mevcut pencere öne gelir; açık değilse yeni sekme; "açılınca tam ekran" (zaten tam ekransa dokunmaz)
- [x] H5: yatay düzen — sekmeler solda dikey, touchpad tam genişlik, kumanda iki sütun (ekrana sığar)
- [x] H6: dosyalar Windows'un gerçek İndirilenler klasörüne (OneDrive yönlendirmesi dahil)
- [x] H7: gecikme göstergesi üst çubuktan kalktı (ayarlarda); bilgisayar adı tam görünür
- [x] H8: kurulum anındaki günlük satırları kaybolmuyor
- [x] Başlat menüsü kısayolu (uygulama kimliğiyle); eski kurulumlara da kendiliğinden eklenir, kaldırmada silinir
- [x] Yönetici izninden önce açıklama penceresi
- [x] Dosya gelince Windows bildirimi (tıklayınca klasör açılır)

#### Klavye sekmesi yenilendi (2026-10-01)
- [x] Ekranda tam Türkçe Q klavye (ğ ü ş ı i ö ç; İ/I doğru büyütme), 123 / #+= / Fn katmanları (₺ € dahil, F1–F12, home/end/pg/ins/del/prt sc, Geri al/Kes/Kopyala/Yapıştır/Tümü)
- [x] PC tuş şeridi: esc, tab, ctrl, alt, win, oklar; değiştiriciler tek dokunuş = sonraki tuş, çift dokunuş = kilit (shift kilidi = büyük harf kilidi)
- [x] iOS görünümü: harf balonu, titreşim, sil/boşluk/ok basılı tutunca tekrar; tuşlar parmak değdiği an gönderilir
- [x] Klavye ekranında küçük touchpad (tüm hareketler); yatayda gizlenir, klavye büyür
- [x] İsteğe bağlı "Metin çubuğu" modu: iPhone klavyesi (otomatik düzeltme, dikte); seçim ayar olarak hatırlanır
- [x] tools/preview-server.mjs: PC programı olmadan arayüz önizleme (/preview)

### Sürüm 0.2.2 — güvenlik ve dayanıklılık turu (2026-10-01)
- [x] Ağ sertleştirme (gent/src/net.rs): eşzamanlı bağlantı sınırı (256 / IP başına 48), TLS el sıkışması 10 sn + HTTP başlık 15 sn zaman aşımı, hatalı deneme sınırlayıcı (IP başına dk'da 10)
- [x] Güvenlik başlıkları: katı CSP (uygulama sadece kendi betikleri), X-Frame-Options, Referrer-Policy, COOP/CORP, Permissions-Policy
- [x] WebSocket: kimlik doğrulamadan önce 2 KB mesaj sınırı; bağlantı başına komut hız sınırı (ekran görüntüsü vb. ağır komutlar ayrıca)
- [x] Kestirmeler: hatalı jeton sınırı (429); jeton yenileme; **kayıp telefon**: tepsi menüsü + yönetim sayfasında "Tüm eşleşmeleri kaldır" (jetonu da yeniler)
- [x] Dosya yükleme: Mark-of-the-Web, disk boşluğu kontrolü, en fazla 3 eşzamanlı yükleme
- [x] Cihaz adı temizleme (günlük zehirlenmesi), panik günlüğü, zehirlenmeyen kilitler (sync.rs)
- [x] cargo audit: bilinen açık yok; SECURITY.md (tehdit modeli, kayıp telefon adımları)
- [x] Performans ölçümü: boşta CPU %0,05, RAM ~30 MB, 35 thread (ek optimizasyon gerekmedi)
### Sürüm 0.3 — görsel yeniden tasarım "canlı ve akıcı" (2026-10-01)
Kullanıcı kararları: zengin ve canlı yön, dengeli hareket, kumanda + touchpad/hava faresi + uygulamalar + iskelet, yatak modunda da çalışsın. Tasarım katmanı web/ui.css (app.css'in üstüne biner).
- [x] Hareket sistemi: yay eğrileri (CSS linear(), yedek bezier), süre token'ları, sekme geçişleri yöne göre kayar + içerik sırayla yükselir
- [x] İskelet: durum kapsülü (bağlıyken yayılan halka), cam üst düğmeler, **kayan göstergeli sekme çubuğu**, **kayan göstergeli segment kontrolleri** (CSS-only, :has()), ortam ışığı
- [x] Paneller: yay gibi açılır, **parmakla aşağı çekilince kapanır** (hız + mesafe), bulanık arka plan
- [x] Kumanda: kapaktan baskın renk türetilir (ortam ışığı + kart + oynat düğmesi gölgesi); büyük kapak ve kart içinde ilerleme; uygulamaya özel düğmeler tek satır kaydırmalı; oynat/duraklat simgesi dönüşür; ses + çıkış + mikser tek "ses kartı"; 390×844'te kaydırmasız
- [x] Touchpad / hava faresi: dokunuş dalgası, parmağı izleyen ışık, nokta desenli yüzey, dokunsal tık düğmeleri, hava modunda telefonun yönünü gösteren canlı nokta
- [x] Uygulamalar: gradyanlı/derinlikli kutucuklar, **Steam oyun kapakları** (yerel önbellekten küçültülmüş JPEG, game-art komutu), yatay kapak kartları, "son kullanılanlar", yükleme iskeleti
- [x] Aktar/ayarlar: renkli gradyan rozetler, yükleme çubuğunda ışıltı; çevrimdışı ekranında dalga animasyonu
- [x] Yatak modu: efektler kısılır (kehribar, parlama yok); yatay düzen için ızgara; prefers-reduced-motion tüm hareketi kapatır
- [x] İlk kurulum akışı yenilendi (0.3.1): yönetim sayfasında canlı ilerleme izleyici (QR okutuldu → profil yüklendi → bağlandı, sunucu `setupSeen`/`profileSeen` bildirir), bağlanınca kutlama, "yeni telefon ekle" modu; iPhone kurulum sayfasında güvenlik kodu en üstte (`%%CA_CODE%%`) ve numaralı adım kartları; uygulama içi rehber (zıplayan Paylaş ipucu) ve 6 kutulu otomatik gönderen PIN ekranı
- [ ] Epic oyun kapakları (yerel kapak önbelleği yok; renkli harf kartı gösterilir)
### Faz 3 — Profiller, Makrolar ve Kestirmeler (3–4 hafta)
**Öne çekilen (Faz 1 sonu, kurulum denemesinden önce) — Uygulamalar sekmesi:**
- [x] Hızlı başlat: Netflix, YouTube, Spotify, Prime Video, Disney+, Twitch, YouTube Music, tabii, Exxen, Steam Big Picture (marka rengi + baş harf; resmi logo yok)
- [x] Web siteleri Chrome'da açılır (Chrome yoksa varsayılan tarayıcı); Spotify kurulu uygulamayla (`spotify:` protokolü), yoksa web oynatıcı; Steam `steam://`
- [x] Servis içi arama (YouTube, Netflix, Spotify, Prime Video, Twitch, YouTube Music); sorgu PC'de kodlanır
- [x] Başlat menüsündeki programlar (aranabilir liste; kaldırıcı/belge/sistem araçları süzülür)
- [x] Medya servisi açılınca telefon Kumanda sekmesine geçer
- [x] **Güvenlik (URL şema filtresi):** telefon sadece PC'nin listesindeki kimliği seçer; adres ve dosya yolu gönderemez; sadece https ve bilinen protokoller (birim testleriyle)
- [x] Mağaza (UWP) uygulamaları: Get-StartApps (AppID'sinde "!" olanlar) → shell:AppsFolder ile açılır; 10 dk önbellek
- [x] Profil motoru: öndeki pencereden profil algılama (700 ms), tam ekran durumu telefona bildirilir ("Tam ekran" ↔ "Küçült"), isteğe bağlı "servis öne gelince kumandaya geç"
- [x] YouTube, Netflix, Prime, Disney+, tabii, Exxen, Twitch, VLC, Spotify, YouTube Music profilleri; ±10 sn düğmeleri uygulamanın kendi tuşunu kullanır (YouTube j/l, Netflix/Prime/Disney+ oklar, VLC alt+ok)
- [ ] Buton/makro editörü, makro kaydedici
- [ ] "TV'ye geç" / "Masaya dön" makroları
- [x] Oyunlar bölümü: Steam (kütüphane klasörleri + appmanifest) ve Epic (Manifests/*.item) kurulu oyunları; arama kutusu hem uygulamaları hem oyunları süzer
- [x] **Kestirmeler API** (POST /api/v1/action/{eylem}?v=değer): oynat/sonraki/önceki, ±10 sn sarma, ses aç/kıs/sessiz/ayarla (0–100), uygulama aç, serviste ara, kilitle, ekranı kapat, uyut, uyku zamanlayıcısı. Yeniden başlat/kapat bilerek YOK (jeton sızarsa zarar sınırlı)
- [x] Telefonda Ayarlar → "Siri ve Kestirmeler": jeton başlığı ve her eylemin adresi tek dokunuşla kopyalanır + adım adım rehber
- [ ] Hazır iCloud kestirme paketi: Mac/iCloud bağlantısı olmadan oluşturulamıyor; şimdilik elle kurulum rehberi
- [ ] Kapsamlı (eylem bazlı) jetonlar
- [ ] Gamepad düzen editörü

### Faz 4 — Yatak Modu ve Kolaylıklar (2–3 hafta)
- [x] Yatak modu (kehribar tema; "akşamları otomatik"; iOS parlaklığı web'den ayarlanamaz)
- [x] Uyku zamanlayıcısı + sesin yavaşça kısılması
- [x] Ses çıkış cihazı değiştirme (Kumanda → Ses çıkışı; IPolicyConfig, sadece etkin cihazlar seçilebilir)`n- [x] Ses mikseri: Kumanda → Uygulama sesleri; uygulama başına ses ve sessiz (Windows ses karıştırıcısı oturumları, aynı programın oturumları tek satırda, sistem yardımcıları gizli)
- [x] Pano paylaşımı, dosya gönderme
- [x] Ekran görüntüsü: Aktar → Ekran; tüm ekranlar veya tek monitör (JPEG, en uzun kenar 1920), yakınlaştırma, paylaş/kaydet; Kestirmeler'de `screenshot` eylemi görüntü döndürür
- [ ] Sesli komut
- [ ] Çoklu monitör atlama hareketi
- [x] iOS 18 dokunma titreşimi hilesi (`<input type="checkbox" switch>`)

### Faz 5 — Jiroskop, Önizleme ve Performans (3–4 hafta)
- [x] **Jiroskop / hava faresi:** Touchpad sekmesinde "Hava faresi" modu. Dokunduğun sürece telefonun yönelimi imleci sürer, bırakınca durur (kavrama). Eğim/bilek bükmeden bağımsız, titreme süzülmüş, işaretçi ivmeli, hız ayarı. Çekirdek `web/air.js`, sentetik testler `tools/air-test.mjs` (29 test). Gamepad nişanı yok (gamepad kaldırıldı)
- [ ] Uygulama içi kamerayla QR okuma (yeni PC ekleme)
- [-] ~~**Ekran önizleme** + mutlak (tablet) mod~~ — **YAPILMAYACAK** (kullanıcı kararı, 2026-10-01): CPU/GPU, pil, ağ gecikmesi ve gizlilik maliyeti değmez; tek kare ekran görüntüsü yeterli
- [ ] WebRTC DataChannel (p95 < 15 ms hedefi)
- [ ] Tarayıcı eklentisi (YouTube/Netflix derin kontrol)
- [ ] Çoklu PC desteği
- [ ] **Kilit ekranında PIN girme:** SYSTEM olarak çalışan isteğe bağlı servis + güvenli masaüstü yardımcısı (Windows kilit ekranı ve UAC pencerelerine girdi)

### Faz 5.5 — Yayın Hazırlığı (2 hafta)
- [ ] Kod imzalama: açık kaynaksa **SignPath Foundation (ücretsiz)**; değilse Azure Trusted Signing (ücretli, isteğe bağlı). Antivirüs yanlış pozitif bildirimleri (ücretsiz)
- [ ] İmzalı otomatik güncelleme
- [ ] Türkçe/İngilizce, erişilebilirlik denetimi
- [ ] Test paketinin tamamlanması (protokol, WebKit, güvenlik)
- [ ] ~~Plex tarzı alan adı~~ → Faz 1'e taşındı (DuckDNS + Let's Encrypt, ücretsiz)
- [x] GitHub deposu (özel): github.com/YusufUzun03/couch-remote, MIT lisansı, README/KURULUM/SECURITY, v0.3.0 sürümü (CouchRemote.exe)`n- [ ] Depoyu herkese açma + indirme sayfasını (site/) GitHub Pages ile yayınlama (özel depoda Pages ücretli plan ister)`n- [ ] Web sitesi, kurulum videosu
- [x] İngilizce dil desteği (0.4.0): Türkçe kaynak metin anahtar, `web/i18n-en.js` sözlüğü (~390 giriş), `web/i18n.js` DOM yürüyücüsü + `tl()`; dil telefonu izler, Ayarlar'dan değişir; yönetim/kurulum sayfaları, indirme sitesi, Rust bildirimleri/tepsi (`i18n.rs`, Windows dilini izler), README/INSTALL/SECURITY.en
- [x] Erişilebilirlik turu (0.4.0): WCAG AA kontrast (ipucu metni, dolgulu mavi düğmeler, yatak modu, açık temalı sayfalar), VoiceOver rolleri/etiketleri (touchpad, fare tuşları, paneller, uyarılar, toast, `aria-pressed`/`aria-current`), panel odak yönetimi + Esc, `:focus-visible`, ≥44 pt dokunma alanı, harici klavye ile touchpad, Ayarlar › Erişilebilirlik: imleç yön düğmeleri, `prefers-contrast` ve `prefers-reduced-transparency`
- [x] LG TV kumandası (0.5.0): PC, TV'ye webOS SSAP ile bağlanır (`agent/src/tv.rs`), telefon `tv` komutları yollar. SSDP tarama / adres girme, TV ekranında izinle eşleşme (anahtar DPAPI'li), ses/mute/kanal/oynatma, D-pad ve işaretçi (sihirli kumanda gibi), uygulama ve giriş değiştirme, TV'ye yazma, Wake-on-LAN ile açma. Kumanda sekmesinde Bilgisayar | LG TV seçici. Sahte webOS TV ile testli; gerçek TV ile denenmedi
- [x] Çoklu TV + cihaz seçici (0.5.1): en fazla 8 LG TV kaydedilir (`tvs.json`, eski `tv.json` otomatik taşınır); sol üstteki bilgisayar adı/ikonu artık cihaz seçici (Bilgisayar + TV'ler, durum noktası), TV seçilince Kumanda sekmesi o TV'nin kumandası olur; Ayarlar › Cihazlar'dan TV ekle/sil
- [x] TV markaları (0.6.0): Samsung Tizen (8002/8001 WebSocket, jeton) ve Android/Google TV (Remote v2: RSA istemci sertifikası + 6 karakter kod eşleşmesi) sürücüleri `agent/src/tv/`; markadan bağımsız `Act` eylemleri, arayüz yeteneklere (`caps`) göre düzenlenir. Yalnızca sahte TV'lerle test edildi (`tools/mock-tvs.mjs`); Android TV deneysel
- [x] Marka otomatik tanıma (0.6.2): tarama SSDP + mDNS ile aday toplar, portlardan marka tanınır; elle adres de otomatik
- [x] 0.7.0: seçili TV'yi Touchpad/hava faresi/Klavye de sürer (işaretçisiz TV'lerde parmak = yön tuşları), Siri/Kestirmeler `tv-*` eylemleri, Sahneler (Film/Uyku/Her şeyi kapat), iPad/geniş ekran düzeni
- [ ] Sıradaki: birden fazla bilgisayar (telefonda cihaz listesi) — şimdilik istenmiyor; Roku/diğer markalar — şimdilik istenmiyor

### Faz 6 — Gerçek iOS Uygulaması (Mac gerekir)
**Ön koşullar:** Mac (ikinci el M1 Mac mini yeterli), Apple Developer hesabı (yıllık 99 $), Live Activity için küçük bir **push aracı sunucusu**.

**PC programına eklenecekler:** UDP uç noktası, yerel uygulama için eşleşme, APNs aracı sunucusuyla konuşma. Mevcut protokol ve özellikler aynen kalır. Ancak yerel uygulama ayrı bir istemci olduğu için **bir kez yeniden eşleşme** gerekir.

- [ ] **Kilit ekranı kontrolcüsü / Dynamic Island (Live Activity)**
  - PC'de çalan medya ve kontroller telefon açılmadan görünür.
  - Uygulama arka plandayken güncelleme **APNs push** ile gelir: PC programı → aracı sunucu → Apple. Bu yüzden aracı sunucu gerekir.
- [ ] **Widget'lar:**
  - Ana ekran ve kilit ekranı widget'ları, etkileşimli butonlar (iOS 17+)
  - *Not:* Widget'lar günde sınırlı sayıda yenilenir, "canlı" medya bilgisi için değil hızlı eylemler için tasarlanır.
- [ ] **Kontrol Merkezi düğmeleri** (iOS 18+, yerel)
- [ ] **Siri / App Intents:** Kestirmeler entegrasyonunun yerel ve daha hızlı hali; tüm makrolar otomatik Siri komutu olur
- [ ] **Eylem Düğmesi** (yerel)
- [ ] **CoreMotion jiroskop:** daha hassas air mouse ve nişan
- [ ] **Core Haptics:** gerçek tıklama hissi + oyun kolu titreşimi
- [ ] **Ses tuşlarıyla PC ses kontrolü** (App Store politikası önceden kontrol edilir)
- [ ] **"Paylaş → PC'de aç"** paylaşım uzantısı (YouTube videosu, link, dosya)
- [ ] **Wake-on-LAN** (Network.framework ile UDP)
- [ ] Ekran kenarı hareketlerini erteleme (oyun kolunda iOS hareketleri araya girmez)
- [ ] **Apple Watch uygulaması:** oynat/duraklat, ses, Digital Crown ile ses/kaydırma. İletişim **iPhone üzerinden aktarılır** (watchOS düşük seviye ağ erişimini kısıtlar)
- [ ] iPad düzeni
- [ ] Doğrudan UDP (Network.framework) → en düşük gecikme
- [ ] App Store yayını

### Faz 7 — Genişleme (ileride)
- [ ] macOS / Linux PC programı
- [ ] **USB dongle (ESP32-S3):** PC'ye program kurmadan çalışır (TV, PlayStation, BIOS); ayrıca ağda sürekli açık olduğu için **Wake-on-LAN aracısı** olarak da kullanılır
- [ ] Kendi sanal HID sürücümüz (ViGEm bağımlılığını kaldırmak için)
- [ ] Topluluk profil paylaşımı
- [ ] Ev dışından erişim (Tailscale; `100.64.0.0/10` izin listesine eklenir)

---

## 12. Riskler ve Önlemler

| Risk | Etki | Önlem |
|---|---|---|
| iOS `start_url` davranışı beklenenden farklı | Eşleşme kodu ana ekran uygulamasına geçmez | Faz 0 Deney 2; yedek PIN |
| Kullanıcı sertifika profilini yüklemekte zorlanır | Kurulum terk edilir | Adım adım ekran görüntülü rehber; Faz 5.5'te alan adı alternatifi |
| Windows ağı "Genel" | Hiçbir bağlantı çalışmaz | Kurulumda tespit + tek tıkla düzeltme |
| Bilgisayar adı değişir | Adres değişir | Programın kendi sabit mDNS adı |
| WSS (TCP) paket kaybında gecikme biriktirir | Anlık takılma | Delta birleştirme, durum protokolü; Faz 5'te WebRTC |
| Çift fare hızlanması | Tutarsız imleç | Mutlak konumlandırma (Bölüm 6.2) |
| Zararlı site localhost'a bağlanır | Bilgisayar ele geçirilir | Origin/Host kontrolü, parolalı ayarlar, CSRF |
| Farklı klavye düzenleri | Kısayollar yanlış tuşa basar | Karakter tabanlı kısayollar + `VkKeyScanEx` |
| Ekran yönü kilitlenemez | Gamepad dikey kalır | Döndürülmüş CSS düzeni |
| ViGEmBus geliştirilmiyor ve sürücü kurulumu gerektirir | Uyumluluk / kurulum zorluğu | İsteğe bağlı; soyutlama katmanı; Faz 7'de kendi sürücümüz |
| SmartScreen / antivirüs uyarısı | Kullanıcı programı çalıştırmaz | Kod imzalama, yanlış pozitif bildirimi |
| Kilit ekranı ve UAC'ye girdi gitmez | Yataktan PIN girilemez | Faz 5'te SYSTEM servisi |
| Web'den WoL gönderilemez | PC uzaktan açılamaz | Faz 6 uygulaması / Faz 7 dongle / modem WoL |
| Live Activity push sunucusu gerektirir | Ek altyapı ve maliyet | Faz 6 ön koşulu olarak planlandı; küçük sunucusuz fonksiyon yeterli |
| Admin pencerelerine girdi gitmez (UIPI) | Bazı pencereler kontrol edilemez | Opsiyonel yönetici modu |
| Safari güncellemeleri davranışı değiştirir | Dokunma/kurulum sorunları | Her iOS sürümünde kontrol listesi |

---

## 13. v2 → v3 Düzeltme Özeti

| # | v2'deki hata | v3'teki düzeltme |
|---|---|---|
| 1 | HTTPS Faz 5'teydi; Faz 1–4 özellikleri (Wake Lock, Service Worker, pano, kripto) HTTPS'siz çalışmaz, sonradan geçiş eşleşmeleri bozar | HTTPS Faz 1'de, adres baştan sabit (4.1) |
| 2 | "PC kapalıysa Uyandır" web'de imkânsız (sayfa açılmaz, tarayıcı UDP gönderemez) | Çevrimdışı ekran; WoL Faz 6/7'ye (7.9) |
| 3 | Çift fare hızlanması | Mutlak konumlandırma (6.2) |
| 4 | localhost ayar sayfası ve WS'ye zararlı site erişebilir | Origin/Host kontrolü, parola, CSRF (5) |
| 5 | "Sadece özel ağ" asıl tehdide karşı etkisiz; Tailscale'i engeller | Ek katman olarak yeniden tanımlandı; Tailscale izin listesi (5) |
| 6 | Eşleşme kodu Safari'de harcanabilir | Sadece ana ekran uygulamasında; 10 dk ömür; PIN (4.3) |
| 7 | Bilgisayar adına güvenmek kırılgan | Programın kendi mDNS adı |
| 8 | Windows "Genel" ağ profili kontrol edilmiyordu | Kurulumda tespit (4.3) |
| 9 | Tek yönlü gecikme ölçümü senkron olmayan saatlerle yapılamaz | RTT + saat farkı + 240 fps test (6.4) |
| 10 | Kısayollar klavye düzenine bağımlıydı | Karakter tabanlı (7.2) |
| 11 | Gamepad için yatay kilit varsayılmıştı | Döndürülmüş düzen, güvenli bölgeler (7.5) |
| 12 | "Tek dosya, kurulumsuz" sözü ViGEm için geçersizdi | İsteğe bağlı sürücü olarak açıkça belirtildi |
| 13 | Kilit ekranı / UAC'ye girdi sorunu eksikti | Faz 5 SYSTEM servisi |
| 14 | Live Activity'nin push sunucusu gerektirdiği ve widget yenileme sınırı eksikti; "PC programı aynen kalır" yanlıştı | Faz 6 ön koşulları ve PC programı eklemeleri yazıldı |
| 15 | Ekran önizleme v2'de kaybolmuştu | Faz 5'e geri eklendi |
| 16 | Apple Watch'ın doğrudan ağ erişimi varsayılmıştı | iPhone üzerinden aktarım |

---

## 14. Başarı Ölçütleri
- Gecikme: **p95 < 20 ms** (WSS), **p95 < 15 ms** (WebRTC / iOS uygulaması). 240 fps testte parmaktan ekrana < 50 ms (ekran yenileme dahil)
- İlk kurulum: **< 2 dakika** (profil yükleme dahil); alan adı yöntemiyle < 1 dakika
- Sonraki açılışlar: **~300 ms'de arayüz, < 1 sn'de bağlantı**, sıfır kullanıcı adımı
- PC programı: **< 30 MB RAM**, boşta ~%0 CPU
- Kopan bağlantı: **< 2 sn**'de yeniden kurulur
- Güvenlik testleri: kötü niyetli Origin/Host isteklerinin **%100'ü reddedilir**

---

## 15. İlk Adım
**Faz 0:** Rust HTTPS/WSS sunucu + web touchpad + gecikme aracı + **4 doğrulama deneyi** (sertifika profili, `start_url`, HTTPS özellikleri, Kestirmeler). Bu deneyler iOS'a bağlı varsayımları test ediyor; olumsuz sonuç çıkarsa plan kod yazılmadan önce revize edilir.
