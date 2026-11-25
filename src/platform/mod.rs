//! 平台功能的唯一入口：依作業系統轉出同名函式，其餘模組不直接呼叫系統 API。

#[cfg(windows)]
mod windows;

#[cfg(windows)]
pub use self::windows::*;
