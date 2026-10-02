// Performans ölçümü: PC'de boşta kaynak kullanımı, telefon arayüzünün ağ yükü ve bağlantı kurulum süresi.
// Kullanım: node tools/perf.mjs <etiket> [--cold]   → tools/perf-results/<etiket>.json
//           node tools/perf.mjs --compare <etiket1> <etiket2> ...   → yan yana tablo
// Telefon bağlıyken ölçüm sonucu bozulur; ölçüm sırasında bağlı cihaz sayısı sonuca yazılır.
import fs from 'node:fs';
import path from 'node:path';
import https from 'node:https';
import zlib from 'node:zlib';
import { execFileSync, spawn } from 'node:child_process';
import WebSocket from 'ws';

const dir = path.join(import.meta.dirname, 'perf-results');
fs.mkdirSync(dir, { recursive: true });
const args = process.argv.slice(2);

if (args[0] === '--compare') {
  const labels = args.slice(1);
  const rs = labels.map((l) => JSON.parse(fs.readFileSync(path.join(dir, l + '.json'), 'utf8')));
  const rows = [
    ['Boşta işlemci (tek çekirdeğin %)', (r) => r.idle.cpuPercentOfOneCore],
    ['Boşta RAM, özel (MB)', (r) => r.idle.privateMB],
    ['Boşta RAM, çalışma kümesi (MB)', (r) => r.idle.workingSetMB],
    ['İş parçacığı sayısı', (r) => r.idle.threads],
    ['İlk açılışta ağdan inen (KB)', (r) => r.web.wireKB],
    ['Arayüz dosyaları açılmış boyut (KB)', (r) => r.web.rawKB],
    ['Tekrar açılışta ağdan inen (KB)', (r) => r.web.repeatWireKB ?? r.web.wireKB],
    ['İlk açılış indirme süresi (ms)', (r) => r.web.loadMs],
    ['Tekrar açılış süresi (ms)', (r) => r.web.repeatMs ?? r.web.loadMs],
    ['WebSocket hazır, ilk bağlantı (ms)', (r) => r.ws.firstMs],
    ['WebSocket hazır, sonrakiler ort. (ms)', (r) => r.ws.restAvgMs],
    ['Exe boyutu (MB)', (r) => r.exeMB],
    ['Soğuk başlangıç: program hazır (ms)', (r) => r.cold?.readyMs ?? '–'],
    ['Soğuk başlangıç: LAN üzerinden ilk istek yanıtı (ms)', (r) => r.cold?.lanFirstMs ?? r.cold?.listenMs ?? '–'],
    ['Soğuk başlangıç + 5 sn RAM, özel (MB)', (r) => r.cold?.privateMB ?? '–'],
  ];
  const w = Math.max(...rows.map((r) => r[0].length));
  console.log('Ölçüm'.padEnd(w + 2) + labels.map((l) => l.padStart(16)).join(''));
  for (const [name, f] of rows) console.log(name.padEnd(w + 2) + rs.map((r) => String(f(r)).padStart(16)).join(''));
  if (rs.length > 1) {
    console.log('\nİlk ölçüme göre değişim:');
    for (const [name, f] of rows) {
      const a = f(rs[0]), b = f(rs.at(-1));
      if (typeof a === 'number' && typeof b === 'number' && a > 0) console.log('  ' + name.padEnd(w) + `  ${a} → ${b}  (${(((b - a) / a) * 100).toFixed(0)}%)`);
    }
  }
  process.exit(0);
}

const label = args[0] || 'olcum';
const state = JSON.parse(fs.readFileSync(path.join(process.env.APPDATA, 'CouchRemote', 'state.json'), 'utf8'));
const HOST = `couch-${state.id}.local`;
const PORT = 47801;
const CA = state.ca.cert_pem;
const token = fs.readFileSync(path.join(process.env.APPDATA, 'CouchRemote', 'admin.token'), 'utf8').trim();
const status = await (await fetch('http://127.0.0.1:47802/api/status', { headers: { Cookie: `couch_admin=${token}` } })).json();
const LAN_IP = status.lanIp;
const result = { label, at: new Date().toISOString(), connectedDevices: status.connections ?? status.connected ?? null };

// ---------------------------------------------------------------- boşta kaynak kullanımı
const exeDir = path.join(process.env.LOCALAPPDATA, 'Programs', 'CouchRemote');
result.exeMB = +(fs.statSync(path.join(exeDir, 'CouchRemote.exe')).size / 1048576).toFixed(2);
console.log('Boşta kaynak ölçülüyor (30 sn)…');
result.idle = JSON.parse(execFileSync('powershell', ['-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', path.join(import.meta.dirname, 'perf-proc.ps1'), '-Seconds', '30'], { encoding: 'utf8' }));

// ---------------------------------------------------------------- arayüz ağ yükü
function get(p, enc, inm) {
  return new Promise((resolve) => {
    const t0 = performance.now();
    const r = https.request({ host: LAN_IP, port: PORT, path: p, servername: HOST, ca: CA, headers: { Host: `${HOST}:${PORT}`, 'Accept-Encoding': enc, ...(inm ? { 'If-None-Match': inm } : {}) } }, (res) => {
      const chunks = [];
      res.on('data', (d) => chunks.push(d));
      res.on('end', () => {
        const wire = Buffer.concat(chunks);
        const ce = res.headers['content-encoding'];
        let raw = wire;
        try { raw = ce === 'gzip' ? zlib.gunzipSync(wire) : ce === 'br' ? zlib.brotliDecompressSync(wire) : wire; } catch {}
        resolve({ p, status: res.statusCode, wire: wire.length, raw: raw.length, enc: ce ?? 'yok', etag: res.headers.etag, ms: performance.now() - t0 });
      });
    });
    r.on('error', () => resolve({ p, status: 0, wire: 0, raw: 0, enc: 'hata', ms: 0 }));
    r.end();
  });
}
const sw = fs.readFileSync(path.join(import.meta.dirname, '..', 'web', 'sw.js'), 'utf8');
const shell = [...sw.match(/const SHELL = \[([^\]]*)\]/)[1].matchAll(/'([^']+)'/g)].map((m) => m[1]).filter((p) => !/png$/.test(p) && p !== '/i18n-en.js');
const modules = ['/tv.js', '/brand-icons.js', '/air.js', '/i18n.js', '/design.css'];
const assets = [...new Set([...shell, ...modules])];
// Türkçe telefon: i18n-en.js gerekmez; yine de mevcut sürümde yükleniyorsa sayılır (ölçüm gerçeği yansıtsın)
console.log('Arayüz yükü ölçülüyor…');
const t0 = performance.now();
const got = await Promise.all(assets.map((a) => get(a, 'br, gzip')));
const loadMs = performance.now() - t0;
result.web = {
  files: got.map((g) => ({ p: g.p, status: g.status, rawKB: +(g.raw / 1024).toFixed(1), wireKB: +(g.wire / 1024).toFixed(1), enc: g.enc })),
  rawKB: +(got.reduce((s, g) => s + g.raw, 0) / 1024).toFixed(1),
  wireKB: +(got.reduce((s, g) => s + g.wire, 0) / 1024).toFixed(1),
  loadMs: +loadMs.toFixed(0),
};
// Tekrar açılış: tarayıcı ETag'li koşullu istek gönderir (ETag yoksa her şey yeniden iner)
const t1 = performance.now();
const again = await Promise.all(got.map((g) => get(g.p, 'br, gzip', g.etag)));
result.web.repeatWireKB = +(again.reduce((s, g) => s + g.wire, 0) / 1024).toFixed(1);
result.web.repeatMs = +(performance.now() - t1).toFixed(0);

// ---------------------------------------------------------------- bağlantı kurulumu (TLS + WebSocket, challenge'a kadar)
const agent = new https.Agent({ keepAlive: false, maxCachedSessions: 50 });
async function wsOnce() {
  return new Promise((resolve) => {
    const t = performance.now();
    const w = new WebSocket(`wss://${LAN_IP}:${PORT}/ws`, { servername: HOST, ca: CA, agent, headers: { Host: `${HOST}:${PORT}` }, origin: `https://${HOST}:${PORT}` });
    w.on('message', (d) => { if (String(d).includes('challenge')) { const ms = performance.now() - t; w.close(); resolve(ms); } });
    w.on('error', () => resolve(NaN));
    setTimeout(() => { try { w.terminate(); } catch {} resolve(NaN); }, 4000);
  });
}
console.log('Bağlantı süreleri ölçülüyor…');
const times = [];
for (let i = 0; i < 12; i++) { times.push(await wsOnce()); await new Promise((r) => setTimeout(r, 150)); }
const ok = times.filter(Number.isFinite);
result.ws = { firstMs: +ok[0]?.toFixed(1), restAvgMs: +(ok.slice(1).reduce((a, b) => a + b, 0) / Math.max(1, ok.length - 1)).toFixed(1), all: ok.map((x) => +x.toFixed(1)) };

// ---------------------------------------------------------------- soğuk başlangıç (isteğe bağlı: programı kapatıp açar)
if (args.includes('--cold')) {
  console.log('Soğuk başlangıç ölçülüyor (program yeniden başlatılır)…');
  try { execFileSync('powershell', ['-NoProfile', '-Command', 'Get-Process CouchRemote,couch-agent -ErrorAction SilentlyContinue | Stop-Process -Force']); } catch {}
  await new Promise((r) => setTimeout(r, 1500));
  const start = performance.now();
  const child = spawn(path.join(exeDir, 'CouchRemote.exe'), [], { detached: true, stdio: 'ignore' });
  child.unref();
  let readyMs = NaN;
  const net = (await import('node:net')).default;
  for (let t = 0; t < 15000; t += 5) {
    const up = await new Promise((res) => { const s = net.connect(47800, '127.0.0.1'); s.on('connect', () => { s.destroy(); res(true); }); s.on('error', () => res(false)); });
    if (up) { readyMs = performance.now() - start; break; }
    await new Promise((r) => setTimeout(r, 5));
  }
  const lanStart = performance.now();
  const lanOk = await new Promise((res) => { const q = https.request({ host: LAN_IP, port: PORT, path: '/app', servername: HOST, ca: CA, headers: { Host: `${HOST}:${PORT}` } }, (r) => { r.resume(); r.on('end', () => res(r.statusCode === 200)); }); q.on('error', () => res(false)); q.end(); });
  const lanFirstMs = performance.now() - lanStart;
  await new Promise((r) => setTimeout(r, 5000));
  const m = JSON.parse(execFileSync('powershell', ['-NoProfile', '-Command', "$p=Get-Process CouchRemote -ErrorAction SilentlyContinue|Select-Object -First 1; [pscustomobject]@{privateMB=[math]::Round($p.PrivateMemorySize64/1MB,1); workingSetMB=[math]::Round($p.WorkingSet64/1MB,1)}|ConvertTo-Json"], { encoding: 'utf8' }));
  result.cold = { readyMs: +readyMs.toFixed(0), lanFirstMs: lanOk ? +lanFirstMs.toFixed(0) : null, privateMB: m.privateMB, workingSetMB: m.workingSetMB };
}

fs.writeFileSync(path.join(dir, label + '.json'), JSON.stringify(result, null, 2));
console.log(JSON.stringify({ ...result, web: { ...result.web, files: undefined }, idle: { ...result.idle, hotThreads: result.idle.hotThreads } }, null, 1));
