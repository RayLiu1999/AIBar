//! Provider：各 AI 服務的用量來源（規格書第 5 節）。
//! 每個 Provider 的端點細節集中在自己的模組，端點改版時只需修改一個檔案。

pub mod antigravity;
pub mod claude;
pub mod codex;
pub mod command_code;
pub mod openrouter;

use std::path::PathBuf;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime};

use serde_json::Value;
use ureq::Agent;
use ureq::tls::{RootCerts, TlsConfig, TlsProvider};

use crate::model::{FetchFailure, Snapshot};

pub trait UsageProvider: Send + Sync {
    fn id(&self) -> &'static str;
    /// 憑證檔是否存在（只檢查檔案，不讀內容）；用於首次啟動自動偵測
    fn credentials_present(&self) -> bool;
    /// 目前登入帳號的雜湊（只讀本機檔案，不連網）；讀不到時為 None。
    /// 用於啟動時判斷存下的快照是否屬於目前帳號。
    fn current_account_hash(&self) -> Option<String>;
    /// 背景刷新間隔；端點限流較嚴的 Provider 可覆寫成更長
    fn refresh_interval(&self) -> Duration {
        crate::scheduler::DEFAULT_REFRESH_INTERVAL
    }
    fn fetch(&self) -> Result<Snapshot, FetchFailure>;
}

/// 所有支援的 Provider（預設顯示順序；實際順序由 config.json 決定）
pub fn all() -> Vec<std::sync::Arc<dyn UsageProvider>> {
    vec![
        std::sync::Arc::new(claude::Claude),
        std::sync::Arc::new(codex::Codex),
        std::sync::Arc::new(antigravity::Antigravity),
        std::sync::Arc::new(command_code::CommandCode),
        std::sync::Arc::new(openrouter::OpenRouter),
    ]
}

/// 每個請求的逾時
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

/// `--probe` 模式：印出回應結構（遮蔽字串值）以便比對端點格式
pub static PROBE: AtomicBool = AtomicBool::new(false);

fn agent() -> &'static Agent {
    static AGENT: OnceLock<Agent> = OnceLock::new();
    AGENT.get_or_init(|| {
        Agent::config_builder()
            .timeout_global(Some(REQUEST_TIMEOUT))
            // 自行依狀態碼判斷失敗原因
            .http_status_as_error(false)
            .user_agent(concat!("AIBar/", env!("CARGO_PKG_VERSION")))
            .tls_config(
                TlsConfig::builder()
                    .provider(TlsProvider::NativeTls)
                    .root_certs(RootCerts::PlatformVerifier)
                    .build(),
            )
            .build()
            .into()
    })
}

/// 只連本機（127.0.0.1）用的 agent：本機 language server 使用自簽憑證，因此不驗證憑證。
/// 絕不可用於外部網址（`post_json_local` 會檢查）。
fn local_agent() -> &'static Agent {
    static AGENT: OnceLock<Agent> = OnceLock::new();
    AGENT.get_or_init(|| {
        Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(5)))
            .http_status_as_error(false)
            .tls_config(
                TlsConfig::builder()
                    .provider(TlsProvider::NativeTls)
                    .root_certs(RootCerts::PlatformVerifier)
                    .disable_verification(true)
                    .build(),
            )
            .build()
            .into()
    })
}

/// 送出 GET 並解析 JSON，依狀態碼轉成失敗原因。
/// 日誌只記錄 Provider、狀態碼與耗時，不記錄 header（含 token）與回應內容。
pub fn get_json(provider: &str, url: &str, headers: &[(&str, &str)]) -> Result<Value, FetchFailure> {
    let mut req = agent().get(url);
    for (k, v) in headers {
        req = req.header(*k, *v);
    }
    let started = Instant::now();
    let resp = req.call().map_err(|e| {
        log!("[{provider}] 連線失敗：{e}");
        FetchFailure::Network
    })?;
    handle_response(provider, started, resp)
}

/// 對本機服務送出 JSON POST（只接受 127.0.0.1）
pub fn post_json_local(
    provider: &str,
    url: &str,
    headers: &[(&str, &str)],
    body: &Value,
) -> Result<Value, FetchFailure> {
    let local = url.starts_with("https://127.0.0.1:") || url.starts_with("http://127.0.0.1:");
    if !local {
        return Err(FetchFailure::Network);
    }
    let mut req = local_agent().post(url).header("Content-Type", "application/json");
    for (k, v) in headers {
        req = req.header(*k, *v);
    }
    let started = Instant::now();
    let resp = req.send(body.to_string()).map_err(|_| FetchFailure::Network)?;
    handle_response(provider, started, resp)
}

fn handle_response(
    provider: &str,
    started: Instant,
    mut resp: ureq::http::Response<ureq::Body>,
) -> Result<Value, FetchFailure> {
    let status = resp.status().as_u16();
    log!("[{provider}] HTTP {status}，{} ms", started.elapsed().as_millis());

    match status {
        200..=299 => {}
        401 | 403 => return Err(FetchFailure::TokenExpired),
        429 => {
            // Retry-After 為秒數時採用；沒有則由排程器決定退避時間
            let after = resp
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.trim().parse::<u64>().ok())
                .unwrap_or(0);
            return Err(FetchFailure::RateLimited {
                retry_at: SystemTime::now() + Duration::from_secs(after),
            });
        }
        500..=599 => return Err(FetchFailure::Network),
        _ => return Err(FetchFailure::BadResponse),
    }

    let body = resp.body_mut().read_to_string().map_err(|_| FetchFailure::Network)?;
    let json: Value = serde_json::from_str(&body).map_err(|_| FetchFailure::BadResponse)?;
    if PROBE.load(Ordering::Relaxed) {
        eprintln!("[{provider}] 回應結構：");
        print_shape(&json, 1);
    }
    Ok(json)
}

/// 印出 JSON 結構：數字、布林照印；代號型字串（如 "session"）與 ISO 時間照印，其餘字串遮蔽
fn print_shape(v: &Value, indent: usize) {
    let pad = "  ".repeat(indent);
    match v {
        Value::Object(map) => {
            for (k, x) in map {
                if x.is_object() || x.is_array() {
                    eprintln!("{pad}{k}:");
                    print_shape(x, indent + 1);
                } else {
                    eprintln!("{pad}{k}: {}", leaf(x));
                }
            }
        }
        Value::Array(items) => {
            eprintln!("{pad}[{} 項]", items.len());
            for x in items.iter().take(8) {
                eprintln!("{pad}-");
                print_shape(x, indent + 1);
            }
        }
        _ => eprintln!("{pad}{}", leaf(v)),
    }
}

fn leaf(v: &Value) -> String {
    match v {
        Value::String(s) => {
            let code_like = s.len() <= 40
                && s.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
            let iso_time = s.len() <= 40 && chrono::DateTime::parse_from_rfc3339(s).is_ok();
            if code_like || iso_time { format!("{s:?}") } else { format!("<字串 {} 字>", s.len()) }
        }
        other => other.to_string(),
    }
}

/// 使用者家目錄
pub fn home_dir() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE").map(PathBuf::from)
}

/// 讀取 JSON 檔；檔案不存在或無法解析時視為未登入
pub fn read_json_file(path: &std::path::Path) -> Result<Value, FetchFailure> {
    let text = std::fs::read_to_string(path).map_err(|_| FetchFailure::NotLoggedIn)?;
    serde_json::from_str(text.trim_start_matches('\u{feff}')).map_err(|_| FetchFailure::NotLoggedIn)
}

/// 帳號識別的雜湊（FNV-1a 64 位元）。只用來判斷帳號是否更換，不儲存原始 id。
pub fn account_hash(id: &str) -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in id.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("{h:016x}")
}

/// 依視窗秒數命名：18000 →「5 小時」、604800 →「每週」，其他換算成小時或天數
pub fn window_name_from_seconds(secs: u64) -> String {
    match secs {
        18000 => "5 小時".into(),
        604800 => "每週".into(),
        s if s >= 86400 && s % 86400 == 0 => format!("{} 天", s / 86400),
        s => format!("{} 小時", (s as f64 / 3600.0).round() as u64),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 視窗命名() {
        assert_eq!(window_name_from_seconds(18000), "5 小時");
        assert_eq!(window_name_from_seconds(604800), "每週");
        assert_eq!(window_name_from_seconds(86400), "1 天");
        assert_eq!(window_name_from_seconds(3600), "1 小時");
    }

    #[test]
    fn 雜湊固定且不含原值() {
        let h = account_hash("abc-123");
        assert_eq!(h, account_hash("abc-123"));
        assert_ne!(h, account_hash("abc-124"));
        assert!(!h.contains("abc"));
    }
}
