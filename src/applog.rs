//! 日誌：寫入 `%APPDATA%\AiBar\aibar.log`（規格書第 8 節），debug 版同時印到主控台。
//!
//! 超過 1 MB 時改名為 `aibar.log.old`（覆蓋舊的），最多保留兩份。
//! 只記錄 Provider、狀態碼、耗時與錯誤種類；絕不記錄 token、cookie 或回應內容。

use std::io::Write;
use std::sync::Mutex;

const FILE_NAME: &str = "aibar.log";
const MAX_SIZE: u64 = 1024 * 1024;

static LOCK: Mutex<()> = Mutex::new(());

/// 寫入一行日誌（由 `log!` 巨集呼叫）
pub fn write(line: &str) {
    #[cfg(debug_assertions)]
    eprintln!("{line}");
    // 單元測試不寫入使用者真正的日誌檔
    if cfg!(test) {
        return;
    }

    let Some(dir) = crate::config::app_dir() else { return };
    let _guard = LOCK.lock();
    let path = dir.join(FILE_NAME);
    if std::fs::metadata(&path).is_ok_and(|m| m.len() > MAX_SIZE) {
        let _ = std::fs::rename(&path, dir.join(format!("{FILE_NAME}.old")));
    }
    let _ = std::fs::create_dir_all(&dir);
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
        let _ = writeln!(f, "{now} {line}");
    }
}

/// 寫一行日誌，用法同 `format!`
macro_rules! log {
    ($($arg:tt)*) => {
        $crate::applog::write(&format!($($arg)*))
    };
}
