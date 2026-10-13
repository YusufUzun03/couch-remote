# CouchRemote

**English** · [Türkçe](README.tr.md)

Turn your **iPhone into a touchpad, keyboard and media remote for your Windows PC**. Start a movie from bed, change the volume, move the cursor. You install one `.exe` on the PC; **nothing is installed on the phone** (it's a web app you add to your Home Screen from Safari). No cloud, no account — everything stays on your local network.

## Features

- **Touchpad:** one-finger cursor, inertial two-finger scrolling, three-finger desktop gestures, drag and drop
- **Air mouse:** point by turning your phone; tilt-independent and tremor-filtered
- **Keyboard:** full on-screen keyboard (Turkish Q or English QWERTY) or the iPhone keyboard (dictation and autocorrect work)
- **Remote:** now-playing card, play/seek, D-pad, app-specific buttons (YouTube, Netflix, VLC, Spotify…)
- **Audio:** master volume, per-app volume mixer, output device switching
- **Apps:** quick launch for Netflix/YouTube/Spotify, Start menu and Store apps, Steam/Epic games (with covers)
- **Transfer:** clipboard, files, screenshots
- **Power:** lock, sleep, shut down, sleep timer (volume fades out gently)
- **Siri / Shortcuts:** `POST /api/v1/action/{action}` for Siri, the Action Button and Control Center
- **Bed mode**, spring-animated UI, landscape layout, **Turkish and English** interface

Pair once; after that you just open the app.

## Install

See [INSTALL.md](INSTALL.md) (install on the PC + pair your iPhone, ~3 min). Ready-made `CouchRemote.exe` is on the [Releases](../../releases) page.

> The file isn't code-signed yet: Windows SmartScreen will ask you to click "More info → Run anyway".

## Support

CouchRemote is free and open source (MIT). If it's useful to you, you can support its development on [GitHub Sponsors](https://github.com/sponsors/YusufUzun03). Sponsorships go toward costs such as code signing (which removes the Windows SmartScreen warning) and testing devices. Bug reports and stars help just as much.

## How it works

```
iPhone (Safari PWA)  ⇄  WSS  ⇄  couch-agent.exe (Rust)  →  SendInput / Core Audio / SMTC / ...
        local network, HTTPS (our own name-constrained local CA), signed device authentication
```

- **Agent:** Rust (`agent/`): axum, rustls, SendInput, WinRT media, Core Audio, its own mDNS responder
- **UI:** plain JavaScript + CSS (`web/`), no build step; embedded into the program
- **Security model:** [SECURITY.en.md](SECURITY.en.md)
- **Plan and decision log:** [PLAN.md](PLAN.md) (Turkish)

## Languages

The phone app and setup pages follow your phone's language (Turkish or English) and can be switched in Settings. The PC program's own menus and dialogs follow Windows' display language. To add a language, see `web/i18n.js` and `web/i18n-en.js` (the dictionary maps Turkish source strings to translations).

## Build from source

You need Rust (stable), Windows with the Visual Studio Build Tools, and Node.js (for the tests).

```powershell
cd agent
cargo run                      # development: reads web/ from disk
cargo test                     # unit tests
cd ..
powershell -File tools\release.ps1   # dist\CouchRemote.exe
```

Tests:

```powershell
cd tools; npm install
node air-test.mjs              # air-mouse core (no phone needed)
node smoke-test.mjs            # end-to-end (needs a running agent)
node preview-server.mjs        # UI preview: http://127.0.0.1:47890/preview
```

## License

[MIT](LICENSE) © 2026 Yusuf Uzun

Third-party components and their licenses: [THIRD-PARTY-LICENSES.md](THIRD-PARTY-LICENSES.md).

**Name and logo:** the MIT License covers the code, not the "CouchRemote" name or logo. If you publish a modified version, please give it a different name so users don't mistake it for the original. Brand logos shown in the app (Netflix, YouTube, Spotify, ...) belong to their owners and are used only to identify the services the app can open; CouchRemote is not affiliated with or endorsed by them.
