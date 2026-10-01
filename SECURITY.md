# CouchRemote — Güvenlik modeli

[English](SECURITY.en.md) · **Türkçe**

Bu belge, uygulamanın neye karşı korunduğunu, neye karşı **korunmadığını** ve bir sorun olduğunda ne yapılacağını anlatır.

## Temel güven modeli

- Bilgisayardaki program (`CouchRemote.exe`) yerel ağda dinler. İnternete açılmaz, merkezi bir sunucu yoktur.
- Bir telefon **eşleştirildiğinde** o bilgisayar için **tam güvenilir** olur: fareyi/klavyeyi sürer, uygulama açar, panoyu okur/yazar, ekran görüntüsü alır, dosya gönderir, bilgisayarı kilitler/uyutur/kapatır. Eşleşme, bilgisayarın başındaki kişi QR/PIN ile onay verdiğinde olur.
- Bu yüzden asıl koruma: **kimin eşleşebildiği** ve **eşleşmiş bir cihazın nasıl geri alınabildiği**.

## Neye karşı korunuyoruz

| Tehdit | Önlem |
|---|---|
| Ağdaki yabancı cihaz komut göndermeye çalışır | Her bağlantı cihaz anahtarıyla imzalı sınama ister (ECDSA P-256, özel anahtar telefondan çıkamaz). Kimliği doğrulanmamış bağlantı 5 sn içinde kapanır, 2 KB'tan büyük mesaj gönderemez |
| Eşleşme kodunu tahmin etme | Kod 18 bayt rastgele; PIN 6 hane ama 5 yanlışta 5 dk kilit; kod sadece "Telefonu bağla" sayfası açıkken üretilir |
| Ağ trafiğini dinleme / araya girme | HTTPS (kendi yerel CA'mız, ad kısıtlamalı: yalnızca `couch-xxxx.local` için geçerli); CA'nın doğrulama kodu PC ekranında gösterilir |
| Kötü niyetli web sitesi telefonu/PC'yi yönetmeye çalışır (DNS rebinding, CSRF) | Her istekte `Host` ve `Origin` kontrolü; yönetim sayfası ayrıca rastgele anahtar + özel başlık ister |
| Yönetim sayfasına başka kullanıcı/site erişir | Sadece `127.0.0.1`; her açılışta yeni anahtar; çerez HttpOnly + SameSite=Strict |
| Sayfa çerçeveleme / dış betik / XSS | Katı `Content-Security-Policy` (uygulama sayfası yalnızca kendi betiklerini çalıştırır), `X-Frame-Options: DENY`, `Referrer-Policy: no-referrer`; arayüz dinamik içeriği `textContent` ile yazar; yönetim sayfası HTML kaçışlı |
| Bağlantı doldurma / yarım bırakılan bağlantılar (DoS) | Toplam 256, IP başına 48 eşzamanlı bağlantı; TLS el sıkışması 10 sn, HTTP başlık 15 sn zaman aşımı |
| Jeton / kimlik tahmini | Yanlış jeton veya kimlikle IP başına dakikada 10 denemeden sonra 429 (1 dk engel); jeton 192 bit rastgele |
| Komut yağdırma | Bağlantı başına hız sınırı; ekran görüntüsü gibi ağır komutlar ~saniyede 1 ile sınırlı |
| Disk doldurma / tehlikeli dosya | Yükleme: en fazla 3 eşzamanlı, 2 GB sınır, disk boşluğu kontrolü; dosya adı temizlenir (yol gezme, ayrılmış adlar); dosyalar "internetten indirildi" (Mark-of-the-Web) işaretiyle kaydedilir, Windows çalıştırılabilirlerde uyarır |
| Günlük zehirlenmesi | Telefonun bildirdiği ad kontrol karakterlerinden arındırılır, 40 karakter; kişisel içerik (pano, dosya adı, arama sorgusu) günlüğe yazılmaz |
| Sırların diskte okunması | Özel anahtarlar ve Kestirmeler jetonu Windows DPAPI ile şifreli (sadece bu Windows kullanıcısı çözer) |
| Kaldırılan telefonun bağlı kalması | Eşleşme kaldırılınca açık bağlantı anında kesilir |
| Programın tek bir hatayla çökmesi | Kilitler zehirlenmez; paniklar günlüğe yazılır |
| TV köprüsü (LG, Samsung, Android TV) | Telefon TV'ye değil PC'ye komut yollar; PC yalnızca sabit bir komut listesini çevirir (serbest adres/URI geçmez), TV adresi yalnızca yerel ağ adresi olabilir, eşleşme anahtarı DPAPI ile şifreli. TV'nin öz imzalı sertifikası doğrulanmaz (yerel ağ, tüm LG istemcilerinde böyle) |
| TV bağlantısı kopar / adresi değişir | TV'ye 12 sn'de bir yeniden bağlanılır; üst üste başarısız olursa MAC adresinden yeni adres aranır (yalnızca yerel ağ adresleri, başka kayıtlı TV'nin adresi alınmaz). TV'den gelen WebSocket mesajları 1 MB ile sınırlıdır |
| Bağımlılık açıkları | `cargo audit` ile tarandı: bilinen açık yok. Tek istisna `rsa` kütüphanesindeki RUSTSEC-2023-0071 (zamanlama yan kanalı): kütüphane yalnızca Android TV eşleşme sertifikası için anahtar üretir, özel anahtarla şifre çözme/imzalama `ring` tarafından yapılır; bu yüzden etkilenmeyiz (`agent/.cargo/audit.toml`) |

## Neye karşı korunmuyoruz (bilerek)

- **Kilidi açık / ele geçirilmiş eşleşmiş telefon:** Telefonu eline alan kişi (uygulama ana ekranda kayıtlı) PC'yi kontrol edebilir. Telefonunun ekran kilidini kullan. Telefon kaybolursa aşağıdaki "Kayıp telefon" adımlarını uygula.
- **Bilgisayara fiziksel/yönetici erişimi olan biri:** Veri klasörünü (`%APPDATA%\CouchRemote`) okuyabilir. Anahtarlar DPAPI ile şifreli ama bu kullanıcı oturumundan çözülebilir.
- **Korumalı içerik:** Ekran görüntüsünde DRM'li videolar siyah çıkar.
- **Aynı ağdaki ağ yöneticisi:** Trafik şifreli ama bağlantı varlığı/zamanlaması görülebilir.
- **Kilit ekranı / UAC:** Program kullanıcı oturumunda çalışır; kilitliyken girdi gönderemez (tasarım gereği).

## Kayıp / çalınan telefon

1. Bilgisayarda saatin yanındaki **CouchRemote** simgesine sağ tıkla → **Tüm telefonların eşleşmesini kaldır…**
   (veya yönetim sayfasında **Hepsini kaldır**). Bu: tüm eşleşmeleri siler, açık bağlantıları keser ve Siri/Kestirmeler jetonunu yeniler.
2. Telefonu yeniden bağla (tepsi → *Telefonu bağla*).
3. Siri kestirmeleri kullanıyorsan yeni jetonla güncelle (Ayarlar → Siri ve Kestirmeler).

Sadece jetonu yenilemek için: yönetim sayfası → *Siri ve Kestirmeler* → **Jetonu yenile**.

## Test ve denetim

- `tools/smoke-test.mjs`: uçtan uca güvenlik testleri (TLS/ad kısıtı, DNS rebinding, Origin, yönetim anahtarı, PIN kilidi, dosya adı temizleme, bağlantı sınırı, zaman aşımı, hız sınırı, jeton yenileme).
- `cargo test`: birim testleri (ayrıştırıcılar, sınırlayıcılar, dosya adı temizleme).
- `cargo audit`: bağımlılık açığı taraması (yayın öncesi çalıştırılmalı).
- Yıkıcı testler (`COUCH_TEST_DESTRUCTIVE=1`) yalnızca izole bir veri klasörüyle çalıştırılır.

## Sorun bildirme

Güvenlik açığı bulursan lütfen herkese açık paylaşmadan önce proje sahibine özel olarak bildir.
