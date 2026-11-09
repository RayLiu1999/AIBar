//! Codex（訂閱）：借用 Codex CLI 已登入的 OAuth token 查詢用量。
//!
//! - 憑證：`%USERPROFILE%\.codex\auth.json` 的 `tokens.access_token`、`tokens.account_id`
//!   （設定 `CODEX_HOME` 時改讀該目錄）
//! - 端點：`GET https://chatgpt.com/backend-api/wham/usage`
//!
//! 只讀取憑證檔，絕不寫入；token 過期時不自行 refresh（ADR-0001）。

use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde_json::Value;

use super::{
    UsageProvider, account_hash, get_json, home_dir, read_json_file, window_name_from_seconds,
};
use crate::model::{FetchFailure, Snapshot, UsageWindow};

const USAGE_URL: &str = "https://chatgpt.com/backend-api/wham/usage";

pub struct Codex;

fn auth_path() -> Option<PathBuf> {
    let dir = match std::env::var_os("CODEX_HOME") {
        Some(dir) => PathBuf::from(dir),
        None => home_dir()?.join(".codex"),
    };
    Some(dir.join("auth.json"))
}

impl UsageProvider for Codex {
    fn id(&self) -> &'static str {
        "codex"
    }

    fn credentials_present(&self) -> bool {
        auth_path().is_some_and(|p| p.exists())
    }

    fn fetch(&self) -> Result<Snapshot, FetchFailure> {
        let path = auth_path().ok_or(FetchFailure::NotLoggedIn)?;
        let auth = read_json_file(&path)?;
        let tokens = &auth["tokens"];
        let token = tokens["access_token"].as_str().ok_or(FetchFailure::NotLoggedIn)?;
        let account_id = tokens["account_id"].as_str().ok_or(FetchFailure::NotLoggedIn)?;

        // access token 是 JWT，到期時間在 payload 的 exp（秒）
        if let Some(exp) = jwt_exp(token)
            && exp <= SystemTime::now()
        {
            return Err(FetchFailure::TokenExpired);
        }

        let bearer = format!("Bearer {token}");
        let body = get_json(
            self.id(),
            USAGE_URL,
            &[("Authorization", &bearer), ("ChatGPT-Account-Id", account_id)],
        )?;
        let windows = parse_usage(&body).ok_or(FetchFailure::BadResponse)?;
        let plan = body["plan_type"].as_str().filter(|s| !s.is_empty()).map(plan_name);

        Ok(Snapshot {
            plan,
            account_hash: account_hash(account_id),
            windows,
            fetched_at: SystemTime::now(),
        })
    }
}

/// 解出 JWT payload 的 exp；格式不符時回傳 None（交給伺服器判斷）
fn jwt_exp(token: &str) -> Option<SystemTime> {
    let payload = token.split('.').nth(1)?;
    let bytes = URL_SAFE_NO_PAD.decode(payload.trim_end_matches('=')).ok()?;
    let claims: Value = serde_json::from_slice(&bytes).ok()?;
    Some(UNIX_EPOCH + Duration::from_secs(claims["exp"].as_u64()?))
}

/// `plus` →「Plus」
fn plan_name(plan_type: &str) -> String {
    let mut chars = plan_type.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

fn parse_window(w: &Value, suffix: Option<&str>) -> Option<UsageWindow> {
    let secs = w["limit_window_seconds"].as_u64()?;
    let base = window_name_from_seconds(secs);
    Some(UsageWindow {
        name: match suffix {
            Some(s) => format!("{base}・{s}"),
            None => base,
        },
        used_percent: w["used_percent"].as_f64()?,
        resets_at: w["reset_at"].as_u64().map(|s| UNIX_EPOCH + Duration::from_secs(s)),
        window_secs: Some(secs),
    })
}

/// 一組 rate_limit 的主要與次要視窗
fn parse_rate_limit(rl: &Value, suffix: Option<&str>) -> Vec<UsageWindow> {
    ["primary_window", "secondary_window"]
        .into_iter()
        .filter_map(|key| parse_window(&rl[key], suffix))
        .collect()
}

/// 解析用量回應；`rate_limit` 無法解析時回傳 None（回應格式改變）
fn parse_usage(v: &Value) -> Option<Vec<UsageWindow>> {
    let mut windows = parse_rate_limit(&v["rate_limit"], None);
    if windows.is_empty() {
        return None;
    }
    // 額外的額度（有值時一併顯示），以其名稱作為視窗名稱後綴
    if let Some(extra) = v["additional_rate_limits"].as_array() {
        for item in extra {
            let label = item["limit_name"]
                .as_str()
                .or_else(|| item["metered_feature"].as_str())
                .unwrap_or("其他");
            let rl = if item["rate_limit"].is_object() { &item["rate_limit"] } else { item };
            windows.extend(parse_rate_limit(rl, Some(label)));
        }
    }
    Some(windows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn 解析用量() {
        let v = json!({
            "plan_type": "plus",
            "rate_limit": {
                "primary_window": { "used_percent": 7, "limit_window_seconds": 18000, "reset_at": 1794242712 },
                "secondary_window": { "used_percent": 41, "limit_window_seconds": 604800, "reset_at": 1794390434 }
            },
            "additional_rate_limits": null
        });
        let w = parse_usage(&v).unwrap();
        assert_eq!(w.len(), 2);
        assert_eq!(w[0].name, "5 小時");
        assert_eq!(w[0].used_percent, 7.0);
        assert_eq!(w[1].name, "每週");
        assert_eq!(w[1].resets_at, Some(UNIX_EPOCH + Duration::from_secs(1794390434)));
        assert_eq!(w[1].window_secs, Some(604800));
    }

    #[test]
    fn 額外額度() {
        let v = json!({
            "rate_limit": { "primary_window": { "used_percent": 1, "limit_window_seconds": 18000 } },
            "additional_rate_limits": [
                { "limit_name": "codex_review", "rate_limit": {
                    "primary_window": { "used_percent": 50, "limit_window_seconds": 604800 } } }
            ]
        });
        let w = parse_usage(&v).unwrap();
        assert_eq!(w[1].name, "每週・codex_review");
    }

    #[test]
    fn 無法解析() {
        assert!(parse_usage(&json!({ "rate_limit": null })).is_none());
    }

    #[test]
    fn 解出_jwt_到期時間() {
        let payload = URL_SAFE_NO_PAD.encode(br#"{"exp":1800000000}"#);
        let token = format!("eyJhbGciOiJub25lIn0.{payload}.sig");
        assert_eq!(jwt_exp(&token), Some(UNIX_EPOCH + Duration::from_secs(1800000000)));
        assert_eq!(jwt_exp("not-a-jwt"), None);
    }

    #[test]
    fn 方案名稱() {
        assert_eq!(plan_name("plus"), "Plus");
        assert_eq!(plan_name("pro"), "Pro");
    }
}
