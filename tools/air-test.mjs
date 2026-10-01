// Hava faresi çekirdeği testleri (telefon gerekmez): bilinen hareketleri sentetik olarak üretip imleç çıktısını doğrular.
// Çalıştır: node tools/air-test.mjs
import { AirMouse, orientationMatrix, mul, rotX, rotY, rotZ, DEG, AIR_DEFAULTS } from '../web/air.js';

let failed = 0;
const check = (name, ok, detail = '') => {
  if (!ok) failed++;
  console.log(`${ok ? '✓' : '✗'} ${name}${detail ? `  (${detail})` : ''}`);
};

const HZ = 60;
/** Başlangıç yönelimi `R0`'dan başlayıp her adımda `step(i, n)` ile döndürülen matrisle `seconds` sn süren hareketin toplam çıktısı */
function run(R0, motion, seconds, cfg) {
  const air = new AirMouse(1, cfg);
  const n = Math.round(seconds * HZ);
  let sx = 0, sy = 0, t = 0;
  air.updateMatrix(R0, t);
  for (let i = 1; i <= n; i++) {
    t += 1000 / HZ;
    const out = air.updateMatrix(motion(i / n), t);
    if (out) {
      sx += out.dx;
      sy += out.dy;
    }
  }
  return { sx, sy };
}

// Dünya çerçevesinde (dikey eksen etrafında) dönüş: Rz sol çarpım. Cihaz çerçevesinde: sağ çarpım.
const worldYaw = (R0, deg) => mul(rotZ(deg * DEG), R0);
const deviceRot = (R0, rot) => mul(R0, rot);
/** Rodrigues: keyfi eksen etrafında dönüş matrisi */
function axisRot(axis, ang) {
  const n = Math.hypot(...axis);
  const [x, y, z] = axis.map((v) => v / n);
  const c = Math.cos(ang), s = Math.sin(ang), k = 1 - c;
  return [c + x * x * k, x * y * k - z * s, x * z * k + y * s, y * x * k + z * s, c + y * y * k, y * z * k - x * s, z * x * k - y * s, z * y * k + x * s, c + z * z * k];
}
/** Dünya çerçevesinde "burnu kaldır": üst kenarın (dünyadaki yönü) yukarı doğru döndüğü yatay eksen etrafında */
function worldPitch(R0, deg) {
  const yW = [R0[1], R0[4], R0[7]]; // üst kenarın dünyadaki yönü
  const axis = [yW[1], -yW[0], 0]; // yW × ẑ
  return mul(axisRot(axis, deg * DEG), R0);
}

const poses = {
  'düz (ekran yukarı)': orientationMatrix(0, 0, 0),
  'remote gibi eğik (45°)': orientationMatrix(30, 45, 0),
  'dikeye yakın (70°)': orientationMatrix(100, 70, 5),
  'yan yatık (gamma 25°)': orientationMatrix(200, 35, 25),
};

for (const [name, R0] of Object.entries(poses)) {
  // 1) Sağa dön: dünya dikey ekseni etrafında −20° (saat yönü, yukarıdan bakınca) → imleç SAĞA, dikey ≈ 0
  const right = run(R0, (f) => worldYaw(R0, -20 * f), 1);
  check(`[${name}] sağa dönünce imleç sağa gider`, right.sx > 200 && Math.abs(right.sy) < 0.05 * Math.abs(right.sx), `dx=${right.sx.toFixed(0)} dy=${right.sy.toFixed(0)}`);

  // 2) Sola dön
  const left = run(R0, (f) => worldYaw(R0, 20 * f), 1);
  check(`[${name}] sola dönünce imleç sola gider`, left.sx < -200 && Math.abs(left.sy) < 0.05 * Math.abs(left.sx), `dx=${left.sx.toFixed(0)}`);

  // 3) Burnu kaldır: cihaz x ekseni etrafında +15° → imleç YUKARI (dy negatif), yatay ≈ 0
  const up = run(R0, (f) => worldPitch(R0, 15 * f), 1);
  check(`[${name}] burun kalkınca imleç yukarı gider`, up.sy < -150 && Math.abs(up.sx) < 0.05 * Math.abs(up.sy), `dx=${up.sx.toFixed(0)} dy=${up.sy.toFixed(0)}`);

  // 4) Burnu indir
  const down = run(R0, (f) => worldPitch(R0, -15 * f), 1);
  check(`[${name}] burun inince imleç aşağı gider`, down.sy > 150 && Math.abs(down.sx) < 0.05 * Math.abs(down.sy), `dy=${down.sy.toFixed(0)}`);

  // 5) Bileği bük (üst kenar ekseni etrafında roll): imleç oynamamalı
  // Bilek bükme = işaret ekseni etrafında dönüş: yatık tutuşta üst kenar (y), dik tutuşta arka yüz (z)
  const steep = name.startsWith('dikeye');
  const roll = run(R0, (f) => deviceRot(R0, (steep ? rotZ : rotY)(25 * DEG * f)), 1);
  check(`[${name}] bilek bükülünce (roll) imleç oynamaz`, Math.hypot(roll.sx, roll.sy) < 20, `${Math.hypot(roll.sx, roll.sy).toFixed(1)} px`);
}

// 6) Büyüklük: 20°'lik dönüş, ~30 px/° ve ivme çarpanıyla makul bir mesafe tutmalı (ekran genişliği 1920)
{
  const R0 = poses['düz (ekran yukarı)'];
  const slow = run(R0, (f) => worldYaw(R0, -10 * f), 2); // 5°/sn: yavaş
  const fast = run(R0, (f) => worldYaw(R0, -10 * f), 0.15); // ~67°/sn: hızlı
  check('Aynı açı, hızlı hareket daha uzağa gider (işaretçi ivmesi)', fast.sx > slow.sx * 1.3, `yavaş ${slow.sx.toFixed(0)} px, hızlı ${fast.sx.toFixed(0)} px`);
  check('Büyüklük makul (10° ≈ 100–500 px)', slow.sx > 100 && fast.sx < 700, `yavaş ${slow.sx.toFixed(0)}, hızlı ${fast.sx.toFixed(0)}`);
}

// 7) El titremesi: ±0.4° (≈ 6 Hz) titreme imleci kıpırdatmamalı
{
  const R0 = poses['remote gibi eğik (45°)'];
  const air = new AirMouse(1);
  let t = 0, total = 0, maxStep = 0;
  air.updateMatrix(R0, t);
  for (let i = 1; i <= 120; i++) {
    t += 1000 / HZ;
    const tremor = 0.4 * Math.sin(2 * Math.PI * 6 * (t / 1000));
    const out = air.updateMatrix(worldYaw(R0, tremor), t);
    if (out) {
      total += Math.hypot(out.dx, out.dy);
      maxStep = Math.max(maxStep, Math.hypot(out.dx, out.dy));
    }
  }
  check('El titremesi (±0,4°, 6 Hz) imleci sürüklemez', total / 120 < 1.2, `ortalama ${(total / 120).toFixed(2)} px/örnek, en büyük ${maxStep.toFixed(2)}`);
}

// 8) Hiç hareket yokken hiçbir şey olmaz
{
  const R0 = poses['remote gibi eğik (45°)'];
  const still = run(R0, () => R0, 2);
  check('Durağan telefonda imleç sabit', Math.hypot(still.sx, still.sy) === 0);
}

// 9) Açı sarması: alpha 359° → 1° (yönelim açıları sarar) sıçrama yapmamalı
{
  const air = new AirMouse(1);
  let t = 0, big = 0;
  air.update(358, 30, 0, t);
  for (const a of [359, 0, 1, 2]) {
    t += 1000 / HZ;
    const o = air.update(a, 30, 0, t);
    if (o) big = Math.max(big, Math.hypot(o.dx, o.dy));
  }
  check('Açı sarması (359° → 1°) sıçrama yapmıyor', big < 40, `en büyük adım ${big.toFixed(1)} px`);
}

// 10) Aralık atlayınca (olay kaçtı) ve reset sonrası ilk örnekte sıçrama yok
{
  const R0 = poses['düz (ekran yukarı)'];
  const air = new AirMouse(1);
  air.updateMatrix(R0, 0);
  const jump = air.updateMatrix(worldYaw(R0, -40), 500); // 0,5 sn boşluk
  check('Uzun boşluktan sonra ilk örnek atlanır', jump === null);
  air.reset();
  check('reset sonrası ilk örnek sadece başlangıç alır', air.updateMatrix(worldYaw(R0, -40), 1000) === null);
}

// 11) Dik tutuşta (üst kenar tavana bakıyor) NaN / sonsuz çıkmaz
{
  const R0 = orientationMatrix(0, 90, 0);
  const r = run(R0, (f) => deviceRot(R0, rotX(10 * DEG * f)), 1);
  check('Dik tutuşta sayısal sorun yok', Number.isFinite(r.sx) && Number.isFinite(r.sy), `dx=${r.sx.toFixed(0)} dy=${r.sy.toFixed(0)}`);
}

// 12) Hassasiyet doğrusal ölçekler
{
  const R0 = poses['düz (ekran yukarı)'];
  const a1 = new AirMouse(1), a2 = new AirMouse(2);
  let t = 0, s1 = 0, s2 = 0;
  a1.updateMatrix(R0, 0); a2.updateMatrix(R0, 0);
  for (let i = 1; i <= 60; i++) {
    t += 1000 / HZ;
    const R = worldYaw(R0, -20 * (i / 60));
    s1 += a1.updateMatrix(R, t)?.dx ?? 0;
    s2 += a2.updateMatrix(R, t)?.dx ?? 0;
  }
  check('Hassasiyet 2× → mesafe 2×', Math.abs(s2 / s1 - 2) < 0.01, `oran ${(s2 / s1).toFixed(3)}`);
}

console.log(failed ? `\n${failed} test BAŞARISIZ\n` : '\nTüm hava faresi testleri geçti ✓\n');
process.exit(failed ? 1 : 0);
