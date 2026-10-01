//! Pano paylaşımı (sadece metin). Telefon → PC ve PC → telefon; içerik günlüğe yazılmaz.
use anyhow::{anyhow, Result};
use windows::Win32::Foundation::{HANDLE, HGLOBAL};
use windows::Win32::System::DataExchange::{CloseClipboard, EmptyClipboard, GetClipboardData, OpenClipboard, SetClipboardData};
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
use windows::Win32::System::Ole::CF_UNICODETEXT;

pub const MAX_CHARS: usize = 100_000;

/// Pano başka bir program tarafından kısa süreliğine kilitli olabilir: birkaç kez dene
fn open() -> Result<()> {
    for _ in 0..10 {
        if unsafe { OpenClipboard(None) }.is_ok() {
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    Err(anyhow!("Pano açılamadı"))
}

pub fn get_text() -> Result<String> {
    open()?;
    let result = (|| unsafe {
        let h = GetClipboardData(CF_UNICODETEXT.0 as u32).map_err(|_| anyhow!("Panoda metin yok"))?;
        let p = GlobalLock(HGLOBAL(h.0)) as *const u16;
        if p.is_null() {
            return Err(anyhow!("Pano okunamadı"));
        }
        let mut len = 0usize;
        while len < MAX_CHARS && *p.add(len) != 0 {
            len += 1;
        }
        let text = String::from_utf16_lossy(std::slice::from_raw_parts(p, len));
        let _ = GlobalUnlock(HGLOBAL(h.0));
        Ok(text)
    })();
    unsafe {
        let _ = CloseClipboard();
    }
    result
}

pub fn set_text(text: &str) -> Result<()> {
    let wide: Vec<u16> = text.chars().take(MAX_CHARS).collect::<String>().encode_utf16().chain(std::iter::once(0)).collect();
    open()?;
    let result = (|| unsafe {
        EmptyClipboard()?;
        let mem = GlobalAlloc(GMEM_MOVEABLE, wide.len() * 2)?;
        let p = GlobalLock(mem) as *mut u16;
        if p.is_null() {
            return Err(anyhow!("Bellek ayrılamadı"));
        }
        std::ptr::copy_nonoverlapping(wide.as_ptr(), p, wide.len());
        let _ = GlobalUnlock(mem);
        // Başarılı olursa belleğin sahibi sistem olur (serbest bırakılmaz)
        SetClipboardData(CF_UNICODETEXT.0 as u32, Some(HANDLE(mem.0)))?;
        Ok(())
    })();
    unsafe {
        let _ = CloseClipboard();
    }
    result
}
