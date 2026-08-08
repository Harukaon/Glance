//! Encrypts the LLM API key at rest.
//!
//! On Windows the value is protected with the DPAPI
//! (`CryptProtectData`/`CryptUnprotectData`), so it is only readable by the
//! current Windows user account. Encrypted values are stored as
//! `enc:v1:<base64>`. Values without the prefix are treated as legacy
//! plaintext and returned unchanged (they get encrypted on the next save).
//! On other platforms the value is stored as-is.

/// Encrypt a plaintext secret for storage. Never fails: falls back to
/// plaintext when DPAPI is unavailable (e.g. non-Windows or unusual setups).
pub fn encrypt(value: &str) -> String {
    #[cfg(target_os = "windows")]
    {
        if !value.is_empty() {
            if let Some(enc) = encrypt_windows(value) {
                return format!("enc:v1:{enc}");
            }
        }
    }
    value.to_string()
}

/// Decrypt a stored secret. Values that are not `enc:v1:` prefixed are
/// returned as-is (legacy plaintext).
pub fn decrypt(stored: &str) -> String {
    if let Some(b64) = stored.strip_prefix("enc:v1:") {
        #[cfg(target_os = "windows")]
        {
            if let Some(plain) = decrypt_windows(b64) {
                return plain;
            }
        }
        // Encrypted value but no decryptor available: return as-is rather
        // than corrupting the secret.
        return stored.to_string();
    }
    stored.to_string()
}

#[cfg(target_os = "windows")]
fn encrypt_windows(plain: &str) -> Option<String> {
    use base64::Engine;
    use windows_sys::Win32::Foundation::{LocalFree, HLOCAL};
    use windows_sys::Win32::Security::Cryptography::{
        CryptProtectData, CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN,
    };

    let bytes = plain.as_bytes();
    let mut in_blob = CRYPT_INTEGER_BLOB {
        cbData: bytes.len() as u32,
        pbData: bytes.as_ptr() as *mut u8,
    };
    let mut out_blob = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: std::ptr::null_mut(),
    };

    let ok = unsafe {
        CryptProtectData(
            &mut in_blob,
            std::ptr::null_mut(),
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut out_blob,
        )
    };
    if ok == 0 {
        return None;
    }
    let data = unsafe { std::slice::from_raw_parts(out_blob.pbData, out_blob.cbData as usize) };
    let encoded = base64::engine::general_purpose::STANDARD.encode(data);
    unsafe {
        let _ = LocalFree(out_blob.pbData as HLOCAL);
    }
    Some(encoded)
}

#[cfg(target_os = "windows")]
fn decrypt_windows(b64: &str) -> Option<String> {
    use base64::Engine;
    use windows_sys::Win32::Foundation::{LocalFree, HLOCAL};
    use windows_sys::Win32::Security::Cryptography::{
        CryptUnprotectData, CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN,
    };

    let bytes = base64::engine::general_purpose::STANDARD
        .decode(b64)
        .ok()?;
    let mut in_blob = CRYPT_INTEGER_BLOB {
        cbData: bytes.len() as u32,
        pbData: bytes.as_ptr() as *mut u8,
    };
    let mut out_blob = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: std::ptr::null_mut(),
    };

    let ok = unsafe {
        CryptUnprotectData(
            &mut in_blob,
            std::ptr::null_mut(),
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut out_blob,
        )
    };
    if ok == 0 {
        return None;
    }
    let data = unsafe { std::slice::from_raw_parts(out_blob.pbData, out_blob.cbData as usize) };
    let text = String::from_utf8(data.to_vec()).ok();
    unsafe {
        let _ = LocalFree(out_blob.pbData as HLOCAL);
    }
    text
}
