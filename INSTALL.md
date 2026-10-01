# CouchRemote installation guide

**English** · [Türkçe](KURULUM.md)

Turn your iPhone into a touchpad, keyboard and media remote for your PC. Control movies from bed, change the volume, move the cursor.

**You need:** a Windows 10/11 PC, an iPhone, both on the **same Wi-Fi** (or the PC on Ethernet and the phone on the same router).

## 1. Install on the PC (2 minutes)
1. Double-click `CouchRemote.exe`.
2. If Windows shows a "Windows protected your PC" (SmartScreen) warning: **More info → Run anyway**. (The file isn't code-signed yet, so this appears.)
3. Click **Yes** in the permission window. This only adds a firewall rule so your phone can connect from your local network.
4. The "Connect your phone" page opens. Keep it open.

## 2. Pair your iPhone (once)
1. Point the iPhone **Camera** at the QR code on the page and tap the link that appears.
2. Follow the steps in Safari: first **download the security profile**, then install it under **Settings → General → VPN & Device Management**. Finally turn on trust for "CouchRemote" under **Settings → General → About → Certificate Trust Settings**.
   The verification code on the page (e.g. `7F3A-91C2`) must match the one on your PC screen.
3. In Safari tap **Share → Add to Home Screen**.
4. Open the **Couch** icon on your Home Screen. Pairing happens automatically.

From now on you just open the app; it connects by itself.

## Using it
- **Touchpad:** one finger moves the cursor, tap = click, two fingers scroll, three fingers switch desktops.
- **Air mouse:** choose "Air mouse" on the Touchpad screen, hold and turn your phone to point.
- **Keyboard:** full on-screen keyboard, or type with the iPhone keyboard.
- **Remote:** play/pause, volume, app-specific buttons (YouTube, Netflix…).
- **Apps:** launch Netflix/YouTube/Spotify and Steam games from your phone.
- **Transfer:** send the clipboard and files between phone and PC, take a screenshot.
- **Televisions:** control LG, Samsung and Android/Google TV (experimental) from your phone. TVs are found automatically under Settings › Devices › Televisions. Tap the device name at the top left to switch between the computer and your TVs; the selected TV is driven by the Remote, Touchpad and Keyboard tabs. **Scenes** in the power menu (Movie mode, Sleep mode) and Siri shortcuts (`tv-off`, `tv-volup`…) work too.
- **iPad:** the layout adapts to larger screens.
- **Language:** follows your phone; change it in Settings → Language.

## Troubleshooting
- **"Can't reach your computer":** the PC may be asleep; wake it and wait a few seconds. Check that both devices are on the same Wi-Fi.
- **It won't connect:** set the Windows network profile to **Private** (Settings → Network & internet → Wi-Fi/Ethernet → Properties). The "Connect your phone" page warns you and fixes it in one click.
- **Lost phone:** right-click the CouchRemote icon next to the clock → **Remove all phone pairings**.
- **Uninstall:** the same icon → **Uninstall…**, or Windows Settings → Apps.

Privacy: no data leaves your local network; everything is encrypted. See [SECURITY.en.md](SECURITY.en.md).
