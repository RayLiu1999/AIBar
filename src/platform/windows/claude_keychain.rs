//! Windows 的 Claude Code 只把憑證存在檔案，沒有 Keychain。

use crate::platform::SecretRead;

pub fn present() -> bool {
    false
}

pub fn read() -> SecretRead {
    SecretRead::Missing
}
