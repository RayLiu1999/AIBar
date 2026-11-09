//! 資料模型（規格書第 6 節）。名詞定義見 CONTEXT.md。
//!
//! 之後實作 snapshot_store 時會再為 Snapshot、UsageWindow 加上 serde 序列化。

use std::time::SystemTime;

/// 某個 Provider 最後一次「成功」取得的用量資料。抓取失敗不會覆蓋快照。
#[derive(Clone, Debug)]
pub struct Snapshot {
    /// 方案名稱，例如「Max 5x」「Plus」
    pub plan: Option<String>,
    /// 帳號識別的雜湊，不存原始 id；用於啟動時判斷帳號是否已更換
    #[allow(dead_code)] // 帳號不符的判斷在 snapshot_store 實作時才會用到
    pub account_hash: String,
    pub windows: Vec<UsageWindow>,
    pub fetched_at: SystemTime,
}

/// 額度視窗：一段計量期間與其用量百分比
#[derive(Clone, Debug)]
pub struct UsageWindow {
    /// 例如「5 小時」「每週」「每週・Fable」
    pub name: String,
    /// 0.0–100.0
    pub used_percent: f64,
    pub resets_at: Option<SystemTime>,
    /// 視窗長度（秒），用於計算進度；推不出來時為 None
    pub window_secs: Option<u64>,
}

/// 最近一次刷新沒有成功的理由
#[derive(Clone, Debug)]
#[allow(dead_code)] // 部分種類要等 Provider 實作後才會產生
pub enum FetchFailure {
    /// 找不到憑證檔
    NotLoggedIn,
    /// 憑證到期時間已過，或回應 401／403
    TokenExpired,
    /// 回應 429
    RateLimited { retry_at: SystemTime },
    /// 逾時、連線失敗、5xx
    Network,
    /// 回應格式無法解析
    BadResponse,
    /// v1：Antigravity 未執行
    NotRunning,
}

/// 排程器交給 UI 的每個 Provider 狀態
#[derive(Clone, Debug)]
pub struct ProviderStatus {
    pub provider_id: &'static str,
    pub snapshot: Option<Snapshot>,
    /// 最近一次刷新成功時為 None
    pub last_failure: Option<FetchFailure>,
    pub refreshing: bool,
    /// 此 Provider 的背景刷新間隔（各 Provider 可不同）
    pub refresh_interval: std::time::Duration,
}
