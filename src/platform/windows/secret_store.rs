//! 機密的保存：`%APPDATA%\AiBar\secrets.bin`，整份以 DPAPI（`CryptProtectData`，
//! 範圍 CurrentUser）加密：只有同一個 Windows 使用者能解開，檔案被複製到別台電腦也無用。

use windows::Win32::Foundation::{HLOCAL, LocalFree};
use windows::Win32::Security::Cryptography::{
    CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData, CryptUnprotectData,
};

use super::app_dir;
use crate::config::write_atomic;

const FILE_NAME: &str = "secrets.bin";

fn blob(data: &[u8]) -> CRYPT_INTEGER_BLOB {
    CRYPT_INTEGER_BLOB {
        cbData: data.len() as u32,
        pbData: data.as_ptr() as *mut u8,
    }
}

/// 把 DPAPI 回傳的緩衝區複製出來並釋放
fn take(out: CRYPT_INTEGER_BLOB) -> Vec<u8> {
    let v = unsafe { std::slice::from_raw_parts(out.pbData, out.cbData as usize) }.to_vec();
    unsafe {
        let _ = LocalFree(Some(HLOCAL(out.pbData as *mut core::ffi::c_void)));
    }
    v
}

fn protect(plain: &[u8]) -> Option<Vec<u8>> {
    let mut out = CRYPT_INTEGER_BLOB::default();
    unsafe {
        CryptProtectData(
            &blob(plain),
            None,
            None,
            None,
            None,
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut out,
        )
    }
    .ok()?;
    Some(take(out))
}

fn unprotect(cipher: &[u8]) -> Option<Vec<u8>> {
    let mut out = CRYPT_INTEGER_BLOB::default();
    unsafe {
        CryptUnprotectData(
            &blob(cipher),
            None,
            None,
            None,
            None,
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut out,
        )
    }
    .ok()?;
    Some(take(out))
}

/// 讀出保存的內容；沒有或解不開時為 None
pub fn load() -> Option<Vec<u8>> {
    let cipher = std::fs::read(app_dir()?.join(FILE_NAME)).ok()?;
    unprotect(&cipher)
}

/// 加密並保存內容（覆蓋舊的）
pub fn store(plain: &[u8]) -> bool {
    let Some(cipher) = protect(plain) else {
        log!("[機密] DPAPI 加密失敗");
        return false;
    };
    match write_atomic(FILE_NAME, &cipher) {
        Ok(()) => true,
        Err(e) => {
            log!("[機密] 無法寫入 {FILE_NAME}：{e}");
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dpapi_加解密() {
        let cipher = protect(b"sk-or-test").unwrap();
        assert!(!cipher.windows(10).any(|w| w == b"sk-or-test"), "密文不應含明文");
        assert_eq!(unprotect(&cipher).unwrap(), b"sk-or-test");
    }
}
