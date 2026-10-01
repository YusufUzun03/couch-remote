// CouchRemote Faz 0 — iPhone web uygulaması: eşleşme, WSS bağlantısı, touchpad, gecikme ölçümü, iOS deneyleri.
import { AirMouse } from '/air.js';
import { initTv, bindClickpad } from '/tv.js';
import { BRAND } from '/brand-icons.js';
import { t as tl, lang, ready, translateDom, langPref, setLangPref } from '/i18n.js';

await ready; // İngilizce sözlük yüklenir (Türkçede anında geçer)
translateDom(); // statik HTML'i çevir
const $ = (id) => document.getElementById(id);
const standalone = navigator.standalone === true || matchMedia('(display-mode: standalone)').matches;
const params = new URLSearchParams(location.search);
const startCode = params.get('c');
const startHref = location.href;
const MM_PER_PX = 25.4 / 160; // iPhone: ~160 CSS pikseli / inç
const now = () => performance.timeOrigin + performance.now();
const evTime = (e) => performance.timeOrigin + e.timeStamp;

// ============================================================ iOS deney raporu (Faz 0)
const exp = { startHref, codeInStartUrl: !!startCode };

function features() {
  return {
    ua: navigator.userAgent,
    // iOS 26 kullanıcı ajanında işletim sistemi sürümü 18_x olarak dondurulmuş; gerçek sürüm Safari 'Version/' alanında
    iosVersion: navigator.userAgent.match(/Version\/(\d+(?:\.\d+)*)/)?.[1] ?? null,
    standalone,
    navigatorStandalone: navigator.standalone,
    secureContext: isSecureContext,
    wakeLockApi: 'wakeLock' in navigator,
    serviceWorkerApi: 'serviceWorker' in navigator,
    swControlled: !!navigator.serviceWorker?.controller,
    clipboardWrite: !!navigator.clipboard?.writeText,
    clipboardRead: !!navigator.clipboard?.readText,
    coalescedEvents: typeof PointerEvent !== 'undefined' && 'getCoalescedEvents' in PointerEvent.prototype,
    subtleCrypto: !!crypto.subtle,
    deviceMotionPermissionApi: typeof DeviceMotionEvent !== 'undefined' && typeof DeviceMotionEvent.requestPermission === 'function',
    speechRecognition: 'SpeechRecognition' in window || 'webkitSpeechRecognition' in window,
    inputSwitch: 'switch' in HTMLInputElement.prototype,
    vibrate: 'vibrate' in navigator,
    maxTouchPoints: navigator.maxTouchPoints,
    dpr: devicePixelRatio,
    screen: `${screen.width}x${screen.height}`,
  };
}

// Tanılama raporu sadece eşleşmiş ve doğrulanmış bağlantı üzerinden gider (bağlanana kadar bekletilir)
const pendingReports = [];
function report(phase, extra = {}) {
  const r = { type: 'report', phase, ...features(), ...exp, ...extra };
  if (connected && sendJson(r)) return;
  pendingReports.push(r);
  if (pendingReports.length > 5) pendingReports.shift();
}

function measureFps() {
  return new Promise((resolve) => {
    let frames = 0;
    const start = performance.now();
    const tick = () => {
      frames++;
      if (performance.now() - start < 1000) requestAnimationFrame(tick);
      else resolve(Math.round((frames * 1000) / (performance.now() - start)));
    };
    requestAnimationFrame(tick);
  });
}

// ============================================================ kimlik (cihaz anahtarı)
const ALG = { name: 'ECDSA', namedCurve: 'P-256' };

function idb() {
  return new Promise((resolve, reject) => {
    const r = indexedDB.open('couch', 1);
    r.onupgradeneeded = () => r.result.createObjectStore('kv');
    r.onsuccess = () => resolve(r.result);
    r.onerror = () => reject(r.error);
  });
}
async function kv(mode, key, value) {
  const db = await idb();
  return new Promise((resolve, reject) => {
    const store = db.transaction('kv', mode === 'get' ? 'readonly' : 'readwrite').objectStore('kv');
    const r = mode === 'get' ? store.get(key) : mode === 'set' ? store.put(value, key) : store.delete(key);
    r.onsuccess = () => resolve(r.result);
    r.onerror = () => reject(r.error);
  });
}

async function loadIdentity() {
  try {
    const v = await kv('get', 'identity');
    if (v?.privateKey) return v;
  } catch (e) {
    exp.idbReadError = String(e);
  }
  try {
    const saved = JSON.parse(localStorage.getItem('identity') ?? 'null');
    if (saved) {
      saved.privateKey = await crypto.subtle.importKey('jwk', saved.privateJwk, ALG, false, ['sign']);
      return saved;
    }
  } catch (e) {
    exp.lsReadError = String(e);
  }
  return null;
}

async function forgetIdentity() {
  try { await kv('delete', 'identity'); } catch {}
  localStorage.removeItem('identity');
}

// Tercih: dışa aktarılamayan özel anahtar, IndexedDB'de. Olmazsa: JWK olarak localStorage.
async function createKeys() {
  try {
    const kp = await crypto.subtle.generateKey(ALG, false, ['sign', 'verify']);
    await kv('set', 'probe', kp.privateKey);
    await kv('delete', 'probe');
    return { kp, mode: 'idb-nonextractable' };
  } catch (e) {
    exp.nonExtractableError = String(e);
  }
  const kp = await crypto.subtle.generateKey(ALG, true, ['sign', 'verify']);
  return { kp, mode: 'localstorage-jwk' };
}

async function pair(secret) {
  const { kp, mode } = await createKeys();
  const publicKeyJwk = await crypto.subtle.exportKey('jwk', kp.publicKey);
  const r = await fetch('/api/pair', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ ...secret, publicKeyJwk, keyMode: mode, name: /iPad/.test(navigator.userAgent) ? 'iPad' : 'iPhone' }),
  });
  const body = await r.json().catch(() => ({}));
  if (!r.ok) throw Object.assign(new Error(body.message ?? tl('Eşleşme başarısız')), { code: body.error });
  const identity = { deviceId: body.deviceId, mode };
  if (mode === 'idb-nonextractable') {
    await kv('set', 'identity', { ...identity, privateKey: kp.privateKey });
  } else {
    localStorage.setItem('identity', JSON.stringify({ ...identity, privateJwk: await crypto.subtle.exportKey('jwk', kp.privateKey) }));
  }
  exp.keyMode = mode;
  return { ...identity, privateKey: kp.privateKey };
}

// ============================================================ yardımcılar
const b64e = (buf) => btoa(String.fromCharCode(...new Uint8Array(buf)));
const b64d = (s) => Uint8Array.from(atob(s), (c) => c.charCodeAt(0));

let toastTimer;
function toast(text) {
  const el = $('toast');
  el.textContent = tl(text);
  el.classList.add('show');
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => el.classList.remove('show'), 2200);
}

// iOS 18+: switch tipindeki checkbox'a tıklamak hafif titreşim üretir (sadece kullanıcı dokunuşu sırasında)
const hapticLabel = $('haptic');
const haptic = () => {
  if (settings.haptics) hapticLabel.click();
};

function show(id) {
  for (const s of ['guide', 'pin', 'main']) $(s).hidden = s !== id;
}

// ============================================================ bağlantı
let identity = null;
let ws = null;
let connected = false;
let retry = 0;
let retryTimer = null;

const inBuf = new ArrayBuffer(20);
const inView = new DataView(inBuf);
const pingBuf = new ArrayBuffer(12);
const pingView = new DataView(pingBuf);
const offBuf = new ArrayBuffer(20);
const offView = new DataView(offBuf);
let seq = 0;
let buttons = 0;
let lastSend = 0;

const T = { MOVE: 1, SCROLL: 2, STATE: 3, AIR: 4, PING: 0x10, PONG: 0x11, OFFSET: 0x12 };

let tvUi = null;
let pcConn = { state: '', text: '' };
function setConn(stateName, text) {
  pcConn = { state: stateName, text };
  if (tvUi) tvUi.refresh();
  else {
    $('dot').className = stateName;
    $('conn').textContent = text;
  }
}

// Bağlantı koptuğunda ekranı kapatmadan ince şerit; denetimler soluklaşır
function showBanner(on) {
  $('connBanner').hidden = !on;
  document.body.classList.toggle('offline', on);
}
$('connRetry').onclick = () => {
  retry = 0;
  connect();
};

function connect() {
  clearTimeout(retryTimer);
  if (ws && ws.readyState <= 1) return;
  setConn('', tl('Bağlanıyor…'));
  ws = new WebSocket(`wss://${location.host}/ws`);
  ws.binaryType = 'arraybuffer';

  ws.onmessage = async (ev) => {
    if (typeof ev.data === 'string') {
      const m = JSON.parse(ev.data);
      if (m.type === 'challenge') {
        const sig = await crypto.subtle.sign({ name: 'ECDSA', hash: 'SHA-256' }, identity.privateKey, b64d(m.nonce));
        ws.send(JSON.stringify({ type: 'auth', deviceId: identity.deviceId, sig: b64e(sig) }));
      } else if (m.type === 'ok') {
        connected = true;
        retry = 0;
        $('offline').hidden = true;
        showBanner(false);
        setConn('ok', m.name || m.host);
        if (!views.apps.hidden) requestApps(true);
        pingBurst();
        for (const r of pendingReports.splice(0)) sendJson(r);
      } else if (m.type === 'unknown-device' || m.type === 'revoked') {
        // Eşleşme bilgisayardan kaldırıldı: bu telefondaki anahtarı da sil, eşleşme ekranına dön
        await forgetIdentity();
        location.replace('/app');
      } else if (m.type === 'stats') {
        serverStats = m;
        renderHud();
      } else if (m.type === 'media') {
        onMedia(m);
      } else if (m.type === 'volume') {
        onVolume(m);
      } else if (m.type === 'context') {
        onContext(m);
      } else if (m.type === 'timer') {
        onTimer(m);
      } else if (m.type === 'clipboard') {
        onClipboard(m);
      } else if (m.type === 'game-art') {
        onGameArt(m);
      } else if (m.type === 'screenshot') {
        onShot(m);
      } else if (m.type === 'mixer') {
        onMixer(m);
      } else if (m.type === 'audio-devices') {
        onAudioDevices(m);
      } else if (m.type === 'tvs') {
        tvUi.onTvs(m);
      } else if (m.type === 'tv-add') {
        tvUi.onAdd(m);
      } else if (m.type === 'tv-scan') {
        tvUi.onScan(m);
      } else if (m.type === 'shortcut-info') {
        siriInfo = m;
        renderSiri();
      } else if (m.type === 'clipboard-set') {
        onClipboardSet(m);
      } else if (m.type === 'upload-token') {
        onUploadToken(m);
      } else if (m.type === 'apps') {
        onApps(m);
      } else if (m.type === 'launched') {
        onLaunched(m);
      } else if (m.type === 'launch-error') {
        toast(m.message || tl('Açılamadı'));
      }
      return;
    }
    const dv = new DataView(ev.data);
    if (dv.getUint8(0) === T.PONG) onPong(dv.getFloat64(4, true), dv.getFloat64(12, true));
  };

  ws.onclose = () => {
    connected = false;
    setConn('down', tl('Bağlantı yok'));
    if (document.visibilityState !== 'visible') return;
    retry++;
    showBanner(true);
    if (retry >= 10) $('offline').hidden = false;
    retryTimer = setTimeout(connect, Math.min(250 * 2 ** retry, 3000));
  };
  ws.onerror = () => {};
}

function sendInput(type, a, b, t) {
  if (window.__preview) (window.__sentInput ??= []).push({ type, a, b, buttons }); // önizlemede gönderilecekler incelenebilsin
  if (tvUi && tvUi.input(type, a, b, buttons)) return; // hedef TV ise touchpad/hava faresi TV'yi sürer
  if (!connected) return;
  inView.setUint8(0, type);
  inView.setUint8(1, buttons);
  seq = (seq + 1) & 0xffff;
  inView.setUint16(2, seq, true);
  inView.setFloat32(4, a, true);
  inView.setFloat32(8, b, true);
  inView.setFloat64(12, t, true);
  ws.send(inBuf);
  lastSend = performance.now();
  sentThisSecond++;
}
const sendState = (t = now()) => sendInput(T.STATE, 0, 0, t);

// Heartbeat: PC 300 ms paket görmezse basılı butonları bırakır
setInterval(() => {
  if (connected && performance.now() - lastSend > 90) sendState();
}, 100);

// ============================================================ gecikme ölçümü
// Tek yönlü gecikme saatler senkron olmadığından doğrudan ölçülemez:
// en düşük RTT'li ping'lerden saat farkı tahmin edilir, PC bununla tek yönlü süreyi hesaplar.
const rtts = [];
const offsets = [];
let serverStats = null;
let sentThisSecond = 0;
let sendRate = 0;
let fps = null;

function ping() {
  if (!connected) return;
  pingView.setUint8(0, T.PING);
  pingView.setFloat64(4, now(), true);
  ws.send(pingBuf);
}
function pingBurst() {
  for (let i = 0; i < 8; i++) setTimeout(ping, i * 120);
}
setInterval(ping, 1000);
setInterval(() => {
  sendRate = sentThisSecond;
  sentThisSecond = 0;
}, 1000);

function onPong(t0, ts) {
  const t1 = now();
  const rtt = t1 - t0;
  rtts.push(rtt);
  if (rtts.length > 300) rtts.shift();
  offsets.push({ rtt, off: ts - (t0 + t1) / 2 });
  if (offsets.length > 30) offsets.shift();
  const best = offsets.reduce((a, b) => (b.rtt < a.rtt ? b : a));
  offView.setUint8(0, T.OFFSET);
  offView.setFloat64(4, best.off, true);
  offView.setFloat64(12, best.rtt, true);
  ws.send(offBuf);
  renderHud();
}

function pct(arr, q) {
  if (!arr.length) return '–';
  const s = [...arr].sort((a, b) => a - b);
  return s[Math.min(s.length - 1, Math.floor(q * s.length))].toFixed(1);
}

const hl = (s) => tl(s).padEnd(13);
function renderHud() {
  const owP50 = serverStats?.oneWay?.p50;
  const rttHalf = rtts.length ? Number(pct(rtts, 0.5)) / 2 : null;
  const ms = owP50 ?? rttHalf;
  $('lat').textContent = connected && ms != null ? `${Math.round(ms)} ms` : '';
  const hud = $('hud');
  if (hud.classList.contains('off')) return;
  const ow = serverStats?.oneWay;
  const pr = serverStats?.proc;
  hud.textContent =
    `${hl('RTT')}p50 ${pct(rtts, 0.5)}  p95 ${pct(rtts, 0.95)} ms\n` +
    `${hl('Tek yön*')}${ow ? `p50 ${ow.p50}  p95 ${ow.p95}  p99 ${ow.p99} ms (n=${ow.n})` : tl('parmağını touchpad\'de gezdir')}\n` +
    `${hl('PC işleme')}${pr ? `p50 ${pr.p50}  p99 ${pr.p99} ms` : '–'}\n` +
    `${hl('Paket/sn')}iPhone ${sendRate} · PC ${serverStats?.rate ?? '–'}   ${tl('Ekran yenileme')} ${fps ?? '…'} Hz\n` +
    tl("* dokunma anından PC'ye varışa, saat farkı tahminiyle");
}

// ============================================================ hava faresi
// Parmak (veya birkaç parmaktan ilki) pad'e dokunduğu sürece telefonun yönelimi imleci sürer; bırakınca imleç durur
// (fareyi kaldırıp yeniden yerleştirmek gibi). Hesap air.js'te: eğim/roll'dan bağımsız, titreme süzülmüş, ivmeli.
const air = new AirMouse(1);
let airMode = false; // arayüz modu
let airReady = false; // sensör izni alındı ve dinleniyor
let airPointing = false; // şu an dokunuyor ve imleç sürülüyor
let airTouchAt = 0;
const AIR_DELAY_MS = 130; // dokunuşun ilk anındaki sarsıntı imleci oynatmasın (tık bu sürede biter)
const AIR_TAP_MS = 190; // hava modunda "hızlı dokun = tık" süresi
let airEvents = 0;
const airDot = { x: 0, y: 0 };
let airFirstAt = 0;
let airAwaitTimer = 0;

function setAirNote(text) {
  $('airNote').textContent = text;
}

async function ensureAirSensors() {
  if (airReady) return true;
  if (typeof DeviceOrientationEvent === 'undefined') {
    setAirNote(tl('Bu cihazda hareket sensörü yok.'));
    return false;
  }
  // iOS 13+: izin sadece kullanıcı dokunuşu sırasında istenebilir (bu fonksiyon dokunuş olayından çağrılır)
  if (typeof DeviceOrientationEvent.requestPermission === 'function') {
    let result = 'denied';
    try {
      result = await DeviceOrientationEvent.requestPermission();
    } catch {
      // izin penceresi gösterilemedi
    }
    if (result !== 'granted') {
      setAirNote(tl('Hareket izni verilmedi. Ayarlar › Safari › Hareket ve Yön Erişimi\'ni aç, sonra tekrar dene.'));
      return false;
    }
  }
  window.addEventListener('deviceorientation', onAirOrientation);
  airReady = true;
  airEvents = 0;
  airFirstAt = 0;
  // Sensörden hiç olay gelmiyorsa kullanıcıya söyle
  clearTimeout(airAwaitTimer);
  airAwaitTimer = setTimeout(() => {
    if (airMode && airEvents === 0) setAirNote(tl('Hareket verisi gelmiyor. Ayarlar › Safari › Hareket ve Yön Erişimi\'ni kontrol et.'));
  }, 2500);
  return true;
}

function onAirOrientation(e) {
  airEvents++;
  if (!airFirstAt) airFirstAt = performance.now();
  if (airEvents === 1) setAirNote('');
  // Ayar için: "Gecikme ayrıntıları" açıksa sensör hızı gösterilir (iPhone'da genelde 60 Hz)
  if (airEvents === 120 && settings.hud) {
    const hz = Math.round(120 / ((performance.now() - airFirstAt) / 1000));
    setAirNote(tl('Sensör: {hz} Hz', { hz }));
    report('air-rate', { hz });
  }
  if (!airPointing || !airMode) return;
  if (performance.now() - airTouchAt < AIR_DELAY_MS) {
    air.reset(); // bekleme süresince başlangıç yönelimi güncel tutulur
    return;
  }
  const o = air.update(e.alpha, e.beta, e.gamma, e.timeStamp || performance.now());
  if (o && (o.dx || o.dy)) {
    sendInput(T.AIR, o.dx, o.dy, evTime(e));
    // Halkadaki nokta telefonun gittiği yönü gösterir (görsel geri bildirim; hesaba katılmaz)
    airDot.x = Math.max(-26, Math.min(26, airDot.x * 0.72 + o.dx * 1.1));
    airDot.y = Math.max(-26, Math.min(26, airDot.y * 0.72 + o.dy * 1.1));
    $('airDot').style.transform = `translate(${airDot.x}px, ${airDot.y}px)`;
  }
}

function airTouchStart() {
  if (!airMode) return;
  airPointing = true;
  airTouchAt = performance.now();
  air.reset();
  $('pad').classList.add('pointing');
}
function airTouchEnd() {
  if (!airPointing) return;
  airPointing = false;
  air.reset();
  airDot.x = airDot.y = 0;
  $('airDot').style.transform = '';
  $('pad').classList.remove('pointing');
}

async function setAirMode(on) {
  if (on && !(await ensureAirSensors())) on = false;
  airMode = on;
  $('pad').classList.toggle('air', on);
  for (const b of document.querySelectorAll('#padMode [data-v]')) b.classList.toggle('on', b.dataset.v === (on ? 'air' : 'touch'));
  if (!on) airTouchEnd();
  else report('air-on', { events: airEvents, hasPermissionApi: typeof DeviceOrientationEvent?.requestPermission === 'function' });
}
for (const b of document.querySelectorAll('#padMode [data-v]')) {
  b.addEventListener('click', () => {
    haptic();
    setAirMode(b.dataset.v === 'air'); // dokunuş olayının içinde çağrılır: iOS izin penceresi için gerekli
  });
}
document.addEventListener('visibilitychange', () => document.hidden && airTouchEnd());

// ============================================================ touchpad
const pad = $('pad');
const pts = new Map(); // pointerId → {x, y, sx, sy}
const SLOP_PX = 10;
const TAP_MS = 250;
const DOUBLE_TAP_MS = 300;
let gestureStart = 0;
let maxPts = 0;
let moved = false;
let dragPending = false;
let dragging = false;
let lastTap = { t: -1e9, x: 0, y: 0 };
let swipe3 = { x: 0, y: 0 }; // 3 parmak hareketinin toplamı (piksel)

// ---- ivmeli kaydırma: parmaklar kalkınca kaydırma iPhone'daki gibi yavaşlayarak sürer
const scrollSamples = []; // son 2 parmak kaydırma adımları {t, dx, dy} (mm)
const INERTIA_WINDOW_MS = 80;
const INERTIA_DECAY_PER_MS = 0.995; // iOS'taki "normal" yavaşlama hızına yakın
const INERTIA_MIN_SPEED = 0.01; // mm/ms
let inertiaFrame = 0;

function stopInertia() {
  if (inertiaFrame) cancelAnimationFrame(inertiaFrame);
  inertiaFrame = 0;
}

function startInertia(endTime) {
  const recent = scrollSamples.filter((s) => endTime - s.t <= INERTIA_WINDOW_MS);
  scrollSamples.length = 0;
  if (recent.length < 2) return false;
  const span = Math.max(endTime - recent[0].t, 16);
  let vx = recent.reduce((a, s) => a + s.dx, 0) / span;
  let vy = recent.reduce((a, s) => a + s.dy, 0) / span;
  if (Math.hypot(vx, vy) < INERTIA_MIN_SPEED * 5) return false; // yavaş bırakıldıysa atalet yok
  let last = performance.now();
  const step = (nowT) => {
    const dt = Math.min(nowT - last, 50);
    last = nowT;
    const decay = Math.pow(INERTIA_DECAY_PER_MS, dt);
    vx *= decay;
    vy *= decay;
    if (Math.hypot(vx, vy) < INERTIA_MIN_SPEED) {
      inertiaFrame = 0;
      sendState();
      return;
    }
    sendInput(T.SCROLL, vx * dt, vy * dt, now());
    inertiaFrame = requestAnimationFrame(step);
  };
  inertiaFrame = requestAnimationFrame(step);
  return true;
}

// ---- 3 parmak: sanal masaüstü, görev görünümü, masaüstünü göster (Windows touchpad hareketleriyle aynı)
const SWIPE3_MIN_PX = 40;
function finishSwipe3() {
  const { x, y } = swipe3;
  if (Math.max(Math.abs(x), Math.abs(y)) < SWIPE3_MIN_PX) return false;
  if (Math.abs(x) > Math.abs(y)) {
    sendJson({ type: 'key', key: x < 0 ? 'right' : 'left', mods: ['ctrl', 'win'] }); // parmak sola → sonraki masaüstü
  } else if (y < 0) {
    sendJson({ type: 'key', key: 'tab', mods: ['win'] }); // yukarı → görev görünümü
  } else {
    sendJson({ type: 'combo', char: 'd', mods: ['win'] }); // aşağı → masaüstünü göster
  }
  haptic();
  return true;
}

function click(bit, t) {
  buttons |= bit;
  sendState(t);
  buttons &= ~bit;
  sendState(t);
  haptic();
}

let wakeLock = null;
let wakeLockResult = 'denenmedi';
async function requestWakeLock() {
  if (!('wakeLock' in navigator)) {
    wakeLockResult = 'api yok';
    return;
  }
  try {
    wakeLock = await navigator.wakeLock.request('screen');
    wakeLockResult = 'ok';
  } catch (e) {
    wakeLockResult = `hata: ${e.name}`;
  }
}

// H3: ekranı sonsuza kadar açık tutma — ayardaki süre boyunca dokunulmazsa Wake Lock bırakılır,
// iPhone kendi otomatik kilit süresiyle kararır. Tekrar dokununca yeniden alınır.
let lastActivity = performance.now();
function onActivity() {
  lastActivity = performance.now();
  if (firstTouchReported && (!wakeLock || wakeLock.released)) requestWakeLock();
}
document.addEventListener('pointerdown', onActivity, { capture: true, passive: true });
document.addEventListener('keydown', onActivity, true);
setInterval(() => {
  if (settings.screenMin === 'always' || !wakeLock || wakeLock.released) return;
  if (performance.now() - lastActivity > Number(settings.screenMin) * 60_000) {
    wakeLock.release().catch(() => {});
    wakeLock = null;
  }
}, 15_000);

let firstTouchReported = false;
async function onFirstTouch() {
  if (firstTouchReported) return;
  firstTouchReported = true;
  pad.classList.add('used');
  try { localStorage.setItem('padHinted', '1'); } catch {}
  await requestWakeLock();
  report('first-touch', { wakeLockResult, fps });
}

// Aynı hareketler hem ana touchpad'de hem klavye ekranındaki küçük touchpad'de çalışır
// Dokunuş geri bildirimi: parmağın değdiği yerde dalga, parmağı izleyen yumuşak ışık
function touchFx(el, e) {
  if (matchMedia('(prefers-reduced-motion: reduce)').matches) return;
  const r = el.getBoundingClientRect();
  const x = e.clientX - r.left;
  const y = e.clientY - r.top;
  const ripple = document.createElement('i');
  ripple.className = 'ripple';
  ripple.style.left = `${x}px`;
  ripple.style.top = `${y}px`;
  ripple.addEventListener('animationend', () => ripple.remove(), { once: true });
  el.append(ripple);
}
function moveGlow(el, e) {
  const g = el === pad ? $('fingerGlow') : null;
  if (!g) return;
  const r = el.getBoundingClientRect();
  g.style.transform = `translate3d(${e.clientX - r.left - 70}px, ${e.clientY - r.top - 70}px, 0)`;
}
function bindPad(el) {
  el.addEventListener('pointerdown', (e) => {
    e.preventDefault();
    el.setPointerCapture(e.pointerId);
    pts.set(e.pointerId, { x: e.clientX, y: e.clientY, sx: e.clientX, sy: e.clientY });
    stopInertia(); // dokununca kayan sayfa durur
    if (pts.size === 1) {
      gestureStart = e.timeStamp;
      maxPts = 1;
      moved = false;
      swipe3 = { x: 0, y: 0 };
      scrollSamples.length = 0;
      dragPending = e.timeStamp - lastTap.t < DOUBLE_TAP_MS && Math.hypot(e.clientX - lastTap.x, e.clientY - lastTap.y) < 40;
      if (el === pad && airMode) {
        airTouchStart();
        // Hava modunda çift dokun + tut = sürükle: parmak hareket etmez, imleci telefon sürer; tuş hemen basılır
        if (dragPending) {
          dragging = true;
          buttons |= 1;
          sendState(evTime(e));
        }
      }
    } else {
      maxPts = Math.max(maxPts, pts.size);
      if (el === pad) airTouchEnd(); // ikinci parmak = kaydırma hareketi: işaret etme durur
    }
    el.classList.add('active');
    onFirstTouch();
    touchFx(el, e);
  });

  el.addEventListener('pointermove', (e) => {
    const p = pts.get(e.pointerId);
    if (!p) return;
    e.preventDefault();
    moveGlow(el, e);
    const evs = e.getCoalescedEvents?.() ?? [];
    let dx = 0;
    let dy = 0;
    for (const ev of evs.length ? evs : [e]) {
      dx += ev.clientX - p.x;
      dy += ev.clientY - p.y;
      p.x = ev.clientX;
      p.y = ev.clientY;
    }
    if (!dx && !dy) return;
    if (!moved && Math.hypot(p.x - p.sx, p.y - p.sy) > SLOP_PX) moved = true;

    if (el === pad && airMode && pts.size === 1 && maxPts === 1) {
      // hava modunda tek parmak imleci SÜRMEZ (telefon sürer)
    } else if (pts.size === 1 && maxPts === 1) {
      if (dragPending && moved && !dragging) {
        dragging = true;
        buttons |= 1;
      }
      sendInput(T.MOVE, dx * MM_PER_PX * settings.sens, dy * MM_PER_PX * settings.sens, evTime(e));
    } else if (pts.size === 2 && maxPts === 2) {
      // her parmak toplam hareketin yarısını katkılar → ortalama
      const k = MM_PER_PX * settings.scroll * (settings.natural ? 1 : -1);
      const sx = (dx / 2) * k;
      const sy = (dy / 2) * k;
      sendInput(T.SCROLL, sx, sy, evTime(e));
      scrollSamples.push({ t: e.timeStamp, dx: sx, dy: sy });
      if (scrollSamples.length > 64) scrollSamples.shift();
    } else if (maxPts === 3) {
      swipe3.x += dx / 3;
      swipe3.y += dy / 3;
    }
  });

  const endPointer = (e) => {
    if (!pts.delete(e.pointerId)) return;
    if (pts.size) return;
    el.classList.remove('active');
    if (el === pad) airTouchEnd();
    const dur = e.timeStamp - gestureStart;
    const t = evTime(e);
    if (dragging) {
      dragging = false;
      buttons &= ~1;
      sendState(t);
    } else if (!moved && dur < (el === pad && airMode ? AIR_TAP_MS : TAP_MS) + (maxPts > 1 ? 80 : 0)) {
      if (maxPts === 1) {
        click(1, t);
        lastTap = { t: e.timeStamp, x: e.clientX, y: e.clientY };
      } else if (maxPts === 2) {
        click(2, t);
      } else if (maxPts === 3) {
        click(4, t); // orta tık
      }
    } else if (maxPts === 3) {
      finishSwipe3();
    } else if (!(maxPts === 2 && startInertia(e.timeStamp))) {
      sendState(t); // kaydırma birikimini sıfırla
    }
  };
  el.addEventListener('pointerup', endPointer);
  el.addEventListener('pointercancel', endPointer);
}
bindPad(pad);

// Alt butonlar: basılı tutulabilir (bir parmak butonda, diğeri touchpad'de = sürükle)
for (const el of document.querySelectorAll('.mb')) {
  const bit = Number(el.dataset.bit);
  el.addEventListener('pointerdown', (e) => {
    e.preventDefault();
    el.setPointerCapture(e.pointerId);
    el.classList.add('down');
    buttons |= bit;
    sendState(evTime(e));
    haptic();
    onFirstTouch();
  });
  const up = (e) => {
    if (!el.classList.contains('down')) return;
    el.classList.remove('down');
    buttons &= ~bit;
    sendState(evTime(e));
  };
  el.addEventListener('pointerup', up);
  el.addEventListener('pointercancel', up);
}

// iPhone klavyesi açılınca görünür alan küçülür: arayüzü ona göre boyutlandır
if (window.visualViewport) {
  const fitViewport = () => {
    const h = Math.round(visualViewport.height);
    // Sayfa yüklenirken / arka plandayken yükseklik geçici olarak 0 okunabilir: o zaman tam ekrana dön
    if (h > 100) document.documentElement.style.setProperty('--vvh', `${h}px`);
    else document.documentElement.style.removeProperty('--vvh');
    window.scrollTo(0, 0); // iOS odaklanınca sayfayı kaydırabiliyor
  };
  visualViewport.addEventListener('resize', fitViewport);
  window.addEventListener('orientationchange', () => setTimeout(fitViewport, 300));
  document.addEventListener('visibilitychange', fitViewport);
  fitViewport();
}

// Safari'nin yakınlaştırma / sayfa kaydırma hareketlerini engelle
document.addEventListener('gesturestart', (e) => {
  if (!$('main').hidden) e.preventDefault(); // kontrol yüzeyinde sayfa yakınlaşmasın; rehber ve PIN sayfalarında yakınlaştırma serbest
});
// Kaydırılabilir alanlar (kumanda ekranı, yazı kutusu, tuş şeridi) hariç sayfa kaymasın
document.addEventListener('touchmove', (e) => {
  if (!$('main').hidden && !e.target.closest('#v-remote, #v-apps, #v-share, #typer, #clipText, .keys.scroll, .sheet, .shot-box')) e.preventDefault();
}, { passive: false });

// Uygulamadan çıkınca: butonları bırak, bağlantıyı kapat. Dönünce: anında yeniden bağlan.
document.addEventListener('visibilitychange', () => {
  if (!identity) return;
  if (document.visibilityState === 'hidden') {
    buttons = 0;
    if (connected) sendState();
    ws?.close(1000);
  } else {
    retry = 0;
    connect();
    if (wakeLock?.released !== false && firstTouchReported) requestWakeLock();
  }
});
$('retryBtn').onclick = () => {
  retry = 0;
  connect();
};

// ============================================================ JSON komutları (klavye, medya)
function sendJson(obj) {
  if (window.__preview) (window.__sent ??= []).push(obj); // önizlemede gönderilecekler incelenebilsin
  if (obj.type !== 'tv' && tvUi && tvUi.json(obj)) return true; // hedef TV ise klavye TV'ye yazar
  if (!connected) return false;
  ws.send(JSON.stringify(obj));
  lastSend = performance.now();
  return true;
}

// ============================================================ sekmeler
const views = { pad: $('v-pad'), kbd: $('v-kbd'), remote: $('v-remote'), apps: $('v-apps'), share: $('v-share') };
let currentViewName = null;
function showView(name) {
  // Yeni ekran, sekme sırasına göre sağdan veya soldan süzülür
  const order = Object.keys(views);
  const dir = currentViewName && currentViewName !== name ? Math.sign(order.indexOf(name) - order.indexOf(currentViewName)) : 0;
  $('main').style.setProperty('--dx', `${dir * 26}px`);
  currentViewName = name;
  if (tvUi) tvUi.onView();
  for (const [k, el] of Object.entries(views)) el.hidden = k !== name;
  for (const b of document.querySelectorAll('#tabs button')) {
    b.classList.toggle('on', b.dataset.view === name);
    b.setAttribute('aria-current', b.dataset.view === name ? 'page' : 'false');
  }
  try {
    localStorage.setItem('view', name);
  } catch {
    // özel sekme / depolama kapalı: sorun değil
  }
  if (name === 'remote') renderMedia();
  if (name === 'apps') requestApps(false);
}
for (const b of document.querySelectorAll('#tabs button')) {
  b.addEventListener('click', () => {
    haptic();
    showView(b.dataset.view);
    // Metin çubuğu modunda klavye sekmesine geçince iOS klavyesini aç (dokunma olayı içinde olmalı)
    if (b.dataset.view === 'kbd' && settings.kbdMode === 'text') typer.focus();
  });
}

// ============================================================ klavye
// İki kullanım: "Tuşlar" = ekranda tam Türkçe Q klavye (+ küçük touchpad), "Metin çubuğu" = iPhone'un
// kendi klavyesi (otomatik düzeltme, dikte, emoji). Üstteki PC tuş şeridi ikisinde de var.
const typer = $('typer');
const kbEl = $('kb');
const SENTINEL = '​'; // görünmez karakter: kutu "boş"ken de silme tuşu algılansın
let typed = SENTINEL;

// ---- değiştirici tuşlar: bir dokunuş = sadece sonraki tuş için, çift dokunuş = kilitli.
// Shift kilidi = büyük harf kilidi (Caps Lock gibi; enter/oklara shift eklemez)
const modState = new Map(); // 'ctrl' | 'alt' | 'win' | 'shift' → 'on' | 'lock'
const modTapAt = {};
const MOD_ORDER = ['ctrl', 'alt', 'shift', 'win'];
const modList = () => MOD_ORDER.filter((m) => modState.get(m) === 'on' || (m !== 'shift' && modState.get(m) === 'lock'));
const shiftOn = () => modState.has('shift');
function renderMods() {
  for (const b of document.querySelectorAll('#v-kbd [data-mod]')) {
    const st = modState.get(b.dataset.mod);
    b.classList.toggle('on', !!st);
    b.classList.toggle('lock', st === 'lock');
  }
  const sh = modState.get('shift');
  kbEl.classList.toggle('upper', !!sh);
  const use = kbEl.querySelector('.shift use');
  if (use) use.setAttribute('href', sh === 'lock' ? '#i-caps' : sh ? '#i-shift-on' : '#i-shift');
  kbEl.querySelector('.shift')?.classList.toggle('on', !!sh);
  kbEl.querySelector('.shift')?.classList.toggle('lock', sh === 'lock');
}
function toggleMod(m) {
  const st = modState.get(m);
  const t = performance.now();
  if (st === 'on' && t - (modTapAt[m] || 0) < 350) modState.set(m, 'lock');
  else if (st) modState.delete(m);
  else modState.set(m, 'on');
  modTapAt[m] = t;
  renderMods();
}
function clearMods() {
  let changed = false;
  for (const [m, st] of modState) {
    if (st === 'on') {
      modState.delete(m);
      changed = true;
    }
  }
  if (changed) renderMods();
}

// ---- gönderme
function sendChar(ch) {
  const mods = modList().filter((m) => m !== 'shift');
  if (mods.length) {
    // Ctrl/Alt/Win + harf: kısayol (ör. ctrl+c). Shift de açıksa değiştirici olarak eklenir (ctrl+shift+t).
    sendJson({ type: 'combo', char: ch, mods: shiftOn() && /\p{L}/u.test(ch) ? [...mods, 'shift'] : mods });
  } else {
    sendJson({ type: 'text', text: shiftOn() ? ch.toLocaleUpperCase(kbLocale()) : ch });
  }
  clearMods();
}
function sendNamedKey(key) {
  sendJson({ type: 'key', key, mods: modList() });
  clearMods();
}

// ---- tuş düzenleri (Türkçe Q). Metin = karakter tuşu; nesne = özel tuş
const K = {
  shift: { t: 'shift', cls: 'fn wide shift', icon: 'i-shift', label: tl('Shift') },
  bksp: { t: 'key', key: 'backspace', cls: 'fn wide', icon: 'i-backspace', label: tl('Sil'), repeat: true },
  enter: { t: 'key', key: 'enter', cls: 'fn enter', icon: 'i-return', label: tl('Enter') },
  space: { t: 'key', key: 'space', cls: 'space', text: tl('boşluk'), repeat: true },
  n123: { t: 'layer', to: 'num', cls: 'fn', text: '123' },
  abc: { t: 'layer', to: 'abc', cls: 'fn', text: 'ABC' },
  sym: { t: 'layer', to: 'sym', cls: 'fn wide', text: '#+=' },
  num: { t: 'layer', to: 'num', cls: 'fn wide', text: '123' },
  fn: { t: 'layer', to: 'fn', cls: 'fn', text: tl('Fn') },
};
const named = (key, text) => ({ t: 'key', key, cls: 'fn small', text, repeat: key === 'delete' });
const combo = (char, text) => ({ t: 'combo', char, cls: 'fn small', text });
const bottom = (first) => [first, K.fn, ',', K.space, '.', K.enter];
// Ekran klavyesi düzeni: 'tr' = Türkçe Q, 'us' = İngilizce QWERTY. Ayarlar henüz yüklenmediği için doğrudan depodan okunur.
function kbKind() {
  try {
    const v = JSON.parse(localStorage.getItem('settings') || '{}').kbLayout;
    if (v === 'tr' || v === 'us') return v;
  } catch {
    // bozuk kayıt
  }
  return lang === 'tr' ? 'tr' : 'us';
}
// Büyük harf dönüşümü düzene göre: Türkçede i → İ, ı → I
const kbLocale = () => (kbKind() === 'tr' ? 'tr' : 'en');
const upper = (s) => s.toLocaleUpperCase(lang === 'tr' ? 'tr' : undefined);
const LAYOUTS = {
  get abc() {
    return kbKind() === 'tr'
      ? [[...'qwertyuıopğü'], [...'asdfghjklşi'], [K.shift, ...'zxcvbnmöç', K.bksp], bottom(K.n123)]
      : [[...'qwertyuiop'], [...'asdfghjkl'], [K.shift, ...'zxcvbnm', K.bksp], bottom(K.n123)];
  },
  get num() {
    return [[...'1234567890'], [...(kbKind() === 'tr' ? '-/:;()₺&@"' : '-/:;()$&@"')], [K.sym, ...".,?!'", K.bksp], bottom(K.abc)];
  },
  get sym() {
    return [[...'[]{}#%^*+='], [...(kbKind() === 'tr' ? '_\\|~<>€$£`' : '_\\|~<>€£¥•')], [K.num, ...".,?!'", K.bksp], bottom(K.abc)];
  },
  fn: [
    Array.from({ length: 12 }, (_, i) => named('f' + (i + 1), 'F' + (i + 1))),
    [named('home', 'home'), named('end', 'end'), named('pageup', 'pg up'), named('pagedown', 'pg dn'), named('insert', 'ins'), named('delete', 'del'), named('printscreen', 'prt sc')],
    [combo('z', tl('Geri al')), combo('x', tl('Kes')), combo('c', tl('Kopyala')), combo('v', tl('Yapıştır')), combo('a', tl('Tümü')), K.bksp],
    bottom(K.abc),
  ],
};
let layer = 'abc';

function renderKb() {
  kbEl.textContent = '';
  kbEl.dataset.layer = layer;
  kbEl.dataset.kb = kbKind();
  for (const row of LAYOUTS[layer]) {
    const r = document.createElement('div');
    r.className = 'kb-row';
    for (const k of row) {
      const b = document.createElement('button');
      b.type = 'button';
      if (typeof k === 'string') {
        b.className = 'kk ch';
        b.textContent = k;
        // Harfler shift'e göre büyük görünür (Türkçe: i → İ, ı → I)
        b.dataset.up = k.toLocaleUpperCase(kbLocale());
        b._k = { t: 'char', ch: k };
      } else {
        b.className = 'kk ' + (k.cls || '');
        if (k.icon) b.innerHTML = `<svg class="ic"><use href="#${k.icon}"/></svg>`;
        else b.textContent = k.text;
        if (k.label) b.setAttribute('aria-label', k.label);
        if (k.t === 'layer' && k.to === layer) b.classList.add('on');
        b._k = k;
      }
      r.append(b);
    }
    kbEl.append(r);
  }
  renderMods();
}

// ---- basılı tutunca tekrar (sil, boşluk, oklar)
let repeatTimer = 0;
function stopRepeat() {
  clearTimeout(repeatTimer);
  clearInterval(repeatTimer);
  repeatTimer = 0;
}
function withRepeat(fn) {
  stopRepeat();
  fn();
  repeatTimer = setTimeout(() => {
    repeatTimer = setInterval(fn, 55);
  }, 420);
}

// ---- iOS'taki gibi büyüyen harf balonu
const pop = document.createElement('div');
pop.id = 'keyPop';
pop.hidden = true;
document.body.append(pop);
function showPop(b) {
  const r = b.getBoundingClientRect();
  pop.textContent = kbEl.classList.contains('upper') ? b.dataset.up : b.textContent;
  const w = Math.max(r.width + 18, 48);
  pop.style.width = `${w}px`;
  pop.style.left = `${Math.min(Math.max(r.left + r.width / 2 - w / 2, 2), innerWidth - w - 2)}px`;
  pop.style.top = `${r.top - 58}px`;
  pop.hidden = false;
}

function pressKey(b, k) {
  switch (k.t) {
    case 'char':
      showPop(b);
      sendChar(k.ch);
      break;
    case 'key':
      if (k.key === 'space' && !modList().length) withRepeat(() => sendChar(' '));
      else if (k.repeat) withRepeat(() => sendNamedKey(k.key));
      else sendNamedKey(k.key);
      break;
    case 'combo':
      sendJson({ type: 'combo', char: k.char, mods: ['ctrl'] });
      clearMods();
      break;
    case 'shift':
      toggleMod('shift');
      break;
    case 'layer':
      layer = layer === k.to && k.to === 'fn' ? 'abc' : k.to;
      renderKb();
      break;
  }
}

// Tuşlar parmak değdiği an basılır: en düşük gecikme, iki başparmakla hızlı yazarken sıra korunur
const downKeys = new Map(); // pointerId → tuş
kbEl.addEventListener('pointerdown', (e) => {
  const b = e.target.closest('.kk');
  if (!b) return;
  e.preventDefault();
  downKeys.set(e.pointerId, b);
  b.classList.add('down');
  haptic();
  pressKey(b, b._k);
});
function releaseKey(e) {
  const b = downKeys.get(e.pointerId);
  if (!b) return;
  downKeys.delete(e.pointerId);
  b.classList.remove('down');
  if (![...downKeys.values()].some((d) => d.classList.contains('ch'))) pop.hidden = true;
  stopRepeat();
}
kbEl.addEventListener('pointerup', releaseKey);
kbEl.addEventListener('pointercancel', releaseKey);
renderKb();

// PC tuş şeridi: esc, tab, ctrl/alt/win, oklar (oklar basılı tutunca tekrarlar)
for (const b of document.querySelectorAll('#strip .sk')) {
  b.addEventListener('pointerdown', (e) => {
    e.preventDefault(); // iPhone klavyesi açıksa kapanmasın
    b.setPointerCapture(e.pointerId);
    b.classList.add('down');
    haptic();
    if (b.dataset.mod) toggleMod(b.dataset.mod);
    else if (b.classList.contains('ar')) withRepeat(() => sendNamedKey(b.dataset.sk));
    else sendNamedKey(b.dataset.sk);
  });
  const up = () => {
    b.classList.remove('down');
    stopRepeat();
  };
  b.addEventListener('pointerup', up);
  b.addEventListener('pointercancel', up);
}

bindPad($('miniPad'));

// ---- Tuşlar / Metin çubuğu seçimi (ayar olarak hatırlanır)
function applyKbdMode(focus) {
  const text = settings.kbdMode === 'text';
  $('v-kbd').classList.toggle('text-mode', text);
  for (const b of document.querySelectorAll('#kbdMode [data-v]')) b.classList.toggle('on', b.dataset.v === settings.kbdMode);
  if (text && focus) typer.focus();
  if (!text) typer.blur();
}
for (const b of document.querySelectorAll('#kbdMode [data-v]')) {
  b.addEventListener('click', () => {
    haptic();
    settings.kbdMode = b.dataset.v;
    modState.clear(); // shift kilidi vb. diğer moda taşınmasın
    renderMods();
    saveSettings();
    applyKbdMode(true);
  });
}

// ---- metin çubuğu (iPhone klavyesi)
function resetTyper() {
  typer.value = typed = SENTINEL;
  typer.setSelectionRange(typed.length, typed.length);
}
resetTyper();

typer.addEventListener('focus', () => {
  if (!typer.value.startsWith(SENTINEL)) resetTyper();
});

// Önceki içerikle fark: silinen karakter sayısı kadar geri tuşu, eklenen kısım metin olarak gider.
// Otomatik düzeltme, kelime önerisi ve dikte de böylece doğru aktarılır.
typer.addEventListener('input', () => {
  const sentinelGone = !typer.value.startsWith(SENTINEL);
  const cur = sentinelGone ? SENTINEL + typer.value.replaceAll(SENTINEL, '') : typer.value;
  const a = Array.from(typed);
  const b = Array.from(cur);
  let p = 0;
  while (p < a.length && p < b.length && a[p] === b[p]) p++;
  // Görünmez karakter silindiyse kutu zaten boştu: bu bir geri tuşu
  const deletions = sentinelGone && a.length === 1 ? 1 : a.length - p;
  const inserted = b.slice(p).join('');

  if (modList().length && deletions === 0 && Array.from(inserted).length === 1) {
    // Ctrl/Alt/Win/Shift + harf: kısayol olarak gönder, kutuya yazma
    sendJson({ type: 'combo', char: inserted, mods: modList() });
    clearMods();
    typer.value = typed;
    return;
  }
  for (let i = 0; i < deletions; i++) sendJson({ type: 'key', key: 'backspace', mods: [] });
  if (inserted) sendJson({ type: 'text', text: inserted });
  typed = cur;
  if (sentinelGone || inserted.includes('\n') || typed.length > 400) resetTyper();
});

// Tuş düğmeleri (kumanda ekranında)
function pressFeedback(el) {
  el.addEventListener('pointerdown', () => {
    el.classList.add('down');
    haptic();
  });
  for (const ev of ['pointerup', 'pointercancel', 'pointerleave']) el.addEventListener(ev, () => el.classList.remove('down'));
}
for (const el of document.querySelectorAll('[data-key]')) {
  pressFeedback(el);
  el.addEventListener('click', () => {
    const inRemote = !!el.closest('#v-remote');
    sendJson({ type: 'key', key: el.dataset.key, mods: [], ...(inRemote ? { target: 'media' } : {}) });
  });
}
for (const el of document.querySelectorAll('[data-combo]')) {
  pressFeedback(el);
  el.addEventListener('click', () => sendJson({ type: 'combo', char: el.dataset.combo, mods: [], target: 'media' }));
}
for (const el of document.querySelectorAll('[data-media]')) {
  pressFeedback(el);
  el.addEventListener('click', () => sendJson({ type: 'media', action: el.dataset.media }));
}
for (const el of document.querySelectorAll('[data-seekby]')) {
  pressFeedback(el);
  el.addEventListener('click', () => seekBy(Number(el.dataset.seekby)));
}

// ============================================================ şu an çalıyor
let media = { active: false };
let mediaAt = 0;
let seeking = false;
const seek = $('npSeek');

function fmt(sec) {
  if (!Number.isFinite(sec) || sec < 0) sec = 0;
  const h = Math.floor(sec / 3600);
  const m = Math.floor((sec % 3600) / 60);
  const ss = String(Math.floor(sec % 60)).padStart(2, '0');
  return h ? `${h}:${String(m).padStart(2, '0')}:${ss}` : `${m}:${ss}`;
}

// Kapak görselinden baskın renk: kumandanın ortam ışığı ve kart gölgesi o renge boyanır (yatak modunda CSS bastırır)
const tintCanvas = document.createElement('canvas');
tintCanvas.width = tintCanvas.height = 24;
function applyTint(src) {
  const main = $('main');
  if (!src) {
    main.style.removeProperty('--tint');
    return;
  }
  const img = new Image();
  img.onload = () => {
    try {
      const ctx = tintCanvas.getContext('2d', { willReadFrequently: true });
      ctx.drawImage(img, 0, 0, 24, 24);
      const d = ctx.getImageData(0, 0, 24, 24).data;
      let r = 0, g = 0, b = 0, wsum = 0;
      for (let i = 0; i < d.length; i += 4) {
        const mx = Math.max(d[i], d[i + 1], d[i + 2]);
        const mn = Math.min(d[i], d[i + 1], d[i + 2]);
        const sat = mx ? (mx - mn) / mx : 0;
        const lum = (mx + mn) / 510;
        // Canlı (doygun) ve ne çok koyu ne çok açık pikseller baskın sayılır
        const w = 0.04 + sat * sat * Math.max(0.1, 1 - Math.abs(lum - 0.5) * 1.4);
        r += d[i] * w; g += d[i + 1] * w; b += d[i + 2] * w; wsum += w;
      }
      r /= wsum * 255; g /= wsum * 255; b /= wsum * 255;
      const mx = Math.max(r, g, b), mn = Math.min(r, g, b);
      let h = 0;
      if (mx !== mn) {
        const dd = mx - mn;
        h = mx === r ? ((g - b) / dd + (g < b ? 6 : 0)) : mx === g ? (b - r) / dd + 2 : (r - g) / dd + 4;
        h *= 60;
      }
      let l = (mx + mn) / 2;
      let s = mx === mn ? 0 : (mx - mn) / (1 - Math.abs(2 * l - 1));
      s = Math.min(1, Math.max(s, 0.55));
      l = Math.min(0.58, Math.max(l, 0.42));
      main.style.setProperty('--tint', `hsla(${Math.round(h)}, ${Math.round(s * 100)}%, ${Math.round(l * 100)}%, 0.4)`);
    } catch {
      main.style.removeProperty('--tint');
    }
  };
  img.src = src;
}

function onMedia(m) {
  if ('thumb' in m) {
    if (m.thumb) $('npThumb').src = m.thumb;
    else $('npThumb').removeAttribute('src');
    applyTint(m.thumb || '');
  }
  media = m;
  mediaAt = performance.now();
  renderMedia();
}

// PC her saniye konum gönderir; arada çalıyorsa geçen süre eklenerek akıcı gösterilir
function currentPosition() {
  const elapsed = media.playing ? (performance.now() - mediaAt) / 1000 : 0;
  const pos = (media.position ?? 0) + elapsed;
  return media.duration > 0 ? Math.min(pos, media.duration) : pos;
}

function renderMedia() {
  if (views.remote.hidden) return;
  const active = !!media.active;
  $('np').hidden = !active; // çalan bir şey yoksa kart hiç görünmez
  $('np').classList.toggle('empty', !active || !$('npThumb').getAttribute('src'));
  $('npTitle').textContent = active ? media.title || tl('Başlıksız') : tl('Çalan bir şey yok');
  $('npArtist').textContent = active ? media.artist || '' : tl('Bilgisayarda bir şey oynatınca burada görünür');
  $('npApp').textContent = active ? media.app || '' : '';
  $('ppBtn').classList.toggle('playing', active && !!media.playing);
  $('ppLabel').textContent = active && media.playing ? tl('Duraklat') : tl('Oynat');
  const dur = media.duration || 0;
  $('npBar').hidden = !(active && dur > 0);
  if (!seeking) {
    seek.max = String(Math.max(1, Math.round(dur)));
    seek.value = String(Math.round(currentPosition()));
    $('npPos').textContent = fmt(currentPosition());
  }
  seek.style.setProperty('--p', `${dur > 0 ? (Number(seek.value) / Number(seek.max)) * 100 : 0}%`);
  $('npDur').textContent = fmt(dur);
}
setInterval(renderMedia, 500);

seek.addEventListener('pointerdown', () => (seeking = true));
seek.addEventListener('input', () => {
  $('npPos').textContent = fmt(Number(seek.value));
  seek.style.setProperty('--p', `${(Number(seek.value) / Number(seek.max)) * 100}%`);
});
seek.addEventListener('change', () => {
  sendJson({ type: 'media', action: 'seek', value: Number(seek.value) });
  media.position = Number(seek.value);
  mediaAt = performance.now();
  seeking = false;
  haptic();
});

// ============================================================ uygulamalar (başlatıcı)
// Telefon sadece PC'nin listesindeki bir kimliği seçer; adresleri ve dosya yollarını PC kurar.
let appsData = null;
let appsLoadedAt = 0;
let searchTarget = 'youtube';
let showAllApps = false;
const searchInput = $('searchInput');

function requestApps(force) {
  if (!force && appsData && performance.now() - appsLoadedAt < 60_000) return;
  appsBusy = true;
  syncBusy();
  clearTimeout(appsBusyTimer);
  appsBusyTimer = setTimeout(() => { appsBusy = false; syncBusy(); }, 8000); // yanıt gelmezse çubuk takılı kalmasın
  sendJson({ type: 'apps' });
}
let appsBusy = false;
let appsBusyTimer = 0;
function syncBusy() {
  $('topProgress').hidden = !(appsBusy || document.body.classList.contains('tv-busy'));
}

function trLower(s) {
  return s.toLocaleLowerCase('tr');
}

// ---- oyun kapakları: PC'den küçük JPEG olarak gelir, telefonda saklanır (yoksa renkli harf kartı)
const artCache = new Map();
try {
  for (const [k, v] of Object.entries(JSON.parse(localStorage.getItem('art') || '{}'))) artCache.set(k, v);
} catch {
  // depolama kapalı
}
const artAsked = new Set();
const artQueue = [];
let artTimer = 0;
function saveArt() {
  try {
    localStorage.setItem('art', JSON.stringify(Object.fromEntries(artCache)));
  } catch {
    // dolu / kapalı
  }
}
function requestArt(id) {
  if (!id.startsWith('steam:') || artCache.has(id) || artAsked.has(id)) return;
  artAsked.add(id);
  artQueue.push(id);
  if (!artTimer) {
    artTimer = setInterval(() => {
      const next = artQueue.shift();
      if (!next) {
        clearInterval(artTimer);
        artTimer = 0;
        return;
      }
      sendJson({ type: 'game-art', id: next });
    }, 140); // sunucu hız sınırına takılmadan sırayla
  }
}
function onGameArt(m) {
  artCache.set(m.id, m.art || 0); // 0 = kapak yok (bir daha sorma)
  saveArt();
  for (const card of document.querySelectorAll(`[data-art-id="${CSS.escape(m.id)}"] .cover-art`)) fillCover(card, m.id);
}
const nameHue = (s) => [...s].reduce((h, c) => (h * 31 + c.charCodeAt(0)) % 360, 7);
function fillCover(el, id) {
  const art = artCache.get(id);
  el.querySelector('img')?.remove();
  if (art) {
    const img = document.createElement('img');
    img.alt = '';
    img.src = art;
    el.append(img);
    el.classList.add('has-art');
  }
}
// Servis simgesi: marka işareti varsa o (beyaz, düz renkli kare üstünde), yoksa kısa monogram
function brandMark(icon, a) {
  const d = BRAND[a.id];
  if (d) {
    icon.classList.add('has-logo');
    icon.innerHTML = `<svg viewBox="0 0 24 24" aria-hidden="true"><path fill="currentColor" d="${d}"/></svg>`;
  } else {
    icon.textContent = a.mark;
  }
}

function coverArt(g) {
  const el = document.createElement('div');
  el.className = 'cover-art ' + g.store;
  el.style.setProperty('--h', String(nameHue(g.name)));
  const ini = document.createElement('b');
  ini.textContent = upper(g.name.trim().charAt(0));
  el.append(ini);
  fillCover(el, g.id);
  return el;
}

// ---- son kullanılanlar (telefonda tutulur; sadece kimlik, açarken yine PC'deki listeden seçilir)
function recordRecent(id) {
  let list = [];
  try {
    list = JSON.parse(localStorage.getItem('recent') || '[]');
  } catch {
    // bozuk kayıt
  }
  list = [id, ...list.filter((x) => x !== id)].slice(0, 8);
  try {
    localStorage.setItem('recent', JSON.stringify(list));
  } catch {
    // depolama kapalı
  }
}
function renderRecent() {
  let list = [];
  try {
    list = JSON.parse(localStorage.getItem('recent') || '[]');
  } catch {
    // bozuk kayıt
  }
  const items = [];
  for (const id of list) {
    const cat = appsData.catalog.find((a) => a.id === id);
    if (cat) items.push({ id, name: cat.name, kind: 'cat', color: cat.color, mark: cat.mark });
    const game = (appsData.games ?? []).find((a) => a.id === id);
    if (game) items.push({ id, name: game.name, kind: 'game', game });
    const app = appsData.installed.find((a) => a.id === id);
    if (app) items.push({ id, name: app.name, kind: 'app' });
  }
  $('recentBox').hidden = !items.length || !!searchInput.value.trim();
  $('recentRow').replaceChildren(
    ...items.map((it) => {
      const b = document.createElement('button');
      b.className = 'recent-chip';
      let icon;
      if (it.kind === 'cat') {
        icon = document.createElement('div');
        icon.className = 'tile-icon mini';
        icon.style.setProperty('--c', it.color);
        brandMark(icon, it);
      } else if (it.kind === 'game') {
        icon = coverArt(it.game);
        icon.classList.add('mini');
      } else {
        icon = document.createElement('div');
        icon.className = 'initial';
        icon.textContent = upper(it.name.trim().charAt(0));
      }
      b.append(icon, Object.assign(document.createElement('span'), { textContent: it.name }));
      pressFeedback(b);
      b.onclick = () => launchApp(it.id, b);
      return b;
    }),
  );
}

function renderApps() {
  if (!appsData) return;
  // Arama hedefleri (servis içi arama destekleyenler)
  const searchable = appsData.catalog.filter((a) => a.search);
  if (!searchable.some((a) => a.id === searchTarget) && searchable.length) searchTarget = searchable[0].id;
  $('searchChips').replaceChildren(
    ...searchable.map((a) => {
      const b = document.createElement('button');
      b.type = 'button';
      b.className = 'chip-btn' + (a.id === searchTarget ? ' on' : '');
      b.textContent = a.name;
      b.onclick = () => {
        haptic();
        searchTarget = a.id;
        renderApps();
      };
      return b;
    }),
  );
  $('searchChips').hidden = !searchInput.value.trim();
  const target = searchable.find((a) => a.id === searchTarget);
  searchInput.placeholder = target ? tl('{name} içinde ara · uygulama bul', { name: target.name }) : tl('Uygulama bul');

  // Hızlı başlat
  $('tiles').replaceChildren(
    ...appsData.catalog.map((a) => {
      const b = document.createElement('button');
      b.className = 'tile';
      b.dataset.id = a.id;
      const icon = document.createElement('div');
      icon.className = 'tile-icon';
      icon.style.setProperty('--c', a.color);
      brandMark(icon, a);
      const name = document.createElement('span');
      name.textContent = a.name;
      b.append(icon, name);
      pressFeedback(b);
      b.onclick = () => launchApp(a.id, b);
      return b;
    }),
  );

  // Oyunlar (Steam, Epic): yatay kapak kartları; arama kutusuna göre süzülür
  const gfilter = trLower(searchInput.value.trim());
  const games = (appsData.games ?? []).filter((g) => !gfilter || trLower(g.name).includes(gfilter));
  $('gamesBox').hidden = games.length === 0;
  $('gamesNote').textContent = `· ${(appsData.games ?? []).length}`;
  $('gameList').replaceChildren(
    ...games.map((g) => {
      const b = document.createElement('button');
      b.className = 'cover';
      b.dataset.artId = g.id;
      b.append(coverArt(g), Object.assign(document.createElement('span'), { className: 'cover-name', textContent: g.name }));
      pressFeedback(b);
      b.onclick = () => launchApp(g.id, b);
      requestArt(g.id);
      return b;
    }),
  );
  renderRecent();

  // Bu bilgisayardaki uygulamalar (arama kutusuna göre süzülür)
  $('browserNote').textContent = appsData.chrome === false ? tl('· web siteleri varsayılan tarayıcıda açılır') : tl("· web siteleri Chrome'da açılır");
  const filter = trLower(searchInput.value.trim());
  const items = appsData.installed.filter((a) => !filter || trLower(a.name).includes(filter));
  const list = $('appList');
  if (!items.length) {
    const li = document.createElement('li');
    li.className = 'muted-row';
    li.textContent = filter ? tl('Eşleşen uygulama yok. "Ara"ya basarsan seçili serviste aranır.') : tl('Başlat menüsünde uygulama bulunamadı.');
    list.replaceChildren(li);
    return;
  }
  const COLLAPSED = 6;
  const collapsed = !filter && !showAllApps && items.length > COLLAPSED + 2;
  const shown = collapsed ? items.slice(0, COLLAPSED) : items;
  list.replaceChildren(
    ...shown.map((a) => {
      const li = document.createElement('li');
      const b = document.createElement('button');
      const initial = document.createElement('span');
      initial.className = 'initial';
      initial.textContent = upper(a.name.trim().charAt(0));
      const name = document.createElement('span');
      name.className = 'name';
      name.textContent = a.name;
      b.append(initial, name);
      b.insertAdjacentHTML('beforeend', '<svg class="ic"><use href="#i-chevron"/></svg>');
      pressFeedback(b);
      b.onclick = () => launchApp(a.id, b);
      li.append(b);
      return li;
    }),
  );
  if (collapsed || (showAllApps && !filter && items.length > COLLAPSED + 2)) {
    const li = document.createElement('li');
    const b = document.createElement('button');
    b.className = 'more-row';
    b.textContent = collapsed ? tl('Tümünü göster ({n})', { n: items.length }) : tl('Daha az göster');
    b.onclick = () => {
      showAllApps = !showAllApps;
      renderApps();
    };
    li.append(b);
    list.append(li);
  }
}

function launchApp(id, el) {
  if (!sendJson({ type: 'launch', id, fullscreen: settings.autoFullscreen })) {
    toast(tl('Bilgisayara bağlı değil'));
    return;
  }
  recordRecent(id);
  el?.classList.add('busy');
  setTimeout(() => el?.classList.remove('busy'), 1500);
}

function onApps(m) {
  appsBusy = false;
  clearTimeout(appsBusyTimer);
  syncBusy();
  appsData = m;
  appsLoadedAt = performance.now();
  renderApps();
}

function onLaunched(m) {
  toast(m.search ? tl('{name} içinde aranıyor…', { name: m.name }) : tl('{name} açılıyor…', { name: m.name }));
  // Video/müzik servisi açıldıysa kumandaya geç: bir sonraki iş muhtemelen oynat/duraklat
  if (m.media) setTimeout(() => showView('remote'), 700);
}

searchInput.addEventListener('input', renderApps);
$('searchForm').addEventListener('submit', (e) => {
  e.preventDefault();
  const q = searchInput.value.trim();
  if (!q) return;
  if (!sendJson({ type: 'search', id: searchTarget, q })) {
    toast(tl('Bilgisayara bağlı değil'));
    return;
  }
  haptic();
  searchInput.blur(); // klavyeyi kapat
});

// ============================================================ ayarlar
const SETTINGS_DEFAULT = { sens: 1, scroll: 1, natural: true, lefty: false, bed: false, bedAuto: false, haptics: true, hud: false, screenMin: '10', autoFullscreen: true, autoRemote: false, kbdMode: 'keys', airSens: 1, kbLayout: lang === 'tr' ? 'tr' : 'us', nudge: false, theme: 'dark', accent: 'blue' };
const settings = { ...SETTINGS_DEFAULT };
try {
  Object.assign(settings, JSON.parse(localStorage.getItem('settings') || '{}'));
  if (localStorage.getItem('bed') === '1') settings.bed = true; // eski sürümden
} catch {
  // depolama kapalı: varsayılanlar
}
function saveSettings() {
  try {
    localStorage.setItem('settings', JSON.stringify(settings));
  } catch {
    // depolama kapalı
  }
}
const isNight = () => {
  const h = new Date().getHours();
  return h >= 22 || h < 7;
};
// Tema ve vurgu rengi: yatak modu her zaman koyu; "otomatik" telefonun sistem temasını izler
const lightQuery = matchMedia('(prefers-color-scheme: light)');
function applyTheme(bed) {
  let mode = settings.theme === 'auto' ? (lightQuery.matches ? 'light' : 'dark') : settings.theme === 'light' ? 'light' : 'dark';
  if (bed) mode = 'dark';
  const root = document.documentElement;
  root.dataset.mode = mode;
  root.dataset.accent = settings.accent || 'blue';
  document.querySelector('meta[name="theme-color"]')?.setAttribute('content', mode === 'light' ? '#f2f2f7' : '#000000');
  for (const b of document.querySelectorAll('#setTheme [data-v]')) b.classList.toggle('on', b.dataset.v === settings.theme);
  for (const b of document.querySelectorAll('#setAccent [data-v]')) {
    b.classList.toggle('on', b.dataset.v === (settings.accent || 'blue'));
    b.setAttribute('aria-pressed', String(b.dataset.v === (settings.accent || 'blue')));
  }
}
lightQuery.addEventListener?.('change', () => applySettings());
function applySettings() {
  const bed = settings.bed || (settings.bedAuto && isNight());
  document.body.classList.toggle('bed', bed);
  applyTheme(bed);
  document.body.classList.toggle('lefty', settings.lefty);
  $('hud').classList.toggle('off', !settings.hud);
  $('lat').hidden = !settings.hud; // H7: üst çubukta yer kaplamasın
  for (const b of document.querySelectorAll('#setScreen [data-v]')) b.classList.toggle('on', b.dataset.v === settings.screenMin);
  $('setFullscreen').checked = settings.autoFullscreen;
  $('setAutoRemote').checked = settings.autoRemote;
  $('setSens').value = settings.sens;
  $('setSensOut').textContent = `${Number(settings.sens).toFixed(1)}×`;
  for (const b of document.querySelectorAll('#setKbLayout [data-v]')) b.classList.toggle('on', b.dataset.v === settings.kbLayout);
  if (kbEl.dataset.kb !== settings.kbLayout) renderKb(); // düzen değişti: klavye yeniden çizilir
  for (const b of document.querySelectorAll('#setLang [data-v]')) b.classList.toggle('on', b.dataset.v === langPref());
  $('nudge').hidden = !settings.nudge;
  $('setNudge').checked = settings.nudge;
  $('setAirSens').value = settings.airSens;
  $('setAirSensOut').textContent = `${Number(settings.airSens).toFixed(1)}×`;
  air.setSens(Number(settings.airSens));
  $('setScroll').value = settings.scroll;
  $('setScrollOut').textContent = `${Number(settings.scroll).toFixed(1)}×`;
  $('setNatural').checked = settings.natural;
  $('setLefty').checked = settings.lefty;
  $('setBed').checked = settings.bed;
  $('setBedAuto').checked = settings.bedAuto;
  $('setHaptics').checked = settings.haptics;
  $('setHud').checked = settings.hud;
  applyKbdMode(false);
  renderHud();
}
const bindSetting = (id, key, parse) =>
  $(id).addEventListener('input', () => {
    settings[key] = parse($(id));
    saveSettings();
    applySettings();
  });
bindSetting('setSens', 'sens', (el) => Number(el.value));
bindSetting('setScroll', 'scroll', (el) => Number(el.value));
bindSetting('setAirSens', 'airSens', (el) => Number(el.value));
for (const b of document.querySelectorAll('#setKbLayout [data-v]')) {
  b.onclick = () => {
    settings.kbLayout = b.dataset.v;
    saveSettings();
    applySettings();
    haptic();
  };
}
// Dil değişince sayfa yeniden yüklenir (tüm metinler yeni dilde kurulur)
for (const b of document.querySelectorAll('#setLang [data-v]')) {
  b.onclick = () => {
    haptic();
    setLangPref(b.dataset.v);
    location.reload();
  };
}
for (const b of document.querySelectorAll('#setTheme [data-v]')) {
  b.onclick = () => {
    settings.theme = b.dataset.v;
    saveSettings();
    applySettings();
    haptic();
  };
}
for (const b of document.querySelectorAll('#setAccent [data-v]')) {
  b.onclick = () => {
    settings.accent = b.dataset.v;
    saveSettings();
    applySettings();
    haptic();
  };
}
for (const b of document.querySelectorAll('#setScreen [data-v]')) {
  b.onclick = () => {
    settings.screenMin = b.dataset.v;
    saveSettings();
    applySettings();
    haptic();
  };
}
for (const [id, key] of [['setNatural', 'natural'], ['setLefty', 'lefty'], ['setBed', 'bed'], ['setBedAuto', 'bedAuto'], ['setHaptics', 'haptics'], ['setHud', 'hud'], ['setFullscreen', 'autoFullscreen'], ['setAutoRemote', 'autoRemote'], ['setNudge', 'nudge']]) {
  $(id).addEventListener('change', () => {
    settings[key] = $(id).checked;
    saveSettings();
    applySettings();
    haptic();
  });
}
setInterval(applySettings, 60_000); // "akşamları otomatik" için
$('aboutLine').textContent = tl('CouchRemote · ayarlar bu telefonda saklanır');

// ============================================================ alttan açılan paneller
let sheetCloseTimer = 0;
function resetSheet(s) {
  s.classList.remove('closing', 'dragging');
  s.style.transform = '';
  s.style.transition = '';
}
function openSheet(id) {
  clearTimeout(sheetCloseTimer);
  for (const s of document.querySelectorAll('.sheet')) {
    resetSheet(s);
    if (s.id !== id) s.hidden = true;
  }
  const bd = $('backdrop');
  bd.classList.remove('closing');
  bd.style.opacity = '';
  bd.style.transition = '';
  bd.hidden = false;
  const sh = $(id);
  sh.hidden = false;
  // Kaydırılabilir paneller dikey kaydırmayı kendisi yönetir; sığanlar her yerinden aşağı çekilip kapatılabilir
  sh.style.touchAction = sh.scrollHeight > sh.clientHeight + 1 ? 'pan-y' : 'none';
  haptic();
}
// fromDrag: parmakla çekilip bırakıldı (panel olduğu yerden kayarak çıkar)
function closeSheets(fromDrag = false) {
  const open = [...document.querySelectorAll('.sheet')].filter((s) => !s.hidden);
  const bd = $('backdrop');
  if (!open.length) {
    bd.hidden = true;
    return;
  }
  clearTimeout(sheetCloseTimer);
  for (const s of open) {
    if (fromDrag) {
      s.classList.remove('dragging');
      s.style.transition = 'transform 0.3s var(--ease-sheet)';
      s.style.transform = 'translateY(105%)';
    } else {
      s.classList.add('closing');
    }
  }
  if (fromDrag) {
    bd.style.transition = 'opacity 0.3s var(--ease)';
    bd.style.opacity = '0';
  } else {
    bd.classList.add('closing');
  }
  sheetCloseTimer = setTimeout(() => {
    for (const s of open) {
      s.hidden = true;
      resetSheet(s);
    }
    bd.hidden = true;
    bd.classList.remove('closing');
    bd.style.opacity = '';
    bd.style.transition = '';
  }, 300);
}
$('backdrop').onclick = () => closeSheets();

// Panel parmakla aşağı çekilince kapanır (hızlı fırlatma da); yetmezse yay gibi yerine döner.
// Kaydırılabilir panellerde sadece üst başlık (tutamaç + başlık) çekilir, içerik kaydırmayı bozmaz.
for (const sheet of document.querySelectorAll('.sheet')) {
  let drag = null;
  sheet.addEventListener('pointerdown', (e) => {
    const scrollable = sheet.scrollHeight > sheet.clientHeight + 1;
    const onHeader = !!e.target.closest('.grabber, h3');
    if (scrollable && !onHeader) return;
    if (e.target.closest('input, button, textarea, a, select')) return;
    drag = { y: e.clientY, lastY: e.clientY, lastT: e.timeStamp, v: 0, id: e.pointerId, moved: false };
  });
  sheet.addEventListener('pointermove', (e) => {
    if (!drag || e.pointerId !== drag.id) return;
    const dy = e.clientY - drag.y;
    if (!drag.moved) {
      if (Math.abs(dy) < 6) return;
      if (dy < 0) {
        drag = null; // yukarı çekme: panel hareket etmez
        return;
      }
      drag.moved = true;
      sheet.setPointerCapture(e.pointerId);
      sheet.classList.add('dragging');
    }
    const y = Math.max(0, dy);
    // Panel çekildikçe parmağın altında kalır; üstte hafif direnç yok, altta serbest
    sheet.style.transform = `translateY(${y}px)`;
    $('backdrop').style.opacity = String(Math.max(0, 1 - y / (sheet.clientHeight * 0.9)));
    const dt = e.timeStamp - drag.lastT;
    if (dt > 0) drag.v = (e.clientY - drag.lastY) / dt;
    drag.lastY = e.clientY;
    drag.lastT = e.timeStamp;
  });
  const end = (e) => {
    if (!drag || e.pointerId !== drag.id) return;
    const d = drag;
    drag = null;
    if (!d.moved) return;
    const dy = e.clientY - d.y;
    if (dy > 110 || d.v > 0.6) {
      closeSheets(true);
    } else {
      sheet.classList.remove('dragging');
      sheet.style.transition = 'transform 0.5s var(--spring)';
      sheet.style.transform = '';
      $('backdrop').style.transition = 'opacity 0.3s var(--ease)';
      $('backdrop').style.opacity = '';
      setTimeout(() => {
        sheet.style.transition = '';
        $('backdrop').style.transition = '';
      }, 520);
    }
  };
  sheet.addEventListener('pointerup', end);
  sheet.addEventListener('pointercancel', end);
}
$('padHelp').onclick = () => openSheet('padHelpSheet');
$('openPower').onclick = () => {
  closeSheets();
  openSheet('powerSheet');
};

// ---- Siri ve Kestirmeler kurulum ekranı: jeton ve adresler tek dokunuşla kopyalanır
const SIRI_EXAMPLES = { volume: '?v=30', launch: '?v=netflix', search: '?app=youtube&v=lofi', timer: '?v=30' };
let siriInfo = null;
async function copyText(text, okMsg) {
  haptic();
  try {
    await navigator.clipboard.writeText(text);
    toast(okMsg);
  } catch {
    toast(tl('Kopyalanamadı'));
  }
}
function renderSiri() {
  const box = $('siriActions');
  box.textContent = '';
  for (const a of siriInfo.actions) {
    const url = `${siriInfo.base}/${a.id}${SIRI_EXAMPLES[a.id] ?? ''}`;
    const row = document.createElement('button');
    row.className = 'set-row link';
    const label = document.createElement('span');
    label.textContent = a.about;
    const small = document.createElement('small');
    small.textContent = SIRI_EXAMPLES[a.id] ? `${a.id}${SIRI_EXAMPLES[a.id]}` : a.id;
    label.append(small);
    row.append(label);
    row.insertAdjacentHTML('beforeend', '<svg class="ic sm"><use href="#i-clipboard"/></svg>');
    row.onclick = () => copyText(url, tl('Adres kopyalandı'));
    box.append(row);
  }
}
$('openSiri').onclick = () => {
  closeSheets();
  openSheet('siriSheet');
  if (!siriInfo && !sendJson({ type: 'shortcut-info' })) toast(tl('Bilgisayara bağlı değil'));
  else if (siriInfo) renderSiri();
};
$('siriCopyToken').onclick = () => (siriInfo ? copyText(`Bearer ${siriInfo.token}`, tl('Jeton başlığı kopyalandı')) : toast(tl('Bilgi yükleniyor…')));
$('settingsBtn').onclick = () => openSheet('settingsSheet');
$('timerPill').onclick = () => openSheet('powerSheet');

function confirmDialog(text) {
  return new Promise((resolve) => {
    $('confirmText').textContent = text;
    $('confirm').hidden = false;
    const done = (v) => {
      $('confirm').hidden = true;
      resolve(v);
    };
    $('confirmYes').onclick = () => done(true);
    $('confirmNo').onclick = () => done(false);
  });
}

// ============================================================ güç menüsü
const POWER_TOAST = { screenoff: tl('Ekran kapatıldı'), lock: tl('Bilgisayar kilitlendi'), sleep: tl('Bilgisayar uyutuluyor'), restart: tl('Yeniden başlatılıyor'), shutdown: tl('Bilgisayar kapatılıyor') };
for (const b of document.querySelectorAll('[data-power]')) {
  pressFeedback(b);
  b.onclick = async () => {
    const action = b.dataset.power;
    let confirm = false;
    if (b.dataset.confirm) {
      if (!(await confirmDialog(b.dataset.confirm))) return;
      confirm = true;
    }
    if (!sendJson({ type: 'power', action, confirm })) {
      toast(tl('Bilgisayara bağlı değil'));
      return;
    }
    toast(POWER_TOAST[action]);
    closeSheets();
  };
}

// ============================================================ uyku zamanlayıcısı
let timerMin = 30;
let timerAction = 'stop';
let timerState = { active: false };
let timerAt = 0;
for (const b of document.querySelectorAll('#timerMinutes [data-min]')) {
  b.onclick = () => {
    timerMin = Number(b.dataset.min);
    for (const x of document.querySelectorAll('#timerMinutes [data-min]')) x.classList.toggle('on', x === b);
    haptic();
  };
}
for (const b of document.querySelectorAll('#timerAction [data-action]')) {
  b.onclick = () => {
    timerAction = b.dataset.action;
    for (const x of document.querySelectorAll('#timerAction [data-action]')) x.classList.toggle('on', x === b);
    haptic();
  };
}
$('timerStart').onclick = () => {
  if (!sendJson({ type: 'timer', minutes: timerMin, action: timerAction })) return toast(tl('Bilgisayara bağlı değil'));
  haptic();
  closeSheets();
  toast(tl('Zamanlayıcı kuruldu: {min} dk', { min: timerMin }));
};
$('timerCancel').onclick = () => {
  if (sendJson({ type: 'timer', cancel: true })) {
    haptic();
    toast(tl('Zamanlayıcı iptal edildi'));
  }
};
const TIMER_WHAT = { stop: tl('sonra medya durur, ekran kapanır'), sleep: tl('sonra bilgisayar uyur'), shutdown: tl('sonra bilgisayar kapanır') };
function fmtTimer(sec) {
  sec = Math.ceil(sec);
  const h = Math.floor(sec / 3600);
  const m = Math.floor((sec % 3600) / 60);
  const s2 = String(sec % 60).padStart(2, '0');
  return h ? `${h}:${String(m).padStart(2, '0')}:${s2}` : `${m}:${s2}`;
}
function onTimer(m) {
  timerState = m;
  timerAt = performance.now();
  if (m.fired) toast(tl('Uyku zamanlayıcısı doldu'));
  renderTimer();
}
function renderTimer() {
  const on = !!timerState.active;
  $('timerActive').hidden = !on;
  $('timerSetup').hidden = on;
  $('timerPill').hidden = !on;
  if (!on) return;
  const left = Math.max(0, (timerState.remaining ?? 0) - (performance.now() - timerAt) / 1000);
  const txt = fmtTimer(left);
  $('timerLeft').textContent = txt;
  $('timerPill').querySelector('span').textContent = txt;
  $('timerWhat').textContent = TIMER_WHAT[timerState.action] ?? '';
}
setInterval(renderTimer, 1000);

// ============================================================ ses kaydırıcısı
const volSlider = $('volSlider');
let volDragging = false;
let volSendTimer = 0;
let lastVolume = null;
function paintVolume(pct) {
  volSlider.style.setProperty('--p', `${pct}%`);
  $('volPct').textContent = `${Math.round(pct)}%`;
}
function onVolume(m) {
  lastVolume = m;
  $('muteBtn').classList.toggle('muted', !!m.muted);
  if (!volDragging) {
    volSlider.value = String(Math.round(m.level * 100));
    paintVolume(volSlider.value);
  }
}
volSlider.addEventListener('pointerdown', () => (volDragging = true));
volSlider.addEventListener('pointercancel', () => (volDragging = false));
volSlider.addEventListener('input', () => {
  paintVolume(volSlider.value);
  // Sürüklerken saniyede ~16 güncelleme yeterli
  if (!volSendTimer) {
    volSendTimer = setTimeout(() => {
      volSendTimer = 0;
      sendJson({ type: 'volume', level: Number(volSlider.value) / 100 });
    }, 60);
  }
});
volSlider.addEventListener('change', () => {
  volDragging = false;
  sendJson({ type: 'volume', level: Number(volSlider.value) / 100 });
  haptic();
});
pressFeedback($('muteBtn'));
$('muteBtn').onclick = () => sendJson({ type: 'volume', mute: !lastVolume?.muted });

// ---- ses çıkış cihazı: PC'deki varsayılan hoparlörü telefondan değiştir
let outDevices = [];
// "Kulaklıklar (HyperX Cloud III)" → ana ad + parantez içi ayrıntı
const splitName = (n) => {
  const m = /^(.*?)\s*\((.*)\)\s*$/.exec(n);
  return m ? { main: m[1] || n, sub: m[2] } : { main: n, sub: '' };
};
function onAudioDevices(m) {
  outDevices = m.devices ?? [];
  const cur = outDevices.find((d) => d.default);
  $('outBtn').hidden = outDevices.length < 2; // tek cihaz varsa seçecek bir şey yok
  $('outName').textContent = cur ? splitName(cur.name).main : '–';
  renderOutList();
}
function renderOutList() {
  const box = $('outList');
  box.textContent = '';
  for (const d of outDevices) {
    const { main, sub } = splitName(d.name);
    const row = document.createElement('button');
    row.className = 'set-row link' + (d.default ? ' on' : '');
    const label = document.createElement('span');
    label.textContent = main;
    if (sub) {
      const small = document.createElement('small');
      small.textContent = sub;
      label.append(small);
    }
    row.append(label);
    if (d.default) row.insertAdjacentHTML('beforeend', '<svg class="ic sm check"><use href="#i-check"/></svg>');
    row.onclick = () => {
      if (d.default) return closeSheets();
      haptic();
      sendJson({ type: 'audio-device', id: d.id });
      closeSheets();
      toast(tl('Ses çıkışı: {name}', { name: main }));
    };
    box.append(row);
  }
}
// ---- ekran görüntüsü
let shotMonitor = null; // null = tüm ekranlar
let shotBlob = null;
let shotUrl = '';
function requestShot(monitor = shotMonitor) {
  shotMonitor = monitor;
  if (!sendJson({ type: 'screenshot', ...(monitor === null ? {} : { monitor }) })) {
    $('shotLoad').hidden = false;
    $('shotLoad').textContent = tl('Bilgisayara bağlı değil');
    return;
  }
  $('shotLoad').hidden = false;
  $('shotLoad').textContent = tl('Ekran alınıyor…');
  $('shotRefresh').disabled = true;
}
function renderShotMonitors(count) {
  const box = $('shotMons');
  box.hidden = count < 2;
  if (count < 2) return;
  const items = [[tl('Tümü'), null], ...Array.from({ length: count }, (_, i) => [String(i + 1), i])];
  box.replaceChildren(
    ...items.map(([label, idx]) => {
      const b = document.createElement('button');
      b.textContent = label;
      b.classList.toggle('on', idx === shotMonitor);
      b.onclick = () => {
        haptic();
        requestShot(idx);
      };
      return b;
    }),
  );
}
function onShot(m) {
  $('shotRefresh').disabled = false;
  renderShotMonitors(m.monitors ?? 1);
  if (m.error || !m.jpeg) {
    $('shotLoad').hidden = false;
    $('shotLoad').textContent = m.error || tl('Ekran alınamadı');
    return;
  }
  const bin = atob(m.jpeg);
  const bytes = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i);
  shotBlob = new Blob([bytes], { type: 'image/jpeg' });
  if (shotUrl) URL.revokeObjectURL(shotUrl);
  shotUrl = URL.createObjectURL(shotBlob);
  const img = $('shotImg');
  img.src = shotUrl;
  img.hidden = false;
  $('shotLoad').hidden = true;
  $('shotShare').disabled = false;
}
$('shotOpen').onclick = () => {
  haptic();
  $('shotView').hidden = false;
  $('shotImg').hidden = true;
  $('shotShare').disabled = true;
  $('shotBox').classList.remove('zoom');
  requestShot(null);
};
$('shotClose').onclick = () => {
  $('shotView').hidden = true;
  $('shotImg').removeAttribute('src');
  if (shotUrl) URL.revokeObjectURL(shotUrl);
  shotUrl = '';
  shotBlob = null;
};
$('shotRefresh').onclick = () => {
  haptic();
  requestShot();
};
$('shotImg').onclick = () => $('shotBox').classList.toggle('zoom');
$('shotShare').onclick = async () => {
  if (!shotBlob) return;
  haptic();
  const file = new File([shotBlob], `ekran-${new Date().toISOString().slice(0, 19).replace(/[:T]/g, '-')}.jpg`, { type: 'image/jpeg' });
  try {
    if (navigator.canShare?.({ files: [file] })) await navigator.share({ files: [file] });
    else toast(tl('Görüntüye uzun basıp "Fotoğraflara Kaydet"i seç'));
  } catch {
    // paylaşım iptal edildi
  }
};

// ---- uygulama başına ses (karıştırıcı)
let mixApps = [];
const mixDragging = new Set(); // parmak altındaki kaydırıcılar: PC'den gelen değer onları zıplatmasın
const mixRows = new Map(); // id → { row, range, pct, mute }
const mixLastSend = {};

function sendMixer(id, patch) {
  sendJson({ type: 'mixer', id, ...patch });
}
function mixKey(apps) {
  return apps.map((a) => a.id).join('|');
}
function buildMixRow(a) {
  const row = document.createElement('div');
  row.className = 'mix-row';
  const top = document.createElement('div');
  top.className = 'mix-top';
  const name = document.createElement('span');
  name.className = 'mix-name';
  name.textContent = tl(a.name);
  const pct = document.createElement('span');
  pct.className = 'mix-pct';
  const mute = document.createElement('button');
  mute.className = 'mix-mute';
  mute.setAttribute('aria-label', tl('{name} sesini aç/kapat', { name: tl(a.name) }));
  mute.innerHTML = '<svg class="ic sm on-icon"><use href="#i-vol-down"/></svg><svg class="ic sm off-icon"><use href="#i-mute"/></svg>';
  top.append(mute, name, pct);
  const range = document.createElement('input');
  range.type = 'range';
  range.min = 0;
  range.max = 100;
  range.step = 1;
  range.setAttribute('aria-label', tl('{name} ses seviyesi', { name: tl(a.name) }));
  row.append(top, range);

  range.addEventListener('pointerdown', () => mixDragging.add(a.id));
  const stop = () => mixDragging.delete(a.id);
  range.addEventListener('pointerup', stop);
  range.addEventListener('pointercancel', stop);
  range.addEventListener('input', () => {
    pct.textContent = `${range.value}%`;
    const t = performance.now();
    if (t - (mixLastSend[a.id] || 0) > 70) {
      mixLastSend[a.id] = t;
      sendMixer(a.id, { level: Number(range.value) / 100 });
    }
  });
  range.addEventListener('change', () => {
    mixDragging.delete(a.id);
    sendMixer(a.id, { level: Number(range.value) / 100 });
    haptic();
  });
  mute.addEventListener('click', () => {
    haptic();
    const cur = mixApps.find((x) => x.id === a.id);
    sendMixer(a.id, { mute: !cur?.muted });
  });
  const refs = { row, range, pct, mute, name };
  mixRows.set(a.id, refs);
  return refs;
}
function updateMixRow(refs, a) {
  refs.row.classList.toggle('muted', a.muted);
  refs.row.classList.toggle('idle', !a.active);
  refs.mute.classList.toggle('off', a.muted);
  if (!mixDragging.has(a.id)) {
    refs.range.value = Math.round(a.level * 100);
    refs.pct.textContent = a.muted ? tl('Sessiz') : `${Math.round(a.level * 100)}%`;
  }
}
function renderMixer(rebuild) {
  const list = $('mixList');
  $('mixEmpty').hidden = mixApps.length > 0;
  if (rebuild) {
    mixRows.clear();
    list.replaceChildren(
      ...mixApps.map((a) => {
        const refs = buildMixRow(a);
        updateMixRow(refs, a);
        return refs.row;
      }),
    );
    return;
  }
  for (const a of mixApps) {
    const refs = mixRows.get(a.id);
    if (refs) updateMixRow(refs, a);
  }
}
function onMixer(m) {
  const next = m.apps ?? [];
  const rebuild = mixKey(next) !== mixKey(mixApps);
  mixApps = next;
  $('mixBtn').hidden = mixApps.length === 0;
  const names = mixApps.filter((a) => a.active).map((a) => tl(a.name));
  $('mixSummary').textContent = names.length ? names.slice(0, 3).join(', ') : tl('{n} uygulama', { n: mixApps.length });
  if (!$('mixerSheet').hidden) renderMixer(rebuild);
}
$('mixBtn').onclick = () => {
  renderMixer(true);
  openSheet('mixerSheet');
};

$('outBtn').onclick = () => {
  renderOutList();
  openSheet('audioSheet');
};

// ============================================================ öndeki uygulamaya özel düğmeler
// Tuşlar karakter olarak gönderilir; PC öndeki pencerenin klavye düzenine göre doğru tuşa çevirir.
// Her profil: buton listesi + (varsa) o uygulamanın kendi ±10 sn sarma tuşları.
// Tuşlar karakter olarak gönderilir; PC öndeki pencerenin klavye düzenine göre doğru tuşa çevirir.
// "Tam ekran" ve ses düğmeleri Kumanda ekranında hep var; burada sadece uygulamaya özel olanlar.
const ARROWS = { back: { key: 'left' }, fwd: { key: 'right' } }; // Netflix, Prime, Disney+, tabii, Exxen: ok tuşları 10 sn
const PROFILES = {
  netflix: { name: 'Netflix', seek: ARROWS, buttons: [{ label: tl('İntroyu atla'), char: 's', accent: true }, { label: tl('Sonraki bölüm'), char: 'N' }, { label: tl('Sessiz'), char: 'm' }] },
  youtube: { name: 'YouTube', seek: { back: { char: 'j' }, fwd: { char: 'l' } }, buttons: [{ label: tl('Altyazı'), char: 'c' }, { label: tl('Yavaşlat'), char: '<' }, { label: tl('Hızlandır'), char: '>' }, { label: tl('Sinema modu'), char: 't' }, { label: tl('Mini oynatıcı'), char: 'i' }, { label: tl('Önceki video'), char: 'P' }, { label: tl('Sonraki video'), char: 'N', accent: true }] },
  ytmusic: { name: 'YouTube Music', buttons: [{ label: tl('Karışık'), char: 's' }, { label: tl('Tekrar'), char: 'r' }, { label: tl('Sessiz'), char: 'm' }] },
  primevideo: { name: 'Prime Video', seek: ARROWS, buttons: [{ label: tl('Altyazı'), char: 'c' }, { label: tl('Sessiz'), char: 'm' }] },
  disneyplus: { name: 'Disney+', seek: ARROWS, buttons: [{ label: tl('Sessiz'), char: 'm' }] },
  tabii: { name: 'tabii', seek: ARROWS, buttons: [{ label: tl('Sessiz'), char: 'm' }] },
  exxen: { name: 'Exxen', seek: ARROWS, buttons: [{ label: tl('Sessiz'), char: 'm' }] },
  twitch: { name: 'Twitch', buttons: [{ label: tl('Sinema modu'), char: 't', mods: ['alt'] }, { label: tl('Sessiz'), char: 'm' }] },
  spotify: { name: 'Spotify', buttons: [{ label: tl('Karışık'), char: 's', mods: ['ctrl'] }, { label: tl('Tekrar'), char: 'r', mods: ['ctrl'] }] },
  vlc: { name: 'VLC', seek: { back: { key: 'left', mods: ['alt'] }, fwd: { key: 'right', mods: ['alt'] } }, buttons: [{ label: tl('Altyazı'), char: 'v' }, { label: tl('Ses kanalı'), char: 'b' }, { label: tl('Yavaşlat'), char: '[' }, { label: tl('Hızlandır'), char: ']' }, { label: tl('Normal hız'), char: '=' }, { label: tl('Önceki'), char: 'p' }, { label: tl('Sonraki'), char: 'n' }, { label: tl('En-boy oranı'), char: 'a' }] },
};
let currentProfile = null;
let currentFullscreen = false;
let contextSeen = false;

function onContext(m) {
  const profile = m.profile ?? 'none';
  const fullscreen = !!m.fullscreen;
  const first = !contextSeen;
  contextSeen = true;
  if (fullscreen !== currentFullscreen) {
    currentFullscreen = fullscreen;
    renderFullscreenPill();
  }
  if (profile === currentProfile) return;
  const was = currentProfile;
  currentProfile = profile;
  const p = PROFILES[profile];
  $('ctx').hidden = !p;
  // Bir servis öne geldiğinde (bağlanırken gelen ilk durum hariç) kumandaya geç
  if (p && !first && was === 'none' && settings.autoRemote && views.kbd.hidden) showView('remote'); // yazarken sekme değişmesin
  if (!p) return;
  $('ctxTitle').textContent = tl('{name} için', { name: p.name });
  $('ctxBtns').replaceChildren(
    ...p.buttons.map((b) => {
      const el = document.createElement('button');
      el.className = 'ctx-btn' + (b.accent ? ' accent' : '');
      el.textContent = b.label;
      pressFeedback(el);
      // Sadece o uygulamanın penceresine gider; pencere yoksa PC hata döner, tuş başka yere yazılmaz
      el.onclick = () => sendJson({ type: 'combo', char: b.char, mods: b.mods || [], profile: currentProfile });
      return el;
    }),
  );
}

function renderFullscreenPill() {
  const label = $('fsLabel');
  if (label) label.textContent = currentFullscreen ? tl('Küçült') : tl('Tam ekran');
}

// ±10 sn: uygulamanın kendi tuşu varsa (ör. YouTube j / l) ona gider, yoksa genel medya kontrolüne
function seekBy(sec) {
  const k = PROFILES[currentProfile]?.seek?.[sec < 0 ? 'back' : 'fwd'];
  if (!k) return sendJson({ type: 'media', action: 'seekby', value: sec });
  return sendJson(
    k.key
      ? { type: 'key', key: k.key, mods: k.mods || [], profile: currentProfile }
      : { type: 'combo', char: k.char, mods: k.mods || [], profile: currentProfile },
  );
}

// ============================================================ aktar: pano
let clipWaiter = null;
async function clipFromPc() {
  if (!connected) return toast(tl('Bilgisayara bağlı değil'));
  haptic();
  const textPromise = new Promise((resolve, reject) => {
    clipWaiter = { resolve, reject };
    setTimeout(() => reject(new Error('zaman aşımı')), 5000);
  });
  sendJson({ type: 'clipboard-get' });
  try {
    // Safari: panoya yazma dokunma anında başlatılmalı; metin sonradan gelen bir söz (promise) olarak verilir
    if (window.ClipboardItem && navigator.clipboard?.write) {
      await navigator.clipboard.write([new ClipboardItem({ 'text/plain': textPromise.then((t) => new Blob([t], { type: 'text/plain' })) })]);
    } else {
      await navigator.clipboard.writeText(await textPromise);
    }
    toast(tl('PC panosu telefona kopyalandı'));
  } catch {
    try {
      $('clipText').value = await textPromise;
      toast(tl('Metin kutuya kondu, oradan kopyalayabilirsin'));
    } catch (e) {
      toast(e.message === 'zaman aşımı' ? tl('Pano alınamadı') : e.message);
    }
  }
}
function onClipboard(m) {
  if (!clipWaiter) return;
  if (m.error) clipWaiter.reject(new Error(m.error));
  else clipWaiter.resolve(m.text || '');
  clipWaiter = null;
}
async function clipFromPhone() {
  haptic();
  try {
    const t = await navigator.clipboard.readText();
    if (!t) return toast(tl('Telefonun panosu boş'));
    if (!sendJson({ type: 'clipboard-set', text: t })) toast(tl('Bilgisayara bağlı değil'));
  } catch {
    toast(tl('Pano okunamadı; metni kutuya yapıştır'));
  }
}
function sendClipText(paste) {
  const t = $('clipText').value;
  if (!t) return toast(tl('Önce bir metin yaz veya yapıştır'));
  if (sendJson({ type: 'clipboard-set', text: t, paste })) haptic();
  else toast(tl('Bilgisayara bağlı değil'));
}
// Yön seçici: Telefon → PC (yaz/yapıştır, gönder) veya PC → Telefon (PC panosunu al)
let clipDir = 'toPc';
function renderClipDir() {
  for (const b of document.querySelectorAll('#clipDir [data-v]')) b.classList.toggle('on', b.dataset.v === clipDir);
  $('clipMain').textContent = clipDir === 'toPc' ? tl('PC panosuna gönder') : tl("PC'nin panosunu al");
  $('clipText').hidden = clipDir !== 'toPc';
  $('clipTextPaste').hidden = clipDir !== 'toPc';
}
for (const b of document.querySelectorAll('#clipDir [data-v]')) b.onclick = () => { clipDir = b.dataset.v; haptic(); renderClipDir(); };
$('clipMain').onclick = () => {
  if (clipDir === 'toPhone') return clipFromPc();
  if ($('clipText').value) return sendClipText(false);
  return clipFromPhone(); // kutu boşsa telefonun panosu gönderilir
};
renderClipDir();
$('clipTextPaste').onclick = () => sendClipText(true);
function onClipboardSet(m) {
  toast(m.ok ? (m.paste ? tl('Gönderildi ve yapıştırıldı') : tl('PC panosuna kopyalandı')) : tl('Pano ayarlanamadı'));
}

// ============================================================ aktar: dosya gönderme
let uploadToken = null;
let tokenAt = 0;
const tokenWaiters = [];
function getUploadToken() {
  if (uploadToken && performance.now() - tokenAt < 8 * 60_000) return Promise.resolve(uploadToken);
  return new Promise((resolve, reject) => {
    tokenWaiters.push({ resolve, reject });
    if (tokenWaiters.length === 1 && !sendJson({ type: 'upload-token' })) {
      tokenWaiters.splice(0).forEach((w) => w.reject(new Error('bağlı değil')));
    }
    setTimeout(() => {
      const i = tokenWaiters.findIndex((w) => w.resolve === resolve);
      if (i >= 0) tokenWaiters.splice(i, 1)[0].reject(new Error('zaman aşımı'));
    }, 5000);
  });
}
function onUploadToken(m) {
  uploadToken = m.token;
  tokenAt = performance.now();
  tokenWaiters.splice(0).forEach((w) => w.resolve(m.token));
}
const fmtBytes = (n) => (n < 1024 * 1024 ? `${Math.max(1, Math.round(n / 1024))} KB` : `${(n / 1024 / 1024).toFixed(1)} MB`);

function uploadOne(file, token) {
  return new Promise((resolve) => {
    const li = document.createElement('li');
    const name = document.createElement('div');
    name.className = 'up-name';
    name.textContent = file.name;
    const bar = document.createElement('div');
    bar.className = 'up-bar';
    const fill = document.createElement('i');
    bar.append(fill);
    const state = document.createElement('div');
    state.className = 'up-state';
    state.textContent = tl('Gönderiliyor…');
    li.append(name, bar, state);
    const list = $('uploads');
    list.hidden = false;
    list.prepend(li);
    while (list.children.length > 10) list.lastChild.remove();

    const xhr = new XMLHttpRequest();
    xhr.open('POST', '/api/upload');
    xhr.setRequestHeader('X-Upload-Token', token);
    xhr.setRequestHeader('X-File-Name', encodeURIComponent(file.name));
    xhr.upload.onprogress = (e) => {
      if (!e.lengthComputable) return;
      fill.style.width = `${(e.loaded / e.total) * 100}%`;
      state.textContent = `${fmtBytes(e.loaded)} / ${fmtBytes(e.total)}`;
    };
    xhr.onload = () => {
      if (xhr.status === 200) {
        const r = JSON.parse(xhr.responseText);
        li.classList.add('done');
        fill.style.width = '100%';
        state.textContent = tl('Kaydedildi: {name}', { name: r.name });
      } else {
        li.classList.add('fail');
        if (xhr.status === 401) uploadToken = null;
        state.textContent = xhr.status === 413 ? tl('Dosya çok büyük (en fazla 2 GB)') : tl('Gönderilemedi');
      }
      resolve();
    };
    xhr.onerror = () => {
      li.classList.add('fail');
      state.textContent = tl('Bağlantı kesildi');
      resolve();
    };
    xhr.send(file);
  });
}
$('filePick').addEventListener('change', async () => {
  const files = [...$('filePick').files];
  $('filePick').value = '';
  if (!files.length) return;
  let token;
  try {
    token = await getUploadToken();
  } catch {
    return toast(tl('Bilgisayara bağlı değil'));
  }
  for (const f of files) await uploadOne(f, token);
  haptic();
});
$('openUploads').onclick = () => (sendJson({ type: 'open-uploads' }) ? toast(tl('Klasör bilgisayarda açıldı')) : toast(tl('Bilgisayara bağlı değil')));

// ============================================================ PIN ile eşleşme
function showPin(reason) {
  if (reason) $('pinReason').textContent = reason;
  show('pin');
  $('pinInput').focus();
}
// PIN kutuları: gerçek alan görünmez bir katman; rakamlar kutularda gösterilir, 6. rakamda otomatik gönderilir
function renderPinBoxes() {
  const v = $('pinInput').value.replace(/\D/g, '').slice(0, 6);
  if ($('pinInput').value !== v) $('pinInput').value = v;
  [...$('pinBoxes').querySelectorAll('i')].forEach((el, i) => {
    el.textContent = v[i] ?? '';
    el.classList.toggle('on', i === Math.min(v.length, 5) && document.activeElement === $('pinInput'));
    el.classList.toggle('filled', i < v.length);
  });
  return v;
}
$('pinInput').addEventListener('input', () => {
  const v = renderPinBoxes();
  if (v.length === 6) {
    haptic();
    $('pinBtn').click();
  } else {
    $('pinErr').textContent = '';
  }
});
$('pinInput').addEventListener('focus', renderPinBoxes);
$('pinInput').addEventListener('blur', renderPinBoxes);
$('pinBoxes').addEventListener('pointerdown', () => $('pinInput').focus());

let pinBusy = false;
$('pinBtn').onclick = async () => {
  if (pinBusy) return;
  const pin = $('pinInput').value.trim();
  if (!/^\d{6}$/.test(pin)) {
    $('pinErr').textContent = tl('6 haneli PIN gir.');
    shakePin();
    return;
  }
  $('pinErr').textContent = '';
  pinBusy = true;
  $('pinBtn').disabled = true;
  try {
    identity = await pair({ pin });
    exp.pairedVia = 'pin';
    $('pinBoxes').classList.add('ok');
    report('paired', { pairedVia: 'pin' });
    start();
  } catch (e) {
    $('pinErr').textContent = tl(e.message);
    shakePin();
    $('pinInput').value = '';
    renderPinBoxes();
  } finally {
    pinBusy = false;
    $('pinBtn').disabled = false;
  }
};
function shakePin() {
  const b = $('pinBoxes');
  b.classList.remove('shake');
  void b.offsetWidth; // animasyonu yeniden başlat
  b.classList.add('shake');
}

// ============================================================ açılış
// Eşleşme kodunun ömrünü uzatır (kayan süre). Kod geçersizse sessizce yok sayılır.
function touchPairing() {
  if (!startCode) return;
  fetch('/api/pair/touch', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ code: startCode }) }).catch(() => {});
}

function start() {
  show('main');
  let saved = 'pad';
  try { saved = localStorage.getItem('view') || 'pad'; } catch {}
  showView(views[saved] ? saved : 'pad');
  renderHud();
  connect();
}

async function init() {
  if ('serviceWorker' in navigator) {
    navigator.serviceWorker.register('/sw.js').catch((e) => (exp.swError = String(e)));
  }
  measureFps().then((v) => {
    fps = v;
    renderHud();
  });

  if (!standalone) {
    show('guide');
    report('safari');
    // Kullanıcı bu sayfadayken eşleşme kodunun süresi dolmasın (Faz 0 dersi)
    touchPairing();
    document.addEventListener('visibilitychange', () => document.visibilityState === 'visible' && touchPairing());
    return;
  }

  identity = await loadIdentity();
  if (identity) {
    exp.pairedVia = 'kayıtlı anahtar';
  } else if (startCode) {
    try {
      identity = await pair({ code: startCode });
      exp.pairedVia = 'start_url kodu';
      toast(tl('Eşleşme tamam ✓'));
    } catch (e) {
      exp.pairError = `${e.code ?? ''} ${e.message}`;
      showPin(e.code === 'expired' ? tl('Eşleşme kodunun süresi dolmuş. PC\'deki yönetim sayfasında görünen PIN\'i gir.') : undefined);
    }
  } else {
    showPin(tl('Bu ana ekran uygulamasına eşleşme kodu ulaşmadı. PC\'deki yönetim sayfasında görünen PIN\'i gir.'));
  }
  // Kod bir kez kullanıldı; adres çubuğunda kalmasın
  if (startCode) history.replaceState(null, '', '/app');

  report('standalone-launch');
  if (identity) start();
}

applySettings();
// Yönetim sayfasındaki tasarım önizlemesi (sadece PC'de, /preview) için ekranlara erişim
if (location.pathname === '/preview') window.__preview = { onGameArt, air, setAirMode, onShot, onApps, onMixer, onAudioDevices, onSiri: (m) => { siriInfo = m; renderSiri(); }, show, showView, onMedia, onApps, onVolume, onContext, onTimer, openSheet };
else init();


// ============================================================ LG TV (PC aracı)
bindClickpad($('dpad'), (d) => { haptic(); sendJson({ type: 'key', key: d, mods: [], target: 'media' }); });
tvUi = initTv({ $, sendJson, toast, haptic, tl, openSheet: (id) => openSheet(id), closeSheets: () => closeSheets(), pressFeedback, showView: (n) => showView(n), getView: () => currentViewName, getPc: () => pcConn });
tvUi.refresh();
if (window.__preview) Object.assign(window.__preview, { tv: tvUi, setConn });

// ============================================================ erişilebilirlik
// Süs simgeleri ekran okuyucudan gizlenir; düğmelerin adı metinlerinden / aria-label'dan gelir
for (const s of document.querySelectorAll('svg.ic')) {
  s.setAttribute('aria-hidden', 'true');
  s.setAttribute('focusable', 'false');
}
// Ayar kaydırıcılarının adı satır başlığından
for (const r of document.querySelectorAll('.set-row input[type=range]')) {
  const label = r.closest('.set-row')?.querySelector('span');
  if (label && !r.getAttribute('aria-label')) r.setAttribute('aria-label', (label.firstChild?.textContent || label.textContent).trim());
}
// Her panelde sağ üstte "Bitti" (iOS tarzı kapatma)
for (const sh of document.querySelectorAll('.sheet')) {
  const done = document.createElement('button');
  done.className = 'sheet-done';
  done.textContent = tl('Bitti');
  done.addEventListener('click', () => closeSheets());
  sh.prepend(done);
}
// Paneller: iletişim kutusu rolü, başlığa bağlı ad, odak panele alınabilir
for (const sh of document.querySelectorAll('.sheet')) {
  sh.setAttribute('role', 'dialog');
  sh.setAttribute('aria-modal', 'true');
  sh.tabIndex = -1;
  const h = sh.querySelector('h3');
  if (h) {
    h.id ||= `${sh.id}Title`;
    sh.setAttribute('aria-labelledby', h.id);
  }
}
$('keyPop')?.setAttribute('aria-hidden', 'true');

// Seçili durum ekran okuyucuya bildirilir: segment düğmeleri, çipler, değiştirici tuşlar, sessiz
const PRESSABLE = '.seg button, .chip-btn, .sk.mod, .kk.shift, #muteBtn';
const isPressed = (el) => (el.id === 'muteBtn' ? el.classList.contains('muted') : el.classList.contains('on'));
const syncPressed = (el) => el.setAttribute('aria-pressed', String(isPressed(el)));
document.querySelectorAll(PRESSABLE).forEach(syncPressed);
new MutationObserver((records) => {
  for (const m of records) {
    if (m.type === 'attributes') {
      if (m.target.matches?.(PRESSABLE)) syncPressed(m.target);
    } else {
      for (const n of m.addedNodes) {
        if (n.nodeType !== 1) continue;
        if (n.matches?.(PRESSABLE)) syncPressed(n);
        n.querySelectorAll?.(PRESSABLE).forEach(syncPressed);
      }
    }
  }
}).observe(document.body, { attributes: true, attributeFilter: ['class'], subtree: true, childList: true });

// Odak yönetimi: panel açılınca odak panele, kapanınca açan düğmeye döner; Esc kapatır (harici klavye / Anahtar Denetimi)
let sheetOpener = null;
const baseOpen = openSheet;
openSheet = (id) => {
  sheetOpener = document.activeElement;
  baseOpen(id);
  $(id).focus({ preventScroll: true });
};
const baseClose = closeSheets;
closeSheets = (fromDrag) => {
  baseClose(fromDrag);
  if (sheetOpener?.isConnected) sheetOpener.focus({ preventScroll: true });
  sheetOpener = null;
};
document.addEventListener('keydown', (e) => {
  if (e.key !== 'Escape') return;
  if (!$('confirm').hidden) $('confirmNo').click();
  else if (!$('shotView').hidden) $('shotClose').click();
  else if (!$('backdrop').hidden) closeSheets();
});

// Fare tuşları: Enter / Boşluk ile basılır (harici klavye, Anahtar Denetimi)
for (const el of document.querySelectorAll('.mb')) {
  const bit = Number(el.dataset.bit);
  el.addEventListener('keydown', (e) => {
    if ((e.key !== 'Enter' && e.key !== ' ') || e.repeat) return;
    e.preventDefault();
    el.classList.add('down');
    buttons |= bit;
    sendState(now());
  });
  el.addEventListener('keyup', (e) => {
    if (e.key !== 'Enter' && e.key !== ' ') return;
    el.classList.remove('down');
    buttons &= ~bit;
    sendState(now());
  });
}
// Touchpad klavyeyle: oklar imleci oynatır (Shift = hızlı), Enter sol tık, Shift+Enter sağ tık
for (const el of [$('pad'), $('miniPad')]) {
  el.addEventListener('keydown', (e) => {
    const dir = { ArrowUp: [0, -1], ArrowDown: [0, 1], ArrowLeft: [-1, 0], ArrowRight: [1, 0] }[e.key];
    if (dir) {
      e.preventDefault();
      const step = e.shiftKey ? 120 : 24;
      sendInput(T.AIR, dir[0] * step, dir[1] * step, now());
    } else if (e.key === 'Enter' && !e.repeat) {
      e.preventDefault();
      click(e.shiftKey ? 2 : 1, now());
    }
  });
}
// İmleç yön düğmeleri (Ayarlar › Erişilebilirlik): dokunuşla basılı tutunca tekrarlar; VoiceOver etkinleştirmesi (click) tek adım atar
let nudgeByPointer = false;
for (const b of document.querySelectorAll('#nudge .nb')) {
  const [dx, dy] = b.dataset.d.split(',').map(Number);
  const step = () => sendInput(T.AIR, dx * 22, dy * 22, now());
  b.addEventListener('pointerdown', (e) => {
    e.preventDefault();
    nudgeByPointer = true;
    b.setPointerCapture(e.pointerId);
    b.classList.add('down');
    haptic();
    withRepeat(step);
  });
  const up = () => {
    b.classList.remove('down');
    stopRepeat();
  };
  b.addEventListener('pointerup', up);
  b.addEventListener('pointercancel', up);
  b.addEventListener('click', () => {
    if (!nudgeByPointer) step(); // dokunuşsuz etkinleştirme (VoiceOver / klavye)
    nudgeByPointer = false;
  });
}
for (const b of document.querySelectorAll('#tabs button')) b.setAttribute('aria-current', b.classList.contains('on') ? 'page' : 'false');

// Touchpad ipucu yalnızca ilk kullanıma kadar görünür
try { if (localStorage.getItem('padHinted')) $('pad').classList.add('used'); } catch {}

document.addEventListener('busychange', syncBusy);
