// Arayüz önizleme sunucusu (geliştirme): web/ klasörünü diskten sunar, /preview = app.html.
// PC programı gerekmez; bağlantı/eşleşme çalışmaz, sadece tasarım ve etkileşim denenir.
// Kullanım: node tools/preview-server.mjs  →  http://127.0.0.1:47890/preview
import { createServer } from 'node:http';
import { readFile } from 'node:fs/promises';
import { join, extname, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const WEB = join(dirname(fileURLToPath(import.meta.url)), '..', 'web');
// Ajanın gerçek CSP politikalarıyla aynı (agent/src/web.rs): arayüz bu kısıtlarla denenir, ihlaller tarayıcı konsolunda görünür
const CSP_APP = "default-src 'none'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data: blob:; connect-src 'self' wss:; manifest-src 'self'; worker-src 'self'; media-src 'self' blob:; base-uri 'none'; form-action 'self'; frame-ancestors 'none'";
const CSP_PAGE = "default-src 'none'; script-src 'self' 'unsafe-inline'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; connect-src 'self' https:; base-uri 'none'; form-action 'self'; frame-ancestors 'none'";
const TYPES = { '.html': 'text/html; charset=utf-8', '.css': 'text/css', '.js': 'text/javascript', '.svg': 'image/svg+xml' };

// Yönetim sayfası önizlemesi için sahte durum: POST /__mock {…} ile değiştirilir (örn. {"pairing":{"setupSeen":true}})
const QR = '<svg viewBox="0 0 29 29" xmlns="http://www.w3.org/2000/svg"><rect width="29" height="29" fill="#fff"/>' + Array.from({ length: 29 * 29 }, (_, i) => ((i * 7919 + (i >> 3) * 31) % 5 < 2 ? `<rect x="${i % 29}" y="${Math.floor(i / 29)}" width="1" height="1"/>` : '')).join('') + '</svg>';
const mock = {
  host: 'couch-test.local', lanIp: '192.168.1.20', version: '0.3.0', caCode: '7F3A-91C2', pairingLocked: false, networkPublic: false, connected: 0, devices: [],
  pairing: { pin: '482915', expiresIn: 540, setupUrl: 'http://couch-test.local:47800/setup?c=abc', ipUrl: 'http://192.168.1.20:47800/setup?c=abc', qr: QR, setupSeen: false, profileSeen: false },
  shortcut: { url: 'https://couch-test.local:47801/api/v1/action/playpause', token: 'TESTTOKEN', actions: [{ id: 'playpause', params: '', about: 'Oynat' }], last: null },
  latency: { oneWay: null, proc: null, rate: 0 }, reports: [],
};
createServer(async (req, res) => {
  const path = new URL(req.url, 'http://x').pathname;
  if (path === '/api/status') return res.writeHead(200, { 'content-type': 'application/json' }).end(JSON.stringify(mock));
  if (path === '/__mock' && req.method === 'POST') {
    let b = ''; for await (const c of req) b += c;
    const patch = JSON.parse(b || '{}');
    for (const [k, v] of Object.entries(patch)) mock[k] = v && typeof v === 'object' && !Array.isArray(v) && mock[k] && typeof mock[k] === 'object' ? { ...mock[k], ...v } : v;
    return res.writeHead(200, { 'content-type': 'application/json' }).end('{"ok":true}');
  }
  if (req.method === 'POST' && path.startsWith('/api/')) return res.writeHead(200, { 'content-type': 'application/json' }).end('{"ok":true}');
  const file = path === '/preview' ? 'app.html' : path.slice(1);
  if (!/^[\w.-]+$/.test(file)) return res.writeHead(404).end();
  try {
    let body = await readFile(join(WEB, file));
    if (file === 'app.html') body = body.toString().replace('%%MANIFEST_HREF%%', '');
    if (file === 'setup.html') body = body.toString().replaceAll('%%HOST%%', 'couch-test.local').replaceAll('%%APP_ORIGIN%%', 'https://couch-test.local:47801').replaceAll('%%CA_CODE%%', '7F3A-91C2');
    res.writeHead(200, { 'content-type': TYPES[extname(file)] ?? 'application/octet-stream', 'cache-control': 'no-store', 'content-security-policy': file === 'app.html' ? CSP_APP : CSP_PAGE }).end(body);
  } catch {
    res.writeHead(404).end();
  }
}).listen(47890, '127.0.0.1', () => console.log('http://127.0.0.1:47890/preview'));
