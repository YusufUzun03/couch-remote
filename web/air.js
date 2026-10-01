// Hava faresi çekirdeği: telefonun yönelimini (deviceorientation) imleç hareketine çevirir.
// Saf matematik, DOM yok: tools/air-test.mjs sentetik hareketlerle doğruluğunu sınar.
//
// Neden "yönelim farkı"? rotationRate'in birimi/ekseni tarayıcıya göre değişebilir; W3C yönelim açıları
// (alpha, beta, gamma → R = Rz(α)·Rx(β)·Ry(γ)) her yerde aynı tanımlıdır. İki ardışık yönelim arasındaki
// dönüş (cihaz çerçevesinde) doğrudan açısal hızı verir: birim ve işaret karmaşası yok.
//
// İşaret modeli ("lazer işaretçi"): telefonun ÜST KENARI nereye bakıyorsa imleç oraya gider.
// Telefon dike yakın tutulursa (ekran yüze dönük, üst kenar tavanı gösterir) işaretçi kendiliğinden
// telefonun ARKASINA (ekrana "içinden bakar gibi") geçer; geçiş eğime göre yumuşaktır.
//   - sağa-sola döndürme (yatay düzlemde)  → imleç yatay
//   - burnunu kaldırma / indirme           → imleç dikey
//   - bileği kendi ekseninde bükme (roll)  → hiçbir şey (imleç oynamaz)
// Telefon yatık, dik veya eğik tutulsa da aynı çalışır: eksenler yerçekimine göre her an yeniden kurulur.

export const DEG = Math.PI / 180;

/** 3x3 satır-öncelikli matris çarpımı */
export function mul(a, b) {
  const r = new Array(9);
  for (let i = 0; i < 3; i++) for (let j = 0; j < 3; j++) r[i * 3 + j] = a[i * 3] * b[j] + a[i * 3 + 1] * b[3 + j] + a[i * 3 + 2] * b[6 + j];
  return r;
}
const transpose = (m) => [m[0], m[3], m[6], m[1], m[4], m[7], m[2], m[5], m[8]];
export const rotZ = (t) => [Math.cos(t), -Math.sin(t), 0, Math.sin(t), Math.cos(t), 0, 0, 0, 1];
export const rotX = (t) => [1, 0, 0, 0, Math.cos(t), -Math.sin(t), 0, Math.sin(t), Math.cos(t)];
export const rotY = (t) => [Math.cos(t), 0, Math.sin(t), 0, 1, 0, -Math.sin(t), 0, Math.cos(t)];

/** W3C DeviceOrientation: cihaz → dünya (x doğu, y kuzey, z yukarı) dönüşü. Açılar derece. */
export function orientationMatrix(alpha, beta, gamma) {
  return mul(mul(rotZ(alpha * DEG), rotX(beta * DEG)), rotY(gamma * DEG));
}

const clamp = (v, lo, hi) => Math.min(hi, Math.max(lo, v));
const smoothstep = (e0, e1, x) => {
  const t = clamp((x - e0) / (e1 - e0), 0, 1);
  return t * t * (3 - 2 * t);
};
const cross = (a, b) => [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
const dot = (a, b) => a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
const norm = (a) => Math.hypot(a[0], a[1], a[2]);
const scale = (a, k) => [a[0] * k, a[1] * k, a[2] * k];

/**
 * İki yönelim arasındaki işaret hareketi (derece): `yaw` pozitif = sağa, `pitch` pozitif = yukarı.
 * Küçük açı varsayımı (60 Hz'de adım başına birkaç derece) yeterince kesindir.
 */
export function pointerDelta(Rprev, Rnow) {
  // Cihaz çerçevesinde dönüş artışı: dR = Rprevᵀ·Rnow ≈ I + [φ]×
  const d = mul(transpose(Rprev), Rnow);
  const phi = [(d[7] - d[5]) / 2, (d[2] - d[6]) / 2, (d[3] - d[1]) / 2];
  const up = [Rnow[6], Rnow[7], Rnow[8]]; // dünyanın "yukarı"sı cihaz çerçevesinde
  // İşaret yönü: yatık/eğik tutuşta üst kenar, dik tutuşta arka yüz (üst kenar yukarı baktıkça arkaya geçer)
  const w = smoothstep(0.7, 0.9, up[1]);
  const p = scale([0, 1 - w, -w], 1 / Math.hypot(1 - w, w));
  let a = cross(p, up);
  let c;
  if (norm(a) < 0.2) {
    // Neredeyse dik tutuluyor (üst kenar tavana/yere bakıyor): yatay eksen belirsizleşir, cihazın x ve z eksenine düş
    a = [1, 0, 0];
    c = [0, 0, 1];
  } else {
    a = scale(a, 1 / norm(a));
    c = cross(a, p);
    c = scale(c, 1 / norm(c));
  }
  return {
    yaw: -dot(phi, c) / DEG, // c etrafında pozitif dönüş işaret yönünü SOLA götürür
    pitch: dot(phi, a) / DEG, // a etrafında pozitif dönüş işaret yönünü YUKARI götürür
  };
}


/** Ayarlanabilir sabitler (derece, saniye, piksel) */
export const AIR_DEFAULTS = {
  pxPerDeg: 30, // hassasiyet 1.0'da 1° ≈ 30 px (±32° ekranın yarısını tarar)
  gate: 0.8, // °/sn: bunun altındaki hareket el titremesi sayılır ve atılır (yumuşak eşik)
  slowGain: 0.6, // yavaş hareket: hassas
  fastGain: 1.5, // hızlı hareket: uzağa (işaretçi ivmesi)
  slowSpeed: 8, // °/sn: bunun altı "yavaş"
  fastSpeed: 110, // °/sn: bunun üstü "hızlı"
  smoothSlow: 0.35, // yavaşken süzgeç katsayısı (küçük = daha çok yumuşatma)
  maxDt: 0.1, // sn: bundan uzun aralıkta (olay kaçtı / arka plan) örnek atlanır
};

/**
 * Durumlu işaret hesaplayıcı. `update(alpha, beta, gamma, tMs)` imleç değişimini (piksel) döner;
 * `reset()` çağrılınca bir sonraki örnek sadece başlangıç alınır (parmak dokunduğunda imleç sıçramaz).
 */
export class AirMouse {
  constructor(sens = 1, cfg = {}) {
    this.cfg = { ...AIR_DEFAULTS, ...cfg };
    this.sens = sens;
    this.reset();
  }

  reset() {
    this.R = null;
    this.t = 0;
    this.fx = 0; // süzülmüş hız (°/sn)
    this.fy = 0;
  }

  setSens(s) {
    this.sens = s;
  }

  /** @returns {{dx:number, dy:number, speed:number} | null} piksel değişimi; ilk örnekte veya aralık bozukken null */
  update(alpha, beta, gamma, tMs) {
    if (alpha == null || beta == null || gamma == null) return null;
    return this.updateMatrix(orientationMatrix(alpha, beta, gamma), tMs);
  }

  /** Aynısı, yönelim matrisiyle (testler ve ileride quaternion kaynakları için) */
  updateMatrix(R, tMs) {
    const prev = this.R;
    const t0 = this.t;
    this.R = R;
    this.t = tMs;
    if (!prev) return null;
    const dt = (tMs - t0) / 1000;
    if (dt <= 0.001 || dt > this.cfg.maxDt) return null;

    const { yaw, pitch } = pointerDelta(prev, R);
    // Hız (°/sn); yumuşak eşik titremeyi atar ama eşiğin hemen üstünde ani sıçrama yapmaz
    const gate = (v) => Math.sign(v) * Math.max(0, Math.abs(v) - this.cfg.gate);
    const vx = gate(yaw / dt);
    const vy = gate(pitch / dt);
    const raw = Math.hypot(vx, vy);
    // Süzgeç: yavaşken güçlü (titreme gider), hızlıyken yok (gecikme hissedilmez)
    const k = this.cfg.smoothSlow + (1 - this.cfg.smoothSlow) * smoothstep(0, 60, raw);
    this.fx += k * (vx - this.fx);
    this.fy += k * (vy - this.fy);
    const speed = Math.hypot(this.fx, this.fy);
    // İvme eğrisi: yavaş = hassas, hızlı = uzağa
    const c = this.cfg;
    const g = c.slowGain + (c.fastGain - c.slowGain) * smoothstep(c.slowSpeed, c.fastSpeed, speed);
    const px = c.pxPerDeg * this.sens * g * dt;
    return { dx: this.fx * px, dy: -this.fy * px, speed };
  }
}
