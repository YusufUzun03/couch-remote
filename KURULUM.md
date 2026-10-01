# CouchRemote kurulum rehberi

iPhone'unu bilgisayarın için touchpad, klavye ve TV kumandasına çevirir. Yataktan film aç, sesi kıs, imleci sür.

**Gerekenler:** Windows 10/11 bilgisayar, iPhone, ikisi de **aynı Wi-Fi** (veya bilgisayar kablolu, telefon aynı modemde).

## 1. Bilgisayara kur (2 dakika)
1. `CouchRemote.exe` dosyasını çift tıkla.
2. Windows "Bilgisayarınızı korudu" (SmartScreen) uyarısı çıkarsa: **Ek bilgi → Yine de çalıştır**. (Dosya henüz kod imzalı değil, bu yüzden çıkıyor.)
3. İzin penceresine **Evet** de. Bu izin sadece yerel ağdan telefonun bağlanabilmesi için.
4. "Telefonunu bağla" sayfası açılır. Bu sayfayı açık bırak.

## 2. iPhone'u bağla (bir kez)
1. iPhone'un **Kamera**'sıyla sayfadaki QR kodu okut, çıkan bağlantıya dokun.
2. Safari'de adımları izle: önce **güvenlik profilini indir**, sonra **Ayarlar → Genel → VPN ve Aygıt Yönetimi**'nden profili yükle. En sonda **Ayarlar → Genel → Hakkında → Sertifika Güven Ayarları**'ndan "CouchRemote" için güveni aç.
   Sayfadaki doğrulama kodu (ör. `7F3A-91C2`) bilgisayar ekranındakiyle aynı olmalı.
3. Safari'de **Paylaş → Ana Ekrana Ekle** de.
4. Ana ekrandaki **Couch** simgesinden aç. Eşleşme otomatik olur.

Bundan sonra uygulamayı açman yeterli; bağlantı kendiliğinden kurulur.

## Kullanım
- **Touchpad:** bir parmak imleç, dokun = tık, iki parmak kaydır, üç parmak masaüstü geçişi.
- **Hava faresi:** Touchpad ekranında "Hava faresi"ni seç, basılı tutup telefonu çevirerek işaret et.
- **Klavye:** ekranda tam klavye veya iPhone klavyesiyle yazı.
- **Kumanda:** oynat/duraklat, ses, uygulama özel düğmeleri (YouTube, Netflix...).
- **Uygulamalar:** Netflix/YouTube/Spotify ve Steam oyunlarını telefondan aç.
- **Aktar:** panoyu ve dosyaları telefon ↔ bilgisayar gönder, ekran görüntüsü al.

## Sorun olursa
- **"Bilgisayara ulaşılamıyor":** bilgisayar uyku modunda olabilir; uyandırıp birkaç saniye bekle. İkisi de aynı Wi-Fi'de mi bak.
- **Bağlanmıyor:** Windows ağ profilini **Özel** yap (Ayarlar → Ağ ve internet → Wi-Fi/Ethernet → Özellikler → Özel). "Telefonunu bağla" sayfası bunu uyarır ve tek tıkla düzeltir.
- **Telefon kaybolursa:** saatin yanındaki CouchRemote simgesine sağ tıkla → **Tüm telefonların eşleşmesini kaldır**.
- **Kaldırmak için:** aynı simge → **Kaldır**, veya Windows Ayarlar → Uygulamalar.

Güvenlik: Hiçbir veri internete çıkmaz; her şey yerel ağda şifreli çalışır. Ayrıntılar için `SECURITY.md`.
