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

createServer(async (req, res) => {
  const path = new URL(req.url, 'http://x').pathname;
  const file = path === '/preview' ? 'app.html' : path.slice(1);
  if (!/^[\w.-]+$/.test(file)) return res.writeHead(404).end();
  try {
    let body = await readFile(join(WEB, file));
    if (file === 'app.html') body = body.toString().replace('%%MANIFEST_HREF%%', '');
    if (file === 'setup.html') body = body.toString().replaceAll('%%HOST%%', 'couch-test.local').replaceAll('%%APP_ORIGIN%%', 'https://couch-test.local:47801');
    res.writeHead(200, { 'content-type': TYPES[extname(file)] ?? 'application/octet-stream', 'cache-control': 'no-store', 'content-security-policy': file === 'app.html' ? CSP_APP : CSP_PAGE }).end(body);
  } catch {
    res.writeHead(404).end();
  }
}).listen(47890, '127.0.0.1', () => console.log('http://127.0.0.1:47890/preview'));
