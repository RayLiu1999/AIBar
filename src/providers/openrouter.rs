//! OpenRouter：以使用者在設定視窗輸入的 API key 查詢額度（key 以 DPAPI 加密存放，見 secret.rs）。
//!
//! - `GET https://openrouter.ai/api/v1/key`：此 key 的花費上限 `limit`、剩餘 `limit_remaining`、
//!   重置週期 `limit_reset`（daily／weekly／monthly／null，UTC 午夜；週為週一起算）
//! - `GET https://openrouter.ai/api/v1/credits`：帳戶的儲值總額 `total_credits` 與已用 `total_usage`
//!   （部分 key 沒有權限，取不到時略過）

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use chrono::{Datelike, NaiveDate, TimeZone, Utc};
use serde_json::Value;

use super::{UsageProvider, account_hash, get_json};
use crate::model::{Credits, FetchFailure, Snapshot, UsageWindow};
use crate::secret;

pub const ID: &str = "openrouter";
const KEY_URL: &str = "https://openrouter.ai/api/v1/key";
const CREDITS_URL: &str = "https://openrouter.ai/api/v1/credits";

pub struct OpenRouter;

impl UsageProvider for OpenRouter {
    fn id(&self) -> &'static str {
        ID
    }

    fn credentials_present(&self) -> bool {
        secret::has(ID)
    }

    fn current_account_hash(&self) -> Option<String> {
        secret::get(ID).map(|k| account_hash(&k))
    }

    fn fetch(&self) -> Result<Snapshot, FetchFailure> {
        let key = secret::get(ID).ok_or(FetchFailure::NotLoggedIn)?;
        let bearer = format!("Bearer {key}");
        let headers = [("Authorization", bearer.as_str())];
        let key_info = get_json(ID, KEY_URL, &headers)?;
        let credits = get_json(ID, CREDITS_URL, &headers).ok();
        let parsed =
            parse(&key_info, credits.as_ref(), SystemTime::now()).ok_or(FetchFailure::BadResponse)?;
        Ok(Snapshot {
            plan: parsed.plan,
            account_hash: account_hash(&key),
            windows: parsed.windows,
            credits: parsed.credits,
            fetched_at: SystemTime::now(),
        })
    }
}

struct Parsed {
    plan: Option<String>,
    windows: Vec<UsageWindow>,
    credits: Option<Credits>,
}

fn to_system(dt: chrono::DateTime<Utc>) -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(dt.timestamp().max(0) as u64)
}

/// 依重置週期算出下次重置時間與視窗長度（秒）
fn next_reset(period: &str, now: SystemTime) -> Option<(SystemTime, u64)> {
    let now: chrono::DateTime<Utc> = now.into();
    let today = now.date_naive();
    let midnight = |d: NaiveDate| -> Option<chrono::DateTime<Utc>> {
        Some(Utc.from_utc_datetime(&d.and_hms_opt(0, 0, 0)?))
    };
    match period {
        "daily" => Some((to_system(midnight(today.succ_opt()?)?), 86400)),
        "weekly" => {
            let days = 7 - today.weekday().num_days_from_monday() as u64;
            let next = today + chrono::Days::new(days);
            Some((to_system(midnight(next)?), 7 * 86400))
        }
        "monthly" => {
            let (y, m) = if today.month() == 12 { (today.year() + 1, 1) } else { (today.year(), today.month() + 1) };
            let start = NaiveDate::from_ymd_opt(today.year(), today.month(), 1)?;
            let next = NaiveDate::from_ymd_opt(y, m, 1)?;
            Some((to_system(midnight(next)?), (next - start).num_seconds() as u64))
        }
        _ => None,
    }
}

fn parse(key: &Value, credits: Option<&Value>, now: SystemTime) -> Option<Parsed> {
    let data = key.get("data").filter(|d| d.is_object())?;
    let mut windows = Vec::new();

    // 此 key 的花費上限
    if let Some(limit) = data["limit"].as_f64().filter(|l| *l > 0.0) {
        let remaining = data["limit_remaining"].as_f64().unwrap_or(limit).clamp(0.0, limit);
        let period = data["limit_reset"].as_str().unwrap_or("");
        let name = match period {
            "daily" => "每日上限",
            "weekly" => "每週上限",
            "monthly" => "每月上限",
            _ => "Key 上限",
        };
        let reset = next_reset(period, now);
        windows.push(UsageWindow {
            name: name.into(),
            used_percent: ((limit - remaining) / limit * 100.0).clamp(0.0, 100.0),
            resets_at: reset.map(|r| r.0),
            window_secs: reset.map(|r| r.1),
        });
    }

    // 帳戶儲值餘額
    let balance = credits
        .map(|c| &c["data"])
        .and_then(|d| Some((d["total_credits"].as_f64()?, d["total_usage"].as_f64()?)))
        .filter(|(total, _)| *total > 0.0);
    let money = match balance {
        Some((total, used)) => {
            windows.push(UsageWindow {
                name: "儲值餘額".into(),
                used_percent: (used / total * 100.0).clamp(0.0, 100.0),
                resets_at: None,
                window_secs: None,
            });
            Some(Credits { used, limit: total })
        }
        // 沒有餘額資訊時，至少顯示本月花費（上限 0 表示無上限）
        None => data["usage_monthly"].as_f64().map(|used| Credits { used, limit: 0.0 }),
    };

    let plan = match data["is_free_tier"].as_bool() {
        Some(true) => Some("免費".to_string()),
        Some(false) => Some("付費".to_string()),
        None => None,
    };
    Some(Parsed { plan, windows, credits: money })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn at(y: i32, m: u32, d: u32, h: u32) -> SystemTime {
        to_system(Utc.with_ymd_and_hms(y, m, d, h, 0, 0).unwrap())
    }

    #[test]
    fn 上限與餘額() {
        let key = json!({ "data": {
            "limit": 100, "limit_remaining": 74.5, "limit_reset": "monthly",
            "usage_monthly": 25.5, "is_free_tier": false
        }});
        let credits = json!({ "data": { "total_credits": 50, "total_usage": 20 } });
        let p = parse(&key, Some(&credits), at(2026, 11, 10, 3)).unwrap();
        assert_eq!(p.plan.as_deref(), Some("付費"));
        assert_eq!(p.windows[0].name, "每月上限");
        assert!((p.windows[0].used_percent - 25.5).abs() < 1e-9);
        assert_eq!(p.windows[0].resets_at, Some(at(2026, 12, 1, 0)));
        assert_eq!(p.windows[0].window_secs, Some(30 * 86400));
        assert_eq!(p.windows[1].name, "儲值餘額");
        assert_eq!(p.credits, Some(Credits { used: 20.0, limit: 50.0 }));
    }

    #[test]
    fn 週與日的重置() {
        // 2026-11-10 是週二 → 下週一 11/16
        assert_eq!(next_reset("weekly", at(2026, 11, 10, 3)).unwrap().0, at(2026, 11, 16, 0));
        assert_eq!(next_reset("daily", at(2026, 11, 10, 23)).unwrap().0, at(2026, 11, 11, 0));
        assert_eq!(next_reset("monthly", at(2026, 12, 5, 0)).unwrap().0, at(2027, 1, 1, 0));
        assert!(next_reset("", at(2026, 1, 1, 0)).is_none());
    }

    #[test]
    fn 無上限也無餘額() {
        let key = json!({ "data": { "limit": null, "usage_monthly": 3.2, "is_free_tier": true } });
        let p = parse(&key, None, at(2026, 11, 10, 0)).unwrap();
        assert!(p.windows.is_empty());
        assert_eq!(p.credits, Some(Credits { used: 3.2, limit: 0.0 }));
        assert!(parse(&json!({ "error": 1 }), None, at(2026, 11, 10, 0)).is_none());
    }
}
