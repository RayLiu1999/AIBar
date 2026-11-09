//! 假資料：在接上真正的 Provider 之前，用來檢視面板與托盤的各種顯示狀態。
//! 按「立即刷新」會依序切換情境。接上真資料後即刪除本模組。

use std::time::{Duration, SystemTime};

use crate::model::{FetchFailure, ProviderStatus, Snapshot, UsageWindow};

pub const SCENARIO_COUNT: usize = 3;

const MIN: i64 = 60;
const HOUR: i64 = 60 * MIN;
const DAY: i64 = 24 * HOUR;

fn at(now: SystemTime, secs: i64) -> SystemTime {
    if secs >= 0 {
        now + Duration::from_secs(secs as u64)
    } else {
        now - Duration::from_secs((-secs) as u64)
    }
}

fn win(name: &str, pct: f64, reset_in: i64, now: SystemTime) -> UsageWindow {
    UsageWindow { name: name.into(), used_percent: pct, resets_at: Some(at(now, reset_in)) }
}

fn snap(plan: &str, age: i64, windows: Vec<UsageWindow>, now: SystemTime) -> Option<Snapshot> {
    Some(Snapshot {
        plan: Some(plan.into()),
        account_hash: "demo".into(),
        windows,
        fetched_at: at(now, -age),
    })
}

fn status(id: &'static str, snapshot: Option<Snapshot>, fail: Option<FetchFailure>) -> ProviderStatus {
    ProviderStatus { provider_id: id, snapshot, last_failure: fail, refreshing: false }
}

/// 第 `i` 個情境：0 正常、1 Token 過期與未登入、2 限流
pub fn scenario(i: usize, now: SystemTime) -> Vec<ProviderStatus> {
    match i % SCENARIO_COUNT {
        0 => vec![
            status(
                "claude",
                snap("Max 5x", 2 * MIN, vec![
                    win("5 小時", 61.0, 2 * HOUR + 13 * MIN, now),
                    win("每週", 30.0, 3 * DAY + 4 * HOUR, now),
                    win("每週・Fable", 92.0, 3 * DAY + 4 * HOUR, now),
                ], now),
                None,
            ),
            status(
                "codex",
                snap("Plus", 2 * MIN, vec![
                    win("5 小時", 75.0, HOUR + 40 * MIN, now),
                    win("每週", 18.0, 6 * DAY + 2 * HOUR, now),
                ], now),
                None,
            ),
        ],
        1 => vec![
            status(
                "claude",
                // 40 分鐘前的舊快照；5 小時視窗已過重置時間
                snap("Max 5x", 40 * MIN, vec![
                    win("5 小時", 88.0, -10 * MIN, now),
                    win("每週", 45.0, 3 * DAY, now),
                ], now),
                Some(FetchFailure::TokenExpired),
            ),
            status("codex", None, Some(FetchFailure::NotLoggedIn)),
        ],
        _ => vec![
            status(
                "claude",
                snap("Max 5x", 0, vec![
                    win("5 小時", 12.0, 4 * HOUR + 50 * MIN, now),
                    win("每週", 31.0, 3 * DAY, now),
                ], now),
                None,
            ),
            status(
                "codex",
                snap("Plus", 6 * MIN, vec![
                    win("5 小時", 97.0, 25 * MIN, now),
                    win("每週", 64.0, 2 * DAY, now),
                ], now),
                Some(FetchFailure::RateLimited { retry_at: at(now, 10 * MIN) }),
            ),
        ],
    }
}
