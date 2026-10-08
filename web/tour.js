// İlk açılış turu: ekrandaki öğeleri sırayla ışık halkasıyla gösterir, anlatım kartının üstünde duran karakter hedefe doğru uzanır.
// Kullanım: runTour({ steps, tl, onEnd }). steps: [{ target?: CSS seçici, title, text, before?: () => void }]
// Hedefsiz adım ortada bir karşılama kartı olur. Esc ya da "Atla" turu kapatır.
import { createMascot } from '/mascot.js';

const NS = 'http://www.w3.org/2000/svg';
const SC = 0.8; // karakter ölçeği
const MH = 112; // kartın üstünde karaktere ayrılan yükseklik

export function runTour({ steps, tl, onEnd }) {
  const root = document.createElement('div');
  root.id = 'tour';
  root.setAttribute('role', 'dialog');
  root.setAttribute('aria-modal', 'true');
  root.setAttribute('aria-label', tl('Tanıtım turu'));
  const hole = document.createElement('div');
  hole.className = 'tour-hole';
  const card = document.createElement('div');
  card.className = 'tour-card';
  const svg = document.createElementNS(NS, 'svg');
  svg.setAttribute('class', 'tour-mascot');
  svg.setAttribute('aria-hidden', 'true');
  const layer = document.createElementNS(NS, 'g');
  svg.append(layer);
  root.append(hole, card, svg);
  document.body.append(root);

  let i = -1;
  let ended = false;
  let mascot = null;
  let moveToken = 0;
  const prevFocus = document.activeElement;

  const fitSvg = () => svg.setAttribute('viewBox', `0 0 ${innerWidth} ${innerHeight}`);
  fitSvg();

  function end(done) {
    if (ended) return;
    ended = true;
    moveToken++;
    document.removeEventListener('keydown', onKey, true);
    removeEventListener('resize', onResize);
    root.classList.add('out');
    setTimeout(() => { mascot?.destroy(); root.remove(); }, 260);
    try { prevFocus?.focus?.(); } catch {}
    onEnd?.(done);
  }
  function onKey(e) {
    if (e.key === 'Escape') { e.preventDefault(); end(false); }
  }
  document.addEventListener('keydown', onKey, true);
  const onResize = () => { fitSvg(); const l = place(); moveMascot(l); };
  addEventListener('resize', onResize);

  /** Kartı ve ışık halkasını yerleştirir; kartın piksel konumunu döner (karakter bunun üstünde durur) */
  function place() {
    const s = steps[i];
    const el = s.target ? document.querySelector(s.target) : null;
    const vw = innerWidth, vh = innerHeight;
    const cw = Math.min(vw - 32, 360);
    const ch = card.offsetHeight || 190;
    const left = (vw - cw) / 2;
    let top, tx = null, ty = null;
    if (!el) {
      hole.style.cssText = `left:${vw / 2}px;top:${vh / 2}px;width:0;height:0;border-radius:50%`;
      top = Math.max(MH + 12, (vh - ch) / 2 + MH / 2);
    } else {
      const r = el.getBoundingClientRect();
      const pad = 7;
      hole.style.cssText = `left:${r.left - pad}px;top:${r.top - pad}px;width:${r.width + pad * 2}px;height:${r.height + pad * 2}px;border-radius:${Math.min(22, (r.height + pad * 2) / 2)}px`;
      // Hedef üstteyse kart aşağıda (aradaki boşlukta karakter durur); altaysa kart yukarıda
      const below = r.bottom + pad + 14 + MH + ch < vh - 12;
      top = below ? r.bottom + pad + 14 + MH : Math.max(MH + 12, r.top - pad - 14 - ch);
      tx = r.left + r.width / 2;
      ty = r.top + r.height / 2;
    }
    card.style.left = `${left}px`;
    card.style.top = `${top}px`;
    return { left, top, cw, ch, tx, ty };
  }

  /** Karakteri kartın üst kenarına, hedefin hizasına götürür ve hedefe uzanır */
  async function moveMascot(l, { first = false } = {}) {
    const tok = ++moveToken;
    const s = steps[i];
    const floor = l.top + 3;
    // Hedefin tam üstünde/altında değil, iç tarafında dur: kol çapraz uzanınca işaret ettiği anlaşılır, hedefi de kapatmaz
    const cx = l.left + l.cw / 2;
    const x = l.tx != null ? Math.max(l.left + 34, Math.min(l.left + l.cw - 34, l.tx + (l.tx < cx ? 74 : -74))) : cx;
    if (!mascot) {
      mascot = createMascot(layer, { x: -50, floor, scale: SC });
      mascot.face(1);
    }
    mascot.pose('stand');
    const last = i === steps.length - 1;
    if (first) await mascot.walkTo(x);
    else if (Math.abs(floor - mascot.floor) > 6) await mascot.jumpTo(x, floor, { dur: 560, height: 34 });
    else await mascot.walkTo(x);
    if (tok !== moveToken) return;
    if (last) { mascot.pose('cheer'); mascot.confetti(); }
    else if (l.tx != null) { mascot.aim(l.tx, l.ty); mascot.nudge(); }
    else { mascot.pose('wave'); }
  }

  function render() {
    const s = steps[i];
    const last = i === steps.length - 1;
    card.replaceChildren();
    const h = document.createElement('h3');
    h.textContent = tl(s.title);
    const p = document.createElement('p');
    p.textContent = tl(s.text);
    const dots = document.createElement('div');
    dots.className = 'tour-dots';
    steps.forEach((_, k) => { const d = document.createElement('i'); if (k === i) d.className = 'on'; dots.append(d); });
    const row = document.createElement('div');
    row.className = 'tour-row';
    const skip = document.createElement('button');
    skip.className = 'tour-skip';
    skip.textContent = tl('Atla');
    skip.onclick = () => end(false);
    skip.hidden = last;
    const next = document.createElement('button');
    next.className = 'tour-next';
    next.textContent = tl(last ? 'Başla' : 'İleri');
    next.onclick = () => go(i + 1);
    row.append(skip, next);
    card.append(h, p, dots, row);
    const l = place();
    moveMascot(l, { first: !mascot });
    next.focus({ preventScroll: true });
  }

  function go(n) {
    if (n >= steps.length) return end(true);
    i = n;
    steps[i].before?.();
    card.classList.remove('in');
    // Ekran geçişi (sekme) otursun, sonra ölç
    requestAnimationFrame(() => setTimeout(() => {
      render();
      void card.offsetWidth;
      card.classList.add('in');
    }, steps[i].before ? 120 : 0));
  }
  go(0);
  return { end };
}
