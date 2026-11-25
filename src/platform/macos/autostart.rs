//! 開機自啟。尚未支援，見 `.scratch/macos-port` 工單 11。

pub fn is_enabled() -> bool {
    false
}

pub fn set_enabled(_enabled: bool) -> bool {
    log!("[開機自啟] macOS 版尚未支援");
    false
}
