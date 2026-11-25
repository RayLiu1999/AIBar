//! 開機自啟：以 `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` 的 `AIBar` 值為唯一來源。
//! 不需要系統管理員權限。
//!
//! 使用者若在工作管理員或「設定 → 應用程式 → 啟動」停用，Windows 會在
//! `Explorer\StartupApproved\Run` 另記一筆停用狀態（第一個位元組為奇數），Run 值本身仍在；
//! 因此判斷是否啟用時要一併檢查，從 AIBar 開啟時也要清掉這筆停用狀態。

use windows::Win32::System::Registry::{
    HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_BINARY, RRF_RT_REG_SZ, RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW,
};
use windows::core::w;

const RUN_KEY: windows::core::PCWSTR = w!(r"Software\Microsoft\Windows\CurrentVersion\Run");
const APPROVED_KEY: windows::core::PCWSTR =
    w!(r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run");
const VALUE_NAME: windows::core::PCWSTR = w!("AIBar");

/// 是否已設定開機自啟（Run 值存在，且沒有被使用者在系統設定中停用）
pub fn is_enabled() -> bool {
    let mut size = 0u32;
    let registered = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            RUN_KEY,
            VALUE_NAME,
            RRF_RT_REG_SZ,
            None,
            None,
            Some(&mut size),
        )
    }
    .is_ok();
    registered && !disabled_by_user()
}

/// StartupApproved 中的停用標記：第一個位元組為奇數表示停用
fn disabled_by_user() -> bool {
    let mut data = [0u8; 12];
    let mut size = data.len() as u32;
    let found = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            APPROVED_KEY,
            VALUE_NAME,
            RRF_RT_REG_BINARY,
            None,
            Some(data.as_mut_ptr() as *mut core::ffi::c_void),
            Some(&mut size),
        )
    }
    .is_ok();
    found && size > 0 && data[0] & 1 == 1
}

/// 開啟或關閉開機自啟；開啟時寫入目前執行檔的完整路徑
pub fn set_enabled(enabled: bool) -> bool {
    let result = if enabled {
        let Ok(exe) = std::env::current_exe() else { return false };
        // 路徑加上引號，避免含空白時被拆開
        let command = format!("\"{}\"", exe.display());
        let wide: Vec<u16> = command.encode_utf16().chain(std::iter::once(0)).collect();
        // 清掉使用者先前在系統設定留下的停用標記（不存在時忽略）
        let _ = unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, APPROVED_KEY, VALUE_NAME) };
        unsafe {
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                RUN_KEY,
                VALUE_NAME,
                REG_SZ.0,
                Some(wide.as_ptr() as *const core::ffi::c_void),
                (wide.len() * 2) as u32,
            )
        }
        .ok()
    } else {
        unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, RUN_KEY, VALUE_NAME) }.ok()
    };
    match result {
        // 關閉時值本來就不存在也算成功
        Err(e) if enabled => {
            log!("[開機自啟] 無法寫入登錄檔：{e}");
            false
        }
        _ => true,
    }
}
