//! Windows DPAPI: gizli verileri (CA anahtarı, jetonlar) diskte kullanıcı hesabına bağlı şifreli saklar.
//! Başka bir Windows kullanıcısı veya başka bir bilgisayar bu verileri çözemez.
use anyhow::{Context, Result};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use windows::core::PCWSTR;
use windows::Win32::Foundation::{LocalFree, HLOCAL};
use windows::Win32::Security::Cryptography::{
    CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
};

fn take_blob(out: CRYPT_INTEGER_BLOB) -> Vec<u8> {
    unsafe {
        let v = std::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec();
        let _ = LocalFree(Some(HLOCAL(out.pbData as _)));
        v
    }
}

pub fn protect(data: &[u8]) -> Result<String> {
    let input = CRYPT_INTEGER_BLOB { cbData: data.len() as u32, pbData: data.as_ptr() as *mut u8 };
    let mut out = CRYPT_INTEGER_BLOB::default();
    unsafe {
        CryptProtectData(&input, PCWSTR::null(), None, None, None, CRYPTPROTECT_UI_FORBIDDEN, &mut out)
            .context("DPAPI şifreleme başarısız")?;
    }
    Ok(B64.encode(take_blob(out)))
}

pub fn unprotect(encoded: &str) -> Result<Vec<u8>> {
    let data = B64.decode(encoded)?;
    let input = CRYPT_INTEGER_BLOB { cbData: data.len() as u32, pbData: data.as_ptr() as *mut u8 };
    let mut out = CRYPT_INTEGER_BLOB::default();
    unsafe {
        CryptUnprotectData(&input, None, None, None, None, CRYPTPROTECT_UI_FORBIDDEN, &mut out)
            .context("DPAPI çözme başarısız (veri başka bir kullanıcıya/bilgisayara ait olabilir)")?;
    }
    Ok(take_blob(out))
}

pub fn protect_str(s: &str) -> Result<String> {
    protect(s.as_bytes())
}

pub fn unprotect_str(encoded: &str) -> Result<String> {
    Ok(String::from_utf8(unprotect(encoded)?)?)
}
