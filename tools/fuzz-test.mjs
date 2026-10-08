// Protokol dayanıklılık testi: kimliği doğrulanmış bir bağlantıdan bozuk, uç değerli ve rastgele mesajlar gönderir.
// Beklenti: program çökmez, bağlantılar kontrollü kapanır, sonrasında normal istemci hâlâ bağlanıp ping/pong yapabilir.
// Gerçek bir masaüstünde yan etki olmasın diye: yalnızca YANLIŞ TİPTE alanlar ve bilinmeyen mesaj türleri gönderilir
// (metin yazan, uygulama açan, güç komutu veren geçerli mesajlar asla). İzole ortamda çalıştır:
//   COUCH_DATA_DIR=... COUCH_PORT_BASE=48800 COUCH_DEV=1 couch-agent  →  aynı değişkenlerle: node tools/fuzz-test.mjs
import 'reflect-metadata';
import fs from 'node:fs';
import path from 'node:path';
import { webcrypto as wc } from 'node:crypto';
import WebSocket from 'ws';

const DATA = process.env.COUCH_DATA_DIR ?? path.join(process.env.APPDATA, 'CouchRemote');
const state = JSON.parse(fs.readFileSync(path.join(DATA, 'state.json'), 'utf8'));
const HOST = `couch-${state.id}.local`;
const BASE = Number(process.env.COUCH_PORT_BASE ?? 47800);
const PORT = BASE + 1;
const ORIGIN = `https://${HOST}:${PORT}`;
const ADMIN = `http://127.0.0.1:${BASE + 2}`;
const CA = state.ca.cert_pem;
const COOKIE = { Cookie: `couch_admin=${fs.readFileSync(path.join(DATA, 'admin.token'), 'utf8').trim()}` };
const status = async () => (await fetch(`${ADMIN}/api/status`, { headers: COOKIE })).json();
const LAN_IP = (await status()).lanIp;
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

let failed = 0;
const check = (name, ok, detail = '') => {
  if (!ok) failed++;
  console.log(`${ok ? '✓' : '✗'} ${name}${detail ? `  (${detail})` : ''}`);
};

// Tekrarlanabilir rastgelelik: hata çıkarsa aynı tohumla yeniden üretilebilir
let seed = Number(process.env.FUZZ_SEED ?? 1337);
const rnd = () => ((seed = (seed * 1664525 + 1013904223) >>> 0) / 2 ** 32);
const pick = (a) => a[Math.floor(rnd() * a.length)];
console.log(`\nTohum: ${seed} (FUZZ_SEED ile değiştir)\n`);

const open = () => new WebSocket(`wss://${LAN_IP}:${PORT}/ws`, { servername: HOST, ca: CA, headers: { Host: `${HOST}:${PORT}` }, origin: ORIGIN });

// Eşleş (test cihazı), bitince kaldır
const kp = await wc.subtle.generateKey({ name: 'ECDSA', namedCurve: 'P-256' }, false, ['sign', 'verify']);
const jwk = await wc.subtle.exportKey('jwk', kp.publicKey);
await fetch(`${ADMIN}/api/new-pairing`, { method: 'POST', headers: { 'Content-Type': 'application/json', Origin: ADMIN, 'X-Couch-Admin': '1', ...COOKIE }, body: '{}' });
const pin = (await status()).pairing.pin;
const https = (await import('node:https')).default;
const pairRes = await new Promise((resolve) => {
  const r = https.request({ host: LAN_IP, port: PORT, path: '/api/pair', method: 'POST', servername: HOST, ca: CA,
    headers: { Host: `${HOST}:${PORT}`, Origin: ORIGIN, 'Content-Type': 'application/json' } }, (res) => {
    const c = []; res.on('data', (d) => c.push(d)); res.on('end', () => resolve({ status: res.statusCode, body: Buffer.concat(c).toString() }));
  });
  r.on('error', (e) => resolve({ status: 0, body: e.message }));
  r.end(JSON.stringify({ pin, publicKeyJwk: jwk, keyMode: 'test', name: 'Fuzz testi' }));
});
check('Fuzz: test cihazı eşleşti', pairRes.status === 200, pairRes.body.slice(0, 80));
const deviceId = JSON.parse(pairRes.body).deviceId;

/** Kimlik doğrulanmış bağlantı açar; auth bitince çözülür */
function authed() {
  return new Promise((resolve) => {
    const ws = open();
    ws.on('message', async (d, isBin) => {
      if (isBin) return;
      const m = JSON.parse(d.toString());
      if (m.type === 'challenge') {
        const sig = await wc.subtle.sign({ name: 'ECDSA', hash: 'SHA-256' }, kp.privateKey, Buffer.from(m.nonce, 'base64'));
        ws.send(JSON.stringify({ type: 'auth', deviceId, sig: Buffer.from(sig).toString('base64') }));
      } else if (m.type === 'ok') resolve(ws);
    });
    ws.on('error', () => resolve(null));
    setTimeout(() => resolve(null), 5000);
  });
}
async function alive() {
  const ws = await authed();
  if (!ws) return false;
  const ok = await new Promise((res) => {
    ws.on('message', (d, isBin) => isBin && d[0] === 0x11 && res(true));
    const p = Buffer.alloc(12); p[0] = 0x10; p.writeDoubleLE(performance.now(), 4); ws.send(p);
    setTimeout(() => res(false), 2000);
  });
  ws.close();
  return ok;
}

// Hatalı kimlik denemeleri IP başına geçici olarak sınırlanır (10 / 60 sn): önceki bir çalıştırmadan kalan sınır bitene kadar bekle
async function aliveEventually(maxMs = 75000) {
  const end = Date.now() + maxMs;
  while (Date.now() < end) { if (await alive()) return true; await sleep(3000); }
  return false;
}
check('Fuzz öncesi: kimlik doğrulama ve ping/pong çalışıyor', await aliveEventually());

// --- 1) İkili paketler: yanlış uzunluk, bilinmeyen tür, NaN/Infinity, uç değerler
{
  const ws = await authed();
  let closed = false;
  ws.on('close', () => (closed = true));
  const sent = [];
  for (let len of [0, 1, 2, 3, 4, 11, 12, 19, 21, 64, 1024, 65536]) {
    const b = Buffer.alloc(len); for (let i = 0; i < len; i++) b[i] = Math.floor(rnd() * 256);
    // Güvenli tür baytı: 0x20+ (bilinmeyen). Gerçek hareket türleri (1-4) yalnızca aşağıdaki NaN/Inf testinde.
    if (len) b[0] = 0x20 + Math.floor(rnd() * 0xd0);
    ws.send(b); sent.push(len);
  }
  // Hareket paketleri: NaN, ±Infinity, çok büyük sayılar (sunucu NaN'ı reddedip büyükleri kırpmalı; imleç oynamasın diye hepsi geçersiz/0)
  const mk = (type, a, b) => { const p = Buffer.alloc(20); p[0] = type; p.writeFloatLE(a, 4); p.writeFloatLE(b, 8); p.writeDoubleLE(1, 12); return p; };
  for (const t of [1, 2, 4]) {
    for (const v of [NaN, Infinity, -Infinity]) ws.send(mk(t, v, v));
  }
  await sleep(500);
  check('İkili paketler: bozuk uzunluk/tür/NaN/Infinity gönderildi, program ayakta', await alive(), `${sent.length + 9} paket`);
  ws.close();
}

// --- 2) JSON: bozuk gövde, derin iç içe, çok büyük değer, yanlış tipler
{
  const ws = await authed();
  const types = ['key', 'text', 'combo', 'media', 'launch', 'volume', 'mixer', 'audio-device', 'screenshot', 'clipboard-set', 'game-art',
    'windows', 'window', 'zoom', 'tv', 'timer', 'power', 'launch-search', 'apps', 'upload-token', 'shortcut-info', 'upload-token', 'wake'];
  const wrong = [123, -1, 1e308, null, true, false, [], [1, 2, 3], {}, { a: { b: { c: 1 } } }, '', '\u0000', '😀'.repeat(50), 'A'.repeat(20000)];
  // Dikkat: aşağıdaki değerlerin hiçbiri "geçerli bir eylemi tetikleyen" değer değildir (string alanlara sayı/dizi/null verilir)
  const harmlessStr = (v) => typeof v !== 'string' || v === '' || v.length > 5000;
  let sentCount = 0;
  const raw = ['{', '}', '[', 'null', '123', 'true', '"x"', '{"type":', '{"type":null}', '{"type":123}', '{"type":[]}', '{"type":{"a":1}}',
    '{"type":"' + 'x'.repeat(10000) + '"}', '[' .repeat(5000), '{"a":'.repeat(2000), '\u0000\u0001\u0002', '{"type":"\\ud800"}'];
  for (const r of raw) { ws.send(r); sentCount++; }
  for (let i = 0; i < 600; i++) {
    const msg = { type: pick(types) };
    for (const k of ['key', 'text', 'id', 'value', 'level', 'mute', 'name', 'ch', 'n', 'mods', 'action', 'monitor', 'app', 'input', 'video', 'url', 'op', 'ms', 'minutes', 'code']) {
      if (rnd() < 0.5) msg[k] = pick(wrong.filter(harmlessStr));
    }
    ws.send(JSON.stringify(msg)); sentCount++;
  }
  await sleep(1500);
  check('JSON: bozuk gövde, aşırı iç içe, devasa değer, yanlış tipler gönderildi, program ayakta', await alive(), `${sentCount} mesaj`);
  ws.close();
}

// --- 3) Boyut sınırı: 512 KB üstü mesaj bağlantıyı kapatmalı
{
  const ws = await authed();
  const gone = await new Promise((res) => {
    ws.on('close', () => res(true)); ws.on('error', () => res(true));
    ws.send('x'.repeat(600 * 1024));
    setTimeout(() => res(false), 4000);
  });
  check('512 KB üstü mesaj bağlantıyı kapatıyor', gone);
  check('…ve program sonrasında normal çalışıyor', await alive());
}

// --- 4) Fırtına: çok hızlı küçük paket ve çok kısa ömürlü bağlantı
{
  const ws = await authed();
  const p = Buffer.alloc(12); p[0] = 0x10;
  for (let i = 0; i < 20000; i++) ws.send(p);
  await sleep(1000);
  ws.close();
  for (let i = 0; i < 25; i++) { const w = open(); w.on('error', () => {}); w.on('open', () => w.close()); await sleep(20); }
  await sleep(500);
  check('Paket fırtınası ve hızlı aç/kapa sonrası program ayakta', await alive());
}

// --- 5) Kimlik doğrulama aşaması: bozuk auth mesajları
{
  const cases = [
    { type: 'auth' }, { type: 'auth', deviceId: 123, sig: 'x' }, { type: 'auth', deviceId, sig: 'not-base64!!' },
    { type: 'auth', deviceId, sig: '' }, { type: 'auth', deviceId, sig: 'A'.repeat(100000) }, { type: 'auth', deviceId: null, sig: null },
    { type: 'auth', deviceId: 'A'.repeat(10000), sig: 'AAAA' }, { type: 'text', text: 'kimliksiz' },
  ];
  let accepted = 0;
  for (const c of cases) {
    const ws = open();
    const got = await new Promise((res) => {
      ws.on('message', (d, isBin) => {
        if (isBin) return;
        const m = JSON.parse(d.toString());
        if (m.type === 'challenge') ws.send(JSON.stringify(c));
        else if (m.type === 'ok') res('ok');
        else res(m.type);
      });
      ws.on('close', () => res('closed'));
      ws.on('error', () => res('closed'));
      setTimeout(() => res('timeout'), 3000);
    });
    if (got === 'ok') accepted++;
    ws.close();
  }
  check('Bozuk auth mesajlarının hiçbiri kabul edilmiyor', accepted === 0, `${cases.length} durum`);
  check('Auth denemeleri sonrası program ayakta (hatalı deneme sınırı geçici, süre dolunca normal istemci girebiliyor)', await aliveEventually());
}

// Temizlik: test cihazını kaldır, program hâlâ sağlıklı mı
await fetch(`${ADMIN}/api/remove-device`, { method: 'POST', headers: { 'Content-Type': 'application/json', Origin: ADMIN, 'X-Couch-Admin': '1', ...COOKIE }, body: JSON.stringify({ id: deviceId }) });
const st = await status();
check('Sonuç: yönetim API yanıt veriyor, test cihazı temizlendi', !!st && !st.devices.some((d) => d.id === deviceId));

console.log(failed ? `\n${failed} test BAŞARISIZ\n` : '\nTüm fuzz testleri geçti ✓\n');
process.exit(failed ? 1 : 0);
