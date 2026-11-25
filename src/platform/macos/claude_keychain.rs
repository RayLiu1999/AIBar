//! Claude Code 存在 Keychain 的登入憑證（唯讀）。
//!
//! 以 `/usr/bin/security` 讀取：Claude Code 自己也用它存取這個項目，授權不綁在 AIBar 的簽章上，
//! 更新 AIBar 後不必重新授權。內容只經由管線傳回，不出現在參數、日誌或檔案。

use std::io::Read;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use super::process_tree::{self, ProcessTree};
use crate::platform::SecretRead;

const SECURITY: &str = "/usr/bin/security";
const SERVICE: &str = "Claude Code-credentials";
/// `security` 找不到項目時的結束碼（errSecItemNotFound）
const EXIT_NOT_FOUND: i32 = 44;
/// 含使用者回應授權視窗的時間
const TIMEOUT: Duration = Duration::from_secs(60);
const PRESENT_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_OUTPUT: u64 = 1 << 20;

/// 項目是否存在；只查中繼資料，不讀內容，不會跳出授權視窗
pub fn present() -> bool {
    matches!(
        run(SECURITY, &["find-generic-password", "-s", SERVICE], PRESENT_TIMEOUT),
        SecretRead::Found(_)
    )
}

/// 讀取內容；可能跳出系統的授權視窗
pub fn read() -> SecretRead {
    run(SECURITY, &["find-generic-password", "-s", SERVICE, "-w"], TIMEOUT)
}

fn run(program: &str, args: &[&str], timeout: Duration) -> SecretRead {
    let started = Instant::now();
    let mut command = Command::new(program);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let Ok(mut child) = process_tree::prepare(&mut command).spawn() else {
        log!("[claude] 無法執行 security");
        return SecretRead::Denied;
    };
    let tree = ProcessTree::attach(&child);
    let reader = child.stdout.take().map(|stdout| {
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = stdout.take(MAX_OUTPUT).read_to_end(&mut buf);
            buf
        })
    });
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if started.elapsed() < timeout => std::thread::sleep(Duration::from_millis(20)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
        }
    };
    drop(tree);
    let out = reader.and_then(|r| r.join().ok()).unwrap_or_default();
    match status.map(|s| s.code()) {
        Some(Some(0)) => SecretRead::Found(out),
        Some(Some(EXIT_NOT_FOUND)) => SecretRead::Missing,
        // 使用者拒絕、Keychain 鎖定、逾時
        other => {
            log!("[claude] 無法讀取 Keychain：結束碼 {other:?}");
            SecretRead::Denied
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 結束碼() {
        let sh = |script: &str| run("/bin/sh", &["-c", script], Duration::from_secs(5));
        assert!(matches!(sh("printf abc"), SecretRead::Found(v) if v == b"abc"));
        assert!(matches!(sh("exit 44"), SecretRead::Missing));
        assert!(matches!(sh("exit 128"), SecretRead::Denied));
    }

    #[test]
    fn 逾時視為被拒() {
        let started = Instant::now();
        let r = run("/bin/sh", &["-c", "sleep 30"], Duration::from_millis(200));
        assert!(matches!(r, SecretRead::Denied));
        assert!(started.elapsed() < Duration::from_secs(5));
    }
}
