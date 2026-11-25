//! 平台功能的唯一入口：依作業系統轉出同名函式，其餘模組不直接呼叫系統 API。

/// 從作業系統的憑證儲存讀取的結果
#[cfg_attr(windows, allow(dead_code))] // Windows 只會回報 Missing
pub enum SecretRead {
    Found(Vec<u8>),
    /// 項目不存在
    Missing,
    /// 使用者拒絕、儲存已鎖定或逾時
    Denied,
}

/// 托盤（選單列）選單的動作
pub struct TrayActions {
    pub refresh: Box<dyn Fn()>,
    pub settings: Box<dyn Fn()>,
    pub about: Box<dyn Fn()>,
    pub quit: Box<dyn Fn()>,
}

#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod windows;

#[cfg(target_os = "macos")]
pub use self::macos::*;
#[cfg(windows)]
pub use self::windows::*;
