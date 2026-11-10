//! 機密儲存：`%APPDATA%\AiBar\secrets.bin`（規格書第 8 節）。
//!
//! 內容是 `{"<provider id>": "<API key>"}` 的 JSON，整份以 DPAPI（`CryptProtectData`，
//! 範圍 CurrentUser）加密：只有同一個 Windows 使用者能解開，檔案被複製到別台電腦也無用。
//! 機密只在記憶體中使用，絕不寫入日誌。

use std::collections::BTreeMap;
use std::sync::Mutex;

use windows::Win32::Foundation::{HLOCAL, LocalFree};
use windows::Win32::Security::Cryptography::{
    CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData, CryptUnprotectData,
};

use crate::config::{app_dir, write_atomic_bytes};

const FILE_NAME: &str = "secrets.bin";

/// 讀寫互斥（排程器執行緒讀、UI 執行緒寫）
static LOCK: Mutex<()> = Mutex::new(());

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

fn load_all() -> BTreeMap<String, String> {
    app_dir()
        .and_then(|d| std::fs::read(d.join(FILE_NAME)).ok())
        .and_then(|cipher| unprotect(&cipher))
        .and_then(|plain| serde_json::from_slice(&plain).ok())
        .unwrap_or_default()
}

/// 讀取某個 Provider 的機密；沒有時為 None
pub fn get(id: &str) -> Option<String> {
    let _guard = LOCK.lock();
    load_all().remove(id).filter(|s| !s.is_empty())
}

/// 是否已存有某個 Provider 的機密（不解出內容給呼叫端）
pub fn has(id: &str) -> bool {
    get(id).is_some()
}

/// 設定或清除（`None`）某個 Provider 的機密
pub fn set(id: &str, value: Option<&str>) -> bool {
    let _guard = LOCK.lock();
    let mut all = load_all();
    match value.map(str::trim).filter(|v| !v.is_empty()) {
        Some(v) => {
            all.insert(id.to_string(), v.to_string());
        }
        None => {
            all.remove(id);
        }
    }
    let plain = serde_json::to_vec(&all).unwrap_or_default();
    let Some(cipher) = protect(&plain) else {
        log!("[機密] DPAPI 加密失敗");
        return false;
    };
    match write_atomic_bytes(FILE_NAME, &cipher) {
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
