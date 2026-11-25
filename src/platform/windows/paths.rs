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
