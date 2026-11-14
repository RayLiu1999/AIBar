//! agy CLI（Antigravity 的命令列工具）：Antigravity App 沒開時的後備來源。
//!
//! 執行 `agy -p /usage --output-format json --print-timeout 90s`，讀取非互動的用量報告
//! `{"status":"SUCCESS","command":{"name":"usage","data":{"groups":[…]}}}`，
//! 其中 `data` 與 App 的額度摘要同一種格式（欄位為 snake_case）。只讀用量，不送出模型提示、不耗額度。
//!
//! - agy 1.2.2 起本機 HTTPS 服務需要拿不到的 CSRF token，因此只用報告模式。
//! - 1.1.11 以前沒有非互動的用量報告，`/usage` 可能被當成提示送給模型，一律不執行。
//! - 執行檔：`ANTIGRAVITY_CLI_PATH`（有設定時只用它）→ `%LOCALAPPDATA%\agy\bin\agy.exe` → PATH 中的 `agy.exe`。
//! - 環境：`AGY_CLI_DISABLE_AUTO_UPDATE=true`（不觸發自動更新）、`BROWSER` 指向不存在的程式（不跳出瀏覽器登入）。
//! - 不顯示主控台視窗；行程樹放進 Job Object，結束或逾時時一併終止（agy 會啟動自己的 language server）。
//! - 工作目錄為 `%TEMP%\AiBar-agy`；輸出上限 1 MB、逾時 100 秒。
//!
//! 做法參考 CodexBar 的 Antigravity 文件與實作。

use std::io::Read;
use std::os::windows::io::AsRawHandle;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime};

use serde_json::Value;
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation, SetInformationJobObject,
};
use windows::core::PCWSTR;

use super::{PROBE, account_hash, antigravity::parse_summary, print_shape};
use crate::model::{FetchFailure, Snapshot};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
/// 第一個有非互動用量報告的版本
const MIN_VERSION: (u32, u32, u32) = (1, 1, 11);
const REPORT_TIMEOUT: Duration = Duration::from_secs(100);
const VERSION_TIMEOUT: Duration = Duration::from_secs(15);
const MAX_OUTPUT: u64 = 1 << 20;

/// 找 agy 執行檔
pub fn find() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("ANTIGRAVITY_CLI_PATH") {
        let p = PathBuf::from(p);
        return p.is_file().then_some(p);
    }
    let default = std::env::var_os("LOCALAPPDATA")
        .map(|d| PathBuf::from(d).join("agy").join("bin").join("agy.exe"))
        .filter(|p| p.is_file());
    default.or_else(|| {
        let paths = std::env::var_os("PATH")?;
        std::env::split_paths(&paths)
            .map(|d| d.join("agy.exe"))
            .find(|p| p.is_file())
    })
}

/// 以 agy 的用量報告取得額度
pub fn fetch(exe: &Path) -> Result<Snapshot, FetchFailure> {
    let version = version(exe).ok_or(FetchFailure::BadResponse)?;
    if version < MIN_VERSION {
        log!("[antigravity] agy {version:?} 太舊，沒有非互動的用量報告");
        return Err(FetchFailure::NotRunning);
    }
    let out = run(
        exe,
        &["-p", "/usage", "--output-format", "json", "--print-timeout", "90s"],
        REPORT_TIMEOUT,
    )?;
    let report: Value = serde_json::from_slice(&out).map_err(|_| FetchFailure::BadResponse)?;
    if PROBE.load(std::sync::atomic::Ordering::Relaxed) {
        eprintln!("[antigravity] agy 用量報告結構：");
        print_shape(&report, 1);
    }
    Ok(Snapshot {
        // 報告沒有方案名稱與帳號；方案由排程器沿用上一份快照
        plan: None,
        account_hash: account_hash(""),
        windows: parse_report(&report)?,
        credits: None,
        resets: None,
        fetched_at: SystemTime::now(),
    })
}

/// 用量報告：`status` 不是 SUCCESS 多半是 agy 未登入
fn parse_report(v: &Value) -> Result<Vec<crate::model::UsageWindow>, FetchFailure> {
    if v["command"]["name"].as_str() != Some("usage") {
        return Err(FetchFailure::BadResponse);
    }
    if v["status"].as_str() != Some("SUCCESS") {
        return Err(FetchFailure::NotLoggedIn);
    }
    parse_summary(&v["command"]["data"]).ok_or(FetchFailure::BadResponse)
}

/// `agy --version` 的輸出，例如 "1.5.3"
fn parse_version(s: &str) -> Option<(u32, u32, u32)> {
    let parts: Vec<&str> = s.trim().split('.').collect();
    match parts.as_slice() {
        [a, b, c] => Some((a.parse().ok()?, b.parse().ok()?, c.parse().ok()?)),
        _ => None,
    }
}

/// 執行檔的版本；同一個檔案（路徑、大小、修改時間不變）只問一次
fn version(exe: &Path) -> Option<(u32, u32, u32)> {
    /// 執行檔的識別（路徑、大小、修改時間）與其版本
    type Key = (PathBuf, u64, Option<SystemTime>);
    type Version = (u32, u32, u32);
    static CACHE: Mutex<Option<(Key, Version)>> = Mutex::new(None);

    let meta = std::fs::metadata(exe).ok()?;
    let key: Key = (exe.to_path_buf(), meta.len(), meta.modified().ok());
    if let Some((k, v)) = CACHE.lock().ok()?.as_ref()
        && *k == key
    {
        return Some(*v);
    }
    let out = run(exe, &["--version"], VERSION_TIMEOUT).ok()?;
    let v = parse_version(&String::from_utf8_lossy(&out))?;
    if let Ok(mut cache) = CACHE.lock() {
        *cache = Some((key, v));
    }
    Some(v)
}

/// 執行 agy 並取得 stdout；逾時時終止整個行程樹並回傳 Network
fn run(exe: &Path, args: &[&str], timeout: Duration) -> Result<Vec<u8>, FetchFailure> {
    let dir = std::env::temp_dir().join("AiBar-agy");
    let _ = std::fs::create_dir_all(&dir);
    let started = Instant::now();
    let mut child = Command::new(exe)
        .args(args)
        .current_dir(&dir)
        .env("AGY_CLI_DISABLE_AUTO_UPDATE", "true")
        .env("BROWSER", dir.join("no-browser.exe"))
        .env_remove("ELECTRON_RUN_AS_NODE")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map_err(|e| {
            log!("[antigravity] 無法執行 agy：{e}");
            FetchFailure::Network
        })?;
    let job = Job::for_child(&child);

    let mut stdout = child.stdout.take().ok_or(FetchFailure::Network)?;
    let reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = (&mut stdout).take(MAX_OUTPUT).read_to_end(&mut buf);
        buf
    });

    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if started.elapsed() < timeout => std::thread::sleep(Duration::from_millis(100)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
        }
    };
    // 關閉 Job 會終止 agy 留下的子行程，stdout 的管線也隨之關閉
    drop(job);
    let out = reader.join().unwrap_or_default();
    log!(
        "[antigravity] agy {}：{}，{} ms",
        args.join(" "),
        status.map_or("逾時".to_string(), |s| format!("結束碼 {}", s.code().unwrap_or(-1))),
        started.elapsed().as_millis()
    );
    status.map(|_| out).ok_or(FetchFailure::Network)
}

/// 關閉時終止所有成員行程的 Job Object
struct Job(HANDLE);

impl Job {
    fn for_child(child: &Child) -> Option<Job> {
        unsafe {
            let job = Job(CreateJobObjectW(None, PCWSTR::null()).ok()?);
            let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            SetInformationJobObject(
                job.0,
                JobObjectExtendedLimitInformation,
                &info as *const _ as *const core::ffi::c_void,
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
            .ok()?;
            AssignProcessToJobObject(job.0, HANDLE(child.as_raw_handle())).ok()?;
            Some(job)
        }
    }
}

impl Drop for Job {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn 版本() {
        assert_eq!(parse_version("1.5.3\r\n"), Some((1, 5, 3)));
        assert_eq!(parse_version("agy 1.5"), None);
        assert!(parse_version("1.1.10").unwrap() < MIN_VERSION);
        assert!(parse_version("1.2.0").unwrap() >= MIN_VERSION);
    }

    #[test]
    fn 用量報告() {
        // 2026-11-10 以 agy 1.5.3 實測的格式
        let v = json!({
            "status": "SUCCESS",
            "command": { "name": "usage", "data": { "groups": [
                { "name": "Gemini Models", "buckets": [
                    { "id": "gemini-weekly", "name": "Weekly Limit Remaining", "window": "weekly",
                      "remaining_fraction": 0.8816, "reset_time": "2026-11-16T05:14:02Z" },
                    { "id": "gemini-5h", "name": "Five Hour Limit Remaining", "window": "5h",
                      "remaining_fraction": 1, "reset_time": "2026-11-10T05:19:58Z" }
                ]},
                { "name": "Gemini 4 Argon", "buckets": [
                    { "id": "gemini-argon-wtu-weekly", "name": "Gemini 4 Argon Weekly Limit", "window": "weekly",
                      "remaining_fraction": 0.9797, "reset_time": "2026-11-14T08:20:43Z" }
                ]}
            ]}}
        });
        let w = parse_report(&v).unwrap();
        let names: Vec<&str> = w.iter().map(|x| x.name.as_str()).collect();
        // 回應先列每週，顯示時組內改為 5 小時在前
        assert_eq!(names, ["Gemini・5 小時", "Gemini・每週", "Gemini 4 Argon Weekly"]);
        assert_eq!(w[0].window_secs, Some(18000));
        assert!((w[1].used_percent - 11.84).abs() < 1e-6);
        assert!(w[2].resets_at.is_some());

        let failed = json!({ "status": "ERROR", "command": { "name": "usage", "data": {} } });
        assert!(matches!(parse_report(&failed), Err(FetchFailure::NotLoggedIn)));
        assert!(matches!(
            parse_report(&json!({ "status": "SUCCESS" })),
            Err(FetchFailure::BadResponse)
        ));
    }
}
