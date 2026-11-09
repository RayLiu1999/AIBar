//! 把資料模型轉成畫面要顯示的內容：卡片、托盤顏色與 Tooltip、各種時間文字。
//! 規則見規格書第 6、7 節。

use std::time::{Duration, SystemTime};

use chrono::{DateTime, Datelike, Local, Timelike, Weekday};
use slint::{ModelRc, VecModel};

use crate::model::{FetchFailure, ProviderStatus, Snapshot, UsageWindow};
use crate::{CardData, UsageLevel, UsageRow};

/// 背景刷新週期
pub const REFRESH_INTERVAL: Duration = Duration::from_secs(5 * 60);
/// 超過 3 個背景刷新週期即為舊快照
pub const STALE_AFTER: Duration = Duration::from_secs(REFRESH_INTERVAL.as_secs() * 3);
/// 顏色門檻：低於 70% 綠色、70% 至 90% 黃色、90% 以上紅色
const LEVEL_HIGH: f64 = 70.0;
const LEVEL_CRITICAL: f64 = 90.0;
const DAY: Duration = Duration::from_secs(24 * 60 * 60);

/// 托盤圖示顏色
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrayColor {
    Green,
    Yellow,
    Red,
    Gray,
}

/// 托盤圖示與 Tooltip 的內容
#[derive(Debug, PartialEq)]
pub struct TraySummary {
    pub color: TrayColor,
    /// 是否疊加警示標記
    pub warn: bool,
    pub tooltip: String,
}

/// Provider 的顯示名稱與 CLI 指令名稱（用於失敗原因提示）
fn provider_meta(id: &str) -> (&str, &str) {
    match id {
        "claude" => ("Claude", "claude"),
        "codex" => ("Codex", "codex"),
        "antigravity" => ("Antigravity", "antigravity"),
        "commandcode" => ("Command Code", "commandcode"),
        other => (other, other),
    }
}

/// 已重置：重置時間已過，快照上的用量不再成立
fn is_reset(w: &UsageWindow, now: SystemTime) -> bool {
    w.resets_at.is_some_and(|t| t <= now)
}

/// 實際要顯示的用量：已重置的額度視窗視為 0%
pub fn effective_percent(w: &UsageWindow, now: SystemTime) -> f64 {
    if is_reset(w, now) { 0.0 } else { w.used_percent.clamp(0.0, 100.0) }
}

pub fn level_of(percent: f64) -> UsageLevel {
    if percent >= LEVEL_CRITICAL {
        UsageLevel::Critical
    } else if percent >= LEVEL_HIGH {
        UsageLevel::High
    } else {
        UsageLevel::Normal
    }
}

fn tray_color_of(percent: f64) -> TrayColor {
    match level_of(percent) {
        UsageLevel::Critical => TrayColor::Red,
        UsageLevel::High => TrayColor::Yellow,
        _ => TrayColor::Green,
    }
}

/// 舊快照：取得時間已超過 3 個背景刷新週期
pub fn is_stale(s: &Snapshot, now: SystemTime) -> bool {
    now.duration_since(s.fetched_at).is_ok_and(|d| d > STALE_AFTER)
}

fn weekday_zh(w: Weekday) -> &'static str {
    match w {
        Weekday::Mon => "一",
        Weekday::Tue => "二",
        Weekday::Wed => "三",
        Weekday::Thu => "四",
        Weekday::Fri => "五",
        Weekday::Sat => "六",
        Weekday::Sun => "日",
    }
}

/// 本地時間「11/12（四）14:59」
fn local_datetime(t: SystemTime) -> String {
    let dt: DateTime<Local> = t.into();
    format!(
        "{}/{}（{}）{:02}:{:02}",
        dt.month(),
        dt.day(),
        weekday_zh(dt.weekday()),
        dt.hour(),
        dt.minute()
    )
}

/// 本地時間「14:35」
fn local_hm(t: SystemTime) -> String {
    let dt: DateTime<Local> = t.into();
    format!("{:02}:{:02}", dt.hour(), dt.minute())
}

/// 重置時間（放在每行最右欄，故省略「重置」二字）：
/// 24 小時內顯示相對時間「2 時 13 分後」，超過則顯示「11/12（四）14:59」
pub fn reset_text(w: &UsageWindow, now: SystemTime) -> String {
    let Some(t) = w.resets_at else { return String::new() };
    let left = match t.duration_since(now) {
        Ok(d) if !d.is_zero() => d,
        _ => return "已重置".into(),
    };
    if left >= DAY {
        return local_datetime(t);
    }
    // 無條件進位到分鐘，避免剩 30 秒時顯示「0 分後」
    let mins = left.as_secs().div_ceil(60);
    let (h, m) = (mins / 60, mins % 60);
    if h == 0 { format!("{m} 分後") } else { format!("{h} 時 {m} 分後") }
}

/// 快照取得時間：「剛剛更新」「更新於 N 分鐘前」…
pub fn updated_text(fetched_at: SystemTime, now: SystemTime) -> String {
    let secs = now.duration_since(fetched_at).unwrap_or_default().as_secs();
    match secs {
        0..60 => "剛剛更新".into(),
        60..3600 => format!("更新於 {} 分鐘前", secs / 60),
        3600..86400 => format!("更新於 {} 小時前", secs / 3600),
        _ => format!("更新於 {} 天前", secs / 86400),
    }
}

/// 失敗原因文字（規格書第 7 節）
pub fn failure_text(f: &FetchFailure, cli: &str) -> String {
    match f {
        FetchFailure::NotLoggedIn => format!("找不到登入資訊，請先執行 {cli}"),
        FetchFailure::TokenExpired => format!("Token 已過期，請先執行一次 {cli}"),
        FetchFailure::RateLimited { retry_at } => {
            format!("限流中，{} 自動重試", local_hm(*retry_at))
        }
        FetchFailure::Network => "無法連線，下次排程將重試".into(),
        FetchFailure::BadResponse => "回應格式改變".into(),
        FetchFailure::NotRunning => "未執行".into(),
    }
}

/// 一個 Provider 的卡片內容
pub fn card(s: &ProviderStatus, now: SystemTime) -> CardData {
    let (name, cli) = provider_meta(s.provider_id);
    let snap = s.snapshot.as_ref();
    let rows: Vec<UsageRow> = snap
        .map(|sn| sn.windows.as_slice())
        .unwrap_or_default()
        .iter()
        .map(|w| {
            let p = effective_percent(w, now);
            UsageRow {
                name: w.name.as_str().into(),
                percent: p as f32,
                percent_text: format!("{p:.0}%").into(),
                reset_text: reset_text(w, now).into(),
                level: level_of(p),
            }
        })
        .collect();
    CardData {
        name: name.into(),
        plan: snap.and_then(|sn| sn.plan.as_deref()).unwrap_or_default().into(),
        rows: ModelRc::new(VecModel::from(rows)),
        has_snapshot: snap.is_some(),
        updated_text: snap.map(|sn| updated_text(sn.fetched_at, now)).unwrap_or_default().into(),
        stale: snap.is_some_and(|sn| is_stale(sn, now)),
        failure: s.last_failure.as_ref().map(|f| failure_text(f, cli)).unwrap_or_default().into(),
        refreshing: s.refreshing,
    }
}

/// 托盤圖示顏色、警示標記與 Tooltip。`statuses` 只應包含啟用的 Provider。
pub fn tray_summary(statuses: &[&ProviderStatus], now: SystemTime) -> TraySummary {
    let mut max: Option<f64> = None;
    let mut any_fresh = false;
    let mut warn = false;
    let mut lines = Vec::new();

    for s in statuses {
        let (name, _) = provider_meta(s.provider_id);
        if s.last_failure.as_ref().is_some_and(|f| !matches!(f, FetchFailure::NotRunning)) {
            warn = true;
        }
        let Some(snap) = &s.snapshot else {
            lines.push(format!("{name}：無資料"));
            continue;
        };
        any_fresh |= !is_stale(snap, now);
        // 舊快照也計入最高用量；已重置的視窗以 0% 計
        let top = snap
            .windows
            .iter()
            .map(|w| (w, effective_percent(w, now)))
            .max_by(|a, b| a.1.total_cmp(&b.1));
        match top {
            Some((w, p)) => {
                max = Some(max.map_or(p, |m| m.max(p)));
                lines.push(format!("{name} {p:.0}%（{}）", w.name));
            }
            None => lines.push(format!("{name}：無額度資料")),
        }
    }

    // 所有啟用 Provider 都是舊快照或沒有快照時為灰色
    let color = match (any_fresh, max) {
        (true, Some(p)) => tray_color_of(p),
        _ => TrayColor::Gray,
    };
    let tooltip = if lines.is_empty() {
        "AIBar：沒有啟用的 Provider".into()
    } else {
        lines.join("\n")
    };
    TraySummary { color, warn, tooltip }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn win(pct: f64, resets_in: Option<i64>, now: SystemTime) -> UsageWindow {
        UsageWindow {
            name: "5 小時".into(),
            used_percent: pct,
            resets_at: resets_in.map(|s| offset(now, s)),
        }
    }

    fn offset(now: SystemTime, secs: i64) -> SystemTime {
        if secs >= 0 {
            now + Duration::from_secs(secs as u64)
        } else {
            now - Duration::from_secs((-secs) as u64)
        }
    }

    fn status(id: &'static str, snap: Option<Snapshot>, fail: Option<FetchFailure>) -> ProviderStatus {
        ProviderStatus { provider_id: id, snapshot: snap, last_failure: fail, refreshing: false }
    }

    fn snap(windows: Vec<UsageWindow>, age_secs: u64, now: SystemTime) -> Snapshot {
        Snapshot {
            plan: None,
            account_hash: String::new(),
            windows,
            fetched_at: now - Duration::from_secs(age_secs),
        }
    }

    #[test]
    fn 顏色門檻() {
        assert_eq!(level_of(69.9), UsageLevel::Normal);
        assert_eq!(level_of(70.0), UsageLevel::High);
        assert_eq!(level_of(89.9), UsageLevel::High);
        assert_eq!(level_of(90.0), UsageLevel::Critical);
    }

    #[test]
    fn 已重置視為零() {
        let now = SystemTime::now();
        assert_eq!(effective_percent(&win(80.0, Some(-1), now), now), 0.0);
        assert_eq!(effective_percent(&win(80.0, Some(60), now), now), 80.0);
        assert_eq!(effective_percent(&win(80.0, None, now), now), 80.0);
    }

    #[test]
    fn 重置倒數文字() {
        let now = SystemTime::now();
        assert_eq!(reset_text(&win(0.0, Some(2 * 3600 + 13 * 60), now), now), "2 時 13 分後");
        assert_eq!(reset_text(&win(0.0, Some(30), now), now), "1 分後");
        assert_eq!(reset_text(&win(0.0, Some(-5), now), now), "已重置");
        assert_eq!(reset_text(&win(0.0, None, now), now), "");
        assert!(reset_text(&win(0.0, Some(2 * 86400), now), now).contains('（'));
    }

    #[test]
    fn 更新時間文字() {
        let now = SystemTime::now();
        assert_eq!(updated_text(now - Duration::from_secs(10), now), "剛剛更新");
        assert_eq!(updated_text(now - Duration::from_secs(5 * 60), now), "更新於 5 分鐘前");
        assert_eq!(updated_text(now - Duration::from_secs(2 * 3600), now), "更新於 2 小時前");
    }

    #[test]
    fn 托盤取最高用量() {
        let now = SystemTime::now();
        let a = status("claude", Some(snap(vec![win(61.0, None, now), win(30.0, None, now)], 60, now)), None);
        let b = status("codex", Some(snap(vec![win(75.0, None, now)], 60, now)), None);
        let s = tray_summary(&[&a, &b], now);
        assert_eq!(s.color, TrayColor::Yellow);
        assert!(!s.warn);
        assert_eq!(s.tooltip, "Claude 61%（5 小時）\nCodex 75%（5 小時）");
    }

    #[test]
    fn 托盤全是舊快照時為灰色() {
        let now = SystemTime::now();
        let a = status("claude", Some(snap(vec![win(95.0, None, now)], 20 * 60, now)), None);
        let b = status("codex", None, Some(FetchFailure::NotLoggedIn));
        let s = tray_summary(&[&a, &b], now);
        assert_eq!(s.color, TrayColor::Gray);
        assert!(s.warn);
    }

    #[test]
    fn 托盤舊快照仍計入最高用量() {
        let now = SystemTime::now();
        let fresh = status("claude", Some(snap(vec![win(10.0, None, now)], 60, now)), None);
        let stale = status("codex", Some(snap(vec![win(95.0, None, now)], 20 * 60, now)), None);
        assert_eq!(tray_summary(&[&fresh, &stale], now).color, TrayColor::Red);
    }

    #[test]
    fn 未執行不加警示() {
        let now = SystemTime::now();
        let a = status("antigravity", None, Some(FetchFailure::NotRunning));
        assert!(!tray_summary(&[&a], now).warn);
    }
}
