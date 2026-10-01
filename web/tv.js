// Cihaz seçici (Bilgisayar + kayıtlı TV'ler) ve TV kumandası arayüzü.
// TV'lere PC bağlanır (agent/src/tv.rs); telefon yalnızca `tv` komutları yollar ve `tvs` durum olayını gösterir.
const TV_VIEWS = ['remote', 'pad', 'kbd'];
const T = { MOVE: 1, SCROLL: 2, STATE: 3, AIR: 4 };
const BRANDS = { lg: 'LG webOS', samsung: 'Samsung', android: 'Android TV' };

const STATE = {
  on: 'Bağlı',
  off: 'Kapalı veya ulaşılamıyor',
  connecting: 'Bağlanıyor…',
  pairing: 'TV ekranında izin bekleniyor',
  code: 'Eşleşme kodu bekleniyor',
  unpaired: 'Eşleşme gerekli',
};

/** Clickpad: parmakla kaydırınca yön tuşu (Siri Remote gibi), dokununca düğmeler çalışır */
export function bindClickpad(root, onDir) {
  const STEP = 30;
  let p = null;
  let swiped = false;
  root.addEventListener('pointerdown', (e) => { p = { x: e.clientX, y: e.clientY }; swiped = false; });
  root.addEventListener('pointermove', (e) => {
    if (!p) return;
    const dx = e.clientX - p.x, dy = e.clientY - p.y;
    if (Math.max(Math.abs(dx), Math.abs(dy)) < STEP) return;
    onDir(Math.abs(dx) > Math.abs(dy) ? (dx > 0 ? 'right' : 'left') : (dy > 0 ? 'down' : 'up'));
    p = { x: e.clientX, y: e.clientY };
    swiped = true;
  });
  const end = () => { p = null; };
  root.addEventListener('pointerup', end);
  root.addEventListener('pointercancel', end);
  // Kaydırma sonrası gelen tıklama bir düğmeye basmış sayılmasın
  root.addEventListener('click', (e) => { if (swiped) { e.stopPropagation(); e.preventDefault(); swiped = false; } }, true);
}

export function initTv({ $, sendJson, toast, haptic, tl, openSheet, closeSheets, pressFeedback, showView, getView, getPc }) {
  const root = $('v-remote');
  let tvs = [];
  let target = 'pc'; // 'pc' veya TV kimliği
  try { target = localStorage.getItem('target') || 'pc'; } catch {}
  let volDragging = false;
  let pendingSelect = false;

  const cur = () => (target === 'pc' ? null : tvs.find((t) => t.id === target) || null);
  const send = (o) => sendJson({ type: 'tv', id: cur()?.id, ...o });
  const saveTarget = () => { try { localStorage.setItem('target', target); } catch {} };
  const stateText = (t) => tl(STATE[t.state] || STATE.off);

  // ---- üstteki cihaz düğmesi (ad + simge + durum)
  function refresh() {
    const pc = getPc();
    const t = cur();
    const showTv = !!t && pc.state === 'ok' && TV_VIEWS.includes(getView());
    document.body.classList.toggle('tv-target', showTv);
    $('devIcon').firstElementChild.setAttribute('href', showTv ? '#i-tv' : '#i-desktop');
    $('dot').className = showTv ? (t.state === 'on' ? 'ok' : t.state === 'connecting' || t.state === 'pairing' ? '' : 'down') : pc.state;
    $('conn').textContent = showTv ? t.name : pc.text;
    $('devBtn').setAttribute('aria-label', tl('Cihaz seç') + ': ' + (showTv ? t.name : pc.text));
  }

  // ---- Kumanda sekmesi: hedef TV ise TV paneli
  function applyTarget() {
    if (target !== 'pc' && !cur()) { target = 'pc'; saveTarget(); }
    const mode = !!cur();
    root.classList.toggle('tv-mode', mode);
    $('tvRemote').hidden = !mode;
    refresh();
  }

  function renderTv() {
    const t = cur();
    document.body.classList.toggle('tv-busy', !!t && (t.state === 'connecting' || t.state === 'pairing'));
    document.dispatchEvent(new Event('busychange'));
    if (!t) return;
    $('tvName').textContent = t.name || 'TV';
    const st = $('tvState');
    st.textContent = stateText(t);
    st.classList.toggle('ok', t.state === 'on');
    const note = $('tvNote');
    const btn = $('tvNoteBtn');
    $('tvBody').hidden = t.state !== 'on';
    $('tvCodeForm').hidden = t.state !== 'code';
    const caps = t.caps || {};
    for (const el of $('tvRemote').querySelectorAll('[data-cap]')) el.hidden = !caps[el.dataset.cap];
    for (const el of $('tvRemote').querySelectorAll('[data-nocap]')) el.hidden = !!caps[el.dataset.nocap];
    note.hidden = t.state === 'on' || t.state === 'connecting';
    if (t.state === 'off') {
      $('tvNoteText').textContent = t.canWake ? tl('TV kapalı görünüyor. Açma sinyali gönderebilirsin.') : tl('TV kapalı veya ağda değil. Bir kez bağlandıktan sonra buradan açabilirsin.');
      btn.textContent = tl("TV'yi aç");
      btn.hidden = !t.canWake;
      btn.onclick = () => { send({ op: 'on' }); toast(tl('Açma sinyali gönderildi')); };
    } else if (t.state === 'pairing') {
      $('tvNoteText').textContent = t.brand === 'android' ? tl('TV ile eşleşme başlatılıyor…') : tl('TV ekranında çıkan soruda kumandayla “Evet”i veya “İzin ver”i seç.');
      btn.hidden = true;
    } else if (t.state === 'code') {
      $('tvNoteText').textContent = tl('TV ekranında görünen 6 karakterlik kodu yaz.');
      btn.hidden = true;
    } else if (t.state === 'unpaired') {
      $('tvNoteText').textContent = tl('TV ile eşleşme tamamlanmadı. TV açıkken tekrar dene.');
      btn.textContent = tl('Tekrar dene');
      btn.hidden = false;
      btn.onclick = () => send({ op: 'retry' });
    }
    if (t.state !== 'on') return;
    if (!volDragging && typeof t.volume === 'number') {
      $('tvVol').value = String(t.volume);
      paintVol(t.volume);
    }
    $('tvMute').classList.toggle('muted', !!t.muted);
    $('tvMute').setAttribute('aria-pressed', String(!!t.muted));
    renderApps(t);
    renderInputs(t.inputs || []);
    $('tvInputsTitle').hidden = !(caps.inputs && (t.inputs || []).length);
  }

  function paintVol(v) {
    $('tvVol').style.setProperty('--p', `${v}%`);
    $('tvVolPct').textContent = `${Math.round(v)}%`;
  }

  // ---- olaylar
  function onTvs(m) {
    const before = new Set(tvs.map((t) => t.id));
    tvs = Array.isArray(m.tvs) ? m.tvs : [];
    if (pendingSelect) {
      const fresh = tvs.find((t) => !before.has(t.id));
      if (fresh) {
        pendingSelect = false;
        target = fresh.id;
        saveTarget();
        showView('remote');
      }
    }
    applyTarget();
    renderTv();
    renderDevices();
    renderSaved();
    $('sceneMovie').hidden = !tvs.length;
    $('tvRowNote').textContent = tvs.length ? tvs.map((t) => t.name).join(', ') : tl('Televizyonu telefondan kumanda et');
  }

  // ---- cihaz seçme paneli
  function row(icon, title, sub, selected) {
    const b = document.createElement('button');
    b.className = 'set-row link dev-row' + (selected ? ' sel' : '');
    const s = document.createElement('span');
    s.append(Object.assign(document.createElement('b'), { textContent: title }), Object.assign(document.createElement('small'), { textContent: sub }));
    b.insertAdjacentHTML('beforeend', `<svg class="ic sm" aria-hidden="true"><use href="#${icon}"/></svg>`);
    b.append(s);
    if (selected) b.insertAdjacentHTML('beforeend', '<svg class="ic sm check" aria-hidden="true"><use href="#i-check"/></svg>');
    b.setAttribute('aria-pressed', String(!!selected));
    return b;
  }
  function renderDevices() {
    const box = $('devList');
    box.replaceChildren();
    const pc = getPc();
    const pcRow = row('i-desktop', pc.state === 'ok' ? pc.text : tl('Bu bilgisayar'), pc.state === 'ok' ? tl('Bu bilgisayar') : pc.text, target === 'pc');
    pcRow.onclick = () => pick('pc');
    box.append(pcRow);
    for (const t of tvs) {
      const r = row('i-tv', t.name, `${stateText(t)} · ${t.ip}`, target === t.id);
      r.onclick = () => pick(t.id);
      box.append(r);
    }
  }
  function pick(id) {
    target = id;
    saveTarget();
    haptic();
    closeSheets();
    applyTarget();
    renderTv();
    if (id !== 'pc') showView('remote');
  }
  $('devBtn').onclick = () => {
    renderDevices();
    closeSheets();
    openSheet('devSheet');
  };
  $('devAddTv').onclick = () => openTvSheet();

  // ---- uygulama ve giriş düğmeleri (liste markaya göre sunucudan gelir)
  function renderApps(t) {
    const box = $('tvApps');
    const sig = t.id + JSON.stringify(t.apps || []);
    if (box.dataset.sig !== sig) {
      box.dataset.sig = sig;
      box.replaceChildren();
      for (const [label, id] of t.apps || []) {
        const b = document.createElement('button');
        b.className = 'chip-btn';
        b.dataset.id = id;
        b.textContent = tl(label);
        b.onclick = () => { haptic(); send({ op: 'launch', app: id }); };
        box.append(b);
      }
    }
    for (const b of box.children) b.classList.toggle('on', b.dataset.id === t.app);
  }
  function renderInputs(list) {
    const box = $('tvInputs');
    const sig = JSON.stringify(list);
    if (box.dataset.sig === sig) return;
    box.dataset.sig = sig;
    box.replaceChildren();
    for (const d of list) {
      if (!d?.id) continue;
      const b = document.createElement('button');
      b.className = 'chip-btn';
      b.textContent = String(d.label || d.id);
      if (d.connected) b.append(Object.assign(document.createElement('small'), { textContent: '●' }));
      b.onclick = () => { haptic(); send({ op: 'input', input: String(d.id) }); };
      box.append(b);
    }
  }

  // ---- düğmeler
  for (const el of document.querySelectorAll('#tvRemote [data-tv]')) {
    pressFeedback(el);
    el.addEventListener('click', () => {
      const op = el.dataset.tv;
      if (op === 'key') send({ op, name: el.dataset.name });
      else if (op === 'scroll') send({ op, dy: Number(el.dataset.dy) });
      else send({ op });
    });
  }
  bindClickpad($('tvDpad'), (d) => { haptic(); send({ op: 'key', name: d.toUpperCase() }); });
  $('tvCodeForm').addEventListener('submit', (e) => {
    e.preventDefault();
    const code = $('tvCode').value.trim();
    if (!/^[0-9a-fA-F]{6}$/.test(code)) return toast(tl('Kod 6 karakter olmalı (0-9, A-F)'));
    send({ op: 'pair-code', code });
    $('tvCode').value = '';
  });
  pressFeedback($('tvMute'));
  $('tvMute').onclick = () => send({ op: 'mute', mute: !cur()?.muted });
  $('tvOff').onclick = () => {
    if (confirm(tl('TV kapatılsın mı?'))) send({ op: 'off' });
  };
  $('tvSetup').onclick = () => openTvSheet();

  // ---- ses kaydırıcısı
  const vol = $('tvVol');
  let volTimer = 0;
  vol.addEventListener('pointerdown', () => (volDragging = true));
  vol.addEventListener('pointercancel', () => (volDragging = false));
  vol.addEventListener('input', () => {
    paintVol(vol.value);
    if (!volTimer) {
      volTimer = setTimeout(() => { volTimer = 0; send({ op: 'vol', level: Number(vol.value) }); }, 90);
    }
  });
  vol.addEventListener('change', () => {
    volDragging = false;
    send({ op: 'vol', level: Number(vol.value) });
    haptic();
  });

  // ---- yazı (TV'deki arama kutusu)
  $('tvTypeForm').addEventListener('submit', (e) => {
    e.preventDefault();
    const t = $('tvText').value;
    if (t) send({ op: 'text', text: t });
    send({ op: 'enter' });
    $('tvText').value = '';
    haptic();
  });
  $('tvBksp').onclick = () => { send({ op: 'backspace' }); haptic(); };

  // ---- imleç yüzeyi: sürükle = sihirli kumanda imleci, dokun = tıkla
  const pad = $('tvPad');
  let last = null;
  let moved = 0;
  let acc = { x: 0, y: 0 };
  let flush = 0;
  const SPEED = 1.8;
  pad.addEventListener('pointerdown', (e) => {
    e.preventDefault();
    pad.setPointerCapture(e.pointerId);
    last = { x: e.clientX, y: e.clientY, t: performance.now() };
    moved = 0;
    pad.classList.add('active');
  });
  pad.addEventListener('pointermove', (e) => {
    if (!last) return;
    const dx = e.clientX - last.x;
    const dy = e.clientY - last.y;
    last.x = e.clientX;
    last.y = e.clientY;
    moved += Math.abs(dx) + Math.abs(dy);
    acc.x += dx * SPEED;
    acc.y += dy * SPEED;
    if (!flush) {
      flush = setTimeout(() => {
        flush = 0;
        const x = Math.round(acc.x), y = Math.round(acc.y);
        acc = { x: acc.x - x, y: acc.y - y };
        if (x || y) send({ op: 'move', dx: x, dy: y });
      }, 33);
    }
  });
  const end = (e) => {
    if (!last) return;
    if (e.type === 'pointerup' && moved < 8 && performance.now() - last.t < 350) {
      send({ op: 'click' });
      haptic();
    }
    last = null;
    pad.classList.remove('active');
  };
  pad.addEventListener('pointerup', end);
  pad.addEventListener('pointercancel', end);

  // ---- Touchpad / hava faresi / klavye: hedef TV ise girdi PC yerine TV'ye gider
  const routed = () => {
    const t = cur();
    return t && TV_VIEWS.includes(getView()) && getView() !== 'remote' ? t : null;
  };
  let lastWarn = 0;
  const live = (t) => {
    if (t.state === 'on') return true;
    if (performance.now() - lastWarn > 3000) { lastWarn = performance.now(); toast(tl('TV bağlı değil')); }
    return false;
  };
  let prevButtons = 0;
  const tacc = { mx: 0, my: 0, sx: 0, sy: 0, dx: 0, dy: 0 };
  let moveTimer = 0;
  const keyOnce = (name) => { haptic(); send({ op: 'key', name }); };
  function flushMove() {
    moveTimer = 0;
    const t = cur();
    if (!t) return;
    const dx = Math.round(tacc.mx), dy = Math.round(tacc.my);
    if (dx || dy) { tacc.mx -= dx; tacc.my -= dy; send({ op: 'move', dx, dy }); }
    const sy = Math.round(tacc.sy);
    if (sy) { tacc.sy -= sy; send({ op: 'scroll', dy: sy }); }
  }
  function input(type, a, b, buttons) {
    const t = routed();
    if (!t) return false;
    if (!live(t)) return true;
    const pointer = !!t.caps?.pointer;
    if (type === T.STATE) {
      if (buttons & 1 && !(prevButtons & 1)) { if (pointer) { haptic(); send({ op: 'click' }); } else keyOnce('ENTER'); }
      if (buttons & 2 && !(prevButtons & 2)) keyOnce('BACK');
      prevButtons = buttons;
      return true;
    }
    if (type !== T.MOVE && type !== T.AIR && type !== T.SCROLL) return true;
    if (pointer) {
      const k = type === T.MOVE ? 12 : type === T.AIR ? 1.2 : 0;
      tacc.mx += a * k;
      tacc.my += b * k;
      if (type === T.SCROLL) tacc.sy += -b * 6;
      if (!moveTimer) moveTimer = setTimeout(flushMove, 33);
    } else {
      // İşaretçisi olmayan TV'lerde parmak yön tuşlarına çevrilir (yeterince kayınca bir tuş)
      const k = type === T.MOVE ? 1 : type === T.AIR ? 0.05 : 0;
      if (type === T.SCROLL) tacc.dy += -b; else { tacc.dx += a * k; tacc.dy += b * k; }
      const STEP = 6;
      if (Math.abs(tacc.dx) >= STEP || Math.abs(tacc.dy) >= STEP) {
        if (Math.abs(tacc.dx) > Math.abs(tacc.dy)) keyOnce(tacc.dx > 0 ? 'RIGHT' : 'LEFT');
        else keyOnce(tacc.dy > 0 ? 'DOWN' : 'UP');
        tacc.dx = tacc.dy = 0;
      }
    }
    return true;
  }
  const NAV = { up: 'UP', down: 'DOWN', left: 'LEFT', right: 'RIGHT', esc: 'BACK' };
  function json(o) {
    const t = routed();
    if (!t || getView() !== 'kbd') return false;
    if (!live(t)) return true;
    if (o.type === 'text' && typeof o.text === 'string') {
      if (!t.caps?.text) return true;
      if (o.text === ' ') send({ op: 'text', text: ' ' }); else send({ op: 'text', text: o.text });
    } else if (o.type === 'key') {
      if (o.key === 'backspace') send({ op: 'backspace' });
      else if (o.key === 'enter') (t.caps?.text ? send({ op: 'enter' }) : send({ op: 'key', name: 'ENTER' }));
      else if (o.key === 'space') { if (t.caps?.text) send({ op: 'text', text: ' ' }); }
      else if (NAV[o.key]) send({ op: 'key', name: NAV[o.key] });
    }
    return true;
  }

  // ---- sahneler: tek dokunuşla birkaç cihazı birden yönet
  function scene(kind) {
    const on = tvs.filter((t) => t.state === 'on');
    const allOff = () => { for (const t of on) sendJson({ type: 'tv', op: 'off', id: t.id }); };
    if (kind === 'movie') {
      const t = cur() || tvs[0];
      if (!t) return toast(tl('Önce bir TV ekle'));
      if (t.state !== 'on') sendJson({ type: 'tv', op: 'on', id: t.id });
      closeSheets();
      pick(t.id);
      toast(t.state === 'on' ? tl('Film modu') : tl('Film modu: TV açılıyor'));
    } else if (kind === 'sleep') {
      allOff();
      sendJson({ type: 'timer', minutes: 30, action: 'stop' });
      closeSheets();
      toast(tl('Uyku modu: 30 dakika sonra bilgisayar da duracak'));
    } else if (kind === 'alloff') {
      allOff();
      sendJson({ type: 'power', action: 'screenoff' });
      closeSheets();
      toast(tl('Her şey kapatılıyor'));
    }
    haptic();
  }
  for (const b of document.querySelectorAll('[data-scene]')) b.addEventListener('click', () => scene(b.dataset.scene));

  // ---- TV ekleme / yönetme paneli
  function openTvSheet() {
    closeSheets();
    openSheet('tvSheet');
    $('tvIp').value = '';
    renderSaved();
    scan();
  }
  function renderSaved() {
    const box = $('tvSaved');
    box.replaceChildren();
    $('tvSavedTitle').hidden = box.hidden = !tvs.length;
    for (const t of tvs) {
      const r = document.createElement('div');
      r.className = 'set-row';
      const s = document.createElement('span');
      s.append(Object.assign(document.createElement('b'), { textContent: t.name }), Object.assign(document.createElement('small'), { textContent: `${t.ip} · ${stateText(t)}` }));
      const del = document.createElement('button');
      del.className = 'pill';
      del.textContent = tl('Sil');
      del.onclick = () => {
        if (confirm(tl('TV kaydı silinsin mi?'))) sendJson({ type: 'tv', op: 'forget', id: t.id });
      };
      r.append(s, del);
      box.append(r);
    }
  }
  function scan() {
    $('tvScan').disabled = true;
    $('tvScan').textContent = tl('Taranıyor…');
    sendJson({ type: 'tv', op: 'scan' });
    setTimeout(() => {
      $('tvScan').disabled = false;
      $('tvScan').textContent = tl("TV'leri tara");
    }, 3600);
  }
  function add(ip) {
    pendingSelect = true;
    sendJson({ type: 'tv', op: 'add', ip });
    closeSheets();
    toast(tl("TV'ye bağlanılıyor…"));
    // Aynı adres zaten kayıtlıysa yeni kayıt oluşmaz: onu seç
    const known = tvs.find((t) => t.ip === ip);
    if (known) { pendingSelect = false; pick(known.id); }
  }
  $('openTv').onclick = openTvSheet;
  $('tvScan').onclick = scan;
  $('tvIpForm').addEventListener('submit', (e) => {
    e.preventDefault();
    const ip = $('tvIp').value.trim();
    if (!/^\d{1,3}(\.\d{1,3}){3}$/.test(ip)) return toast(tl('Geçerli bir adres yaz (ör. 192.168.1.50)'));
    add(ip);
  });
  function onScan(m) {
    const box = $('tvFound');
    box.replaceChildren();
    const list = m.found || [];
    box.hidden = false;
    if (!list.length) {
      const r = document.createElement('div');
      r.className = 'set-row';
      r.append(Object.assign(document.createElement('span'), { textContent: tl('TV bulunamadı. TV açık ve aynı ağda mı? Adresi elle de yazabilirsin.') }));
      box.append(r);
      return;
    }
    for (const f of list) {
      const known = tvs.find((t) => t.ip === f.ip);
      const b = document.createElement('button');
      b.className = 'set-row link';
      const s = document.createElement('span');
      const bn = BRANDS[f.brand] || 'LG webOS';
      s.append(Object.assign(document.createElement('b'), { textContent: known ? known.name : (f.brand === 'android' ? bn : bn + ' TV') }), Object.assign(document.createElement('small'), { textContent: String(f.ip) }));
      b.append(s);
      b.onclick = () => add(String(f.ip));
      box.append(b);
    }
  }

  // Sunucu markayı tanıyamadıysa (adres yanlış ya da TV kapalı) bildirir
  function onAdd(m) {
    if (m.ok) return;
    pendingSelect = false;
    toast(tl(m.reason === 'limit' ? 'En fazla 8 TV eklenebilir' : 'Bu adreste desteklenen bir TV bulunamadı. TV açık ve aynı ağda mı?'));
  }

  return { onTvs, onScan, onAdd, refresh, onView: refresh, input, json, tvList: () => tvs, current: cur, select: pick };
}
