//! Antigravity：探測本機的 language server 讀取額度（不連外網）。
//!
//! 1. 列舉行程，找出 Antigravity 的 `language_server*.exe`（命令列含 `antigravity`），
//!    以 `NtQueryInformationProcess` 讀命令列，取出 `--csrf_token`、`--extension_server_port`、
//!    `--extension_server_csrf_token`。
//! 2. 以 `GetExtendedTcpTable` 列出該行程在聽的 TCP 連接埠，逐一以 `GetUnleashData` 探測。
//! 3. `POST https://127.0.0.1:<port>/exa.language_server_pb.LanguageServerService/<方法>`，
//!    header `X-Codeium-Csrf-Token`、`Connect-Protocol-Version: 1`；憑證為自簽，只對 127.0.0.1 放行。
//!    依序嘗試 `RetrieveUserQuotaSummary`、`GetUserStatus`；HTTPS 不通時改用 extension server 的 HTTP。
//!
//! App 沒開時改用 agy CLI（見 `agy` 模組）；兩者都沒有時回報「未執行」（不視為錯誤）。
//! 做法參考 CodexBar 的 Antigravity 文件。

use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};
use windows::Wdk::System::Threading::{NtQueryInformationProcess, PROCESSINFOCLASS};
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::NetworkManagement::IpHelper::{
    GetExtendedTcpTable, MIB_TCPROW_OWNER_PID, TCP_TABLE_OWNER_PID_LISTENER,
};
use windows::Win32::Networking::WinSock::AF_INET;
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};

use super::{UsageProvider, account_hash, agy, post_json_local};
use crate::model::{FetchFailure, Snapshot, UsageWindow};

const SERVICE: &str = "exa.language_server_pb.LanguageServerService";
/// `ProcessCommandLineInformation`（Windows 8.1 起）
const PROCESS_COMMAND_LINE_INFORMATION: PROCESSINFOCLASS = PROCESSINFOCLASS(60);

pub struct Antigravity;

/// 找到的 language server
#[derive(Debug, PartialEq)]
struct Server {
    pid: u32,
    csrf: String,
    ext_port: Option<u16>,
    ext_csrf: Option<String>,
    /// IDE 擴充套件內的 server（優先使用桌面 App 的）
    ide: bool,
}

fn install_dir() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA").map(|d| PathBuf::from(d).join("Programs").join("Antigravity"))
}

impl UsageProvider for Antigravity {
    fn id(&self) -> &'static str {
        "antigravity"
    }

    /// 有安裝 Antigravity App 或 agy CLI 即視為可用（首次啟動時預設啟用）
    fn credentials_present(&self) -> bool {
        install_dir().is_some_and(|d| d.exists()) || agy::find().is_some()
    }

    /// 帳號只能在 App 執行時問 language server，離線無法判斷
    fn current_account_hash(&self) -> Option<String> {
        None
    }

    fn fetch(&self) -> Result<Snapshot, FetchFailure> {
        let mut servers = find_servers();
        servers.sort_by_key(|s| s.ide);
        let mut last = FetchFailure::NotRunning;
        for server in &servers {
            match fetch_from(server) {
                Ok(snapshot) => return Ok(snapshot),
                Err(e) => last = e,
            }
        }
        // App 沒開（或問不到）時改用 agy CLI；沒裝 CLI 就維持「未執行」或 App 的失敗原因
        match agy::find() {
            Some(path) => agy::fetch(&path),
            None => Err(last),
        }
    }
}

fn metadata_body() -> Value {
    json!({ "metadata": {
        "ideName": "antigravity",
        "extensionName": "antigravity",
        "locale": "en",
        "ideVersion": "unknown"
    }})
}

fn call(base: &str, method: &str, csrf: &str) -> Result<Value, FetchFailure> {
    post_json_local(
        "antigravity",
        &format!("{base}/{SERVICE}/{method}"),
        &[("X-Codeium-Csrf-Token", csrf), ("Connect-Protocol-Version", "1")],
        &metadata_body(),
    )
}

/// 對一個 server 依序找連接埠、取額度
fn fetch_from(server: &Server) -> Result<Snapshot, FetchFailure> {
    let mut bases: Vec<(String, String)> = listening_ports(server.pid)
        .into_iter()
        .map(|p| (format!("https://127.0.0.1:{p}"), server.csrf.clone()))
        .collect();
    if let Some(port) = server.ext_port {
        let token = server.ext_csrf.clone().unwrap_or_else(|| server.csrf.clone());
        bases.push((format!("http://127.0.0.1:{port}"), token));
    }

    let mut last = FetchFailure::Network;
    for (base, csrf) in &bases {
        // 先確認這個連接埠是 language server
        if let Err(e) = call(base, "GetUnleashData", csrf) {
            last = e;
            continue;
        }
        let summary = call(base, "RetrieveUserQuotaSummary", csrf).ok();
        let status = call(base, "GetUserStatus", csrf).ok();
        let windows = summary
            .as_ref()
            .and_then(parse_summary)
            .or_else(|| status.as_ref().and_then(parse_status))
            .ok_or(FetchFailure::BadResponse)?;
        let us = status.as_ref().map(|s| &s["userStatus"]);
        let email = us.and_then(|u| u["email"].as_str()).unwrap_or_default();
        return Ok(Snapshot {
            plan: us.and_then(plan_name),
            account_hash: account_hash(email),
            windows,
            credits: None,
            resets: None,
            fetched_at: SystemTime::now(),
        });
    }
    Err(last)
}

/// 方案名稱：盡量從 userStatus 中常見的位置取得
fn plan_name(us: &Value) -> Option<String> {
    [
        &us["planStatus"]["planInfo"]["planName"],
        &us["userTier"]["name"],
        &us["planStatus"]["planInfo"]["teamsTier"],
    ]
    .into_iter()
    .find_map(|v| v.as_str().filter(|s| !s.is_empty()).map(String::from))
}

/// 重置時間：ISO 8601 或 Unix 秒
fn parse_reset(v: &Value) -> Option<SystemTime> {
    if let Some(s) = v.as_str() {
        if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(s) {
            return Some(UNIX_EPOCH + Duration::from_secs(dt.timestamp().max(0) as u64));
        }
        if let Ok(n) = s.parse::<u64>() {
            return Some(UNIX_EPOCH + Duration::from_secs(n));
        }
    }
    v.as_u64().map(|n| UNIX_EPOCH + Duration::from_secs(n))
}

/// 由識別字推視窗長度
fn window_secs(id: &str) -> Option<u64> {
    let id = id.to_ascii_lowercase();
    if id.contains("5h") || id.contains("session") || id.contains("5-hour") {
        Some(5 * 3600)
    } else if id.contains("week") {
        Some(7 * 86400)
    } else {
        None
    }
}

/// 取第一個存在的欄位（App 的回應用 camelCase，agy CLI 的報告用 snake_case）
fn field<'a>(v: &'a Value, keys: &[&str]) -> &'a Value {
    keys.iter().map(|k| &v[*k]).find(|x| !x.is_null()).unwrap_or(&Value::Null)
}

/// 額度摘要：App 的 `RetrieveUserQuotaSummary`（`response.groups[]`）或 agy CLI 的
/// `/usage` 報告（`command.data.groups[]`）。每個 bucket 有剩餘比例、重置時間、
/// `window`（"5h"／"weekly"）；App 舊版放在 `remaining` 底下
pub(super) fn parse_summary(v: &Value) -> Option<Vec<UsageWindow>> {
    let root = if v["response"].is_object() { &v["response"] } else { v };
    let mut windows = Vec::new();
    for group in root["groups"].as_array()? {
        let group_name = field(group, &["displayName", "display_name", "name"])
            .as_str()
            .unwrap_or_default()
            .trim_end_matches(" Models")
            .trim_end_matches(" models");
        for b in group["buckets"].as_array().into_iter().flatten() {
            let remaining = &b["remaining"];
            let Some(frac) = field(b, &["remainingFraction", "remaining_fraction"])
                .as_f64()
                .or_else(|| field(remaining, &["remainingFraction", "remaining_fraction"]).as_f64())
            else {
                continue;
            };
            let id = field(b, &["bucketId", "bucket_id", "id"]).as_str().unwrap_or_default();
            let window = b["window"].as_str().unwrap_or(id);
            let display = field(b, &["displayName", "display_name", "name"]).as_str().unwrap_or_default();
            // 通用名稱（「Weekly Limit Remaining」等）改用「組名・5 小時／每週」；
            // 有專屬名稱的（例如某個模型的每週額度）照原名，去掉結尾的「Limit」
            let generic = display.is_empty() || display.contains("Limit Remaining");
            let name = match (generic, window_secs(window).or_else(|| window_secs(id))) {
                (true, Some(18000)) => format!("{group_name}・5 小時"),
                (true, Some(_)) => format!("{group_name}・每週"),
                (true, None) => format!("{group_name} {id}"),
                (false, _) => display.trim_end_matches(" Limit").to_string(),
            }
            .trim_start_matches('・')
            .trim()
            .to_string();
            windows.push(UsageWindow {
                name,
                used_percent: ((1.0 - frac) * 100.0).clamp(0.0, 100.0),
                resets_at: parse_reset(field(b, &["resetTime", "reset_time"]))
                    .or_else(|| parse_reset(field(remaining, &["resetTime", "reset_time"]))),
                window_secs: window_secs(window).or_else(|| window_secs(id)),
            });
        }
    }
    (!windows.is_empty()).then_some(windows)
}

/// `GetUserStatus`：各模型的 `quotaInfo`，分成 Gemini 與 Claude + GPT 兩組，各取剩最少者
fn parse_status(v: &Value) -> Option<Vec<UsageWindow>> {
    let configs = v["userStatus"]["cascadeModelConfigData"]["clientModelConfigs"].as_array()?;
    let mut groups: Vec<(&str, f64, Option<SystemTime>)> = Vec::new();
    for c in configs {
        let label = c["label"].as_str().unwrap_or_default();
        let lower = label.to_ascii_lowercase();
        // 略過 Lite、自動完成與圖片模型
        if ["lite", "autocomplete", "image"].iter().any(|k| lower.contains(k)) {
            continue;
        }
        let Some(frac) = c["quotaInfo"]["remainingFraction"].as_f64() else { continue };
        let group = if lower.contains("gemini") {
            "Gemini"
        } else if ["claude", "gpt"].iter().any(|k| lower.contains(k)) {
            "Claude + GPT"
        } else {
            continue;
        };
        let reset = parse_reset(&c["quotaInfo"]["resetTime"]);
        match groups.iter_mut().find(|g| g.0 == group) {
            Some(g) if frac < g.1 => *g = (group, frac, reset),
            Some(_) => {}
            None => groups.push((group, frac, reset)),
        }
    }
    let windows: Vec<UsageWindow> = groups
        .into_iter()
        .map(|(name, frac, reset)| UsageWindow {
            name: name.into(),
            used_percent: ((1.0 - frac) * 100.0).clamp(0.0, 100.0),
            resets_at: reset,
            window_secs: Some(5 * 3600),
        })
        .collect();
    (!windows.is_empty()).then_some(windows)
}

// ---- 行程探測 ----

/// 從命令列取出 `--name value` 或 `--name=value`
fn arg_value(cmdline: &str, name: &str) -> Option<String> {
    let mut parts = cmdline.split_whitespace();
    while let Some(p) = parts.next() {
        let p = p.trim_matches('"');
        if p == name {
            return parts.next().map(|v| v.trim_matches('"').to_string());
        }
        if let Some(v) = p.strip_prefix(name).and_then(|r| r.strip_prefix('=')) {
            return Some(v.to_string());
        }
    }
    None
}

/// 判斷命令列是否為 Antigravity 的 language server，並取出參數
fn parse_cmdline(pid: u32, cmdline: &str) -> Option<Server> {
    let lower = cmdline.to_ascii_lowercase().replace('/', "\\");
    if !lower.contains("antigravity") {
        return None;
    }
    let csrf = arg_value(cmdline, "--csrf_token").filter(|t| !t.is_empty())?;
    let app_dir = arg_value(cmdline, "--app_data_dir").unwrap_or_default();
    Some(Server {
        pid,
        csrf,
        ext_port: arg_value(cmdline, "--extension_server_port").and_then(|p| p.parse().ok()),
        ext_csrf: arg_value(cmdline, "--extension_server_csrf_token"),
        ide: app_dir.contains("ide") || lower.contains("\\extensions\\"),
    })
}

fn find_servers() -> Vec<Server> {
    let mut found = Vec::new();
    let Ok(snap) = (unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }) else {
        return found;
    };
    let mut entry = PROCESSENTRY32W { dwSize: size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
    let mut ok = unsafe { Process32FirstW(snap, &mut entry) }.is_ok();
    while ok {
        let len = entry.szExeFile.iter().position(|&c| c == 0).unwrap_or(entry.szExeFile.len());
        let exe = String::from_utf16_lossy(&entry.szExeFile[..len]).to_ascii_lowercase();
        if (exe.starts_with("language_server") || exe.starts_with("language-server"))
            && let Some(server) =
                command_line(entry.th32ProcessID).and_then(|cmd| parse_cmdline(entry.th32ProcessID, &cmd))
        {
            found.push(server);
        }
        ok = unsafe { Process32NextW(snap, &mut entry) }.is_ok();
    }
    unsafe {
        let _ = CloseHandle(snap);
    }
    found
}

/// 讀取行程的命令列（`ProcessCommandLineInformation` 回傳 UNICODE_STRING 與其內容）
fn command_line(pid: u32) -> Option<String> {
    #[repr(C)]
    struct UnicodeString {
        length: u16,
        maximum_length: u16,
        buffer: *const u16,
    }
    let handle: HANDLE = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.ok()?;
    let mut needed = 0u32;
    let _ = unsafe {
        NtQueryInformationProcess(handle, PROCESS_COMMAND_LINE_INFORMATION, std::ptr::null_mut(), 0, &mut needed)
    };
    let mut result = None;
    if needed as usize >= size_of::<UnicodeString>() {
        // 以 u64 配置以確保對齊
        let mut buf = vec![0u64; (needed as usize).div_ceil(8)];
        let status = unsafe {
            NtQueryInformationProcess(
                handle,
                PROCESS_COMMAND_LINE_INFORMATION,
                buf.as_mut_ptr() as *mut core::ffi::c_void,
                needed,
                &mut needed,
            )
        };
        if status.is_ok() {
            let us = unsafe { &*(buf.as_ptr() as *const UnicodeString) };
            if !us.buffer.is_null() {
                let chars = unsafe { std::slice::from_raw_parts(us.buffer, us.length as usize / 2) };
                result = Some(String::from_utf16_lossy(chars));
            }
        }
    }
    unsafe {
        let _ = CloseHandle(handle);
    }
    result
}

/// 行程在聽的 IPv4 TCP 連接埠
fn listening_ports(pid: u32) -> Vec<u16> {
    let mut size = 0u32;
    let _ = unsafe {
        GetExtendedTcpTable(None, &mut size, false, AF_INET.0 as u32, TCP_TABLE_OWNER_PID_LISTENER, 0)
    };
    if size == 0 {
        return Vec::new();
    }
    let mut buf = vec![0u32; (size as usize).div_ceil(4)];
    let rc = unsafe {
        GetExtendedTcpTable(
            Some(buf.as_mut_ptr() as *mut core::ffi::c_void),
            &mut size,
            false,
            AF_INET.0 as u32,
            TCP_TABLE_OWNER_PID_LISTENER,
            0,
        )
    };
    if rc != 0 {
        return Vec::new();
    }
    // MIB_TCPTABLE_OWNER_PID：dwNumEntries 之後緊接著各列
    let count = buf[0] as usize;
    let rows = unsafe {
        std::slice::from_raw_parts(buf.as_ptr().add(1) as *const MIB_TCPROW_OWNER_PID, count)
    };
    let mut ports: Vec<u16> = rows
        .iter()
        .filter(|r| r.dwOwningPid == pid)
        .map(|r| u16::from_be((r.dwLocalPort & 0xffff) as u16))
        .collect();
    ports.sort_unstable();
    ports.dedup();
    ports
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 解析命令列() {
        let cmd = r#""C:\Users\u\AppData\Local\Programs\Antigravity\resources\bin\language_server.exe" --app_data_dir antigravity --csrf_token abc-123 --extension_server_port 54321 --extension_server_csrf_token=xyz"#;
        let s = parse_cmdline(42, cmd).unwrap();
        assert_eq!(s.csrf, "abc-123");
        assert_eq!(s.ext_port, Some(54321));
        assert_eq!(s.ext_csrf.as_deref(), Some("xyz"));
        assert!(!s.ide);
        // 沒有 token 或不是 Antigravity 的不算
        assert!(parse_cmdline(1, r"C:\x\language_server.exe --app_data_dir antigravity").is_none());
        assert!(parse_cmdline(1, r"C:\windsurf\language_server.exe --csrf_token a").is_none());
        let ide = parse_cmdline(2, r"C:\x\.antigravity\extensions\antigravity\bin\language_server.exe --app_data_dir antigravity-ide --csrf_token t").unwrap();
        assert!(ide.ide);
    }

    #[test]
    fn 解析額度摘要() {
        let v = json!({ "response": { "groups": [
            { "displayName": "Gemini", "buckets": [
                { "bucketId": "gemini-5h", "displayName": "Gemini 5-hour", "remaining": { "remainingFraction": 1.0, "resetTime": "2026-11-10T08:00:00Z" } },
                { "bucketId": "gemini-weekly", "displayName": "Gemini weekly", "remaining": { "remainingFraction": 0.88 } }
            ]},
            { "displayName": "Claude + GPT", "buckets": [
                { "bucketId": "cg-weekly", "displayName": "Claude + GPT weekly", "remainingFraction": 0.5,
                  "resetTime": "2026-11-16T16:52:12Z", "window": "weekly" }
            ]}
        ]}});
        let w = parse_summary(&v).unwrap();
        assert_eq!(w[0].name, "Gemini 5-hour");
        assert_eq!(w[0].used_percent, 0.0);
        assert_eq!(w[0].window_secs, Some(18000));
        assert!(w[0].resets_at.is_some());
        assert!((w[1].used_percent - 12.0).abs() < 1e-9);
        assert_eq!(w[1].window_secs, Some(604800));
        // 新版格式：欄位直接在 bucket 上
        assert_eq!(w[2].used_percent, 50.0);
        assert_eq!(w[2].window_secs, Some(604800));
        assert!(w[2].resets_at.is_some());
    }

    #[test]
    fn 解析使用者狀態() {
        let v = json!({ "userStatus": { "cascadeModelConfigData": { "clientModelConfigs": [
            { "label": "Gemini 3 Pro", "quotaInfo": { "remainingFraction": 0.9, "resetTime": "2026-11-10T08:00:00Z" } },
            { "label": "Gemini 3 Flash", "quotaInfo": { "remainingFraction": 0.6 } },
            { "label": "Gemini Flash Lite", "quotaInfo": { "remainingFraction": 0.1 } },
            { "label": "Claude Sonnet", "quotaInfo": { "remainingFraction": 0.5 } },
            { "label": "GPT-OSS", "quotaInfo": { "remainingFraction": 0.7 } }
        ]}}});
        let w = parse_status(&v).unwrap();
        assert_eq!(w.len(), 2);
        assert_eq!(w[0].name, "Gemini");
        assert!((w[0].used_percent - 40.0).abs() < 1e-9);
        assert_eq!(w[1].name, "Claude + GPT");
        assert!((w[1].used_percent - 50.0).abs() < 1e-9);
    }
}
