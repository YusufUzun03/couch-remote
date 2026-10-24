// Service Worker: arayüzü önbellekte tutar. Uygulama anında açılır (PC'ye sormadan), PC kapalıyken bile
// "Bilgisayara ulaşılamıyor" ekranı görünür. Önbellekten gelen dosya arka planda ETag ile yenilenir;
// değişiklik varsa uygulamaya haber verilir (henüz dokunulmadıysa sayfa kendini yeniler).
const CACHE = 'couch-shell-v17';
const SHELL = ['/app', '/app.js', '/air.js', '/tv.js', '/tour.js', '/mascot.js', '/brand-icons.js', '/app.css', '/ui.css', '/design.css', '/i18n.js', '/i18n-en.js', '/apple-touch-icon.png', '/icon-192.png'];

self.addEventListener('install', (e) => {
  e.waitUntil(caches.open(CACHE).then((c) => c.addAll(SHELL)));
  self.skipWaiting();
});

self.addEventListener('activate', (e) => {
  e.waitUntil(
    (async () => {
      for (const k of await caches.keys()) if (k !== CACHE) await caches.delete(k);
      await self.clients.claim();
    })(),
  );
});

async function revalidate(cache, key, req) {
  const ctrl = new AbortController();
  const timer = setTimeout(() => ctrl.abort(), 4000);
  try {
    const res = await fetch(req, { signal: ctrl.signal, cache: 'no-cache' });
    if (!res.ok) return null;
    const old = await cache.match(key);
    const changed = old && old.headers.get('etag') !== res.headers.get('etag');
    await cache.put(key, res.clone());
    if (changed) for (const c of await self.clients.matchAll()) c.postMessage({ type: 'shell-updated' });
    return res;
  } catch {
    return null;
  } finally {
    clearTimeout(timer);
  }
}

self.addEventListener('fetch', (e) => {
  const url = new URL(e.request.url);
  if (e.request.method !== 'GET' || url.origin !== location.origin) return;
  const navigate = e.request.mode === 'navigate';
  if (!navigate && !SHELL.includes(url.pathname)) return;
  // /app?code=... gibi adresler önbellekteki /app ile cevaplanır; kod sayfa içinden okunur
  const key = navigate ? '/app' : url.pathname;

  e.respondWith(
    (async () => {
      const cache = await caches.open(CACHE);
      const hit = await cache.match(key);
      const fresh = revalidate(cache, key, navigate ? url.href : e.request);
      if (hit) {
        e.waitUntil(fresh);
        return hit;
      }
      return (await fresh) ?? Response.error();
    })(),
  );
});
