// Arayüz testi (WebKit = iPhone Safari'nin motoru): PC programı gerekmez, önizleme sunucusuna karşı çalışır.
// Denetler: konsol/CSP hatası yok, yatay taşma yok, her sekme ve sayfa açılıyor, açılır pencereler açılıp kapanıyor,
// İngilizce sözlükte eksik metin yok, düğmelerin erişilebilir adı var, kurulum sihirbazı adımları doğru ilerliyor.
// Not: gerçek iPhone değil; yerleşim ve JS hatalarını yakalar, dokunma/Wake Lock/sertifika gibi aygıt davranışlarını değil.
// Çalıştır: npx playwright install webkit (bir kez)  →  node tools/ui-test.mjs
import { spawn } from 'node:child_process';
import { webkit, devices } from 'playwright';

const PORT = Number(process.env.PREVIEW_PORT ?? 47891);
const BASE = `http://127.0.0.1:${PORT}`;
const server = spawn(process.execPath, [new URL('./preview-server.mjs', import.meta.url).pathname.replace(/^\/(\w:)/, '$1')], { env: { ...process.env, PREVIEW_PORT: String(PORT) }, stdio: 'ignore' });
const stop = () => server.kill();
process.on('exit', stop);
for (let i = 0; i < 50; i++) { try { await fetch(BASE + '/preview'); break; } catch { await new Promise((r) => setTimeout(r, 100)); } }

let failed = 0;
const check = (name, ok, detail = '') => {
  if (!ok) failed++;
  console.log(`${ok ? '✓' : '✗'} ${name}${detail ? `  (${detail})` : ''}`);
};

const browser = await webkit.launch();
const VIEWPORTS = [
  ['iPhone SE (küçük)', { ...devices['iPhone SE'] }],
  ['iPhone 15 Pro Max (büyük)', { ...devices['iPhone 15 Pro Max'] }],
  ['iPad (dikey)', { ...devices['iPad (gen 7)'] }],
  ['iPad (yatay)', { ...devices['iPad (gen 7) landscape'] }],
];

/** Sayfada biriken hataları toplar */
async function newPage(ctx) {
  const page = await ctx.newPage();
  page.errors = [];
  page.on('pageerror', (e) => page.errors.push('JS: ' + e.message));
  page.on('console', (m) => { if (m.type() === 'error' && !/Failed to load resource/.test(m.text())) page.errors.push('konsol: ' + m.text()); });
  page.on('response', (r) => { if (r.status() >= 400 && !/.png$/.test(r.url())) page.errors.push(r.status() + ' ' + r.url().replace(BASE, '')); });
  page.on('requestfailed', (r) => page.errors.push('istek: ' + r.url()));
  return page;
}
const overflow = (page) => page.evaluate(() => {
  const w = document.documentElement.clientWidth;
  const bad = [...document.querySelectorAll('body *')].filter((el) => {
    const r = el.getBoundingClientRect();
    const st = getComputedStyle(el);
    return r.width > 0 && r.height > 0 && st.visibility !== 'hidden' && st.position !== 'fixed' && r.right > w + 1 && !el.closest('[hidden], .sheet, #shotView, .scroll, .keys, .chips, .cover-row') && ![...(function* () { for (let p = el.parentElement; p; p = p.parentElement) yield p; })()].some((p) => /auto|scroll/.test(getComputedStyle(p).overflowX));
  }).slice(0, 3).map((el) => el.tagName.toLowerCase() + (el.id ? '#' + el.id : '') + '.' + String(el.className).split(' ')[0]);
  return { scroll: document.documentElement.scrollWidth - w, bad };
});

// ---------------------------------------------------------------- uygulama arayüzü, tüm cihaz boyutları
for (const [label, dev] of VIEWPORTS) {
  const ctx = await browser.newContext({ ...dev, locale: 'tr-TR' });
  const page = await newPage(ctx);
  await page.goto(BASE + '/preview');
  await page.waitForFunction(() => window.__preview, null, { timeout: 5000 });
  await page.evaluate(() => { window.__preview.show('main'); });
  const issues = [];
  for (const view of ['pad', 'kbd', 'remote', 'apps', 'share']) {
    await page.evaluate((v) => window.__preview.showView(v), view);
    await page.waitForTimeout(700);
    const visible = await page.evaluate((v) => !document.getElementById('v-' + v).hidden, view);
    const o = await overflow(page);
    if (!visible) issues.push(`${view} açılmadı`);
    if (o.scroll > 1) issues.push(`${view} taşıyor (${o.scroll}px ${o.bad.join(',')})`);
  }
  check(`${label}: 5 sekme açılıyor ve yatay taşma yok`, issues.length === 0, issues.join('; '));

  // Açılır pencereler: aç, görünür mü, kapat
  const sheets = await page.evaluate(() => [...document.querySelectorAll('.sheet')].map((s) => s.id));
  const sheetIssues = [];
  for (const id of sheets) {
    await page.evaluate((i) => window.__preview.openSheet(i), id);
    await page.waitForTimeout(150);
    const r = await page.evaluate((i) => { const s = document.getElementById(i); const b = s.getBoundingClientRect(); return { hidden: s.hidden, w: b.width, left: b.left, right: b.right, vw: innerWidth }; }, id);
    if (r.hidden || r.w === 0) sheetIssues.push(`${id} görünmedi`);
    else if (r.left < -1 || r.right > r.vw + 1) sheetIssues.push(`${id} ekran dışına taşıyor`);
    await page.keyboard.press('Escape');
    await page.waitForTimeout(100);
    await page.evaluate(() => document.querySelectorAll('.sheet').forEach((s) => (s.hidden = true)));
  }
  check(`${label}: ${sheets.length} açılır pencere açılıp ekrana sığıyor`, sheetIssues.length === 0, sheetIssues.join('; '));
  check(`${label}: konsol/CSP/istek hatası yok`, page.errors.length === 0, page.errors.slice(0, 3).join(' | '));
  await ctx.close();
}

// ---------------------------------------------------------------- ilk açılış turu
for (const [label, dev] of [['iPhone SE', devices['iPhone SE']], ['iPhone 15 Pro Max', devices['iPhone 15 Pro Max']], ['iPad', devices['iPad (gen 7)']]]) {
  const ctx = await browser.newContext({ ...dev, locale: 'tr-TR' });
  const page = await newPage(ctx);
  await page.goto(BASE + '/preview');
  await page.waitForFunction(() => window.__preview);
  await page.evaluate(() => { window.__preview.show('main'); window.__preview.startTour(); });
  const problems = [];
  let steps = 0;
  for (let n = 0; n < 12; n++) {
    await page.waitForSelector('.tour-card.in', { timeout: 3000 }).catch(() => problems.push('kart çıkmadı'));
    await page.waitForTimeout(650);
    const info = await page.evaluate(() => {
      const c = document.querySelector('.tour-card')?.getBoundingClientRect();
      const h = document.querySelector('.tour-hole')?.getBoundingClientRect();
      return c && { top: c.top, bottom: c.bottom, left: c.left, right: c.right, vw: innerWidth, vh: innerHeight, hole: h && { w: h.width, top: h.top, bottom: h.bottom }, title: document.querySelector('.tour-card h3')?.textContent, last: !!document.querySelector('.tour-skip')?.hidden };
    });
    if (!info) { problems.push('kart yok'); break; }
    steps++;
    if (info.top < 0 || info.bottom > info.vh + 1 || info.left < 0 || info.right > info.vw + 1) problems.push(`${info.title}: kart ekran dışında`);
    if (info.hole && info.hole.w > 0 && (info.hole.bottom < 0 || info.hole.top > info.vh)) problems.push(`${info.title}: vurgu ekran dışında`);
    if (info.hole && info.hole.w > 0 && info.hole.top < info.bottom && info.hole.bottom > info.top) problems.push(`${info.title}: kart hedefin üstüne biniyor`);
    await page.click('.tour-next');
    if (info.last) break;
  }
  await page.waitForTimeout(500);
  const after = await page.evaluate(() => ({ gone: !document.getElementById('tour'), done: localStorage.getItem('tourDone') }));
  check(`Tur (${label}): 9 adım, kartlar ekrana sığıyor ve hedefi kapatmıyor, bitince kapanıp kaydediliyor`, steps === 9 && after.gone && after.done === '1' && problems.length === 0, `${steps} adım; ${problems.slice(0, 3).join(' | ')}`);
  check(`Tur (${label}): hata yok`, page.errors.length === 0, page.errors.slice(0, 2).join(' | '));
  await ctx.close();
}
{
  // Esc turu kapatır
  const ctx = await browser.newContext({ ...devices['iPhone 15 Pro Max'], locale: 'tr-TR' });
  const page = await newPage(ctx);
  await page.goto(BASE + '/preview');
  await page.waitForFunction(() => window.__preview);
  await page.evaluate(() => { window.__preview.show('main'); window.__preview.startTour(); });
  await page.waitForSelector('.tour-card.in');
  await page.keyboard.press('Escape');
  await page.waitForTimeout(500);
  check('Tur: Esc ile kapanıyor', await page.evaluate(() => !document.getElementById('tour')));
  await ctx.close();
}

// ---------------------------------------------------------------- tema ve vurgu rengi
{
  const ctx = await browser.newContext({ ...devices['iPhone 15 Pro Max'], locale: 'tr-TR' });
  const page = await newPage(ctx);
  await page.goto(BASE + '/preview');
  await page.waitForFunction(() => window.__preview);
  await page.evaluate(() => window.__preview.show('main'));
  const out = [];
  for (const theme of ['light', 'dark']) {
    for (const accent of ['blue', 'purple', 'pink', 'orange', 'green', 'graphite']) {
      await page.evaluate(([t, a]) => { document.documentElement.dataset.mode = t; document.documentElement.dataset.accent = a; }, [theme, accent]);
      await page.waitForTimeout(40);
      const bg = await page.evaluate(() => getComputedStyle(document.body).backgroundColor);
      const lum = (c) => { const m = c.match(/\d+/g).map(Number); return (0.2126 * m[0] + 0.7152 * m[1] + 0.0722 * m[2]) / 255; };
      if (theme === 'light' && lum(bg) < 0.6) out.push(`açık tema koyu (${accent})`);
      if (theme === 'dark' && lum(bg) > 0.4) out.push(`koyu tema açık (${accent})`);
    }
  }
  check('Tema: açık/koyu × 6 vurgu rengi doğru arka plan veriyor', out.length === 0, out.join('; '));
  check('Tema geçişlerinde hata yok', page.errors.length === 0, page.errors.slice(0, 2).join(' | '));
  await ctx.close();
}

// ---------------------------------------------------------------- erişilebilirlik: adsız düğme/girdi
{
  const ctx = await browser.newContext({ ...devices['iPhone 15 Pro Max'], locale: 'tr-TR' });
  const page = await newPage(ctx);
  await page.goto(BASE + '/preview');
  await page.waitForFunction(() => window.__preview);
  await page.evaluate(() => window.__preview.show('main'));
  const nameless = await page.evaluate(() => {
    const out = [];
    for (const el of document.querySelectorAll('button, input, select, [role=button], [role=switch], a[href]')) {
      if (el.type === 'hidden' || el.closest('[hidden]') || getComputedStyle(el).display === 'none') continue;
      const name = (el.getAttribute('aria-label') || el.getAttribute('aria-labelledby') || el.textContent || el.title || el.placeholder || '').trim()
        || el.closest('label')?.textContent.trim() || (el.id && document.querySelector(`label[for="${el.id}"]`)?.textContent.trim()) || '';
      if (!name) out.push(el.tagName.toLowerCase() + (el.id ? '#' + el.id : '') + (el.className ? '.' + String(el.className).split(' ')[0] : ''));
    }
    return out;
  });
  check('Erişilebilirlik: tüm düğme ve girdilerin okunabilir adı var', nameless.length === 0, nameless.slice(0, 6).join(', '));
  const tiny = await page.evaluate(() => {
    document.querySelectorAll('#tabs button, #btns button').forEach(() => {});
    return [...document.querySelectorAll('#tabs button')].filter((b) => { const r = b.getBoundingClientRect(); return r.width < 44 || r.height < 44; }).length;
  });
  check('Erişilebilirlik: alt sekme düğmeleri en az 44 px', tiny === 0, `${tiny} küçük`);
  await ctx.close();
}

// ---------------------------------------------------------------- İngilizce sözlük eksikleri
{
  const ctx = await browser.newContext({ ...devices['iPhone 15 Pro Max'], locale: 'en-US' });
  const page = await newPage(ctx);
  await page.addInitScript(() => { try { localStorage.setItem('lang', 'en'); } catch {} });
  await page.goto(BASE + '/preview');
  await page.waitForFunction(() => window.__preview);
  for (const p of ['/preview', '/setup.html', '/admin.html']) {
    const miss = await page.evaluate(async (u) => (await import('/i18n.js')).missingInPage(u), p);
    const noise = (x) => /^[A-Z0-9][A-Z0-9 \-:]*$/.test(x);
    const miss2 = miss.filter((x) => !noise(x));
    check(`İngilizce: ${p} içinde çevrilmemiş metin yok`, miss2.length === 0, process.env.MISS_ALL ? JSON.stringify(miss2, null, 1) : miss2.slice(0, 4).map((s) => s.slice(0, 40)).join(' | ') + (miss2.length > 4 ? ` … +${miss2.length - 4}` : ''));
  }
  await ctx.close();
}

// ---------------------------------------------------------------- kurulum sihirbazı ve yönetim sayfası
for (const [label, ua] of [['iOS', undefined], ['Android', 'Mozilla/5.0 (Linux; Android 14; Pixel 8) AppleWebKit/537.36 Chrome/120 Mobile Safari/537.36']]) {
  const ctx = await browser.newContext({ ...devices['iPhone 15 Pro Max'], ...(ua ? { userAgent: ua } : {}), locale: 'tr-TR' });
  const page = await newPage(ctx);
  await page.goto(BASE + '/setup.html?c=abc');
  await page.waitForTimeout(600);
  const sel = label === 'iOS' ? '#iosSteps' : '#androidSteps';
  const first = await page.evaluate((s) => [...document.querySelectorAll(s + ' .step')].map((x) => x.className.replace('step', '').trim()), sel);
  check(`Kurulum sayfası (${label}): ilk adım vurgulu, sonrakiler soluk`, first[0] === 'current' && first.slice(1).every((c) => c === 'future'), first.join(','));
  const prog = await page.evaluate(() => document.getElementById('progText').textContent);
  check(`Kurulum sayfası (${label}): ilerleme "Adım 1 / 3"`, prog === 'Adım 1 / 3', prog);
  await page.waitForFunction(() => document.getElementById('stage').dataset.pose === 'point', null, { timeout: 8000 }).catch(() => {});
  const pose = await page.evaluate(() => document.getElementById('stage').dataset.pose);
  check(`Kurulum sayfası (${label}): kanepedeki karakter ilk adımı gösteriyor`, pose === 'point', pose);
  check(`Kurulum sayfası (${label}): yatay taşma yok`, (await overflow(page)).scroll <= 1);
  check(`Kurulum sayfası (${label}): hata yok`, page.errors.filter((e) => !/api\/hello|pair\/touch|ca\./.test(e)).length === 0, page.errors.slice(0, 2).join(' | '));
  await ctx.close();
}
{
  const ctx = await browser.newContext({ ...devices['iPhone 15 Pro Max'], locale: 'tr-TR' });
  const page = await newPage(ctx);
  await page.goto(BASE + '/admin.html');
  await page.waitForTimeout(1500);
  const txt = await page.evaluate(() => document.body.textContent);
  check('Yönetim sayfası: sahte durumla açılıyor, PIN görünüyor, yapımcı yazısı var', /4\s*8\s*2\s*9\s*1\s*5/.test(txt) && /WOOP WOOP/.test(txt));
  check('Yönetim sayfası: yatay taşma yok', (await overflow(page)).scroll <= 1);
  await ctx.close();
}

await browser.close();
stop();
console.log(failed ? `\n${failed} test BAŞARISIZ\n` : '\nTüm arayüz testleri geçti ✓\n');
process.exit(failed ? 1 : 0);
