//! 機密的保存。尚未支援，見 `.scratch/macos-port` 工單 09。

pub fn load() -> Option<Vec<u8>> {
    None
}

pub fn store(_plain: &[u8]) -> bool {
    log!("[機密] macOS 版尚未支援");
    false
}
