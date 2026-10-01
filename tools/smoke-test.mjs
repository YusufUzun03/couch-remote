// CouchRemote PC programı için uçtan uca duman testi. iPhone'u taklit eder:
// TLS + ad kısıtlaması, güvenlik kontrolleri, PIN ile eşleşme, imzalı giriş, imleç gidiş-dönüş, ping/pong.
// Kullanım: PC programı çalışırken `npm run smoke` (imleci kısa bir an sağa kaydırıp geri getirir).
import 'reflect-metadata';
import fs from 'node:fs';
import path from 'node:path';
import https from 'node:https';
import { webcrypto as wc } from 'node:crypto';
import { execSync } from 'node:child_process';
import WebSocket from 'ws';
import * as x509 from '@peculiar/x509';

const state = JSON.parse(fs.readFileSync(path.join(process.env.APPDATA, 'CouchRemote', 'state.json'), 'utf8'));
const HOST = `couch-${state.id}.local`;
const PORT = 47801;
const ORIGIN = `https://${HOST}:${PORT}`;
const ADMIN = 'http://127.0.0.1:47802';
const CA = state.ca.cert_pem;
const ADMIN_TOKEN = fs.readFileSync(path.join(process.env.APPDATA, 'CouchRemote', 'admin.token'), 'utf8').trim();
const ADMIN_COOKIE = { Cookie: `couch_admin=${ADMIN_TOKEN}` };
const status = async () => (await fetch(`${ADMIN}/api/status`, { headers: ADMIN_COOKIE })).json();
const LAN_IP = (await status()).lanIp;

let failed = 0;
const check = (name, ok, detail = '') => {
  if (!ok) failed++;
  console.log(`${ok ? '✓' : '✗'} ${name}${detail ? `  (${detail})` : ''}`);
};

function req(p, { method = 'GET', body, origin = ORIGIN, host = `${HOST}:${PORT}`, headers = {} } = {}) {
  return new Promise((resolve) => {
    const r = https.request(
      { host: LAN_IP, port: PORT, path: p, method, servername: HOST, ca: CA,
        headers: { Host: host, 'Content-Type': 'application/json', ...(origin ? { Origin: origin } : {}), ...headers } },
      (res) => {
        const chunks = [];
        res.on('data', (d) => chunks.push(d));
        res.on('end', () => {
          const raw = Buffer.concat(chunks);
          resolve({ status: res.statusCode, body: raw.toString(), raw, ctype: res.headers['content-type'], headers: res.headers });
        });
      },
    );
    r.on('error', (e) => resolve({ status: 0, body: e.message }));
    r.end(body ? JSON.stringify(body) : undefined);
  });
}
const adminPost = (p, body, origin = ADMIN, custom = true) =>
  fetch(ADMIN + p, { method: 'POST', headers: { 'Content-Type': 'application/json', Origin: origin, ...ADMIN_COOKIE, ...(custom ? { 'X-Couch-Admin': '1' } : {}) }, body: JSON.stringify(body ?? {}) });
const AGENT = path.join(import.meta.dirname, '..', 'agent', 'target', 'debug', 'couch-agent.exe');
const cursor = () => execSync(`"${AGENT}" --print-cursor`).toString().trim();

console.log(`\nHedef: ${HOST} (${LAN_IP})\n`);

// --- Sertifikalar ve depolama
x509.cryptoProvider.set(wc);
const ca = new x509.X509Certificate(CA);
check('CA ad kısıtlaması içeriyor', ca.extensions.some((e) => e.type === '2.5.29.30'));
const raw = fs.readFileSync(path.join(process.env.APPDATA, 'CouchRemote', 'state.json'), 'utf8');
check('Diskte şifrelenmemiş özel anahtar yok (DPAPI)', !raw.includes('PRIVATE KEY'));
check('Diskte şifrelenmemiş Kestirmeler jetonu yok', !raw.includes((await status()).shortcut.token));
const hello = await req('/api/hello');
check('TLS zinciri + doğru ad', hello.status === 200, hello.body);
check('Yanlış Host başlığı reddediliyor (DNS rebinding)', (await req('/api/hello', { host: 'evil.com:47801' })).status === 421);

// --- mDNS (Windows çözümleyicisi üzerinden)
const t0 = Date.now();
try {
  const r = await fetch(`http://${HOST}:47800/setup`, { signal: AbortSignal.timeout(20000) });
  check('mDNS: .local adı çözülüyor', r.status === 200, `${Date.now() - t0} ms (Windows'ta ilk çözümleme yavaş olabilir)`);
} catch (e) {
  check('mDNS: .local adı çözülüyor', false, e.cause?.code ?? e.message);
}

// --- Güvenlik
const kp = await wc.subtle.generateKey({ name: 'ECDSA', namedCurve: 'P-256' }, false, ['sign', 'verify']);
const jwk = await wc.subtle.exportKey('jwk', kp.publicKey);
check('Zararlı Origin ile eşleşme → 403', (await req('/api/pair', { method: 'POST', body: { pin: '000000', publicKeyJwk: jwk }, origin: 'https://evil.com' })).status === 403);
check('Yanlış PIN → 403', (await req('/api/pair', { method: 'POST', body: { pin: '000001', publicKeyJwk: jwk } })).status === 403);
check('Yönetim API, zararlı Origin → 403', (await adminPost('/api/new-pairing', {}, 'https://evil.com')).status === 403);
check('Yönetim API, özel başlık yok → 403', (await adminPost('/api/new-pairing', {}, ADMIN, false)).status === 403);
check('Yönetim API, anahtar yok → 401', (await fetch(`${ADMIN}/api/status`)).status === 401);
check('Yönetim sayfası, anahtar yok → kilit ekranı', (await fetch(`${ADMIN}/`)).status === 401);
const withToken = await fetch(`${ADMIN}/?t=${ADMIN_TOKEN}`, { redirect: 'manual' });
check('Yönetim sayfası, doğru anahtar → çerez + yönlendirme', withToken.status === 303 && /HttpOnly/.test(withToken.headers.get('set-cookie') ?? ''));
check('Yönetim sayfası, yanlış anahtar → kilit ekranı', (await fetch(`${ADMIN}/?t=yanlis`, { redirect: 'manual' })).status === 401);
check('Kestirme, yanlış jeton → 401', (await req('/api/v1/action/ping', { method: 'POST', origin: null, headers: { Authorization: 'Bearer yanlis' } })).status === 401);
const token = (await status()).shortcut.token;

// S2: kimliksiz rapor uç noktası kaldırıldı
check('S2: kimliksiz rapor yazılamıyor', (await req('/api/report', { method: 'POST', body: { phase: 'x' } })).status === 404);
// S3: profil adında doğrulama kodu var ve yönetim sayfasındakiyle aynı
const caCode = (await status()).caCode;
const profile = await fetch(`http://${LAN_IP}:47800/ca.mobileconfig`, { headers: { Host: `${HOST}:47800` } }).then((r) => r.text()).catch(() => '');
check('S3: profil adında doğrulama kodu', /^[0-9A-F]{4}-[0-9A-F]{4}$/.test(caCode) && profile.includes(`CouchRemote · ${caCode}`), caCode);
// S5: PIN günlüğe yazılmıyor (bu çalıştırmada üretilen kodlar)
const logText = fs.readFileSync(path.join(process.env.APPDATA, 'CouchRemote', 'couch.log'), 'utf8');
const runStart = logText.lastIndexOf('çalışıyor —');
check('S5: günlükte PIN yok', !/PIN: \d{6}/.test(logText.slice(runStart)));
// S4: 5 hatalı PIN → eşleşme kilitlenir, yönetim sayfası elle açabilir
{
  const kpx = await wc.subtle.generateKey({ name: 'ECDSA', namedCurve: 'P-256' }, false, ['sign', 'verify']);
  const jwkx = await wc.subtle.exportKey('jwk', kpx.publicKey);
  const realPin = (await status()).pairing.pin;
  const wrong = realPin === '000000' ? '000001' : '000000';
  // Önceki testler aynı oturumda deneme harcamış olabilir: kilit gelene kadar dene (en fazla 5)
  let last = 0;
  for (let i = 0; i < 5 && last !== 429; i++) last = (await req('/api/pair', { method: 'POST', body: { pin: wrong, publicKeyJwk: jwkx } })).status;
  const st4 = await status();
  check('S4: 5 hatalı PIN sonrası kilit', last === 429 && st4.pairingLocked === true && st4.pairing === null);
  await adminPost('/api/new-pairing', {});
  check('S4: yönetim sayfasından kilit kaldırılabiliyor', (await status()).pairing !== null);
}
const ping = await req('/api/v1/action/ping', { method: 'POST', origin: null, headers: { Authorization: `Bearer ${token}` } });
check('Kestirme, doğru jeton → 200', ping.status === 200, ping.body);
const act = (p) => req('/api/v1/action/' + p, { method: 'POST', origin: null, headers: { Authorization: `Bearer ${token}` } });
check('Kestirme: bilinmeyen eylem → 404', (await act('shutdown')).status === 404 && (await act('restart')).status === 404);
check('Kestirme: ses 150 → 400', (await act('volume?v=150')).status === 400);
check('Kestirme: ses parametresi yok → 400', (await act('volume')).status === 400);
check('Kestirme: tanınmayan uygulama → 404', (await act('launch?v=yok-boyle-uygulama')).status === 404);
check('Kestirme: boş arama → 400', (await act('search?app=youtube&v=')).status === 400);
check('Kestirme: zamanlayıcı 0 dk → 400', (await act('timer?v=0')).status === 400);
check('Kestirme: zamanlayıcı başlat/iptal', (await act('timer?v=30')).status === 200 && (await act('timer-cancel')).status === 200);
const shotApi = await act('screenshot');
check('Kestirme: ekran görüntüsü JPEG olarak dönüyor', shotApi.status === 200 && shotApi.ctype === 'image/jpeg' && shotApi.raw[0] === 0xff && shotApi.raw[1] === 0xd8 && shotApi.raw.length > 5000, `${Math.round(shotApi.raw.length / 1024)} KB`);
check('Kestirme: tarayıcıdan (yabancı Origin) → 403', (await req('/api/v1/action/ping', { method: 'POST', origin: 'https://evil.example', headers: { Authorization: `Bearer ${token}` } })).status === 403);

// --- Eşleşme süresinin uzatılması
const setupUrl = (await status()).pairing.setupUrl;
const code = new URL(setupUrl).searchParams.get('c');
const touch = await req('/api/pair/touch', { method: 'POST', body: { code } });
check('Eşleşme kodu süresi uzatılabiliyor', touch.status === 200 && JSON.parse(touch.body).ok === true);
const touchBad = await req('/api/pair/touch', { method: 'POST', body: { code: 'yanlis-kod-123' } });
check('Yanlış kodla süre uzatılamıyor', JSON.parse(touchBad.body).ok === false);

// --- PIN ile eşleşme
const pin = (await status()).pairing.pin;
const pair = await req('/api/pair', { method: 'POST', body: { pin, publicKeyJwk: jwk, keyMode: 'test', name: 'Duman testi' } });
check('Doğru PIN ile eşleşme', pair.status === 200, pair.body);
const deviceId = JSON.parse(pair.body).deviceId;

// --- WebSocket
await new Promise((resolve) => {
  const w = new WebSocket(`wss://${LAN_IP}:${PORT}/ws`, { servername: HOST, ca: CA, headers: { Host: `${HOST}:${PORT}` }, origin: 'https://evil.com' });
  w.on('error', () => { check('WS, zararlı Origin reddediliyor', true); resolve(); });
  w.on('open', () => { check('WS, zararlı Origin reddediliyor', false); w.close(); resolve(); });
});

const ws = new WebSocket(`wss://${LAN_IP}:${PORT}/ws`, { servername: HOST, ca: CA, headers: { Host: `${HOST}:${PORT}` }, origin: ORIGIN });
const waiters = {};
const seen = {};
const waitFor = (type, ms = 3000) => new Promise((res) => { waiters[type] = res; setTimeout(() => { if (waiters[type]) { delete waiters[type]; res(null); } }, ms); });

async function featureTests(ws) {
  // Ses: bağlanınca son ses durumu gelir (sadece okuma, ses değiştirilmez)
  await sleep(300);
  const vol = seen.volume;
  check('Ses seviyesi bilgisi geliyor', !!vol && typeof vol.level === 'number', vol ? `%${Math.round(vol.level * 100)}${vol.muted ? ' (sessiz)' : ''}` : 'yok');
  check('Öndeki uygulama bilgisi geliyor', !!seen.context, seen.context?.profile);
  check('Ses çıkış cihazı listesi geliyor', Array.isArray(seen['audio-devices']?.devices) && seen['audio-devices'].devices.length > 0 && seen['audio-devices'].devices.every((d) => d.id && d.name), `${seen['audio-devices']?.devices?.length} cihaz`);
  // Geçersiz kimlikle cihaz değiştirme sessizce reddedilir (varsayılan aynı kalır)
  const before = seen['audio-devices']?.devices?.find((d) => d.default)?.id;
  ws.send(JSON.stringify({ type: 'audio-device', id: 'yok-boyle-cihaz' }));
  await sleep(1500);
  check('Geçersiz ses cihazı kimliği reddediliyor', seen['audio-devices']?.devices?.find((d) => d.default)?.id === before);
  // Karıştırıcı: uygulama listesi geliyor; mevcut seviyeyi aynen geri yazmak hiçbir şeyi değiştirmez
  const mix = seen.mixer;
  check('Uygulama sesleri listesi geliyor', Array.isArray(mix?.apps) && mix.apps.every((x) => x.id && x.name && typeof x.level === 'number'), `${mix?.apps?.length} uygulama`);
  const app0 = mix?.apps?.[0];
  if (app0) {
    ws.send(JSON.stringify({ type: 'mixer', id: app0.id, level: app0.level }));
    ws.send(JSON.stringify({ type: 'mixer', id: 'yok-boyle-uygulama', level: 0 }));
    await sleep(1200);
    const after = seen.mixer?.apps?.find((x) => x.id === app0.id);
    check('Uygulama sesi komutu çalışıyor (aynı seviye geri yazıldı)', !!after && Math.abs(after.level - app0.level) < 0.02);
  }

  // Uygulamalar: Mağaza uygulamaları (u…) ve oyunlar listede
  ws.send(JSON.stringify({ type: 'apps' }));
  const apps = await waitFor('apps', 8000);
  check('Uygulama listesi: oyunlar alanı var', Array.isArray(apps?.games) && apps.games.every((g) => g.id && g.name && ['steam', 'epic'].includes(g.store)), `${apps?.games?.length} oyun`);
  check('Uygulama listesi: Mağaza uygulamaları geliyor', (apps?.installed ?? []).some((a) => /^u[0-9a-f]{16}$/.test(a.id)), `${(apps?.installed ?? []).filter((a) => a.id.startsWith('u')).length} Mağaza uygulaması`);
  // Oyun kapağı: listedeki Steam oyunu için küçük JPEG (veya kapak yoksa null); listede olmayan kimlik için null
  const firstSteam = (apps?.games ?? []).find((g) => g.store === 'steam');
  if (firstSteam) {
    ws.send(JSON.stringify({ type: 'game-art', id: firstSteam.id }));
    const art = await waitFor('game-art', 4000);
    check('Oyun kapağı: geçerli yanıt (JPEG data adresi veya null)', art?.id === firstSteam.id && (art.art === null || art.art.startsWith("data:image/jpeg;base64,")), art?.art ? `${Math.round(art.art.length / 1024)} KB` : 'kapak yok');
  }
  ws.send(JSON.stringify({ type: 'game-art', id: 'steam:../../etc/passwd' }));
  const badArt = await waitFor('game-art', 3000);
  check('Oyun kapağı: listede olmayan / bozuk kimlik reddediliyor', badArt?.art === null);
  ws.send(JSON.stringify({ type: 'launch', id: 'steam:99999999' }));
  const bad = await waitFor('launch-error', 3000);
  check('Tanınmayan oyun kimliği açılmıyor', !!bad);

  // Ekran görüntüsü
  ws.send(JSON.stringify({ type: 'screenshot' }));
  const shot = await waitFor('screenshot', 8000);
  const head = Buffer.from(shot?.jpeg ?? '', 'base64');
  check('Ekran görüntüsü geliyor (JPEG)', head[0] === 0xff && head[1] === 0xd8 && shot.w > 100 && shot.h > 100 && shot.monitors >= 1, shot ? `${shot.w}x${shot.h}, ${Math.round(head.length / 1024)} KB, ${shot.monitors} monitör` : 'yok');

  // Hız sınırı: art arda 20 ekran görüntüsü isteği — sadece birkaçı yapılmalı, kalanı "çok sık" hatası almalı
  let okShots = 0, busyShots = 0;
  const countShots = (raw) => {
    try {
      const m = JSON.parse(raw.toString());
      if (m.type === 'screenshot') (m.jpeg ? okShots++ : busyShots++);
    } catch {}
  };
  ws.on('message', countShots);
  for (let k = 0; k < 20; k++) ws.send(JSON.stringify({ type: 'screenshot' }));
  await sleep(7000); // yavaş (debug) derlemede yakalama ~1 sn sürer
  ws.off('message', countShots);
  check('Hız sınırı: ağır komut art arda yapılamıyor', okShots >= 1 && okShots <= 6 && busyShots >= 12, `${okShots} yapıldı, ${busyShots} reddedildi`);

  // Zamanlayıcı: 1 dk kur ve hemen iptal et (sesi kısmaya başlamadan)
  let p = waitFor('timer');
  ws.send(JSON.stringify({ type: 'timer', minutes: 1, action: 'stop' }));
  const t1 = await p;
  p = waitFor('timer');
  ws.send(JSON.stringify({ type: 'timer', cancel: true }));
  const t2 = await p;
  check('Uyku zamanlayıcısı kurulup iptal ediliyor', t1?.active === true && t1.remaining > 50 && t2?.active === false);

  // Pano: mevcut metni sakla → test metni yaz → oku → eskisini geri koy (resim varsa atla)
  p = waitFor('clipboard');
  ws.send(JSON.stringify({ type: 'clipboard-get' }));
  const orig = await p;
  if (orig && !orig.error) {
    const probe = 'CouchRemote pano testi ğüşİıçö ' + Date.now();
    p = waitFor('clipboard-set');
    ws.send(JSON.stringify({ type: 'clipboard-set', text: probe }));
    await p;
    p = waitFor('clipboard');
    ws.send(JSON.stringify({ type: 'clipboard-get' }));
    const back = await p;
    p = waitFor('clipboard-set');
    ws.send(JSON.stringify({ type: 'clipboard-set', text: orig.text }));
    await p;
    check('Pano gidiş-dönüş (Türkçe karakterler dahil)', back?.text === probe);
  } else {
    console.log('– Pano testi: ATLANDI (panoda metin yok)');
  }

  // Dosya: jeton al → yükle → dosyayı doğrula ve sil; yanlış jeton ve kötü ad
  p = waitFor('upload-token');
  ws.send(JSON.stringify({ type: 'upload-token' }));
  const tok = (await p)?.token;
  const upload = (name, token, body) => new Promise((res) => {
    const r = https.request({ host: LAN_IP, port: PORT, path: '/api/upload', method: 'POST', servername: HOST, ca: CA,
      headers: { Host: `${HOST}:${PORT}`, Origin: ORIGIN, 'X-Upload-Token': token, 'X-File-Name': encodeURIComponent(name), 'Content-Length': Buffer.byteLength(body) } },
      (x) => { let b = ''; x.on('data', (d) => (b += d)); x.on('end', () => res({ status: x.statusCode, body: b })); });
    r.end(body);
  });
  check('Dosya: yanlış jetonla yükleme reddediliyor', (await upload('a.txt', 'yanlis', 'x')).status === 401);
  const content = 'CouchRemote dosya testi ' + Date.now();
  const up = await upload('..\\..\\Windows\\test ğ.txt', tok, content);
  const saved = up.status === 200 ? JSON.parse(up.body).name : null;
  const dir = path.join(process.env.USERPROFILE, 'Downloads', 'CouchRemote');
  const ok = saved === 'test ğ.txt' || /^test ğ \(\d+\)\.txt$/.test(saved ?? '');
  check('Dosya: yükleme + yol temizleme', ok && fs.readFileSync(path.join(dir, saved), 'utf8') === content, saved);
  if (saved) fs.rmSync(path.join(dir, saved), { force: true });
}
const now = () => performance.timeOrigin + performance.now();
const pkt = (type, a, b, t, buttons = 0) => {
  const bf = Buffer.alloc(20);
  bf[0] = type; bf[1] = buttons; bf.writeFloatLE(a, 4); bf.writeFloatLE(b, 8); bf.writeDoubleLE(t, 12);
  return bf;
};
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
await new Promise((resolve) => {
  let gotPong = false;
  let media = null;
  ws.on('message', async (d, binary) => {
    if (binary) {
      if (d[0] === 0x11) {
        gotPong = true;
        const rtt = now() - d.readDoubleLE(4);
        check('Ping/pong', true, `RTT ${rtt.toFixed(2)} ms (yerel)`);
      }
      return;
    }
    const m = JSON.parse(d.toString());
    if (m.type === 'challenge') {
      const sig = await wc.subtle.sign({ name: 'ECDSA', hash: 'SHA-256' }, kp.privateKey, Buffer.from(m.nonce, 'base64'));
      ws.send(JSON.stringify({ type: 'auth', deviceId, sig: Buffer.from(sig).toString('base64') }));
    }
    if (m.type === 'ok') {
      check('İmzalı kimlik doğrulama', true);
      const p = Buffer.alloc(12); p[0] = 0x10; p.writeDoubleLE(now(), 4); ws.send(p);
      for (let i = 0; i < 50 && !gotPong; i++) await sleep(10);
      const before = cursor();
      for (let i = 0; i < 20; i++) { ws.send(pkt(1, 1.0, 0, now())); await sleep(8); }
      await sleep(150);
      const mid = cursor();
      for (let i = 0; i < 20; i++) { ws.send(pkt(1, -1.0, 0, now())); await sleep(8); }
      await sleep(150);
      const after = cursor();
      check('İmleç hareket etti', mid !== before, `${before} → ${mid}`);
      // Tam piksel doğruluğu --selftest-cursor ile ölçülür; burada test sırasında fare kıpırdarsa diye birkaç piksel tolerans
      const [bx, by] = before.split(',').map(Number), [ax, ay] = after.split(',').map(Number);
      // Program y yönünde hareket göndermiyor: y değiştiyse test sırasında fare elle kullanılmıştır
      const my = Number(mid.split(',')[1]);
      if (Math.abs(my - by) > 1 || Math.abs(ay - by) > 3) {
        console.log(`– İmleç gidiş-dönüş: ATLANDI (test sırasında dış fare hareketi algılandı: ${before} → ${mid} → ${after})`);
      } else {
        check('İmleç başlangıç noktasına döndü (±3 px)', Math.abs(ax - bx) <= 3 && Math.abs(ay - by) <= 3, `${before} → ${after}`);
      }
      // Hava faresi (tip 4): telefonun hesapladığı pikseller ivmesiz uygulanır; NaN reddedilir, aşırı değer kırpılır
      {
        const c0 = cursor().split(',').map(Number);
        for (let i = 0; i < 10; i++) { ws.send(pkt(4, 5.0, 0, now())); await sleep(8); }
        await sleep(120);
        const c1 = cursor().split(',').map(Number);
        ws.send(pkt(4, NaN, 7, now())); ws.send(pkt(4, Infinity, 0, now()));
        await sleep(120);
        const c2 = cursor().split(',').map(Number);
        ws.send(pkt(4, 5000, 0, now()));
        await sleep(120);
        const c3 = cursor().split(',').map(Number);
        ws.send(pkt(4, -300, 0, now())); for (let i = 0; i < 10; i++) { ws.send(pkt(4, -5.0, 0, now())); await sleep(8); }
        await sleep(150);
        const c4 = cursor().split(',').map(Number);
        const noisy = Math.abs(c1[1] - c0[1]) > 1 || Math.abs(c4[1] - c0[1]) > 3;
        if (noisy) console.log('– Hava faresi imleç testi: ATLANDI (test sırasında dış fare hareketi)');
        else {
          check('Hava faresi: piksel olduğu gibi uygulanıyor (10 × 5 px = 50 px)', Math.abs(c1[0] - c0[0] - 50) <= 2, `${c1[0] - c0[0]} px`);
          check('Hava faresi: NaN / sonsuz değer imleci bozmuyor', c2[0] === c1[0] && c2[1] === c1[1], `${c1} → ${c2}`);
          check('Hava faresi: aşırı değer kırpılıyor (5000 → en çok 300 px)', c3[0] - c2[0] > 100 && c3[0] - c2[0] <= 301, `${c3[0] - c2[0]} px`);
          check('Hava faresi: imleç başlangıca döndü (±3 px)', Math.abs(c4[0] - c0[0]) <= 3, `${c0} → ${c4}`);
        }
      }
      await sleep(1100); // istatistik mesajı için
      check('Pong alındı', gotPong);
      await featureTests(ws);
      check('"Şu an çalıyor" bilgisi geliyor', !!media, media ? (media.active ? `${media.app}: ${media.title} (${media.playing ? 'çalıyor' : 'duraklatıldı'})` : 'şu an çalan yok') : 'mesaj yok');
      // S1: bağlantı açıkken eşleşme kaldırılır → sunucu 'revoked' gönderip bağlantıyı hemen kapatmalı
      const closed = new Promise((r) => ws.on('close', r));
      const revokedMsg = waitFor('revoked', 3000);
      await adminPost('/api/remove-device', { id: deviceId });
      const rv = await revokedMsg;
      const closedInTime = await Promise.race([closed.then(() => true), sleep(2000).then(() => false)]);
      check('S1: kaldırılan cihazın bağlantısı anında kesiliyor', !!rv && closedInTime);
      resolve();
    }
    if (m.type === 'media' && !media) media = m;
    seen[m.type] = m;
    if (waiters[m.type]) { waiters[m.type](m); delete waiters[m.type]; }
    if (m.type === 'stats') check('Canlı istatistik mesajı', true, `PC işleme p99 ${m.proc?.p99 ?? '–'} ms`);
  });
});

const self = execSync(`"${AGENT}" --selftest-cursor`).toString().trim().split('\n').pop().trim();
check('Koordinat dönüşümü öz testi (tam piksel)', /^(\d+) \/ \1 /.test(self), self);

check('Test cihazı kaldırıldı', !(await status()).devices.some((d) => d.id === deviceId));

// ---- Ağ sertleştirme
const netMod = await import('node:net');
const appPage = await req('/app');
const csp = appPage.headers['content-security-policy'] ?? '';
check('Güvenlik başlıkları (CSP, çerçeveleme yasağı)', /frame-ancestors 'none'/.test(csp) && /script-src 'self'(;|$)/.test(csp) && appPage.headers['x-frame-options'] === 'DENY' && appPage.headers['referrer-policy'] === 'no-referrer');

{
  // Yarım bırakılan bağlantı (hiçbir şey göndermeyen) zaman aşımıyla kapanmalı
  const t0 = Date.now();
  const idle = netMod.connect(PORT, LAN_IP);
  const closedIn = await new Promise((res) => { idle.on('close', () => res(Date.now() - t0)); idle.on('error', () => {}); setTimeout(() => res(null), 14000); });
  idle.destroy();
  check('Yarım bırakılan bağlantı zaman aşımıyla kapanıyor', closedIn !== null && closedIn < 13000, closedIn ? `${Math.round(closedIn / 1000)} sn` : 'kapanmadı');

  // IP başına bağlantı sınırı: 48'den fazlası hemen kapatılmalı
  const socks = Array.from({ length: 60 }, () => { const s = netMod.connect(PORT, LAN_IP); s.on('error', () => {}); return s; });
  await sleep(1500);
  const dropped = socks.filter((s) => s.destroyed || s.readyState === 'closed').length;
  socks.forEach((s) => s.destroy());
  check('Bağlantı doldurma: IP başına sınır uygulanıyor', dropped >= 8, `${dropped} / 60 bağlantı reddedildi`);
  await sleep(500);
}

{
  // Kimlik doğrulamadan önce büyük mesaj gönderen bağlantı kesilmeli
  const big = new WebSocket(`wss://${LAN_IP}:${PORT}/ws`, { servername: HOST, ca: CA, headers: { Host: `${HOST}:${PORT}` }, origin: ORIGIN });
  const gone = await new Promise((res) => { big.on('open', () => big.send('x'.repeat(5000))); big.on('close', () => res(true)); big.on('error', () => res(true)); setTimeout(() => res(false), 3000); });
  check('Kimlik doğrulamadan önce büyük mesaj bağlantıyı kesiyor', gone);
}

// Yıkıcı testler (jeton yenileme, tüm eşleşmeleri silme): sadece izole deneme ortamında (COUCH_TEST_DESTRUCTIVE=1)
if (process.env.COUCH_TEST_DESTRUCTIVE) {
  const oldToken = (await status()).shortcut.token;
  await adminPost('/api/rotate-shortcut-token', {});
  const newToken = (await status()).shortcut.token;
  const callWith = (tk) => req('/api/v1/action/ping', { method: 'POST', origin: null, headers: { Authorization: `Bearer ${tk}` } });
  check('Jeton yenileme: yeni jeton farklı ve çalışıyor, eskisi reddediliyor', newToken !== oldToken && (await callWith(newToken)).status === 200 && (await callWith(oldToken)).status === 401);
  const raw2 = fs.readFileSync(path.join(process.env.APPDATA, 'CouchRemote', 'state.json'), 'utf8');
  check('Yenilenen jeton diskte şifresiz değil', !raw2.includes(newToken));
  await adminPost('/api/remove-all-devices', {});
  const afterAll = await status();
  check('Tüm eşleşmeler kaldırıldı ve jeton yenilendi', afterAll.devices.length === 0 && afterAll.shortcut.token !== newToken);
}

{
  // Yanlış jeton denemeleri: sınırdan sonra doğru jeton bile 1 dakika reddedilir (tahmin saldırısını yavaşlatır)
  let last = 0;
  for (let k = 0; k < 14; k++) last = (await req('/api/v1/action/ping', { method: 'POST', origin: null, headers: { Authorization: 'Bearer yanlis' + k } })).status;
  const good = await req('/api/v1/action/ping', { method: 'POST', origin: null, headers: { Authorization: `Bearer ${token}` } });
  check('Hatalı jeton denemeleri sınırlanıyor (429)', last === 429 && good.status === 429, `son hatalı: ${last}, sonra doğru: ${good.status}`);
}

console.log(failed ? `\n${failed} test BAŞARISIZ\n` : '\nTüm testler geçti ✓\n');
process.exit(failed ? 1 : 0);
