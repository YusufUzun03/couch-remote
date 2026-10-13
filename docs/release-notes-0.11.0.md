CouchRemote turns your iPhone into a touchpad, keyboard and media remote for your Windows PC. Nothing runs in the cloud, there is no account: pairing and traffic stay on your home network.

## Download and verify
- `CouchRemote.exe` (Windows 10/11, 64-bit). Double-click it; it installs itself for the current user.
- SHA-256: `<SHA256>` (compare with `Get-FileHash CouchRemote.exe` in PowerShell).
- **The file is not code-signed yet**, so Windows SmartScreen shows "Windows protected your PC". Choose **More info › Run anyway**. Code signing is planned.

## First run
1. Run the file and approve the one-time firewall prompt (private networks only).
2. Open the "Connect your phone" page on the PC and scan the QR code with the iPhone camera.
3. Follow the setup page on the phone (install the small certificate profile, ~2 minutes), then add the app to your Home Screen.

Step-by-step guide: [INSTALL.md](https://github.com/YusufUzun03/couch-remote/blob/main/INSTALL.md) (Turkish: [KURULUM.md](https://github.com/YusufUzun03/couch-remote/blob/main/KURULUM.md)).

## What's new
See [CHANGELOG.md](https://github.com/YusufUzun03/couch-remote/blob/main/CHANGELOG.md): WebRTC channel (experimental), guided setup with a mascot, first-run tour, recovery tools, test suite.

## Known limits (please read)
- **Windows only** for the PC program; the phone side is an iPhone web app (Android works but is experimental).
- Samsung and Android TV remotes were tested against simulated TVs only. LG webOS is the best-tested TV path.
- WebRTC (low latency) is experimental and off by default; real-world gain on weak Wi-Fi is still being measured.
- No lock-screen PIN entry, no live screen preview, no multi-PC yet.
- iPhone-side behavior (touch feel, Wake Lock, certificate profile) is verified by hand on a few devices.

## Security
The local certificate is name-constrained to your PC, phones are paired once with a key (no passwords), the admin page is local-only, and private keys are protected with Windows DPAPI. Details and threat model: [SECURITY.md](https://github.com/YusufUzun03/couch-remote/blob/main/SECURITY.md). Found a problem? Open a GitHub issue or use the repository's security advisory form.

MIT licensed. Third-party licenses: [THIRD-PARTY-LICENSES.md](https://github.com/YusufUzun03/couch-remote/blob/main/THIRD-PARTY-LICENSES.md).
