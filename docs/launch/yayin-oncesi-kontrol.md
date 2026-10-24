# Herkese açmadan önce kontrol listesi

Durum: ✅ yapıldı · ⏳ senin kararın/işlemin gerekiyor · ⬜ sonra

## Repo hijyeni
- ✅ Tüm git geçmişinde taranan kalıplar: e-posta, okul adresi, bilgisayar adı, kullanıcı yolu, özel anahtar blokları, API anahtarı/token biçimleri, gerçek ağ adresleri, telefon numarası. Sonuç: yok (yalnızca test amaçlı `192.168.1.x` örnek adresleri ve zaten herkese açık GitHub kullanıcı adı).
- ✅ Geçmişte ikili dosya (exe, günlük, anahtar, durum dosyası) yok. En büyük nesneler arayüz dosyaları ve ekran görüntüleri.
- ✅ Tüm commit'lerde yazar bilgisi noreply adresi.
- ✅ `prototype/` (özel anahtar içerir) hiç izlenmemiş, geçmişte de yok.
- ✅ `CLAUDE.md`, `.claude/`, `docs/KARARLAR.md` iç çalışma notları: yalnızca yerelde yok sayılıyor (`.git/info/exclude`), repoya gitmez.
- ✅ `LICENSE` ve README'deki telif sahibi adı "Yusuf Uzun" olarak kalıyor (karar: 2026-10-13). Uygulamadaki imza "WOOP WOOP" ayrı bir yapımcı imzası.
- ⏳ GitHub profilinde görünen ad, e-posta ve fotoğraf: public yapınca herkes görür. Profil ayarlarını sen kontrol et.
- ⏳ `docs/img/*` ekran görüntüleri: arayüz önizlemesinden alınmış sahte veriler. Yine de public olmadan önce bir kez gözle bak.

## Lisans ve yasal
- ✅ MIT (`LICENSE`), bağımlılıkların hepsi serbest lisanslı (MPL-2.0 olan `option-ext` dosya düzeyinde kopyalayıcı, bizim kodumuzu etkilemez).
- ✅ `THIRD-PARTY-LICENSES.md` üretildi (`cargo about`, `agent/about.toml` + `about.hbs`). Bağımlılık değişince yeniden üret.
- ✅ README'de ad/logo ve marka notu (TR + EN).
- ⬜ Marka logoları: Netflix, YouTube vb. resmî logolar CC0 kaynaktan ama markaları sahiplerine ait. Bir marka itiraz ederse o logoyu sade ikonla değiştirmek 1 saatlik iş.

## Yayın hazırlığı
- ✅ `CHANGELOG.md`, `docs/release-notes-0.11.0.md` (SHA-256 yer tutucu, release anında otomatik dolar).
- ✅ `.github/workflows/release.yml`: etiketle (`v0.11.0`) kaynaktan derler, SHA256SUMS üretir, **taslak** release açar. Henüz hiç çalıştırılmadı.
- ✅ Destek bağlantısı (uygulama + README + `FUNDING.yml`), GitHub Sponsors başvurusu onaylanana kadar çalışmaz.
- ⏳ Repoyu public yapmak (GitHub › Settings › Danger zone). Geri alınabilir ama indekslenen/kopyalanan içerik geri alınamaz.
- ⏳ GitHub Pages ile `site/` yayını: Settings › Pages › Source: GitHub Actions, sonra Actions › Pages › Run workflow (`.github/workflows/pages.yml`, yalnız elle çalışır).
- ⬜ App Store yayınlanınca bağlantıyı `site/index.html` içindeki `APP_STORE_URL` sabitine yaz, Pages'i yeniden çalıştır.
- ⬜ Microsoft'a yanlış alarm bildirimi (https://www.microsoft.com/wdsi/filesubmission): imzasız exe için, yayından sonra, senin Microsoft hesabınla.
- ⬜ VirusTotal'a yükleme: dosya üçüncü tarafa gönderilir ve herkese açık görünür, bu yüzden senin onayınla.
- ⬜ SignPath başvurusu: `docs/launch/signpath-basvuru.md`.

## Destek ve geri bildirim
- ⬜ GitHub Issues şablonu (hata bildirimi: sürüm, Windows sürümü, ne bekledin, günlük).
- ⬜ Beta: 3–5 kullanıcı, kapalı.
