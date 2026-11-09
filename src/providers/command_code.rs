//! Command Code：借用 Command Code CLI 已登入的 API key 查詢用量。
//!
//! - 憑證：`%USERPROFILE%\.commandcode\auth.json` 的 `apiKey`、`userId`
//! - 端點（`Authorization: Bearer <apiKey>`）：
//!   - `GET https://api.commandcode.ai/alpha/billing/credits`：剩餘月額度與 5 小時、每週視窗
//!   - `GET https://api.commandcode.ai/alpha/billing/subscriptions`：方案與計費週期
//! - 每月額度：方案的月額度（Go $10、Pro $30…）減去剩餘的 `monthlyCredits`，
//!   與 CLI 的 `/usage` 算法相同。
//!
//! 只讀取憑證檔，絕不寫入。

use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::Value;

use super::{UsageProvider, account_hash, get_json, home_dir, read_json_file};
use crate::model::{Credits, FetchFailure, Snapshot, UsageWindow};

const BASE: &str = "https://api.commandcode.ai";
const FIVE_HOURS: u64 = 5 * 3600;
const WEEK: u64 = 7 * 86400;

/// 方案代碼前綴 → （顯示名稱, 每月額度美元），與 CLI 內建表一致；長的前綴要排前面
const PLANS: [(&str, &str, f64); 9] = [
    ("individual-provider", "Provider", 15.0),
    ("individual-pro-v1", "Pro", 80.0),
    ("individual-go-v1", "Go", 10.0),
    ("individual-ultra", "Ultra", 300.0),
    ("individual-goat", "GOAT", 70.0),
    ("individual-max", "Max", 150.0),
    ("individual-pro", "Pro", 30.0),
    ("individual-go", "Go", 10.0),
    ("teams-pro", "Teams Pro", 40.0),
];

pub struct CommandCode;

fn auth_path() -> Option<PathBuf> {
    home_dir().map(|h| h.join(".commandcode").join("auth.json"))
}

impl UsageProvider for CommandCode {
    fn id(&self) -> &'static str {
        "commandcode"
    }

    fn credentials_present(&self) -> bool {
        auth_path().is_some_and(|p| p.exists())
    }

    fn current_account_hash(&self) -> Option<String> {
        let auth = read_json_file(&auth_path()?).ok()?;
        auth["userId"].as_str().map(account_hash)
    }

    fn fetch(&self) -> Result<Snapshot, FetchFailure> {
        let auth = read_json_file(&auth_path().ok_or(FetchFailure::NotLoggedIn)?)?;
        let key = auth["apiKey"].as_str().filter(|k| !k.is_empty()).ok_or(FetchFailure::NotLoggedIn)?;
        let user = auth["userId"].as_str().unwrap_or_default();
        let bearer = format!("Bearer {key}");
        let headers = [("Authorization", bearer.as_str())];

        let credits = get_json(self.id(), &format!("{BASE}/alpha/billing/credits"), &headers)?;
        // 訂閱資訊拿不到時仍可顯示 5 小時與每週視窗
        let subscription =
            get_json(self.id(), &format!("{BASE}/alpha/billing/subscriptions"), &headers).ok();

        let parsed = parse(&credits, subscription.as_ref()).ok_or(FetchFailure::BadResponse)?;
        Ok(Snapshot {
            plan: parsed.plan,
            account_hash: account_hash(user),
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

fn plan_info(plan_id: &str) -> Option<(&'static str, f64)> {
    let id = plan_id.to_ascii_lowercase().replace('_', "-");
    PLANS.iter().find(|(prefix, _, _)| id.starts_with(prefix)).map(|(_, name, credits)| (*name, *credits))
}

fn parse_time(v: &Value) -> Option<SystemTime> {
    let dt = chrono::DateTime::parse_from_rfc3339(v.as_str()?).ok()?;
    Some(UNIX_EPOCH + Duration::from_millis(dt.timestamp_millis().try_into().ok()?))
}

/// `windowLimits` 中的一個視窗：`used / cap`；`resetAt` 為毫秒，0 表示視窗尚未開始
fn window(v: &Value, name: &str, secs: u64) -> Option<UsageWindow> {
    let cap = v["cap"].as_f64().filter(|c| *c > 0.0)?;
    let used = v["used"].as_f64().unwrap_or(0.0);
    let resets_at = v["resetAt"].as_u64().filter(|ms| *ms > 0).map(|ms| UNIX_EPOCH + Duration::from_millis(ms));
    Some(UsageWindow {
        name: name.into(),
        used_percent: (used / cap * 100.0).clamp(0.0, 100.0),
        resets_at,
        window_secs: Some(secs),
    })
}

fn parse(credits: &Value, subscription: Option<&Value>) -> Option<Parsed> {
    let limits = &credits["windowLimits"];
    let mut windows: Vec<UsageWindow> = [("fiveHour", "5 小時", FIVE_HOURS), ("weekly", "每週", WEEK)]
        .into_iter()
        .filter_map(|(key, name, secs)| window(&limits[key], name, secs))
        .collect();

    let sub = subscription.map(|s| &s["data"]).filter(|d| d.is_object());
    let active = sub.is_some_and(|d| d["status"].as_str() == Some("active"));
    let plan = sub.and_then(|d| plan_info(d["planId"].as_str()?));

    // 每月額度：方案額度（訂閱有效時）與剩餘量取大者，再加上購買與贈送的額度
    let c = &credits["credits"];
    let monthly_left = c["monthlyCredits"].as_f64().unwrap_or(0.0).max(0.0);
    let extra = c["purchasedCredits"].as_f64().unwrap_or(0.0).max(0.0)
        + c["freeCredits"].as_f64().unwrap_or(0.0).max(0.0);
    let money = match plan.filter(|_| active) {
        Some((_, plan_credits)) => {
            let total = plan_credits.max(monthly_left) + extra;
            let used = total - (monthly_left + extra);
            Some(Credits { used: used.max(0.0), limit: total })
        }
        None => None,
    };
    if let Some(m) = &money
        && m.limit > 0.0
    {
        let start = sub.and_then(|d| parse_time(&d["currentPeriodStart"]));
        let end = sub.and_then(|d| parse_time(&d["currentPeriodEnd"]));
        windows.push(UsageWindow {
            name: "每月".into(),
            used_percent: (m.used / m.limit * 100.0).clamp(0.0, 100.0),
            resets_at: end,
            window_secs: start.zip(end).and_then(|(s, e)| e.duration_since(s).ok()).map(|d| d.as_secs()),
        });
    }

    if windows.is_empty() && money.is_none() {
        return None;
    }
    Some(Parsed { plan: plan.map(|(name, _)| name.to_string()), windows, credits: money })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn 解析用量() {
        let credits = json!({
            "credits": { "monthlyCredits": 5.67, "purchasedCredits": 0, "freeCredits": 0 },
            "windowLimits": {
                "fiveHour": { "used": 0, "cap": 3, "resetAt": 0 },
                "weekly": { "used": 3.0955, "cap": 6, "resetAt": 1794323258214u64 }
            }
        });
        let sub = json!({ "data": {
            "status": "active",
            "planId": "individual-go-v1",
            "currentPeriodStart": "2026-10-17T13:09:09.000Z",
            "currentPeriodEnd": "2026-11-17T13:09:09.000Z"
        }});
        let p = parse(&credits, Some(&sub)).unwrap();
        assert_eq!(p.plan.as_deref(), Some("Go"));
        let names: Vec<&str> = p.windows.iter().map(|w| w.name.as_str()).collect();
        assert_eq!(names, ["5 小時", "每週", "每月"]);
        assert_eq!(p.windows[0].used_percent, 0.0);
        assert!(p.windows[0].resets_at.is_none());
        assert!((p.windows[1].used_percent - 51.59).abs() < 0.01);
        let m = p.credits.unwrap();
        assert!((m.used - 4.33).abs() < 0.001 && m.limit == 10.0);
        assert!((p.windows[2].used_percent - 43.3).abs() < 0.01);
        assert_eq!(p.windows[2].window_secs, Some(31 * 86400));
    }

    #[test]
    fn 方案代碼() {
        assert_eq!(plan_info("individual-pro-v1"), Some(("Pro", 80.0)));
        assert_eq!(plan_info("individual_pro"), Some(("Pro", 30.0)));
        assert_eq!(plan_info("unknown"), None);
    }

    #[test]
    fn 無法解析() {
        assert!(parse(&json!({}), None).is_none());
    }
}
