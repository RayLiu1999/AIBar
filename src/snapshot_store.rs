//! `%APPDATA%\AiBar\snapshots.json`：各 Provider 最後一次成功取得的快照（規格書第 8 節）。
//!
//! 每次刷新成功時寫入，啟動時讀取，讓重開程式或被限流時仍有數字可看。
//! 檔案不含 token、email 或原始帳號 id；帳號只存雜湊。讀不到或格式錯誤時視為沒有快照。

use std::collections::HashMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::config::{app_dir, write_atomic};
use crate::model::{Credits, ResetBank, Snapshot, UsageWindow};

const FILE_NAME: &str = "snapshots.json";
const VERSION: u32 = 1;

#[derive(Serialize, Deserialize)]
struct StoredWindow {
    name: String,
    used_percent: f64,
    /// Unix 秒
    resets_at: Option<u64>,
    window_secs: Option<u64>,
}

#[derive(Serialize, Deserialize)]
struct StoredSnapshot {
    plan: Option<String>,
    account_hash: String,
    /// Unix 秒
    fetched_at: u64,
    windows: Vec<StoredWindow>,
    /// 金額型額度（美元）；舊版檔案沒有此欄位
    #[serde(default)]
    credits: Option<(f64, f64)>,
    /// 保留重置；舊版檔案沒有此欄位
    #[serde(default)]
    reset_bank: Option<StoredResets>,
}

#[derive(Serialize, Deserialize)]
struct StoredResets {
    /// 每一次重置的到期時間（Unix 秒）；null 表示沒有到期時間
    expires: Vec<Option<u64>>,
    usable: Option<u32>,
}

#[derive(Serialize, Deserialize)]
struct StoredFile {
    v: u32,
    providers: HashMap<String, StoredSnapshot>,
}

fn to_secs(t: SystemTime) -> u64 {
    t.duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn from_secs(s: u64) -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(s)
}

impl From<&Snapshot> for StoredSnapshot {
    fn from(s: &Snapshot) -> Self {
        Self {
            plan: s.plan.clone(),
            account_hash: s.account_hash.clone(),
            fetched_at: to_secs(s.fetched_at),
            windows: s
                .windows
                .iter()
                .map(|w| StoredWindow {
                    name: w.name.clone(),
                    used_percent: w.used_percent,
                    resets_at: w.resets_at.map(to_secs),
                    window_secs: w.window_secs,
                })
                .collect(),
            credits: s.credits.as_ref().map(|c| (c.used, c.limit)),
            reset_bank: s.resets.as_ref().map(|r| StoredResets {
                expires: r.expires.iter().map(|t| t.map(to_secs)).collect(),
                usable: r.usable,
            }),
        }
    }
}

impl From<StoredSnapshot> for Snapshot {
    fn from(s: StoredSnapshot) -> Self {
        Self {
            plan: s.plan,
            account_hash: s.account_hash,
            fetched_at: from_secs(s.fetched_at),
            windows: s
                .windows
                .into_iter()
                .map(|w| UsageWindow {
                    name: w.name,
                    used_percent: w.used_percent,
                    resets_at: w.resets_at.map(from_secs),
                    window_secs: w.window_secs,
                })
                .collect(),
            credits: s.credits.map(|(used, limit)| Credits { used, limit }),
            resets: s
                .reset_bank
                .and_then(|r| ResetBank::new(r.expires.into_iter().map(|t| t.map(from_secs)).collect(), r.usable)),
        }
    }
}

fn parse(text: &str) -> HashMap<String, Snapshot> {
    match serde_json::from_str::<StoredFile>(text) {
        Ok(f) if f.v == VERSION => f.providers.into_iter().map(|(k, v)| (k, v.into())).collect(),
        _ => HashMap::new(),
    }
}

fn serialize<'a>(snapshots: impl IntoIterator<Item = (&'a str, &'a Snapshot)>) -> String {
    let file = StoredFile {
        v: VERSION,
        providers: snapshots
            .into_iter()
            .map(|(id, s)| (id.to_string(), s.into()))
            .collect(),
    };
    serde_json::to_string_pretty(&file).unwrap_or_default()
}

/// 讀取所有快照；讀不到或格式錯誤時回傳空的
pub fn load() -> HashMap<String, Snapshot> {
    app_dir()
        .and_then(|d| std::fs::read_to_string(d.join(FILE_NAME)).ok())
        .map(|text| parse(&text))
        .unwrap_or_default()
}

/// 寫入所有快照（整個檔案覆寫）
pub fn save<'a>(snapshots: impl IntoIterator<Item = (&'a str, &'a Snapshot)>) {
    if let Err(e) = write_atomic(FILE_NAME, &serialize(snapshots)) {
        log!("[快照] 無法寫入 {FILE_NAME}：{e}");
    }
}

/// 只保留帳號與目前登入相符的快照。`current` 回傳 None（讀不到憑證）時保留，交給刷新判斷。
pub fn keep_matching(
    snapshots: HashMap<String, Snapshot>,
    current: impl Fn(&str) -> Option<String>,
) -> HashMap<String, Snapshot> {
    snapshots
        .into_iter()
        .filter(|(id, s)| match current(id) {
            Some(hash) if hash != s.account_hash => {
                log!("[快照] {id} 帳號已更換，捨棄舊快照");
                false
            }
            _ => true,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Snapshot {
        Snapshot {
            plan: Some("Pro".into()),
            account_hash: "abc".into(),
            fetched_at: from_secs(1_794_000_000),
            windows: vec![UsageWindow {
                name: "5 小時".into(),
                used_percent: 27.0,
                resets_at: Some(from_secs(1_794_010_000)),
                window_secs: Some(18000),
            }],
            credits: Some(Credits {
                used: 4.33,
                limit: 10.0,
            }),
            resets: ResetBank::new(vec![Some(from_secs(1_795_132_800)), None], Some(0)),
        }
    }

    #[test]
    fn 存入讀回一致() {
        let s = sample();
        let text = serialize([("claude", &s)]);
        let back = parse(&text);
        let b = &back["claude"];
        assert_eq!(b.plan, s.plan);
        assert_eq!(b.fetched_at, s.fetched_at);
        assert_eq!(b.windows[0].name, "5 小時");
        assert_eq!(b.windows[0].resets_at, s.windows[0].resets_at);
        assert_eq!(b.windows[0].window_secs, Some(18000));
        assert_eq!(b.credits, s.credits);
        assert_eq!(b.resets, s.resets);
    }

    #[test]
    fn 格式錯誤視為沒有快照() {
        assert!(parse("not json").is_empty());
        assert!(parse(r#"{"v":99,"providers":{}}"#).is_empty());
    }

    #[test]
    fn 帳號不符時捨棄() {
        let map = HashMap::from([("claude".to_string(), sample()), ("codex".to_string(), sample())]);
        let kept = keep_matching(map, |id| match id {
            "claude" => Some("other".into()),
            _ => None,
        });
        assert!(!kept.contains_key("claude"));
        assert!(kept.contains_key("codex"));
    }
}
