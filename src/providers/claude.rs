//! Claude（訂閱）：借用 Claude Code 已登入的 OAuth token 查詢用量。
//!
//! - 憑證：`%USERPROFILE%\.claude\.credentials.json` 的 `claudeAiOauth`
//!   （設定 `CLAUDE_CONFIG_DIR` 時改讀該目錄）
//! - 帳號識別：`%USERPROFILE%\.claude.json` 的 `oauthAccount.organizationUuid`
//! - 端點：`GET https://api.anthropic.com/api/oauth/usage`
//!
//! 只讀取憑證檔，絕不寫入；token 過期時不自行 refresh（ADR-0001）。
//! 此端點限流嚴格，短時間內連續請求幾次就會回 429。

use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::Value;

use super::{UsageProvider, account_hash, get_json, home_dir, read_json_file};
use crate::model::{FetchFailure, Snapshot, UsageWindow};

const USAGE_URL: &str = "https://api.anthropic.com/api/oauth/usage";
const FIVE_HOURS: u64 = 5 * 3600;
const WEEK: u64 = 7 * 86400;

pub struct Claude;

fn config_dir() -> Option<PathBuf> {
    match std::env::var_os("CLAUDE_CONFIG_DIR") {
        Some(dir) => Some(PathBuf::from(dir)),
        None => home_dir().map(|h| h.join(".claude")),
    }
}

fn credentials_path() -> Option<PathBuf> {
    config_dir().map(|d| d.join(".credentials.json"))
}

/// `.claude.json`：預設在家目錄；設定 `CLAUDE_CONFIG_DIR` 時在該目錄內
fn profile_path() -> Option<PathBuf> {
    match std::env::var_os("CLAUDE_CONFIG_DIR") {
        Some(dir) => Some(PathBuf::from(dir).join(".claude.json")),
        None => home_dir().map(|h| h.join(".claude.json")),
    }
}

impl UsageProvider for Claude {
    fn id(&self) -> &'static str {
        "claude"
    }

    fn credentials_present(&self) -> bool {
        credentials_path().is_some_and(|p| p.exists())
    }

    fn fetch(&self) -> Result<Snapshot, FetchFailure> {
        let path = credentials_path().ok_or(FetchFailure::NotLoggedIn)?;
        let creds = read_json_file(&path)?;
        let oauth = &creds["claudeAiOauth"];
        let token = oauth["accessToken"].as_str().ok_or(FetchFailure::NotLoggedIn)?;

        // 到期時間已過就不發送請求（expiresAt 為毫秒）
        if let Some(ms) = oauth["expiresAt"].as_u64()
            && UNIX_EPOCH + Duration::from_millis(ms) <= SystemTime::now()
        {
            return Err(FetchFailure::TokenExpired);
        }

        let plan = plan_name(oauth["subscriptionType"].as_str(), oauth["rateLimitTier"].as_str());
        let org = profile_path()
            .and_then(|p| read_json_file(&p).ok())
            .and_then(|v| v["oauthAccount"]["organizationUuid"].as_str().map(String::from))
            .unwrap_or_default();

        let auth = format!("Bearer {token}");
        let body = get_json(
            self.id(),
            USAGE_URL,
            &[("Authorization", &auth), ("anthropic-beta", "oauth-2025-04-20")],
        )?;
        let windows = parse_usage(&body).ok_or(FetchFailure::BadResponse)?;

        Ok(Snapshot {
            plan,
            account_hash: account_hash(&org),
            windows,
            fetched_at: SystemTime::now(),
        })
    }
}

/// 方案名稱：`max` + `default_claude_max_5x` →「Max 5x」；`pro` →「Pro」
fn plan_name(subscription: Option<&str>, tier: Option<&str>) -> Option<String> {
    let sub = subscription.filter(|s| !s.is_empty())?;
    let mut name = capitalize(sub);
    // 等級倍數取自 rateLimitTier 結尾，例如 "..._5x"
    if let Some(mult) = tier.and_then(|t| t.rsplit('_').next())
        && mult.len() >= 2
        && mult.ends_with('x')
        && mult[..mult.len() - 1].chars().all(|c| c.is_ascii_digit())
    {
        name.push(' ');
        name.push_str(mult);
    }
    Some(name)
}

fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

fn parse_time(v: &Value) -> Option<SystemTime> {
    let dt = chrono::DateTime::parse_from_rfc3339(v.as_str()?).ok()?;
    Some(UNIX_EPOCH + Duration::from_millis(dt.timestamp_millis().try_into().ok()?))
}

/// 解析用量回應：優先使用 `limits` 陣列，沒有時退回舊欄位 `five_hour`、`seven_day`。
/// 兩者皆無法解析時回傳 None（回應格式改變）。
fn parse_usage(v: &Value) -> Option<Vec<UsageWindow>> {
    if let Some(limits) = v["limits"].as_array() {
        let windows: Vec<UsageWindow> = limits.iter().filter_map(parse_limit).collect();
        if !windows.is_empty() {
            return Some(windows);
        }
    }

    let legacy: Vec<UsageWindow> =
        [("five_hour", "5 小時", FIVE_HOURS), ("seven_day", "每週", WEEK)]
        .into_iter()
        .filter_map(|(key, name, secs)| {
            let w = &v[key];
            Some(UsageWindow {
                name: name.into(),
                used_percent: w["utilization"].as_f64()?,
                resets_at: parse_time(&w["resets_at"]),
                window_secs: Some(secs),
            })
        })
        .collect();
    (!legacy.is_empty()).then_some(legacy)
}

/// `limits` 陣列中的一筆：`session` →「5 小時」、`weekly_all` →「每週」、
/// `weekly_scoped` →「每週・{模型}」；未知的 kind 以原字串命名
fn parse_limit(l: &Value) -> Option<UsageWindow> {
    let kind = l["kind"].as_str()?;
    // 視窗長度由 kind 推得：session 為 5 小時、weekly_* 為 7 天
    let window_secs = match kind {
        "session" => Some(FIVE_HOURS),
        k if k.starts_with("weekly") => Some(WEEK),
        _ => None,
    };
    let name = match kind {
        "session" => "5 小時".to_string(),
        "weekly_all" => "每週".to_string(),
        "weekly_scoped" => match l["scope"]["model"]["display_name"].as_str() {
            Some(model) => format!("每週・{model}"),
            None => "每週（指定模型）".to_string(),
        },
        other => other.to_string(),
    };
    Some(UsageWindow {
        name,
        used_percent: l["percent"].as_f64()?,
        resets_at: parse_time(&l["resets_at"]),
        window_secs,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn 方案名稱() {
        assert_eq!(plan_name(Some("max"), Some("default_claude_max_5x")).as_deref(), Some("Max 5x"));
        assert_eq!(plan_name(Some("max"), Some("default_claude_max_20x")).as_deref(), Some("Max 20x"));
        assert_eq!(plan_name(Some("pro"), Some("default_claude_ai")).as_deref(), Some("Pro"));
        assert_eq!(plan_name(None, None), None);
    }

    #[test]
    fn 解析_limits() {
        let v = json!({
            "limits": [
                { "kind": "session", "percent": 61.0, "resets_at": "2026-11-09T17:00:00Z" },
                { "kind": "weekly_all", "percent": 30 },
                { "kind": "weekly_scoped", "percent": 92.5, "scope": { "model": { "display_name": "Fable" } } },
                { "kind": "something_new", "percent": 5 }
            ],
            "five_hour": { "utilization": 1.0 }
        });
        let w = parse_usage(&v).unwrap();
        let names: Vec<&str> = w.iter().map(|x| x.name.as_str()).collect();
        assert_eq!(names, ["5 小時", "每週", "每週・Fable", "something_new"]);
        assert_eq!(w[0].used_percent, 61.0);
        assert!(w[0].resets_at.is_some());
        assert!(w[1].resets_at.is_none());
        assert_eq!(w[0].window_secs, Some(FIVE_HOURS));
        assert_eq!(w[2].window_secs, Some(WEEK));
        assert_eq!(w[3].window_secs, None);
    }

    #[test]
    fn 退回舊欄位() {
        let v = json!({
            "five_hour": { "utilization": 23.0, "resets_at": "2026-11-09T17:00:00.123+00:00" },
            "seven_day": { "utilization": 40.0, "resets_at": null },
            "seven_day_opus": null
        });
        let w = parse_usage(&v).unwrap();
        assert_eq!(w.len(), 2);
        assert_eq!(w[0].name, "5 小時");
        assert_eq!(w[0].used_percent, 23.0);
        assert!(w[0].resets_at.is_some());
    }

    #[test]
    fn 無法解析() {
        assert!(parse_usage(&json!({ "foo": 1 })).is_none());
        assert!(parse_usage(&json!({ "limits": [] })).is_none());
    }
}
