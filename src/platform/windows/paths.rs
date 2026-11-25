//! 使用者資料夾的位置。

use std::path::PathBuf;

/// AIBar 自己的資料夾：`%APPDATA%\AiBar`
pub fn app_dir() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(|d| PathBuf::from(d).join("AiBar"))
}

/// 使用者家目錄
pub fn home_dir() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE").map(PathBuf::from)
}

/// Antigravity App 的安裝位置
pub fn antigravity_dir() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA").map(|d| PathBuf::from(d).join("Programs").join("Antigravity"))
}

/// agy CLI 的執行檔名
pub const AGY_EXE: &str = "agy.exe";

/// agy CLI 的預設安裝資料夾（先於 PATH 尋找）
pub fn agy_dirs() -> Vec<PathBuf> {
    std::env::var_os("LOCALAPPDATA")
        .map(|d| PathBuf::from(d).join("agy").join("bin"))
        .into_iter()
        .collect()
}
