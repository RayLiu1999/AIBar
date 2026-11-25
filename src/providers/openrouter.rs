//! OpenRouter：以使用者在設定視窗輸入的 API key 查詢額度（key 由作業系統加密保存，見 secret.rs）。
//!
//! - `GET https://openrouter.ai/api/v1/key`：此 key 的花費上限 `limit`、剩餘 `limit_remaining`、
//!   重置週期 `limit_reset`（daily／weekly／monthly／null，UTC 午夜；週為週一起算），
//!   以及此 key 自己的花費 `usage`（累計）、`usage_daily`、`usage_weekly`、`usage_monthly`
//! - `GET https://openrouter.ai/api/v1/credits`：帳戶的儲值總額 `total_credits` 與已用 `total_usage`
//!   （部分 key 沒有權限，取不到時略過）

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use chrono::{Datelike, NaiveDate, TimeZone, Utc};
use serde_json::Value;

use super::{UsageProvider, account_hash, get_json};
use crate::model::{Credits, FetchFailure, KeySpend, Snapshot, UsageWindow};
use crate::secret;

pub const ID: &str = "openrouter";
const KEY_URL: &str = "https://openrouter.ai/api/v1/key";
const CREDITS_URL: &str = "https://openrouter.ai/api/v1/credits";

pub struct OpenRouter {
    /// 憑證組，也是機密的鍵
    group: String,
}

impl OpenRouter {
    pub fn new(group: &str) -> Self {
        Self { group: group.into() }
    }
}

impl UsageProvider for OpenRouter {
    fn id(&self) -> &'static str {
        ID
    }

    fn credentials_present(&self) -> bool {
        secret::has(&self.group)
    }

    fn current_account_hash(&self) -> Option<String> {
        secret::get(&self.group).map(|k| account_hash(&k))
    }

    fn fetch(&self) -> Result<Snapshot, FetchFailure> {
        let key = secret::get(&self.group).ok_or(FetchFailure::NotLoggedIn)?;
        let bearer = format!("Bearer {key}");
        let headers = [("Authorization", bearer.as_str())];
        let key_info = get_json(ID, KEY_URL, &headers)?;
        let credits = get_json(ID, CREDITS_URL, &headers).ok();
        let parsed = parse(&key_info, credits.as_ref(), SystemTime::now()).ok_or(FetchFailure::BadResponse)?;
        Ok(Snapshot {
            plan: parsed.plan,
            account_hash: account_hash(&key),
            windows: parsed.windows,
            credits: parsed.credits,
            key_spend: parsed.key_spend,
            resets: None,
            fetched_at: SystemTime::now(),
        })
    }
}

struct Parsed {
    plan: Option<String>,
    windows: Vec<UsageWindow>,
    credits: Option<Credits>,
    key_spend: Option<KeySpend>,
}

fn to_system(dt: chrono::DateTime<Utc>) -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(dt.timestamp().max(0) as u64)
}

/// 依重置週期算出下次重置時間與視窗長度（秒）
fn next_reset(period: &str, now: SystemTime) -> Option<(SystemTime, u64)> {
    let now: chrono::DateTime<Utc> = now.into();
    let today = now.date_naive();
    let midnight =
        |d: NaiveDate| -> Option<chrono::DateTime<Utc>> { Some(Utc.from_utc_datetime(&d.and_hms_opt(0, 0, 0)?)) };
    match period {
        "daily" => Some((to_system(midnight(today.succ_opt()?)?), 86400)),
        "weekly" => {
            let days = 7 - today.weekday().num_days_from_monday() as u64;
            let next = today + chrono::Days::new(days);
            Some((to_system(midnight(next)?), 7 * 86400))
        }
        "monthly" => {
            let (y, m) = if today.month() == 12 {
                (today.year() + 1, 1)
            } else {
                (today.year(), today.month() + 1)
            };
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
    let credits = balance.map(|(total, used)| {
        windows.push(UsageWindow {
            name: "儲值餘額".into(),
            used_percent: (used / total * 100.0).clamp(0.0, 100.0),
            resets_at: None,
            window_secs: None,
        });
        Credits { used, limit: total }
    });

    let plan = match data["is_free_tier"].as_bool() {
        Some(true) => Some("免費".to_string()),
        Some(false) => Some("付費".to_string()),
        None => None,
    };
    Some(Parsed {
        plan,
        windows,
        credits,
        key_spend: key_spend(data),
    })
}

/// 此 key 自己的花費；缺任何一項就不顯示，避免把「沒有資料」顯示成 $0
fn key_spend(data: &Value) -> Option<KeySpend> {
    Some(KeySpend {
        total: data["usage"].as_f64()?,
        month: data["usage_monthly"].as_f64()?,
        week: data["usage_weekly"].as_f64()?,
        day: data["usage_daily"].as_f64()?,
    })
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
            "usage": 40.25, "usage_monthly": 25.5, "usage_weekly": 3, "usage_daily": 0.0042,
            "is_free_tier": false
        }});
        let credits = json!({ "data": { "total_credits": 50, "total_usage": 20 } });
        let p = parse(&key, Some(&credits), at(2026, 11, 10, 3)).unwrap();
        assert_eq!(p.plan.as_deref(), Some("付費"));
        assert_eq!(p.windows[0].name, "每月上限");
        assert!((p.windows[0].used_percent - 25.5).abs() < 1e-9);
        assert_eq!(p.windows[0].resets_at, Some(at(2026, 12, 1, 0)));
        assert_eq!(p.windows[0].window_secs, Some(30 * 86400));
        assert_eq!(p.windows[1].name, "儲值餘額");
        assert_eq!(
            p.credits,
            Some(Credits {
                used: 20.0,
                limit: 50.0
            })
        );
        // key 自己的花費與帳戶的已用是兩回事
        assert_eq!(
            p.key_spend,
            Some(KeySpend {
                total: 40.25,
                month: 25.5,
                week: 3.0,
                day: 0.0042
            })
        );
    }

    #[test]
    fn 週與日的重置() {
        // 2026-11-10 是週二 → 下週一 11/16
        assert_eq!(
            next_reset("weekly", at(2026, 11, 10, 3)).unwrap().0,
            at(2026, 11, 16, 0)
        );
        assert_eq!(
            next_reset("daily", at(2026, 11, 10, 23)).unwrap().0,
            at(2026, 11, 11, 0)
        );
        assert_eq!(next_reset("monthly", at(2026, 12, 5, 0)).unwrap().0, at(2027, 1, 1, 0));
        assert!(next_reset("", at(2026, 1, 1, 0)).is_none());
    }

    #[test]
    fn 無上限也無餘額() {
        let key = json!({ "data": {
            "limit": null, "usage": 8, "usage_monthly": 3.2, "usage_weekly": 0, "usage_daily": 0,
            "is_free_tier": true
        }});
        let p = parse(&key, None, at(2026, 11, 10, 0)).unwrap();
        assert!(p.windows.is_empty());
        assert_eq!(p.credits, None);
        assert_eq!(p.key_spend.map(|k| (k.total, k.month)), Some((8.0, 3.2)));
        // 缺欄位時不顯示
        let key = json!({ "data": { "limit": null, "usage_monthly": 3.2 } });
        assert_eq!(parse(&key, None, at(2026, 11, 10, 0)).unwrap().key_spend, None);
        assert!(parse(&json!({ "error": 1 }), None, at(2026, 11, 10, 0)).is_none());
    }
}
