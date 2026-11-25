//! 使用者資料夾的位置。

use std::path::{Path, PathBuf};

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

/// agy CLI 的預設安裝資料夾（先於 PATH 尋找）；從 Finder 啟動的程式沒有 shell 的 PATH
pub fn agy_dirs() -> Vec<PathBuf> {
    home_dir()
        .map(|h| h.join(".local").join("bin"))
        .into_iter()
        .chain(["/opt/homebrew/bin", "/usr/local/bin"].map(PathBuf::from))
        .collect()
}

/// 給 `BROWSER` 用、不會開出瀏覽器的程式。要存在且成功結束：
/// 指向不存在的程式時，常見的實作會退回 `/usr/bin/open`
pub fn no_browser(_work_dir: &Path) -> PathBuf {
    PathBuf::from("/usr/bin/true")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 不開瀏覽器的程式存在() {
        assert!(no_browser(Path::new("/")).is_file());
    }
}
