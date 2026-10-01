// CouchRemote çok dilli arayüz (Türkçe + İngilizce).
// Yöntem: anahtar = Türkçe kaynak metin; İngilizce karşılığı i18n-en.js sözlüğünde. Türkçede sözlük hiç yüklenmez.
//   - Dinamik metinler:  t('Bağlanıyor…')  veya  t('{n} telefon bağlı', { n: 2 })
//   - Statik HTML: translateDom() sayfadaki metin düğümlerini ve aria-label / placeholder / title / alt özniteliklerini çevirir.
//     İçinde <b> gibi etiketler olan karışık bloklar tek bir birim sayılır (anahtar = normalleştirilmiş innerHTML).
//   - Sözlükte olmayan metin olduğu gibi (Türkçe) kalır; eksikler için missing() kullan (geliştirme yardımcısı).
// Dil: ayarlardan seçilen ('tr' | 'en') yoksa tarayıcı/telefon diline göre (tr* → Türkçe, diğerleri → İngilizce).

const STORE = 'lang';

function detect() {
  let pref = 'auto';
  try {
    pref = localStorage.getItem(STORE) || 'auto';
  } catch {
    // depolama kapalı: otomatik
  }
  if (pref === 'tr' || pref === 'en') return pref;
  const nav = (navigator.languages?.[0] || navigator.language || 'tr').toLowerCase();
  return nav.startsWith('tr') ? 'tr' : 'en';
}

export const lang = detect();
document.documentElement.lang = lang;

let EN = {};
/** Sayfa başlamadan önce beklenir: İngilizce sözlük yüklenir (Türkçede anında çözülür) */
export const ready = lang === 'en' ? import('/i18n-en.js').then((m) => (EN = m.EN)) : Promise.resolve();

/** Seçili dil tercihi: 'auto' | 'tr' | 'en' */
export function langPref() {
  try {
    return localStorage.getItem(STORE) || 'auto';
  } catch {
    return 'auto';
  }
}
export function setLangPref(v) {
  try {
    if (v === 'auto') localStorage.removeItem(STORE);
    else localStorage.setItem(STORE, v);
  } catch {
    // depolama kapalı
  }
}

const fill = (s, p) => (p ? s.replace(/\{(\w+)\}/g, (_, k) => (k in p ? String(p[k]) : `{${k}}`)) : s);

// "{ad} penceresi bulunamadı" gibi parçalı anahtarlar: sunucudan hazır birleştirilmiş metin gelse de eşleşir
let patterns = null;
function fromPattern(key) {
  patterns ??= Object.entries(EN)
    .filter(([k]) => k.includes('{'))
    .map(([k, v]) => {
      const names = [];
      const src = k.replace(/[.*+?^$()|[\]\\]/g, '\\$&').replace(/\{(\w+)\}/g, (_, n) => (names.push(n), '(.+?)'));
      return { re: new RegExp('^' + src + '$'), names, v };
    });
  for (const { re, names, v } of patterns) {
    const m = re.exec(key);
    if (!m) continue;
    const p = Object.fromEntries(names.map((n, i) => [n, m[i + 1]]));
    return typeof v === 'function' ? v(p) : fill(v, p);
  }
  return null;
}

/** Metni çevirir. Sözlük değeri işlev olabilir (çoğul gibi durumlar için): (params) => metin */
export function t(key, params) {
  if (lang === 'tr') return fill(key, params);
  const v = EN[key];
  if (typeof v === 'function') return v(params ?? {});
  if (v === undefined && !params) return fromPattern(key) ?? key;
  return fill(v ?? key, params);
}

// ------------------------------------------------------------ DOM çevirisi
const SKIP = new Set(['SCRIPT', 'STYLE', 'svg', 'SVG', 'CODE', 'PRE', 'TEXTAREA']);
const ATTRS = ['aria-label', 'placeholder', 'title', 'alt'];
const norm = (s) => s.replace(/\s+/g, ' ').trim();

function isMixed(el) {
  let text = false;
  let elem = false;
  for (const n of el.childNodes) {
    if (n.nodeType === 3 && n.textContent.trim()) text = true;
    else if (n.nodeType === 1 && n.nodeName.toLowerCase() !== 'svg') elem = true; // simge + metin karışık sayılmaz
  }
  return text && elem;
}

/** Her çevrilebilir birim için visit(anahtar, uygula) çağrılır; birim sözlükte varsa true döner */
function walk(root, visit) {
  const go = (el) => {
    if (el.hasAttribute?.('data-no-i18n')) return;
    for (const a of ATTRS) {
      const v = el.getAttribute?.(a);
      if (v && v.trim()) visit(norm(v), (en) => el.setAttribute(a, en));
    }
    if (SKIP.has(el.nodeName)) return; // betik, stil, kod, textarea içeriği çevrilmez
    if (isMixed(el)) {
      // Karışık blok tek birim: sözlükte varsa içeriği topluca değiştirilir, parçalara inilmez
      if (visit(norm(el.innerHTML), (en) => (el.innerHTML = en))) return;
    }
    for (const n of [...el.childNodes]) {
      if (n.nodeType === 3) {
        const raw = n.textContent;
        const key = norm(raw);
        if (key && /[A-Za-zÇĞİÖŞÜçğıöşü]/.test(key)) {
          visit(key, (en) => {
            n.textContent = raw.replace(key, en).replace(/^\s*$/, en);
            // kenar boşlukları korunur
            const lead = raw.match(/^\s*/)[0];
            const trail = raw.match(/\s*$/)[0];
            n.textContent = lead + en + trail;
          });
        }
      } else if (n.nodeType === 1) go(n);
    }
  };
  go(root);
}

/** Statik içeriği çevirir (İngilizcede). `root` verilmezse tüm sayfa. */
export function translateDom(root = document.body) {
  if (lang === 'tr') return;
  document.title = t(document.title);
  walk(root, (key, apply) => {
    const en = EN[key] ?? fromPattern(key); // {host} gibi değişkenli anahtarlar da eşleşir
    if (typeof en !== 'string') return false;
    apply(en);
    return true;
  });
}

/** Geliştirme yardımcısı: İngilizce sözlükte karşılığı olmayan metinler. Konsolda: (await import('/i18n.js')).missing() */
export function missing(root = document.body) {
  const out = new Set();
  walk(root, (key) => {
    if (key in EN || fromPattern(key) !== null) return true;
    if (key.length > 1) out.add(key);
    return false;
  });
  return [...out];
}

/** Geliştirme yardımcısı: bir sayfanın ORİJİNAL (Türkçe) HTML'ini ayrıştırıp sözlükte olmayan metinleri döndürür.
 *  Konsolda (İngilizce modda): await (await import('/i18n.js')).missingInPage('/preview') */
export async function missingInPage(url = location.pathname) {
  await ready;
  const html = await (await fetch(url, { cache: 'no-store' })).text();
  const doc = new DOMParser().parseFromString(html, 'text/html');
  const out = new Set();
  walk(doc.body, (key) => {
    if (key in EN || fromPattern(key) !== null) return true;
    if (key.length > 1 && !/^[\W\d_]+$/.test(key)) out.add(key);
    return false;
  });
  return [...out];
}
