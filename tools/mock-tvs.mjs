// Sahte Samsung ve Android TV: smoke-test.mjs'in marka sürücülerini gerçek TV olmadan sınaması için.
// Protokol belgelerine göre yazıldı (Samsung Tizen WebSocket, Android TV Remote v2); gerçek TV davranışının yerini tutmaz.
import http from 'node:http';
import tls from 'node:tls';
import crypto, { webcrypto as wc } from 'node:crypto';
import { WebSocketServer } from 'ws';
import * as x509 from '@peculiar/x509';

// ---------------------------------------------------------------- Samsung
export function startSamsung(host) {
  const log = [];
  const TOKEN = '98765432';
  const server = http.createServer((req, res) => {
    if (req.url === '/api/v2/') {
      res.writeHead(200, { 'content-type': 'application/json' });
      res.end(JSON.stringify({ device: { modelName: 'QE55Q80', name: '[TV] Samsung', wifiMac: 'AA:BB:CC:11:22:33', PowerState: 'on' } }));
    } else res.writeHead(404).end();
  });
  const wss = new WebSocketServer({ server });
  wss.on('connection', (c, rq) => {
    const q = new URL(rq.url, 'http://x').searchParams;
    log.push({ connect: true, token: q.get('token'), name: q.get('name') });
    const ok = () => c.send(JSON.stringify({ event: 'ms.channel.connect', data: { token: TOKEN, clients: [] } }));
    if (q.get('token') === TOKEN) ok();
    else if (q.get('token')) c.send(JSON.stringify({ event: 'ms.channel.unauthorized' }));
    else setTimeout(ok, 300); // kullanıcı TV'de "İzin ver" der
    c.on('message', (d) => log.push(JSON.parse(d.toString())));
  });
  return new Promise((resolve) => server.listen(8001, host, () => resolve({ log, TOKEN, drop: () => { for (const c of wss.clients) c.close(); }, close: () => { for (const c of wss.clients) c.terminate(); wss.close(); server.close(); } })));
}

// ---------------------------------------------------------------- protobuf
const varint = (v) => { const o = []; v = BigInt(v); do { let b = Number(v & 0x7fn); v >>= 7n; o.push(v ? b | 0x80 : b); } while (v); return Buffer.from(o); };
const fVar = (f, v) => Buffer.concat([varint(f << 3), varint(v)]);
const fBytes = (f, b) => { b = Buffer.from(b); return Buffer.concat([varint((f << 3) | 2), varint(b.length), b]); };
function parse(buf) {
  const out = []; let p = 0;
  const rv = () => { let v = 0n, s = 0n; for (;;) { const b = buf[p++]; v |= BigInt(b & 0x7f) << s; if (!(b & 0x80)) return v; s += 7n; } };
  while (p < buf.length) {
    const tag = Number(rv()); const f = tag >> 3;
    if ((tag & 7) === 0) out.push([f, Number(rv())]);
    else if ((tag & 7) === 2) { const l = Number(rv()); out.push([f, buf.subarray(p, p + l)]); p += l; }
    else break;
  }
  return out;
}
const getB = (m, f) => m.find(([k, v]) => k === f && Buffer.isBuffer(v))?.[1];
const getV = (m, f) => m.find(([k, v]) => k === f && typeof v === 'number')?.[1];
const frame = (b) => Buffer.concat([varint(b.length), b]);
function framer(onFrame) {
  let buf = Buffer.alloc(0);
  return (d) => {
    buf = Buffer.concat([buf, d]);
    for (;;) {
      let p = 0, len = 0, shift = 0, ok = false;
      while (p < buf.length) { const b = buf[p++]; len |= (b & 0x7f) << shift; shift += 7; if (!(b & 0x80)) { ok = true; break; } }
      if (!ok || buf.length < p + len) return;
      const msg = buf.subarray(p, p + len); buf = buf.subarray(p + len); onFrame(msg);
    }
  };
}
const nBytes = (k) => { const j = k.export({ format: 'jwk' }); const t = (s) => { const b = Buffer.from(s, 'base64url'); let i = 0; while (i < b.length - 1 && b[i] === 0) i++; return b.subarray(i); }; return [t(j.n), t(j.e)]; };

// ---------------------------------------------------------------- Android TV
export async function startAndroid(host) {
  x509.cryptoProvider.set(wc);
  const alg = { name: 'RSASSA-PKCS1-v1_5', hash: 'SHA-256', publicExponent: new Uint8Array([1, 0, 1]), modulusLength: 2048 };
  const keys = await wc.subtle.generateKey(alg, true, ['sign', 'verify']);
  const cert = await x509.X509CertificateGenerator.createSelfSigned({ serialNumber: '01', name: 'CN=MockAndroidTV', notBefore: new Date(Date.now() - 3600e3), notAfter: new Date(Date.now() + 86400e3), signingAlgorithm: alg, keys });
  const key = crypto.createPrivateKey({ key: Buffer.from(await wc.subtle.exportKey('pkcs8', keys.privateKey)), format: 'der', type: 'pkcs8' }).export({ type: 'pkcs8', format: 'pem' });
  const certPem = cert.toString('pem');
  const [sMod, sExp] = nBytes(crypto.createPublicKey(certPem));
  const state = { code: null, paired: false, keys: [], links: [], pairLog: [], remoteConnects: 0, pings: 0, clientConfigured: false };
  const opts = { key, cert: certPem, requestCert: true, rejectUnauthorized: false };
  const clientNums = (sock) => nBytes(new crypto.X509Certificate(sock.getPeerCertificate(true).raw).publicKey);

  const pairing = tls.createServer(opts, (s) => {
    s.on('error', () => {});
    if (!s.getPeerCertificate().raw) return s.destroy();
    const [cMod, cExp] = clientNums(s);
    let expected = null;
    const pm = (field, inner, status = 200) => s.write(frame(Buffer.concat([fVar(1, 2), fVar(2, status), fBytes(field, inner)])));
    s.on('data', framer((msg) => {
      const m = parse(msg);
      if (getB(m, 10)) { state.pairLog.push('request'); pm(11, fBytes(1, 'MockTV')); }
      else if (getB(m, 20)) { state.pairLog.push('option'); pm(20, Buffer.concat([fBytes(1, Buffer.concat([fVar(1, 3), fVar(2, 6)])), fVar(3, 1)])); }
      else if (getB(m, 30)) {
        // TV kodu üretir: ilk bayt, (istemci+sunucu anahtarları + kodun kalanı) özetinin ilk baytıdır
        const tail = Buffer.from([0x2b, 0x3c]);
        expected = crypto.createHash('sha256').update(Buffer.concat([cMod, cExp, sMod, sExp, tail])).digest();
        state.code = Buffer.concat([expected.subarray(0, 1), tail]).toString('hex').toUpperCase();
        state.pairLog.push('config'); pm(31, Buffer.alloc(0));
      }
      else if (getB(m, 40)) {
        const h = expected;
        const given = getB(parse(getB(m, 40)), 1);
        const good = given && Buffer.compare(h, given) === 0;
        state.pairLog.push(good ? 'secret-ok' : 'secret-bad');
        if (good) { state.paired = true; pm(41, Buffer.alloc(0)); } else pm(41, Buffer.alloc(0), 400);
      }
    }));
  });

  const remote = tls.createServer(opts, (s) => {
    s.on('error', () => {});
    state.remoteConnects++;
    if (!s.getPeerCertificate().raw || !state.paired) return s.destroy();
    const rm = (field, inner) => s.write(frame(fBytes(field, inner)));
    rm(1, fVar(1, 622));
    s.on('data', framer((msg) => {
      const m = parse(msg);
      if (getB(m, 1)) { state.clientConfigured = true; rm(2, fVar(1, 622)); }
      else if (getB(m, 2)) { rm(40, fVar(1, 1)); rm(50, Buffer.concat([fVar(6, 100), fVar(7, 25), fVar(8, 0)])); rm(8, fVar(1, 7)); }
      else if (getB(m, 9)) state.pings++;
      else if (getB(m, 10)) state.keys.push(getV(parse(getB(m, 10)), 1));
      else if (getB(m, 90)) state.links.push(getB(parse(getB(m, 90)), 1).toString());
    }));
  });
  await new Promise((r) => pairing.listen(6467, host, r));
  await new Promise((r) => remote.listen(6466, host, r));
  return { state, close: () => { pairing.close(); remote.close(); } };
}
