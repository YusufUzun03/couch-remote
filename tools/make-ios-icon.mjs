// Renders the app icon (full-bleed square, no alpha channel: App Store Connect rejects alpha) to a 1024x1024 PNG.
// Usage: node tools/make-ios-icon.mjs
import { webkit } from 'playwright';
import { deflateSync } from 'node:zlib';
import { mkdirSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const out = join(root, 'ios', 'CouchRemote', 'Assets.xcassets', 'AppIcon.appiconset', 'icon-1024.png');
const N = 1024;

// Same artwork as web/logo.svg, but square: iOS applies its own corner mask.
const svg = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100"><defs><linearGradient id="g" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="#0A84FF"/><stop offset="1" stop-color="#5E5CE6"/></linearGradient></defs><rect width="100" height="100" fill="url(#g)"/><g fill="#fff" transform="translate(50 50) scale(1.05) translate(-50 -50)"><rect x="26" y="32" width="48" height="22" rx="7"/><rect x="23" y="50" width="54" height="16" rx="5"/><rect x="17" y="41" width="14" height="26" rx="6"/><rect x="69" y="41" width="14" height="26" rx="6"/><rect x="28.5" y="66" width="5" height="8" rx="1.5"/><rect x="66.5" y="66" width="5" height="8" rx="1.5"/></g></svg>`;

const browser = await webkit.launch();
const page = await browser.newPage();
const rgba = await page.evaluate(async ({ svg, N }) => {
  const img = new Image();
  img.src = 'data:image/svg+xml;charset=utf-8,' + encodeURIComponent(svg);
  await img.decode();
  const c = document.createElement('canvas');
  c.width = c.height = N;
  const ctx = c.getContext('2d');
  ctx.drawImage(img, 0, 0, N, N);
  return Array.from(ctx.getImageData(0, 0, N, N).data);
}, { svg, N });
await browser.close();

// RGBA -> RGB scanlines (filter byte 0), then a minimal PNG container.
const raw = Buffer.alloc(N * (N * 3 + 1));
for (let y = 0; y < N; y++) {
  raw[y * (N * 3 + 1)] = 0;
  for (let x = 0; x < N; x++) {
    const s = (y * N + x) * 4, d = y * (N * 3 + 1) + 1 + x * 3;
    raw[d] = rgba[s]; raw[d + 1] = rgba[s + 1]; raw[d + 2] = rgba[s + 2];
  }
}
const crcTable = Array.from({ length: 256 }, (_, n) => { let c = n; for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1; return c >>> 0; });
const crc = (buf) => { let c = 0xffffffff; for (const b of buf) c = crcTable[(c ^ b) & 0xff] ^ (c >>> 8); return (c ^ 0xffffffff) >>> 0; };
const chunk = (type, data) => {
  const len = Buffer.alloc(4); len.writeUInt32BE(data.length);
  const td = Buffer.concat([Buffer.from(type), data]);
  const c = Buffer.alloc(4); c.writeUInt32BE(crc(td));
  return Buffer.concat([len, td, c]);
};
const ihdr = Buffer.alloc(13);
ihdr.writeUInt32BE(N, 0); ihdr.writeUInt32BE(N, 4); ihdr[8] = 8; ihdr[9] = 2; // 8-bit, truecolor (no alpha)
const png = Buffer.concat([
  Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
  chunk('IHDR', ihdr), chunk('IDAT', deflateSync(raw)), chunk('IEND', Buffer.alloc(0)),
]);
mkdirSync(dirname(out), { recursive: true });
writeFileSync(out, png);
console.log('wrote', out, png.length, 'bytes');
