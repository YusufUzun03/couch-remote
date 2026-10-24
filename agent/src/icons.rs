//! Uygulama simgesi: düz mavi zemin üzerinde beyaz kanepe. Harici araç olmadan, bellekte çizilir.
//! Aynı tasarımın SVG hali web sayfalarında (yönetim, indirme sayfası) kullanılır.
use flate2::{write::ZlibEncoder, Compression};
use std::io::Write;

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    let mut crc = crc32fast::Hasher::new();
    crc.update(kind);
    crc.update(data);
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    out.extend_from_slice(&crc.finalize().to_be_bytes());
}

/// Yuvarlatılmış dikdörtgen için işaretli uzaklık (negatif = içeride)
fn sd_round_rect(px: f64, py: f64, cx: f64, cy: f64, hw: f64, hh: f64, r: f64) -> f64 {
    let qx = (px - cx).abs() - hw + r;
    let qy = (py - cy).abs() - hh + r;
    qx.max(qy).min(0.0) + qx.max(0.0).hypot(qy.max(0.0)) - r
}

const BG: [f64; 3] = [10.0, 132.0, 255.0]; // iOS mavisi #0A84FF

/// 100 birimlik tasarım ızgarasında kanepe parçaları: (merkez x, merkez y, yarı genişlik, yarı yükseklik, köşe)
const SOFA: [(f64, f64, f64, f64, f64); 6] = [
    (50.0, 41.0, 22.0, 12.0, 8.0), // sırtlık
    (50.0, 60.0, 27.0, 8.0, 5.0),  // oturak
    (22.0, 56.0, 8.0, 14.0, 7.0),  // sol kolçak
    (78.0, 56.0, 8.0, 14.0, 7.0),  // sağ kolçak
    (30.0, 73.0, 3.0, 4.0, 1.5),   // sol ayak
    (70.0, 73.0, 3.0, 4.0, 1.5),   // sağ ayak
];
/// Sırtlığı ikiye bölen ince dikiş (zemin renginde)
const SEAM: (f64, f64, f64, f64, f64) = (50.0, 41.0, 1.2, 12.0, 0.6);

/// Ham RGBA (satır satır). `rounded`: köşeler şeffaf (tepsi simgesi); aksi halde tam kare
/// (iOS ana ekran simgesinin köşelerini kendisi yuvarlar).
pub fn rgba(size: u32, rounded: bool) -> Vec<u8> {
    let s = size as f64 / 100.0;
    let half = size as f64 / 2.0;
    let mut out = Vec::with_capacity((size * size * 4) as usize);
    for y in 0..size {
        for x in 0..size {
            let (px, py) = (x as f64 + 0.5, y as f64 + 0.5);
            let glyph = SOFA
                .iter()
                .map(|&(cx, cy, hw, hh, r)| sd_round_rect(px, py, cx * s, cy * s, hw * s, hh * s, r * s))
                .fold(f64::MAX, f64::min);
            let seam = sd_round_rect(px, py, SEAM.0 * s, SEAM.1 * s, SEAM.2 * s, SEAM.3 * s, SEAM.4 * s);
            let white = (0.5 - glyph.max(-seam)).clamp(0.0, 1.0); // kenar yumuşatma
            for c in 0..3 {
                let bg = BG[c];
                out.push((bg + (255.0 - bg) * white).round() as u8);
            }
            let alpha = if rounded { (0.5 - sd_round_rect(px, py, half, half, half, half, size as f64 * 0.22)).clamp(0.0, 1.0) } else { 1.0 };
            out.push((alpha * 255.0).round() as u8);
        }
    }
    out
}

pub fn png(size: u32) -> Vec<u8> {
    let pixels = rgba(size, false);
    let mut raw = Vec::with_capacity(((size * 4 + 1) * size) as usize);
    for row in pixels.chunks((size * 4) as usize) {
        raw.push(0); // filtre yok
        raw.extend_from_slice(row);
    }
    let mut z = ZlibEncoder::new(Vec::new(), Compression::best());
    z.write_all(&raw).expect("zlib");
    let idat = z.finish().expect("zlib");

    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&size.to_be_bytes());
    ihdr.extend_from_slice(&size.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]); // 8 bit, RGBA
    let mut out = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &idat);
    chunk(&mut out, b"IEND", &[]);
    out
}
