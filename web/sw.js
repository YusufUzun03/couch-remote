// Service Worker: arayüzü önbellekte tutar. PC kapalıyken bile uygulama açılır ve
// "Bilgisayara ulaşılamıyor" ekranını gösterebilir (Safari hata sayfası yerine).
// Ağ öncelikli: PC açıksa her zaman en güncel arayüz gelir.
const CACHE = 'couch-shell-v3';
const SHELL = ['/app', '/app.js', '/air.js', '/app.css', '/ui.css', '/apple-touch-icon.png', '/icon-192.png'];

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

self.addEventListener('fetch', (e) => {
  const url = new URL(e.request.url);
  if (e.request.method !== 'GET' || url.origin !== location.origin) return;
  const navigate = e.request.mode === 'navigate';
  if (!navigate && !SHELL.includes(url.pathname)) return;
  const key = navigate ? '/app' : url.pathname;

  e.respondWith(
    (async () => {
      const cache = await caches.open(CACHE);
      const ctrl = new AbortController();
      const timer = setTimeout(() => ctrl.abort(), 2000);
      try {
        const res = await fetch(navigate ? url.href : e.request, { signal: ctrl.signal, cache: 'no-store' });
        if (res.ok) cache.put(key, res.clone());
        return res;
      } catch {
        return (await cache.match(key)) ?? Response.error();
      } finally {
        clearTimeout(timer);
      }
    })(),
  );
});
