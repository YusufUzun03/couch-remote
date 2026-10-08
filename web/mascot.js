// Kanepedeki arkadaş: eklemli (iskelet) bir karakter. Kurulum sayfasında ve uygulama turunda kullanılır.
//
// Neden iskelet: önceki sürüm iki ayrı çizim arasında geçiş yapıyordu. Burada tek bir karakter var; her eklem (kalça, diz, omuz, dirsek, kafa)
// kendi açısına yaylı bir hareketle gider, böylece kalkma, yürüme ve el kaldırma kesintisiz akar. Yürüyüş ve nefes gibi hareketler üstüne eklenir.
// Hepsi SVG + requestAnimationFrame; ek kütüphane yok.
const NS = 'http://www.w3.org/2000/svg';
const rad = (d) => (d * Math.PI) / 180;

// Uzuv uzunlukları (SVG birimi)
const THIGH = 26, SHIN = 26, FOOT = 7, TORSO = 42, UPPER = 19, FORE = 17;
export const STAND_HIP = THIGH + SHIN + FOOT; // zeminden kalçaya yükseklik

const css = (el, props) => { Object.assign(el.style, props); return el; };
function mk(tag, attrs, parent) {
  const e = document.createElementNS(NS, tag);
  for (const [k, v] of Object.entries(attrs || {})) e.setAttribute(k, v);
  parent?.append(e);
  return e;
}

/** Yaylı değer: hedefe yumuşakça yaklaşır; zeta < 1 ise hafif aşar (canlı his) */
class Spring {
  constructor(x, omega = 12, zeta = 0.8) { this.x = x; this.v = 0; this.t = x; this.w = omega; this.z = zeta; }
  step(dt) {
    // yarı örtük Euler, küçük adımlarla kararlı
    const n = Math.ceil(dt / 0.008);
    const h = dt / n;
    for (let i = 0; i < n; i++) {
      const a = this.w * this.w * (this.t - this.x) - 2 * this.z * this.w * this.v;
      this.v += a * h;
      this.x += this.v * h;
    }
  }
  snap(v) { this.x = this.t = v; this.v = 0; }
}

// Pozlar: eklem açıları (derece). 0 = uzuv aşağı bakar; eksi = karakterin sağına (ekranda sağa), artı = sola.
const BASE = { tilt: 0, torso: 0, head: 0, lookX: 0, lookY: 0, thighA: 0, kneeA: 0, thighB: 0, kneeB: 0, shA: 8, elA: 6, shB: -8, elB: -6 };
export const POSES = {
  stand: {},
  sit: { torso: -3, thighA: -88, kneeA: 88, thighB: -84, kneeB: 84, shA: 4, elA: -4, shB: -44, elB: -78, head: 6, lookX: 0.5, lookY: 0.5 },
  point: { shA: 44, elA: -10, shB: -6, elB: -4, head: -5, lookX: -0.5, lookY: 0.7, torso: -2 },
  phone: { shA: 96, elA: -34, shB: -8, elB: -4, head: -4, lookX: -0.8, lookY: -0.1 },
  shield: { shA: 98, elA: -30, shB: -12, elB: -6, head: -3, lookX: -0.7, lookY: -0.1 },
  cheer: { shA: 152, elA: 14, shB: -152, elB: -14, head: 0, lookX: 0, lookY: -0.4 },
  wave: { shA: 6, elA: 4, shB: -150, elB: -30, head: 3, lookX: 0.3, lookY: 0 },
};
const HOLD_PROPS = ['phone', 'shield'];

/**
 * Karakteri `parent` (bir SVG grubu) içine kurar.
 * opts: { x, floor, scale, seat: {x, y}, colors }
 */
export function createMascot(parent, opts = {}) {
  const S = opts.scale ?? 1;
  const colors = { skin: '#f2c7a3', hair: '#2c2a33', shirt: 'var(--accent, #0a84ff)', pants: 'var(--mc-pants, #474c63)', shoe: 'var(--mc-shoe, #ffffff)', ...opts.colors };
  const fill = (e, c) => css(e, { fill: c });
  const stroke = (e, c, w) => css(e, { stroke: c, strokeWidth: w, strokeLinecap: 'round', strokeLinejoin: 'round', fill: 'none' });

  const root = mk('g', {}, parent);
  const body = mk('g', {}, root);

  // ---- bacaklar (arkadaki önce)
  function leg(offsetX) {
    const hip = mk('g', { transform: `translate(${offsetX},0)` }, body);
    const thigh = mk('g', {}, hip);
    stroke(mk('line', { x1: 0, y1: 0, x2: 0, y2: THIGH }, thigh), colors.pants, 11);
    const knee = mk('g', { transform: `translate(0,${THIGH})` }, thigh);
    stroke(mk('line', { x1: 0, y1: 0, x2: 0, y2: SHIN }, knee), colors.pants, 10.5);
    const ankle = mk('g', { transform: `translate(0,${SHIN})` }, knee);
    fill(mk('rect', { x: -6, y: -1, width: 15, height: 8, rx: 4 }, ankle), colors.shoe);
    return { hip, thigh, knee, ankle };
  }
  const legB = leg(5);
  const legA = leg(-5);

  // ---- gövde
  const torso = mk('g', {}, body);
  const torsoInner = mk('g', {}, torso);
  fill(mk('rect', { x: -15, y: -TORSO, width: 30, height: TORSO + 6, rx: 14 }, torsoInner), colors.shirt);

  // ---- kollar (omuz gövdenin üstünde)
  function arm(sx) {
    const sh = mk('g', { transform: `translate(${sx},${-TORSO + 7})` }, torso);
    stroke(mk('line', { x1: 0, y1: 0, x2: 0, y2: UPPER }, sh), colors.shirt, 9);
    const el = mk('g', { transform: `translate(0,${UPPER})` }, sh);
    stroke(mk('line', { x1: 0, y1: 0, x2: 0, y2: FORE }, el), colors.shirt, 8.5);
    const hand = mk('g', { transform: `translate(0,${FORE + 1})` }, el);
    fill(mk('circle', { r: 5 }, hand), colors.skin);
    return { sh, el, hand };
  }
  const armB = arm(13);
  const armA = arm(-13);

  // ---- elde tutulan nesneler (elin dönüşünü izler, dik kalır). Her iki el için ayrı kopya.
  function makeProps(hand) {
    const slot = mk('g', {}, hand);
    const upright = mk('g', {}, slot);
    const phone = mk('g', {}, upright);
    fill(mk('rect', { x: -11, y: -19, width: 22, height: 38, rx: 5.5 }, phone), '#111');
    css(phone.firstChild, { stroke: 'var(--accent, #0a84ff)', strokeWidth: 1.4 });
    fill(mk('rect', { x: -8.5, y: -15.5, width: 17, height: 31, rx: 3 }, phone), 'var(--accent, #0a84ff)');
    const gear = mk('g', {}, phone);
    stroke(mk('circle', { r: 3.6 }, gear), '#fff', 1.8);
    stroke(mk('path', { d: 'M0-7.2v2.4M0 4.8v2.4M-7.2 0h2.4M4.8 0h2.4' }, gear), '#fff', 1.8);
    const shield = mk('g', {}, upright);
    fill(mk('path', { d: 'M0-20l15 5.5v13c0 10-6.5 16.5-15 19.5-8.5-3-15-9.5-15-19.5v-13z' }, shield), 'var(--green, #34c759)');
    stroke(mk('path', { d: 'M-7.5-1l5 5 9-10' }, shield), '#fff', 3);
    return { upright, phone, shield };
  }
  const propsA = makeProps(armA.hand);
  const propsB = makeProps(armB.hand);
  const OFFS = { phone: [-2, -18], shield: [-2, -20] };

  // ---- baş
  const head = mk('g', { transform: `translate(0,${-TORSO - 12})` }, torso);
  fill(mk('circle', { r: 14.8 }, head), colors.skin);
  const hair = mk('path', { d: 'M-15.2-1.6a15.4 15.4 0 0 1 30.4 0c-7.4-4.8-19.8-6-30.4 0z' }, head);
  fill(hair, colors.hair);
  const eyes = mk('g', {}, head);
  const eyeL = fill(mk('circle', { cx: -5, cy: 2, r: 1.7 }, eyes), colors.hair);
  const eyeR = fill(mk('circle', { cx: 5, cy: 2, r: 1.7 }, eyes), colors.hair);
  const smile = mk('path', { d: 'M-4.5 7.6q4.5 3.4 9 0' }, head);
  stroke(smile, colors.hair, 1.8);
  for (const e of [eyeL, eyeR]) css(e, { transformBox: 'fill-box', transformOrigin: 'center' });

  // ---- durum
  const ch = {
    hipX: new Spring(opts.x ?? 100, 9, 1),
    hipY: new Spring((opts.floor ?? 176) - STAND_HIP, 14, 0.7),
    tilt: new Spring(0, 11, 0.7),
    torso: new Spring(0, 12, 0.65),
    head: new Spring(0, 13, 0.6),
    lookX: new Spring(0, 14, 0.8),
    lookY: new Spring(0, 14, 0.8),
    thighA: new Spring(0, 13, 0.72), kneeA: new Spring(0, 13, 0.72),
    thighB: new Spring(0, 13, 0.72), kneeB: new Spring(0, 13, 0.72),
    shA: new Spring(BASE.shA, 15, 0.52), elA: new Spring(BASE.elA, 15, 0.55),
    shB: new Spring(BASE.shB, 15, 0.52), elB: new Spring(BASE.elB, 15, 0.55),
    phoneA: new Spring(0, 16, 0.55), shieldA: new Spring(0, 16, 0.55), phoneB: new Spring(0, 16, 0.55), shieldB: new Spring(0, 16, 0.55),
    squash: new Spring(0, 18, 0.45),
  };
  const st = {
    floor: opts.floor ?? 176,
    standing: true, // false = kanepede oturuyor
    walkTo: null,
    walkPhase: 0,
    vx: 0,
    hop: 0, // sürekli zıplama (kutlama) genliği
    hopT: 0,
    jump: null,
    time: 0,
    blinkAt: 2.5,
    blink: 0,
    dir: 1,
    reduced: matchMedia('(prefers-reduced-motion: reduce)').matches,
  };
  let alive = true, last = 0, raf = 0;

  function target(name) { return { ...BASE, ...POSES[name] }; }
  function applyPose(name, { instant = false } = {}) {
    const p = target(name);
    for (const k of Object.keys(BASE)) if (ch[k]) ch[k].t = p[k];
    ch.thighA.t = p.thighA; ch.kneeA.t = p.kneeA; ch.thighB.t = p.thighB; ch.kneeB.t = p.kneeB;
    ch.torso.t = p.torso; ch.tilt.t = p.tilt;
    if (instant || st.reduced) for (const k of Object.keys(ch)) ch[k].snap(ch[k].t);
  }

  // ---- çizim
  function render() {
    const hx = ch.hipX.x;
    const sq = ch.squash.x; // + = ezilme
    root.setAttribute('transform', `translate(${hx.toFixed(2)},${(ch.hipY.x + (st.hopY || 0)).toFixed(2)}) scale(${S})`);
    body.setAttribute('transform', `rotate(${ch.tilt.x.toFixed(2)}) scale(${(1 + sq * 0.05).toFixed(3)},${(1 - sq * 0.06).toFixed(3)})`);
    const walk = st.walkPhase;
    const wl = st.walkAmt || 0; // 0..1 yürüme şiddeti
    // yürüme: bacaklar sırayla kalkıp ilerler
    const swA = Math.sin(walk), swB = Math.sin(walk + Math.PI);
    const lift = (s) => Math.max(0, Math.cos(s));
    const tA = ch.thighA.x - wl * st.dir * 20 * swA;
    const kA = ch.kneeA.x + wl * 30 * lift(walk);
    const tB = ch.thighB.x - wl * st.dir * 20 * swB;
    const kB = ch.kneeB.x + wl * 30 * lift(walk + Math.PI);
    legA.thigh.setAttribute('transform', `rotate(${tA.toFixed(2)})`);
    legA.knee.setAttribute('transform', `translate(0,${THIGH}) rotate(${kA.toFixed(2)})`);
    legA.ankle.setAttribute('transform', `translate(0,${SHIN}) rotate(${(-(tA + kA)).toFixed(2)})`);
    legB.thigh.setAttribute('transform', `rotate(${tB.toFixed(2)})`);
    legB.knee.setAttribute('transform', `translate(0,${THIGH}) rotate(${kB.toFixed(2)})`);
    legB.ankle.setAttribute('transform', `translate(0,${SHIN}) rotate(${(-(tB + kB)).toFixed(2)})`);
    const breathe = Math.sin(st.time * 2.1) * 0.7;
    const bob = wl * Math.abs(Math.sin(walk)) * -1.8;
    torso.setAttribute('transform', `translate(0,${(bob + breathe * 0.4).toFixed(2)}) rotate(${(ch.torso.x + wl * st.dir * 4).toFixed(2)})`);
    torsoInner.setAttribute('transform', `scale(1,${(1 + breathe * 0.004).toFixed(4)})`);
    const swing = wl * 16;
    const sA = ch.shA.x + swing * swB * 0.5, sB = ch.shB.x + swing * swA * 0.5;
    armA.sh.setAttribute('transform', `translate(-13,${-TORSO + 7}) rotate(${sA.toFixed(2)})`);
    armA.el.setAttribute('transform', `translate(0,${UPPER}) rotate(${ch.elA.x.toFixed(2)})`);
    armB.sh.setAttribute('transform', `translate(13,${-TORSO + 7}) rotate(${sB.toFixed(2)})`);
    armB.el.setAttribute('transform', `translate(0,${UPPER}) rotate(${(ch.elB.x + (st.wave || 0)).toFixed(2)})`);
    // nesneler: elin toplam dönüşünü tersine çevirerek dik durur
    const totalA = ch.torso.x + ch.tilt.x + sA + ch.elA.x;
    const totalB = ch.torso.x + ch.tilt.x + sB + ch.elB.x + (st.wave || 0);
    propsA.upright.setAttribute('transform', `rotate(${(-totalA).toFixed(2)})`);
    propsB.upright.setAttribute('transform', `rotate(${(-totalB).toFixed(2)})`);
    for (const [P, side, sc] of [[propsA, 'A', 1], [propsB, 'B', 0.62]]) {
      for (const n of HOLD_PROPS) {
        const k = Math.max(0, ch[n + side].x);
        P[n].style.opacity = String(Math.min(1, k * 1.4));
        P[n].setAttribute('transform', `translate(${OFFS[n][0] * sc},${OFFS[n][1] * sc}) scale(${((0.15 + 0.85 * Math.min(1.12, k)) * sc).toFixed(3)})`);
      }
    }
    head.setAttribute('transform', `translate(0,${-TORSO - 12}) rotate(${(ch.head.x + Math.sin(st.time * 0.9) * 1.6).toFixed(2)})`);
    const lx = ch.lookX.x, ly = ch.lookY.x;
    eyes.setAttribute('transform', `translate(${(lx * 2.6).toFixed(2)},${(ly * 1.6).toFixed(2)})`);
    smile.setAttribute('transform', `translate(${(lx * 1.6).toFixed(2)},0)`);
    const b = st.blink;
    for (const e of [eyeL, eyeR]) e.style.transform = `scaleY(${b > 0 ? 0.12 : 1})`;
    hair.setAttribute('transform', `translate(${(lx * 0.8).toFixed(2)},0)`);
  }

  function frame(now) {
    if (!alive) return;
    raf = requestAnimationFrame(frame);
    if (document.hidden) { last = 0; return; }
    const dt = Math.min(0.05, last ? (now - last) / 1000 : 0.016);
    last = now;
    st.time += dt;
    // göz kırpma
    if (st.time > st.blinkAt) { st.blink = 0.14; st.blinkAt = st.time + 2.4 + Math.random() * 3; }
    if (st.blink > 0) st.blink -= dt;

    // yürüme hedefi
    if (st.walkTo != null && !st.jump) {
      const dx = st.walkTo - ch.hipX.x;
      const want = Math.sign(dx) * Math.min(150, Math.abs(dx) * 3.2 + 8);
      st.vx += (want - st.vx) * Math.min(1, dt * 7);
      if (Math.abs(dx) < 1.2 && Math.abs(st.vx) < 6) { ch.hipX.snap(st.walkTo); st.walkTo = null; st.vx = 0; st._arrive?.(); st._arrive = null; }
      else { ch.hipX.x += st.vx * dt; ch.hipX.t = ch.hipX.x; ch.hipX.v = 0; }
      if (Math.abs(st.vx) > 4) st.dir = Math.sign(st.vx);
    } else if (!st.jump) st.vx *= 0.8;
    const speed = Math.abs(st.vx);
    st.walkAmt = Math.min(1, speed / 70) * (st.standing ? 1 : 0);
    st.walkPhase += dt * (3 + speed * 0.07) * (st.walkAmt > 0.05 ? 1 : 0);
    ch.tilt.t = (st.tiltBase || 0) + st.dir * Math.min(1, speed / 90) * 7;
    if (st.walkAmt < 0.05) st.walkPhase = Math.round(st.walkPhase / Math.PI) * Math.PI * 0 + st.walkPhase * 0.9; // dururken faz sakinleşir

    // zıplama (atlama)
    st.hopY = 0;
    if (st.jump) {
      const j = st.jump;
      const u = Math.min(1, (performance.now() - j.t0) / j.dur);
      const e = u < 0.5 ? 2 * u * u : 1 - Math.pow(-2 * u + 2, 2) / 2;
      ch.hipX.x = ch.hipX.t = j.x0 + (j.x1 - j.x0) * e;
      const base = j.y0 + (j.y1 - j.y0) * e;
      ch.hipY.x = ch.hipY.t = base - Math.sin(Math.PI * u) * j.h;
      ch.hipY.v = 0;
      ch.thighA.t = ch.thighB.t = -26 * Math.sin(Math.PI * u); ch.kneeA.t = ch.kneeB.t = 44 * Math.sin(Math.PI * u);
      if (u >= 1) { st.jump = null; ch.squash.x = 1.2; ch.squash.t = 0; ch.thighA.t = ch.thighB.t = ch.kneeA.t = ch.kneeB.t = 0; st._land?.(); st._land = null; }
    }
    if (st.hop > 0 && !st.reduced) {
      st.hopT += dt;
      st.hopY = -Math.abs(Math.sin(st.hopT * 5.2)) * st.hop;
      ch.squash.t = Math.abs(Math.sin(st.hopT * 5.2)) < 0.12 ? 0.6 : -0.2;
    }
    if (st.waving && !st.reduced) st.wave = Math.sin(st.time * 9) * 22; else st.wave = 0;

    for (const k of Object.keys(ch)) {
      if (k === 'hipX' && (st.walkTo != null || st.jump)) continue;
      if (st.jump && (k === 'hipY' || k === 'thighA' || k === 'thighB' || k === 'kneeA' || k === 'kneeB')) continue;
      ch[k].step(dt);
    }
    render();
  }

  // ---- genel API
  const sleep = (ms) => new Promise((r) => setTimeout(r, st.reduced ? 0 : ms));
  const api = {
    root,
    /** Duran bir poz (kollar, kafa, nesne). */
    pose(name, { hold = null, hand = 'A', instant = false } = {}) {
      applyPose(name, { instant });
      api.hold(hold, hand);
      st.waving = name === 'wave';
      if (name === 'cheer') { st.hop = 9; st.hopT = 0; } else st.hop = 0;
      api.current = name;
    },
    /** Elde nesne tut: 'phone' | 'shield' | null. hand: 'A' (ekranda sol) ya da 'B' */
    hold(prop, hand = 'A') {
      for (const n of HOLD_PROPS) for (const h of ['A', 'B']) ch[n + h].t = n === prop && h === hand ? 1 : 0;
    },
    /** Kanepeye otur (anında ya da kısa bir yumuşak geçişle) */
    sitAt(x, seatY, { instant = false } = {}) {
      st.standing = false; st.walkTo = null; st.vx = 0;
      ch.hipX.t = x; ch.hipY.t = seatY;
      st.tiltBase = 0;
      applyPose('sit', { instant });
      if (instant) { ch.hipX.snap(x); ch.hipY.snap(seatY); }
      api.hold('phone', 'B');
      if (opts.front) parent.append(root);
      api.current = 'sit';
    },
    /** Yürüyerek kanepeye dön ve otur */
    async sitDown(x, seatY) {
      await api.walkTo(x);
      st.dir = 1;
      api.hold(null);
      await sleep(180);
      if (opts.front) parent.append(root);
      api.sitAt(x, seatY);
      await sleep(500);
    },
    /** Kalk: önce öne eğil, sonra doğrul */
    async standUp() {
      if (opts.front) opts.front.append(root); // kalkınca kolçağın önünde yürür
      api.hold(null);
      if (st.reduced) { st.standing = true; ch.hipY.t = st.floor - STAND_HIP; applyPose('stand', { instant: true }); return; }
      st.tiltBase = 11; ch.shA.t = 34; ch.shB.t = -34; ch.elA.t = -14; ch.elB.t = 14; // ellerle itiş
      await sleep(260);
      st.standing = true;
      ch.hipY.w = 11; ch.hipY.z = 0.62;
      ch.hipY.t = st.floor - STAND_HIP;
      applyPose('stand');
      st.tiltBase = -3; // doğrulurken hafif geriye gerilir
      await sleep(260);
      st.tiltBase = 0;
      await sleep(240);
    },
    walkTo(x) {
      return new Promise((res) => {
        if (st.reduced) { ch.hipX.snap(x); return res(); }
        st.walkTo = x; st._arrive = res;
      });
    },
    /** Farklı bir yere (ya da zemine) zıplayarak geç */
    jumpTo(x, floor, { dur = 520, height = 26 } = {}) {
      return new Promise((res) => {
        const y1 = floor - STAND_HIP;
        if (st.reduced) { ch.hipX.snap(x); ch.hipY.snap(y1); st.floor = floor; return res(); }
        st.floor = floor; st.walkTo = null; st.vx = 0;
        st.dir = Math.sign(x - ch.hipX.x) || st.dir;
        // çömelip fırla
        ch.squash.t = 1; ch.hipY.t = ch.hipY.x + 6;
        setTimeout(() => {
          st.jump = { t0: performance.now(), dur, x0: ch.hipX.x, x1: x, y0: ch.hipY.x, y1, h: height };
          st._land = res;
        }, 130);
      });
    },
    /** Yerinde küçük bir sıçrama (adım değişince, vurgu için) */
    nudge() {
      if (st.reduced) return;
      ch.hipY.v -= 160; ch.squash.x = -0.5;
    },
    look(x, y) { ch.lookX.t = x; ch.lookY.t = y; },
    /** Kolunu (ve bakışını) verilen noktaya (SVG koordinatı) uzatır; yakın kolu kendisi seçer */
    aim(tx, ty) {
      const hx = ch.hipX.x, hy = ch.hipY.x;
      const dxh = tx - hx;
      const left = dxh < 0;
      const sx = hx + (left ? -13 : 13) * S, sy = hy - (TORSO - 7) * S;
      const ang = (Math.atan2(-(tx - sx), ty - sy) * 180) / Math.PI; // 0 = aşağı, + = sola
      // hedef neredeyse tam altındaysa kol gövdeye yapışık kalıp işaret gibi görünmez: en az ~40° açıl
      let a = Math.max(-175, Math.min(175, ang));
      if (left && a < 40) a = 40;
      if (!left && a > -40) a = -40;
      const base = target('stand');
      if (left) { ch.shA.t = a; ch.elA.t = a > 120 ? 6 : -8; ch.shB.t = base.shB; ch.elB.t = base.elB; }
      else { ch.shB.t = a; ch.elB.t = a < -120 ? -6 : 8; ch.shA.t = base.shA; ch.elA.t = base.elA; }
      ch.lookX.t = Math.max(-1, Math.min(1, dxh / 90));
      ch.lookY.t = Math.max(-1, Math.min(1, (ty - (hy - 60 * S)) / 90));
      st.waving = false; st.hop = 0;
      api.current = 'aim';
    },
    setFloor(f) { st.floor = f; ch.hipY.t = f - STAND_HIP; },
    face(dir) { st.dir = dir; },
    get floor() { return st.floor; },
    get x() { return ch.hipX.x; },
    get y() { return ch.hipY.x; },
    handPosition() { // ekran koordinatı için yaklaşık (nesneye bakmak gerekirse)
      const r = armA.hand.getBoundingClientRect();
      return { x: r.left + r.width / 2, y: r.top + r.height / 2 };
    },
    /** Konfeti: karakterin üstünden fışkırır */
    confetti(n = 26) { if (!st.reduced) burst(parent, ch.hipX.x, ch.hipY.x - 70 * S, n); },
    destroy() { alive = false; cancelAnimationFrame(raf); root.remove(); },
    current: 'stand',
  };
  applyPose('stand', { instant: true });
  render();
  raf = requestAnimationFrame(frame);
  return api;
}

/** Konfeti: küçük parçalar yerçekimiyle düşer, soluklaşır */
export function burst(parent, x, y, n = 26) {
  const g = mk('g', {}, parent);
  const colors = ['#ff9f0a', '#ff375f', '#34c759', '#0a84ff', '#bf5af2', '#ffd60a'];
  const parts = Array.from({ length: n }, (_, i) => {
    const a = (-Math.PI / 2) + (Math.random() - 0.5) * 2.3;
    const sp = 90 + Math.random() * 150;
    const e = Math.random() < 0.5 ? mk('circle', { r: 2.4 }, g) : mk('rect', { x: -2.4, y: -1.6, width: 4.8, height: 3.2, rx: 1 }, g);
    e.style.fill = colors[i % colors.length];
    return { e, x, y, vx: Math.cos(a) * sp, vy: Math.sin(a) * sp, r: Math.random() * 360, vr: (Math.random() - 0.5) * 520 };
  });
  const t0 = performance.now();
  let prev = t0;
  (function tick(now) {
    const dt = Math.min(0.05, (now - prev) / 1000);
    prev = now;
    const age = (now - t0) / 1000;
    for (const p of parts) {
      p.vy += 420 * dt; p.vx *= 1 - dt * 0.8;
      p.x += p.vx * dt; p.y += p.vy * dt; p.r += p.vr * dt;
      p.e.setAttribute('transform', `translate(${p.x.toFixed(1)},${p.y.toFixed(1)}) rotate(${p.r.toFixed(0)})`);
      p.e.style.opacity = String(Math.max(0, 1 - Math.max(0, age - 0.9) / 0.9));
    }
    if (age < 1.9) requestAnimationFrame(tick); else g.remove();
  })(t0);
}
