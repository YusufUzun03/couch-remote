# CouchRemote — security model

**English** · [Türkçe](SECURITY.md)

This document explains what the app protects against, what it deliberately does **not** protect against, and what to do if something goes wrong.

## Basic trust model

- The program on the PC (`CouchRemote.exe`) listens on the local network. It is never exposed to the internet and there is no central server.
- A phone, once **paired**, is **fully trusted** by that computer: it can drive the mouse/keyboard, launch apps, read/write the clipboard, take screenshots, send files and lock/sleep/shut down the PC. Pairing happens when the person at the computer approves it with the QR code/PIN.
- So the real protection is **who can pair** and **how a paired device can be revoked**.

## What we protect against

| Threat | Mitigation |
|---|---|
| A stranger's device on the network tries to send commands | Every connection requires a signed challenge from the device key (ECDSA P-256, private key never leaves the phone). Unauthenticated connections close within 5 s and can't send messages over 2 KB |
| Guessing the pairing code | Code is 18 random bytes; the PIN is 6 digits but 5 wrong tries lock pairing for 5 min; codes are only created while the "Connect your phone" page is open |
| Eavesdropping / man-in-the-middle | HTTPS with our own name-constrained local CA (valid only for `couch-xxxx.local`); the CA's verification code is shown on the PC screen |
| A malicious website tries to control the phone/PC (DNS rebinding, CSRF) | `Host` and `Origin` checks on every request; the admin page additionally needs a random key and a custom header |
| Someone else accessing the admin page | `127.0.0.1` only; a new key on every start; HttpOnly + SameSite=Strict cookie |
| Framing / foreign scripts / XSS | Strict `Content-Security-Policy` (the app page only runs its own scripts), `X-Frame-Options: DENY`, `Referrer-Policy: no-referrer`; the UI writes dynamic content with `textContent`; the admin page escapes HTML |
| Connection flooding / half-open connections (DoS) | At most 256 concurrent connections (48 per IP); 10 s TLS handshake and 15 s HTTP header timeouts |
| Token / identity guessing | After 10 wrong tokens or identities per minute per IP: 429 (1 min block); tokens are 192-bit random |
| Command flooding | Per-connection rate limit; heavy commands such as screenshots are limited to about one per second |
| Disk filling / dangerous files | Uploads: at most 3 concurrent, 2 GB limit, free-space check; file names are sanitized (path traversal, reserved names); files are saved with Mark-of-the-Web so Windows warns on executables |
| Log poisoning | The device name reported by the phone has control characters stripped and is limited to 40 chars; personal content (clipboard, file names, search queries) is never logged |
| Reading secrets from disk | Private keys and the Shortcuts token are encrypted with Windows DPAPI (only this Windows user can decrypt them) |
| A removed phone staying connected | Its open connection is cut immediately when the pairing is removed |
| A single bug crashing the whole program | Locks don't poison; panics are written to the log |
| Backup file | The password-protected backup is encrypted with AES-256-GCM, the key derived from your password with PBKDF2-HMAC-SHA256 (600,000 rounds); it contains the CA private key, phone public keys and the Shortcuts token. It can only be made/restored from the admin page (local + keyed + custom header); a wrong password or a corrupt/foreign file changes nothing. The automatic backups are tied to the same Windows user (DPAPI) |
| Open windows | Window titles go only to the paired phone and are never logged; for an action (focus/minimize/close) the id is re-checked against the current list every time and an unknown id does nothing; CouchRemote's own window is not listed |
| Access via LAN address (Android) | The certificate also contains the LAN address; the CA's name constraint still limits DNS names. Host/Origin checks accept only this computer's name and LAN address (foreign Host gets 421) |
| TV bridge (LG, Samsung, Android TV) | The phone sends commands to the PC, not the TV; the PC translates only a fixed command list (no free-form address/URI), the TV address must be a local-network address, the pairing key is DPAPI-encrypted. The TV's self-signed certificate is not verified (local network, as with all LG clients) |
| TV connection drops / address changes | The TV is retried every 12 s; after repeated failures its new address is looked up by MAC (local-network addresses only, never another saved TV's address). WebSocket messages from the TV are capped at 1 MB |
| Dependency vulnerabilities | Scanned with `cargo audit`: no known vulnerabilities. The one exception is RUSTSEC-2023-0071 in the `rsa` crate (timing side channel): it is only used to generate the Android TV pairing certificate key; private-key operations are done by `ring`, so we are not affected (`agent/.cargo/audit.toml`) |

## What we deliberately do not protect against

- **An unlocked or compromised paired phone:** whoever holds the phone (with the app on the Home Screen) can control the PC. Use your phone's screen lock. If it's lost, follow "Lost phone" below.
- **Someone with physical/administrator access to the PC:** they can read the data folder (`%APPDATA%\CouchRemote`). Keys are DPAPI-encrypted but can be decrypted from this user's session.
- **Protected content:** DRM-protected video appears black in screenshots.
- **A network administrator on the same network:** traffic is encrypted but the existence/timing of connections is visible.
- **Lock screen / UAC:** the program runs in the user session and can't send input while the PC is locked (by design).

## Lost or stolen phone

1. On the PC, right-click the **CouchRemote** icon next to the clock → **Remove all phone pairings…**
   (or press **Remove all** on the admin page). This deletes all pairings, cuts open connections and renews the Siri/Shortcuts token.
2. Pair the phone again (tray → *Connect a phone*).
3. If you use Siri shortcuts, update them with the new token (Settings → Siri & Shortcuts).

To renew only the token: admin page → *Siri & Shortcuts* → **Renew token**.

## WebRTC data channel (experimental, off by default)

When Settings › Low latency (WebRTC) is on, movement/scroll packets travel over UDP. The surface this adds, and its limits:

- **The UDP socket exists only when needed.** With the setting off no port is opened; it opens on the first offer and closes when the last session ends. It binds only to the computer's LAN address, and the firewall rule (Private network, this program only) applies unchanged.
- **Setting up a session requires authentication.** An SDP offer is accepted only from an authenticated WSS connection; the ICE password and DTLS fingerprint travel over that encrypted channel. An unpaired device cannot create a session, and sending packets to the UDP port does nothing.
- **A session is pinned to an IP address.** UDP packets not coming from the IP of the WSS connection that created the session are dropped before reaching the library, so the third-party network parser never sees packets from other sources. (An attacker able to spoof that address would still need the ICE/DTLS keys.)
- **The channel has narrow powers.** Only movement, scroll, ping and clock-offset packets of at most 64 bytes are processed: no commands, typing or app launching. NaN and extreme values are dropped/clamped. Button press/release goes over the reliable WebSocket.
- **Resource limits:** at most 8 sessions; a session is deleted if the channel does not open within 15 s; an offer costs more than a normal command in the rate limiter (each creates a DTLS certificate); closing the WebSocket or removing the pairing closes the session.
- **Fault isolation:** a bug (panic) inside the WebRTC library closes only that session; the program and the WebSocket path are unaffected.
- **Residual risk:** WebRTC uses a young third-party library, `str0m` (STUN/DTLS/SCTP parsing). Dependencies are scanned with `cargo audit`, but the library has not had an independent security audit. If that worries you, leave the feature off; with it off there is no UDP surface at all.

## Tests and audits

- `tools/smoke-test.mjs`: end-to-end security tests (TLS/name constraints, DNS rebinding, Origin, admin key, PIN lockout, file-name sanitizing, connection limit, timeouts, rate limit, token renewal).
- `tools/fuzz-test.mjs`: malformed/extreme binary and JSON messages, oversized messages, packet storms, malformed authentication; afterwards the program is verified to be up and reachable (reproducible seed).
- `tools/rtc-test.mjs`: WebRTC: unauthenticated/malformed offers, offer rate limit, socket open only while a session exists, UDP flood from a foreign address, IP pinning, channel timeout, session cleanup.
- `tools/ui-test.mjs`: WebKit UI checks (overflow, CSP/console errors, accessibility, translation gaps).
- `.github/workflows/test.yml`: all of these plus `cargo audit` run on every change.
- `cargo test`: unit tests (parsers, limiters, file-name sanitizing).
- `cargo audit`: dependency vulnerability scan (run before a release).
- Destructive tests (`COUCH_TEST_DESTRUCTIVE=1`) are only run with an isolated data folder.

## Reporting an issue

If you find a vulnerability, please report it privately to the project owner before disclosing it publicly.
