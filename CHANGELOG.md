# Changelog

## 0.11.0
- **WebRTC data channel (experimental, off by default):** movement over UDP, with automatic fallback to the WebSocket. The UDP socket exists only while a session is open; sessions are pinned to the phone's IP. Settings › Low latency (WebRTC).
- **Guided setup:** redesigned phone setup page with a step-by-step wizard, plus an animated couch mascot that shows each step.
- **First-run tour:** a short in-app tour (Settings › Show the intro tour to replay it).
- **Latency details** now show p99, p99.9 and max over about 100 seconds.
- **Test suite:** protocol fuzzing, WebRTC security tests, WebKit UI tests, GitHub Actions.
- **Support** link (GitHub Sponsors) in Settings and the README.

## 0.10.1
- **Recovery:** automatic backups of the settings file with self-recovery, password-protected backup/restore, a connection troubleshooter in the app, a notice when an unregistered phone tries to connect.
- Faster open (cache-first service worker), clearer setup page, maker credit.

## 0.10.0
- Open windows list (switch windows from the phone), pinch-to-zoom, "open link on TV" (YouTube and any URL), Android phone support (experimental).

## 0.9.x
- Design overhaul: flat dark surfaces, light theme, six accent colors, Siri Remote style clickpad, official brand icons, empty/error/loading states.
- Performance: idle CPU 1.85% to 0.05%, UI first load 279 KB to 79 KB (gzip), repeat load 0 KB (ETag).

## 0.8.0 – 0.5.0
- LG webOS, Samsung and Android TV remote (Samsung/Android TV tested against mock TVs only), multi-TV, iPad layout, security review, English interface, accessibility pass.

## 0.4.0 and earlier
- Touchpad, air mouse, keyboard, media remote, app launcher, files/clipboard, audio mixer, screenshots, Siri Shortcuts API, local HTTPS with a name-constrained certificate, one-time phone pairing.
