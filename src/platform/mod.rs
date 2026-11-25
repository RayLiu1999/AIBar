//! 平台功能的唯一入口：依作業系統轉出同名函式，其餘模組不直接呼叫系統 API。

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
