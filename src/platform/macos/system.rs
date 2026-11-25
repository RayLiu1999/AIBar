//! 系統功能：準備資料夾、單一執行個體、複製到剪貼簿、以預設瀏覽器開啟網址。

use std::fs::{File, Permissions, TryLockError};
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::process::{Command, Stdio};

use super::app_dir;

/// 建立 AIBar 的資料夾，只有目前使用者可以存取
pub fn prepare() {
    let Some(dir) = app_dir() else { return };
    let result =
        std::fs::create_dir_all(&dir).and_then(|()| std::fs::set_permissions(&dir, Permissions::from_mode(0o700)));
    if let Err(e) = result {
        log!("[系統] 無法準備資料夾：{e}");
    }
}

/// 取得單一執行個體鎖（檔案鎖）。已有另一份 AIBar 持有時回傳 false。
/// 鎖在程式結束時由系統釋放。
pub fn acquire_single_instance() -> bool {
    let Some(dir) = app_dir() else { return true };
    match File::create(dir.join("aibar.lock")) {
        Ok(file) => match file.try_lock() {
            Ok(()) => {
                // 刻意不關閉：持有到程式結束
                std::mem::forget(file);
                true
            }
            Err(TryLockError::WouldBlock) => false,
            // 無法上鎖時不擋住啟動
            Err(TryLockError::Error(_)) => true,
        },
        Err(_) => true,
    }
}

/// 把文字複製到剪貼簿；失敗時回傳 false
pub fn copy_to_clipboard(text: &str) -> bool {
    let child = Command::new("/usr/bin/pbcopy")
        .env("LC_CTYPE", "UTF-8")
        .stdin(Stdio::piped())
        .spawn();
    let Ok(mut child) = child else { return false };
    let written = child
        .stdin
        .take()
        .is_some_and(|mut s| s.write_all(text.as_bytes()).is_ok());
    child.wait().is_ok_and(|s| s.success()) && written
}

/// 以預設瀏覽器開啟網址（只接受 https）
pub fn open_url(url: &str) {
    if !url.starts_with("https://") {
        return;
    }
    if let Ok(mut child) = Command::new("/usr/bin/open").arg(url).spawn() {
        // 回收子行程，避免留下殭屍行程
        std::thread::spawn(move || child.wait());
    }
}
