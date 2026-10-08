// WebRTC veri kanalı testi (telefonu taklit eder, node-datachannel ile): teklif/yanıt, kanal açılışı, ping/pong,
// istatistik, kimliksiz teklif, kötü UDP paketleri, WebSocket kapanınca oturumun kapanması.
// Masaüstünde yan etki olmasın diye yalnızca SIFIR hareketli paketler gönderilir (imleç oynamaz, düğmeye basılmaz).
// Çalıştır (izole örnekle): COUCH_DATA_DIR=... COUCH_PORT_BASE=48800 COUCH_DEV=1 couch-agent  →  node tools/rtc-test.mjs
import 'reflect-metadata';
import fs from 'node:fs';
import path from 'node:path';
import https from 'node:https';
import dgram from 'node:dgram';
import { webcrypto as wc } from 'node:crypto';
import WebSocket from 'ws';
import ndc from 'node-datachannel';

const DATA = process.env.COUCH_DATA_DIR ?? path.join(process.env.APPDATA, 'CouchRemote');
const state = JSON.parse(fs.readFileSync(path.join(DATA, 'state.json'), 'utf8'));
const HOST = `couch-${state.id}.local`;
const BASE = Number(process.env.COUCH_PORT_BASE ?? 47800);
const PORT = BASE + 1;
const ORIGIN = `https://${HOST}:${PORT}`;
const ADMIN = `http://127.0.0.1:${BASE + 2}`;
const CA = state.ca.cert_pem;
const COOKIE = { Cookie: `couch_admin=${fs.readFileSync(path.join(DATA, 'admin.token'), 'utf8').trim()}` };
const AH = { 'Content-Type': 'application/json', Origin: ADMIN, 'X-Couch-Admin': '1', ...COOKIE };
const status = async () => (await fetch(`${ADMIN}/api/status`, { headers: COOKIE })).json();
const LAN_IP = (await status()).lanIp;
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const until = async (fn, ms = 8000) => { const end = Date.now() + ms; while (Date.now() < end) { if (await fn()) return true; await sleep(100); } return false; };

let failed = 0;
const check = (name, ok, detail = '') => {
  if (!ok) failed++;
  console.log(`${ok ? '✓' : '✗'} ${name}${detail ? `  (${detail})` : ''}`);
};

const open = (addr = LAN_IP) => new WebSocket(`wss://${addr}:${PORT}/ws`, { servername: HOST, ca: CA, headers: { Host: `${HOST}:${PORT}` }, origin: ORIGIN });

// Eşleş
const kp = await wc.subtle.generateKey({ name: 'ECDSA', namedCurve: 'P-256' }, false, ['sign', 'verify']);
const jwk = await wc.subtle.exportKey('jwk', kp.publicKey);
await fetch(`${ADMIN}/api/new-pairing`, { method: 'POST', headers: AH, body: '{}' });
const pin = (await status()).pairing.pin;
const pairRes = await new Promise((resolve) => {
  const r = https.request({ host: LAN_IP, port: PORT, path: '/api/pair', method: 'POST', servername: HOST, ca: CA, headers: { Host: `${HOST}:${PORT}`, Origin: ORIGIN, 'Content-Type': 'application/json' } }, (res) => {
    const c = []; res.on('data', (d) => c.push(d)); res.on('end', () => resolve({ status: res.statusCode, body: Buffer.concat(c).toString() }));
  });
  r.on('error', (e) => resolve({ status: 0, body: e.message }));
  r.end(JSON.stringify({ pin, publicKeyJwk: jwk, keyMode: 'test', name: 'RTC testi' }));
});
const deviceId = JSON.parse(pairRes.body).deviceId;

function authed(addr) {
  return new Promise((resolve) => {
    const ws = open(addr);
    ws.msgs = [];
    ws.on('message', async (d, isBin) => {
      if (isBin) return;
      const m = JSON.parse(d.toString());
      ws.msgs.push(m);
      if (m.type === 'challenge') {
        const sig = await wc.subtle.sign({ name: 'ECDSA', hash: 'SHA-256' }, kp.privateKey, Buffer.from(m.nonce, 'base64'));
        ws.send(JSON.stringify({ type: 'auth', deviceId, sig: Buffer.from(sig).toString('base64') }));
      } else if (m.type === 'ok') resolve(ws);
    });
    ws.on('error', () => resolve(null));
    setTimeout(() => resolve(null), 5000);
  });
}

/** Telefon gibi: WSS üzerinden teklif gönderir, yanıtı alır, veri kanalının açılmasını bekler */
async function connectRtc(ws) {
  const pc = new ndc.PeerConnection('test', { iceServers: [] });
  const dc = pc.createDataChannel('input', { unordered: true, maxRetransmits: 0 });
  const r = { pc, dc, open: false, closed: false, pongs: [], answer: null };
  dc.onOpen(() => (r.open = true));
  dc.onClosed(() => (r.closed = true));
  dc.onMessage((m) => { const b = Buffer.from(m); if (b[0] === 0x11) r.pongs.push(b); });
  await new Promise((res) => { pc.onGatheringStateChange((s) => s === 'complete' && res()); setTimeout(res, 3000); });
  const sdp = pc.localDescription().sdp;
  ws.msgs.length = 0;
  ws.send(JSON.stringify({ type: 'rtc-offer', sdp }));
  await until(() => ws.msgs.some((m) => m.type === 'rtc-answer'), 5000);
  r.answer = ws.msgs.find((m) => m.type === 'rtc-answer');
  if (r.answer?.sdp) pc.setRemoteDescription(r.answer.sdp, 'answer');
  return r;
}
const portFree = () => new Promise((res) => {
  const u = dgram.createSocket('udp4');
  u.once('error', () => { try { u.close(); } catch {} res(false); });
  u.bind(PORT, LAN_IP, () => { u.close(); res(true); });
});
const ping = () => { const p = Buffer.alloc(12); p[0] = 0x10; p.writeDoubleLE(performance.now(), 4); return p; };
const zeroMove = () => { const p = Buffer.alloc(20); p[0] = 1; p.writeDoubleLE(performance.now(), 12); return p; };

// 1) Kimliksiz teklif reddedilir
{
  const ws = open();
  const got = await new Promise((res) => {
    ws.on('message', (d) => { const m = JSON.parse(d.toString()); if (m.type === 'challenge') ws.send(JSON.stringify({ type: 'rtc-offer', sdp: 'v=0' })); else if (m.type === 'rtc-answer') res('yanıt'); });
    ws.on('close', () => res('kapandı')); ws.on('error', () => res('kapandı'));
    setTimeout(() => res('zaman aşımı'), 6000);
  });
  check('Kimliksiz WebRTC teklifi yanıtlanmıyor', got !== 'yanıt', got);
}

// 2) Bozuk teklif: hata döner, program ayakta
{
  const ws = await authed();
  ws.msgs.length = 0;
  ws.send(JSON.stringify({ type: 'rtc-offer', sdp: 'saçma metin' }));
  await until(() => ws.msgs.some((m) => m.type === 'rtc-answer'), 4000);
  const a = ws.msgs.find((m) => m.type === 'rtc-answer');
  check('Bozuk SDP: hata yanıtı dönüyor, çökme yok', !!a?.error && !a.sdp, a?.error);
  ws.send(JSON.stringify({ type: 'rtc-offer' }));
  await sleep(300);
  ws.close();
}

// 2b) Yalnızca oturum varken UDP soketi açık olmalı (yeni saldırı yüzeyi ayar kapalıyken yok)
check('Oturum yokken UDP soketi kapalı (port boşta)', await portFree());
{
  // Teklif yağdırma sınırlanıyor
  const ws = await authed();
  ws.msgs.length = 0;
  for (let i = 0; i < 14; i++) ws.send(JSON.stringify({ type: 'rtc-offer', sdp: 'v=0' }));
  await until(() => ws.msgs.filter((m) => m.type === 'rtc-answer').length >= 14, 4000);
  const errs = ws.msgs.filter((m) => m.type === 'rtc-answer' && /çok sık/.test(m.error ?? '')).length;
  check('Hızlı art arda WebRTC teklifleri sınırlanıyor', errs >= 5, `${errs} / 14 reddedildi`);
  ws.close();
  await sleep(2500); // kova dolsun
}
check('Başarısız tekliflerden sonra UDP soketi kapalı kalıyor', await portFree());

// 3) Gerçek oturum
{
  const ws = await authed();
  check('Kimlik doğrulandı (test cihazı)', !!ws);
  const r = await connectRtc(ws);
  check('Teklif/yanıt: ice-lite ve LAN adayı içeren yanıt', !!r.answer?.sdp && /a=ice-lite/.test(r.answer.sdp) && r.answer.sdp.includes(LAN_IP), r.answer?.error ?? '');
  check('Veri kanalı açıldı (sırasız, yeniden göndermesiz)', await until(() => r.open, 10000));
  if (r.open) {
    check('Oturum varken UDP soketi açık', !(await portFree()));
    for (let i = 0; i < 5; i++) { r.dc.sendMessageBinary(ping()); await sleep(40); }
    check('Kanal üzerinden ping → pong', await until(() => r.pongs.length >= 3, 3000), `${r.pongs.length}/5`);
    // saat farkı (WS'ten) + sıfır hareketler (kanaldan) → tek yön istatistiği
    const off = Buffer.alloc(20); off[0] = 0x12; off.writeDoubleLE(0, 4); off.writeDoubleLE(1, 12);
    ws.send(off);
    ws.send(JSON.stringify({ type: 'stats-reset' }));
    await sleep(300);
    const n0 = (await status()).latency?.oneWay?.n ?? 0;
    for (let i = 0; i < 60; i++) { r.dc.sendMessageBinary(zeroMove()); await sleep(5); }
    await sleep(500);
    const lat = (await status()).latency?.oneWay;
    check('Kanaldan gelen hareket paketleri istatistiğe yazılıyor', !!lat && lat.n >= n0 + 40, `n=${lat?.n}`);
    // Kötü UDP: rastgele yabancı paketler oturumu bozmamalı
    const udp = dgram.createSocket('udp4');
    for (let i = 0; i < 200; i++) udp.send(Buffer.from(Array.from({ length: 1 + (i % 90) }, () => Math.floor(Math.random() * 256))), PORT, LAN_IP);
    await sleep(300);
    udp.close();
    r.pongs.length = 0;
    r.dc.sendMessageBinary(ping());
    check('Rastgele yabancı UDP paketlerinden sonra oturum çalışıyor', await until(() => r.pongs.length >= 1, 3000));
    // Yabancı adresten (127.0.0.1) STUN/DTLS biçimli ve rastgele paket seli: oturumu etkilememeli
    {
      const atk = dgram.createSocket('udp4');
      await new Promise((res) => atk.bind(0, '127.0.0.1', res));
      const stun = () => { const b = Buffer.alloc(20 + 8); b.writeUInt16BE(0x0001, 0); b.writeUInt16BE(8, 2); b.writeUInt32BE(0x2112a442, 4); for (let i = 8; i < 20; i++) b[i] = Math.random() * 256; b.writeUInt16BE(0x0006, 20); b.writeUInt16BE(4, 22); b.write('ab:c', 24); return b; };
      const dtls = () => { const b = Buffer.alloc(13 + 40); b[0] = 0x16; b[1] = 0xfe; b[2] = 0xfd; b.writeUInt16BE(40, 11); for (let i = 13; i < b.length; i++) b[i] = Math.random() * 256; return b; };
      const junk = () => Buffer.from(Array.from({ length: 1 + Math.floor(Math.random() * 1400) }, () => Math.floor(Math.random() * 256)));
      const t0 = Date.now(); let sent = 0; const rtts = []; r.pongs.length = 0;
      let stop = false;
      const flood = (async () => { while (Date.now() - t0 < 4000) { for (let i = 0; i < 300; i++) { atk.send([stun, dtls, junk][i % 3](), PORT, LAN_IP); sent++; } await sleep(1); } stop = true; })();
      let pingsSent = 0;
      while (!stop) { r.dc.sendMessageBinary(ping()); pingsSent++; await sleep(20); }
      await flood; await sleep(300);
      atk.close();
      check('Yabancı adresten UDP seli: oturum etkilenmiyor (pong kaybı < %10)', r.pongs.length >= pingsSent * 0.9, `${sent} paket, ${r.pongs.length}/${pingsSent} pong`);
    }
    // Bozuk / aşırı büyük kanal mesajları
    r.dc.sendMessageBinary(Buffer.alloc(3));
    r.dc.sendMessageBinary(Buffer.alloc(60000, 7));
    r.dc.sendMessage('metin mesajı');
    const nan = Buffer.alloc(20); nan[0] = 1; nan.writeFloatLE(NaN, 4); nan.writeFloatLE(Infinity, 8); r.dc.sendMessageBinary(nan);
    await sleep(500);
    r.pongs.length = 0;
    r.dc.sendMessageBinary(ping());
    check('Bozuk kanal mesajlarından sonra oturum çalışıyor', await until(() => r.pongs.length >= 1, 3000));
    // WebSocket kapanınca oturum da kapanır
    ws.close();
    await sleep(600);
    r.pongs.length = 0;
    for (let i = 0; i < 5; i++) { r.dc.sendMessageBinary(ping()); await sleep(100); }
    await sleep(500);
    check('WebSocket kapanınca sunucu WebRTC oturumunu bırakıyor (artık yanıt yok)', r.pongs.length === 0, r.pongs.length + ' pong');
    check('Son oturum bitince UDP soketi kapanıyor', await until(portFree, 4000));
  }
  try { r.pc.close(); } catch {}
}

// 3a) Oturum, kendisini kuran WebSocket'in IP'sine kilitli: WS 127.0.0.1'den, ICE paketleri LAN adresinden gelirse kabul edilmez
{
  const ws = await authed('127.0.0.1');
  if (!ws) check('IP kilidi: 127.0.0.1 üzerinden kimlik doğrulama (test ortamı)', false);
  else {
    const r = await connectRtc(ws);
    const opened = await until(() => r.open, 5000);
    check('IP kilidi: farklı adresten gelen ICE/DTLS paketleri kabul edilmiyor, kanal açılmıyor', !!r.answer?.sdp && !opened);
    try { r.pc.close(); } catch {}
    ws.close();
    await sleep(500);
  }
}

// 3b) Kanal hiç açılmazsa (tamamlanmayan ICE) oturum silinir ve soket kapanır
{
  const ws = await authed();
  const pc = new ndc.PeerConnection('yarim', { iceServers: [] });
  pc.createDataChannel('input', { unordered: true, maxRetransmits: 0 });
  await new Promise((res) => { pc.onGatheringStateChange((s) => s === 'complete' && res()); setTimeout(res, 3000); });
  ws.msgs.length = 0;
  ws.send(JSON.stringify({ type: 'rtc-offer', sdp: pc.localDescription().sdp }));
  await until(() => ws.msgs.some((m) => m.type === 'rtc-answer'), 4000);
  check('Tamamlanmayan oturum: teklif kabul edildi, soket açık', !!ws.msgs.find((m) => m.type === 'rtc-answer')?.sdp && !(await portFree()));
  check('Kanal açılmazsa oturum ~15 sn sonra silinip soket kapanıyor', await until(portFree, 22000));
  try { pc.close(); } catch {}
  ws.close();
}

// 4) Oturum sonrası: normal WebSocket istemcisi yine çalışıyor, sızıntı yok
{
  const ws = await authed();
  const ok = !!ws;
  ws?.close();
  check('Test sonrası normal bağlantı çalışıyor', ok);
}

await fetch(`${ADMIN}/api/remove-device`, { method: 'POST', headers: AH, body: JSON.stringify({ id: deviceId }) });
ndc.cleanup();
console.log(failed ? `\n${failed} test BAŞARISIZ\n` : '\nTüm WebRTC testleri geçti ✓\n');
process.exit(failed ? 1 : 0);
