// Arayüz önizleme sunucusu (geliştirme): web/ klasörünü diskten sunar, /preview = app.html.
// PC programı gerekmez; bağlantı/eşleşme çalışmaz, sadece tasarım ve etkileşim denenir.
// Kullanım: node tools/preview-server.mjs  →  http://127.0.0.1:47890/preview
import { createServer } from 'node:http';
import { readFile } from 'node:fs/promises';
import { join, extname, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const WEB = join(dirname(fileURLToPath(import.meta.url)), '..', 'web');
const SITE = join(dirname(fileURLToPath(import.meta.url)), '..', 'site'); // landing page, served at /site/
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
  getApp: { url: 'https://yusufuzun03.github.io/couch-remote/', qr: QR },
};
// Performans ölçümü (?perf=1): zamanlayıcı ve animasyon karesi uyanışlarını sayar → window.__wake
const PERF_JS = `(() => {
  const w = (window.__wake = { timer: 0, raf: 0, since: performance.now() });
  const st = window.setTimeout, si = window.setInterval, raf = window.requestAnimationFrame;
  window.setTimeout = (f, ...a) => st(typeof f === 'function' ? (...x) => { w.timer++; return f(...x); } : f, ...a);
  window.setInterval = (f, ...a) => si(typeof f === 'function' ? (...x) => { w.timer++; return f(...x); } : f, ...a);
  window.requestAnimationFrame = (f) => raf((t) => { w.raf++; return f(t); });
  window.__wakeReset = () => { w.timer = 0; w.raf = 0; w.since = performance.now(); };
})();`;
createServer(async (req, res) => {
  const path = new URL(req.url, 'http://x').pathname;
  const perf = new URL(req.url, 'http://x').searchParams.has('perf');
  if (path === '/__perf.js') return res.writeHead(200, { 'content-type': 'text/javascript', 'cache-control': 'no-store' }).end(PERF_JS);
  if (path === '/api/status') return res.writeHead(200, { 'content-type': 'application/json' }).end(JSON.stringify(mock));
  if (path === '/__mock' && req.method === 'POST') {
    let b = ''; for await (const c of req) b += c;
    const patch = JSON.parse(b || '{}');
    for (const [k, v] of Object.entries(patch)) mock[k] = v && typeof v === 'object' && !Array.isArray(v) && mock[k] && typeof mock[k] === 'object' ? { ...mock[k], ...v } : v;
    return res.writeHead(200, { 'content-type': 'application/json' }).end('{"ok":true}');
  }
  if (req.method === 'POST' && path.startsWith('/api/')) return res.writeHead(200, { 'content-type': 'application/json' }).end('{"ok":true}');
  if (path === '/site' || path === '/site/' || path === '/site/logo.svg') {
    const name = path === '/site/logo.svg' ? 'logo.svg' : 'index.html';
    try {
      const body = await readFile(join(SITE, name));
      return res.writeHead(200, { 'content-type': TYPES[extname(name)], 'cache-control': 'no-store' }).end(body);
    } catch {
      return res.writeHead(404).end();
    }
  }
  const file = path === '/preview' ? 'app.html' : path.slice(1);
  if (!/^[\w.-]+$/.test(file)) return res.writeHead(404).end();
  try {
    let body = await readFile(join(WEB, file));
    if (file === 'app.html') body = body.toString().replace('%%MANIFEST_HREF%%', '');
    if (file === 'app.html' && perf) body = body.toString().replace('<head>', '<head><script src="/__perf.js"></script>');
    if (file === 'setup.html') body = body.toString().replaceAll('%%HOST%%', 'couch-test.local').replaceAll('%%APP_ORIGIN%%', 'https://couch-test.local:47801').replaceAll('%%CA_CODE%%', '7F3A-91C2');
    res.writeHead(200, { 'content-type': TYPES[extname(file)] ?? 'application/octet-stream', 'cache-control': 'no-store', 'content-security-policy': file === 'app.html' ? CSP_APP : CSP_PAGE }).end(body);
  } catch {
    res.writeHead(404).end();
  }
}).listen(Number(process.env.PREVIEW_PORT ?? 47890), '127.0.0.1', () => console.log('http://127.0.0.1:' + (process.env.PREVIEW_PORT ?? 47890) + '/preview'));
