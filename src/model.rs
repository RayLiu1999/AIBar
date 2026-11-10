//! 資料模型（規格書第 6 節）。名詞定義見 CONTEXT.md。

use std::time::SystemTime;

/// 某個 Provider 最後一次「成功」取得的用量資料。抓取失敗不會覆蓋快照。
#[derive(Clone, Debug)]
pub struct Snapshot {
    /// 方案名稱，例如「Max 5x」「Plus」
    pub plan: Option<String>,
    /// 帳號識別的雜湊，不存原始 id；用於啟動時判斷帳號是否已更換
    pub account_hash: String,
    pub windows: Vec<UsageWindow>,
    /// 以金額計的額度（Command Code、OpenRouter）；沒有時為 None
    pub credits: Option<Credits>,
    /// 保留重置（Claude、Codex）；服務沒有提供或一次都沒有時為 None
    pub resets: Option<ResetBank>,
    pub fetched_at: SystemTime,
}

/// 保留重置：先存起來、需要時再套用的額度重置
#[derive(Clone, Debug, PartialEq)]
pub struct ResetBank {
    /// 手上每一次重置的到期時間，最快到期的在前；None 表示沒有到期時間或不明（排在最後）
    pub expires: Vec<Option<SystemTime>>,
    /// 此刻能套用的次數（Codex 才有；沒撞到上限時通常為 0）；不明時為 None
    pub usable: Option<u32>,
}

impl ResetBank {
    /// 依到期時間排序後建立；一次都沒有時回傳 None
    pub fn new(mut expires: Vec<Option<SystemTime>>, usable: Option<u32>) -> Option<Self> {
        if expires.is_empty() {
            return None;
        }
        // None（沒有到期時間）排在最後
        expires.sort_by_key(|t| (t.is_none(), *t));
        Some(Self { expires, usable })
    }

    /// 在 `now` 仍有效的重置（排除已過期的）
    pub fn remaining(&self, now: SystemTime) -> Vec<Option<SystemTime>> {
        self.expires.iter().copied().filter(|t| t.is_none_or(|t| t > now)).collect()
    }
}

/// 以美元計的額度：本期已用與上限
#[derive(Clone, Debug, PartialEq)]
pub struct Credits {
    pub used: f64,
    pub limit: f64,
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
pub enum FetchFailure {
    /// 找不到憑證（未登入 CLI，或未在設定輸入 API key）
    NotLoggedIn,
    /// 憑證到期時間已過，或回應 401／403
    TokenExpired,
    /// 回應 429
    RateLimited { retry_at: SystemTime },
    /// 逾時、連線失敗、5xx
    Network,
    /// 回應格式無法解析
    BadResponse,
    /// Antigravity 未執行
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
