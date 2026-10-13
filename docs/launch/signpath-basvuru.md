# SignPath Foundation başvurusu: hazırlık notları

Amaç: Windows SmartScreen "Bilinmeyen yayıncı" uyarısını azaltmak için ücretsiz kod imzalama (açık kaynak projeler için).
Başvuru adresi: https://signpath.org/apply.html. Koşulları başvurmadan önce güncel haliyle oku, şartlar değişebilir.

## Koşullar ve durumumuz
| Koşul (SignPath'in yayınladığı) | Durum |
|---|---|
| OSI onaylı açık kaynak lisansı, ticari çift lisans yok | Tamam: MIT |
| Kapalı kaynak bileşen yok | Tamam: tüm bağımlılıklar serbest lisanslı (`THIRD-PARTY-LICENSES.md`); marka logoları CC0 (Simple Icons) |
| Zararlı yazılım / istenmeyen program yok | Tamam: yerel ağ kumandası; kaynağı açık |
| Proje bakımlı ve **imzalanacak sürüm daha önce yayınlanmış** | **Eksik:** repo henüz özel, release yok |
| Derleme **tam otomatik**, kaynak koddan | **Hazır:** `.github/workflows/release.yml` (etiketle taslak release üretir). Henüz hiç etiketle çalışmadı |
| Özellikler indirme sayfasında/README'de anlatılmış | Tamam: README, `site/index.html` (Pages ile yayınlanınca) |
| Projenin ana sayfasında **kod imzalama politikası** | Taslak: `docs/launch/code-signing-policy.md` (SignPath onayından sonra yayınlanacak) |
| README'de SignPath'e atıf | Onaydan sonra eklenecek (şimdi eklemeyeceğiz, henüz doğru olmaz) |

## Sıra
1. Repoyu public yap (önce `docs/launch/yayin-oncesi-kontrol.md` listesini bitir).
2. `v0.11.0` etiketi gönder → Actions taslak release üretir → gözden geçirip yayınla.
3. SignPath'e başvur: proje adı CouchRemote, repo bağlantısı, lisans MIT, yayınlanmış sürüm bağlantısı, kısa açıklama (aşağıda).
4. Onay gelince: Actions'a SignPath imzalama adımı eklenir, politika sayfası ve README atfı yayınlanır, imzalı sürüm yeni etiketle çıkar.

## Başvuru metni (taslak, İngilizce)
**Project:** CouchRemote
**Repository:** https://github.com/YusufUzun03/couch-remote
**License:** MIT
**Description:** CouchRemote lets an iPhone act as a touchpad, keyboard and media remote for a Windows PC over the local network. A small Windows program (Rust) serves a web app that the phone installs to its Home Screen. It has no cloud service and no accounts; all traffic stays on the user's home network, protected by a local name-constrained certificate and per-phone keys. The source code, build pipeline (GitHub Actions) and a threat model (SECURITY.md) are public.
**What gets signed:** the single executable `CouchRemote.exe` built by `.github/workflows/release.yml` from the tagged source.
**Why signing matters:** the program is installed by non-technical users; an unsigned executable triggers Windows SmartScreen warnings that discourage installation.
**Maintainer:** Yusuf Uzun (sole author, reviewer and approver).

## Dürüst notlar
- Sertifika SignPath Foundation adına çıkar: SmartScreen'de yayıncı olarak "SignPath Foundation" görünür, senin adın değil.
- İmzalı olması SmartScreen itibarını **hemen** sıfırlamaz; yeni dosyalarda bir süre uyarı görülebilir. Sorunsuz olacağını garanti edemem.
- Foruma göre bazı başvurularda "kullanıcı tabanı" soruluyor, resmi koşullarda yazmıyor. İlk kullanıcılar gelince başvurmak şansı artırır.
