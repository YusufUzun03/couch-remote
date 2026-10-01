# CouchRemote

iPhone'unu Windows bilgisayarın için **touchpad, klavye ve TV kumandasına** çevir. Yataktan film aç, sesi kıs, imleci sür. Bilgisayara tek bir `.exe` kurulur; telefona **hiçbir uygulama kurulmaz** (Safari'den ana ekrana eklenen bir web uygulaması). Hiçbir veri internete çıkmaz.

*English: turn your iPhone into a touchpad, keyboard and media remote for your Windows PC. One `.exe` on the PC, nothing to install on the phone (a PWA over your local network, HTTPS with a local name-constrained CA, no cloud).*

## Özellikler

- **Touchpad:** bir parmak imleç, iki parmak kaydırma (ataletli), üç parmak masaüstü hareketleri, sürükle-bırak
- **Hava faresi:** telefonu çevirerek işaret et; eğimden bağımsız, titremesi süzülmüş
- **Klavye:** ekranda tam Türkçe Q klavye + iPhone klavyesiyle yazma (dikte, otomatik düzeltme dahil)
- **Kumanda:** çalan parça/video kartı, oynat/sar, yön halkası, uygulamaya özel düğmeler (YouTube, Netflix, VLC, Spotify…)
- **Ses:** ana ses, uygulama başına ses (mikser), ses çıkış cihazı değiştirme
- **Uygulamalar:** Netflix/YouTube/Spotify hızlı başlat, Başlat menüsü + Mağaza uygulamaları, Steam/Epic oyunları (kapaklarıyla)
- **Aktar:** pano, dosya gönderme, ekran görüntüsü
- **Güç:** kilitle, uyut, kapat, uyku zamanlayıcısı (ses yavaşça kısılır)
- **Siri / Kestirmeler:** `POST /api/v1/action/{eylem}` ile Siri, Eylem Düğmesi, Kontrol Merkezi
- **Yatak modu**, yay animasyonlu arayüz, yatay kullanım

Bir kez eşleştir; sonrasında uygulamayı açman yeterli.

## Kurulum

[KURULUM.md](KURULUM.md) rehberine bak (bilgisayara kurulum + iPhone eşleştirme, ~3 dk). Hazır `CouchRemote.exe` [Releases](../../releases) sayfasında.

> Dosya henüz kod imzalı değil: Windows SmartScreen uyarısında "Ek bilgi → Yine de çalıştır" gerekir.

## Nasıl çalışır

```
iPhone (Safari PWA)  ⇄  WSS  ⇄  couch-agent.exe (Rust)  →  SendInput / Core Audio / SMTC / ...
        yerel ağ, HTTPS (kendi yerel CA'mız), cihaz anahtarıyla imzalı kimlik doğrulama
```

- **Ajan:** Rust (`agent/`): axum, rustls, SendInput, WinRT medya, Core Audio, kendi mDNS yanıtlayıcısı
- **Arayüz:** düz JavaScript + CSS (`web/`), derleme adımı yok; programın içine gömülür
- **Güvenlik modeli:** [SECURITY.md](SECURITY.md)
- **Plan ve karar günlüğü:** [PLAN.md](PLAN.md)

## Kaynaktan derleme

Gerekenler: Rust (stable), Windows + Visual Studio Build Tools, Node.js (testler için).

```powershell
cd agent
cargo run                      # geliştirme: web/ klasörünü diskten okur
cargo test                     # birim testleri
cd ..
powershell -File tools\release.ps1   # dist\CouchRemote.exe
```

Testler:

```powershell
cd tools; npm install
node air-test.mjs              # hava faresi çekirdeği (telefon gerekmez)
node smoke-test.mjs            # uçtan uca (çalışan bir ajan gerekir)
node preview-server.mjs        # arayüz önizlemesi: http://127.0.0.1:47890/preview
```

## Lisans

[MIT](LICENSE) © 2026 Yusuf Uzun
