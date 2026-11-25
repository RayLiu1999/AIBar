//! 使用者資料夾的位置。

use std::path::PathBuf;

/// AIBar 自己的資料夾：`~/Library/Application Support/AIBar`
pub fn app_dir() -> Option<PathBuf> {
    home_dir().map(|h| h.join("Library").join("Application Support").join("AIBar"))
}

/// 使用者家目錄
pub fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").filter(|h| !h.is_empty()).map(PathBuf::from)
}

/// Antigravity App 的安裝位置
pub fn antigravity_dir() -> Option<PathBuf> {
    Some(PathBuf::from("/Applications/Antigravity.app"))
}

/// agy CLI 的執行檔名
pub const AGY_EXE: &str = "agy";

/// agy CLI 的預設安裝資料夾（先於 PATH 尋找）。尚未支援，見 `.scratch/macos-port` 工單 12
pub fn agy_dirs() -> Vec<PathBuf> {
    Vec::new()
}
