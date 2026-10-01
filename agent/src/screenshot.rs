//! Ekran görüntüsü: tüm ekranlar (sanal masaüstü) veya tek bir monitör, JPEG olarak.
//! GDI ile yakalanır (ek program gerekmez). Korumalı içerik (DRM'li video) siyah çıkabilir.
use image::codecs::jpeg::JpegEncoder;
use image::imageops::{resize, FilterType};
use image::RgbImage;
use windows::core::BOOL;
use windows::Win32::Foundation::{LPARAM, RECT};
use windows::Win32::Graphics::Gdi::{
    BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, EnumDisplayMonitors, GetDC, GetDIBits, ReleaseDC,
    SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, CAPTUREBLT, DIB_RGB_COLORS, HDC, HMONITOR, SRCCOPY,
};
use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN};

/// Telefona gönderilen görüntünün en uzun kenarı (piksel). Daha büyük ekranlar küçültülür.
const MAX_SIDE: u32 = 1920;
const JPEG_QUALITY: u8 = 80;

pub struct Shot {
    pub jpeg: Vec<u8>,
    pub width: u32,
    pub height: u32,
    /// Gerçek yakalanan alan (küçültmeden önce)
    pub source: (u32, u32),
}

/// Bağlı monitörlerin alanları (sanal masaüstü koordinatları)
pub fn monitors() -> Vec<RECT> {
    unsafe extern "system" fn each(_: HMONITOR, _: HDC, rect: *mut RECT, lparam: LPARAM) -> BOOL {
        let list = unsafe { &mut *(lparam.0 as *mut Vec<RECT>) };
        list.push(unsafe { *rect });
        BOOL(1)
    }
    let mut list: Vec<RECT> = Vec::new();
    unsafe {
        let _ = EnumDisplayMonitors(None, None, Some(each), LPARAM(&mut list as *mut Vec<RECT> as isize));
    }
    // Soldan sağa: telefondaki sıra ekranların fiziksel dizilişine uysun
    list.sort_by_key(|r| (r.left, r.top));
    list
}

/// `monitor`: None = tüm ekranlar, Some(i) = i. monitör (soldan sağa)
pub fn capture(monitor: Option<usize>) -> Option<Shot> {
    let (x, y, w, h) = match monitor.and_then(|i| monitors().get(i).copied()) {
        Some(r) => (r.left, r.top, r.right - r.left, r.bottom - r.top),
        None => unsafe {
            (GetSystemMetrics(SM_XVIRTUALSCREEN), GetSystemMetrics(SM_YVIRTUALSCREEN), GetSystemMetrics(SM_CXVIRTUALSCREEN), GetSystemMetrics(SM_CYVIRTUALSCREEN))
        },
    };
    if w <= 0 || h <= 0 {
        return None;
    }
    let mut bgra = vec![0u8; (w as usize) * (h as usize) * 4];
    unsafe {
        let screen = GetDC(None);
        if screen.is_invalid() {
            return None;
        }
        let mem = CreateCompatibleDC(Some(screen));
        let bmp = CreateCompatibleBitmap(screen, w, h);
        let old = SelectObject(mem, bmp.into());
        let copied = BitBlt(mem, 0, 0, w, h, Some(screen), x, y, SRCCOPY | CAPTUREBLT).is_ok();
        let mut info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h, // üstten alta
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let lines = if copied { GetDIBits(mem, bmp, 0, h as u32, Some(bgra.as_mut_ptr() as *mut _), &mut info, DIB_RGB_COLORS) } else { 0 };
        SelectObject(mem, old);
        let _ = DeleteObject(bmp.into());
        let _ = DeleteDC(mem);
        ReleaseDC(None, screen);
        if lines == 0 {
            return None;
        }
    }
    // BGRA → RGB
    let mut rgb = Vec::with_capacity((w as usize) * (h as usize) * 3);
    for px in bgra.chunks_exact(4) {
        rgb.extend_from_slice(&[px[2], px[1], px[0]]);
    }
    let mut img = RgbImage::from_raw(w as u32, h as u32, rgb)?;
    let long = img.width().max(img.height());
    if long > MAX_SIDE {
        let k = MAX_SIDE as f32 / long as f32;
        img = resize(&img, ((img.width() as f32 * k).round() as u32).max(1), ((img.height() as f32 * k).round() as u32).max(1), FilterType::Triangle);
    }
    let mut jpeg = Vec::with_capacity(256 * 1024);
    JpegEncoder::new_with_quality(&mut jpeg, JPEG_QUALITY).encode_image(&img).ok()?;
    Some(Shot { jpeg, width: img.width(), height: img.height(), source: (w as u32, h as u32) })
}
