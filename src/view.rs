//! 把資料模型轉成畫面要顯示的內容：卡片、托盤圖示與 Tooltip、各種時間文字。
//! 規則見規格書第 6、7 節。

use std::time::{Duration, SystemTime};

use chrono::{DateTime, Datelike, Local, Timelike, Weekday};
use slint::{Color, ModelRc, VecModel};

use crate::model::{Credits, FetchFailure, KeySpend, ProviderStatus, ResetBank, Snapshot, UsageWindow};
use crate::{CardData, TabData, UsageLevel, WindowRow};

/// 快照年齡超過此數個背景刷新間隔即為舊快照
const STALE_CYCLES: u32 = 3;
/// 剩餘低於此值為「偏低」（黃）
const LOW_REMAINING: f64 = 30.0;
/// 剩餘低於此值為「危急」（紅）
const CRITICAL_REMAINING: f64 = 10.0;

/// 用量顯示方式（剩餘／已使用），存在 config.json
pub use crate::config::Display as DisplayMode;

/// Provider 的固定資訊
pub struct ProviderMeta {
    pub name: &'static str,
    /// 失敗原因提示中要使用者執行的 CLI 指令
    pub cli: &'static str,
    /// 卡片與頁籤上的字母方塊
    pub mono: &'static str,
    /// 代表色
    pub accent: [u8; 3],
    pub source: &'static str,
    pub usage_url: &'static str,
}

pub fn provider_meta(id: &str) -> ProviderMeta {
    match id {
        "claude" => ProviderMeta {
            name: "Claude",
            cli: "claude",
            mono: "C",
            accent: [0xE0, 0x8A, 0x67],
            source: "Claude Code 登入憑證（OAuth）",
            usage_url: "https://claude.ai/settings/usage",
        },
        "codex" => ProviderMeta {
            name: "Codex",
            cli: "codex",
            mono: "X",
            accent: [0x4C, 0xB5, 0xC1],
            source: "Codex CLI 登入憑證（OAuth）",
            usage_url: "https://chatgpt.com/codex/settings/usage",
        },
        "antigravity" => ProviderMeta {
            name: "Antigravity",
            cli: "agy",
            mono: "A",
            accent: [0x7B, 0xC8, 0x8A],
            source: "本機 Antigravity App；App 沒開時改用 agy CLI",
            usage_url: "",
        },
        "commandcode" => ProviderMeta {
            name: "Command Code",
            cli: "commandcode",
            mono: "K",
            accent: [0xA9, 0x8B, 0xE6],
            source: "Command Code CLI 登入憑證（API key）",
            usage_url: "https://commandcode.ai/billing",
        },
        "openrouter" => ProviderMeta {
            name: "OpenRouter",
            cli: "",
            mono: "O",
            accent: [0x8C, 0x9E, 0xF5],
            source: if crate::platform::IS_MACOS {
                "設定中輸入的 API key（存在 Keychain）"
            } else {
                "設定中輸入的 API key（DPAPI 加密）"
            },
            usage_url: "https://openrouter.ai/settings/credits",
        },
        _ => ProviderMeta {
            name: "未知",
            cli: "",
            mono: "?",
            accent: [0x9c, 0xa3, 0xaf],
            source: "",
            usage_url: "",
        },
    }
}

/// 已重置：重置時間已過，快照上的用量不再成立
fn is_reset(w: &UsageWindow, now: SystemTime) -> bool {
    w.resets_at.is_some_and(|t| t <= now)
}

/// 已使用百分比；已重置的額度視窗視為 0%
pub fn used_percent(w: &UsageWindow, now: SystemTime) -> f64 {
    if is_reset(w, now) {
        0.0
    } else {
        w.used_percent.clamp(0.0, 100.0)
    }
}

/// 剩餘百分比；已重置的額度視窗視為 100%
pub fn remaining_percent(w: &UsageWindow, now: SystemTime) -> f64 {
    100.0 - used_percent(w, now)
}

pub fn level_of(remaining: f64) -> UsageLevel {
    if remaining < CRITICAL_REMAINING {
        UsageLevel::Critical
    } else if remaining < LOW_REMAINING {
        UsageLevel::Low
    } else {
        UsageLevel::Normal
    }
}

/// 舊快照：取得時間已超過該 Provider 的 3 個背景刷新間隔
pub fn is_stale(s: &Snapshot, interval: Duration, now: SystemTime) -> bool {
    now.duration_since(s.fetched_at)
        .is_ok_and(|d| d > interval * STALE_CYCLES)
}

/// 刷新間隔的文字：「每 10 分鐘」
fn interval_text(d: Duration) -> String {
    format!("每 {} 分鐘", d.as_secs() / 60)
}

/// 剩餘最少的額度視窗與其剩餘百分比
pub fn tightest(s: &Snapshot, now: SystemTime) -> Option<(&UsageWindow, f64)> {
    s.windows
        .iter()
        .map(|w| (w, remaining_percent(w, now)))
        .min_by(|a, b| a.1.total_cmp(&b.1))
}

/// 進度：已使用比例與時間經過比例的比較
#[derive(Debug, PartialEq)]
pub struct Pace {
    /// 依時間比例「此刻應已使用」的百分比
    pub expected_used: f64,
    /// 預期已使用減實際已使用；正值表示用得比預期少
    pub diff: f64,
    /// 以目前平均速度推算的用完時間；能撐到重置時為 None
    pub runout_at: Option<SystemTime>,
}

/// 計算進度；缺少視窗長度或重置時間、或已重置時回傳 None
pub fn pace(w: &UsageWindow, now: SystemTime) -> Option<Pace> {
    let len = w.window_secs? as f64;
    let left = w.resets_at?.duration_since(now).ok()?.as_secs_f64();
    let elapsed = len - left;
    if len <= 0.0 || elapsed <= 0.0 {
        return None;
    }
    let used = w.used_percent.clamp(0.0, 100.0);
    let expected_used = elapsed / len * 100.0;
    let runout_at = if used >= 100.0 {
        Some(now)
    } else if used > 0.0 {
        // 剩餘量 ÷ 平均速度 = 還能用多久
        let secs = (100.0 - used) / (used / elapsed);
        (secs < left).then(|| now + Duration::from_secs_f64(secs))
    } else {
        None
    };
    Some(Pace {
        expected_used,
        diff: expected_used - used,
        runout_at,
    })
}

/// 進度說明，例如「比預期少用 32% · 可撐到重置」
pub fn pace_text(p: &Pace, now: SystemTime) -> String {
    let head = if p.diff >= 0.5 {
        format!("比預期少用 {:.0}%", p.diff)
    } else if p.diff <= -0.5 {
        format!("比預期多用 {:.0}%", -p.diff)
    } else {
        "與預期相當".to_string()
    };
    let tail = match p.runout_at {
        Some(t) if t <= now => "已用完".to_string(),
        Some(t) => format!("約 {}後用完", duration_text(t.duration_since(now).unwrap_or_default())),
        None => "可撐到重置".to_string(),
    };
    format!("{head} · {tail}")
}

/// 時間長度：「13 分」「2 時 13 分」「3 天 16 小時」（無條件進位到分鐘）
pub fn duration_text(d: Duration) -> String {
    let mins = d.as_secs().div_ceil(60).max(1);
    let (days, hours, m) = (mins / 1440, mins / 60 % 24, mins % 60);
    if days > 0 {
        format!("{days} 天 {hours} 小時")
    } else if hours > 0 {
        format!("{hours} 時 {m} 分")
    } else {
        format!("{m} 分")
    }
}

/// 重置倒數：「2 時 13 分後重置」「3 天 16 小時後重置」「已重置」；沒有重置時間時為空字串
pub fn reset_text(w: &UsageWindow, now: SystemTime) -> String {
    match w.resets_at.map(|t| t.duration_since(now)) {
        None => String::new(),
        Some(Ok(d)) if !d.is_zero() => format!("{}後重置", duration_text(d)),
        Some(_) => "已重置".into(),
    }
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

/// 絕對時間：「今天 01:10」「明天 01:10」「11/13（五）16:00」
pub fn absolute_time(t: SystemTime, now: SystemTime) -> String {
    let dt: DateTime<Local> = t.into();
    let today: DateTime<Local> = now.into();
    let hm = format!("{:02}:{:02}", dt.hour(), dt.minute());
    let days = (dt.date_naive() - today.date_naive()).num_days();
    match days {
        0 => format!("今天 {hm}"),
        1 => format!("明天 {hm}"),
        _ => format!("{}/{}（{}）{hm}", dt.month(), dt.day(), weekday_zh(dt.weekday())),
    }
}

/// 本地時間「14:35」
fn local_hm(t: SystemTime) -> String {
    let dt: DateTime<Local> = t.into();
    format!("{:02}:{:02}", dt.hour(), dt.minute())
}

/// 快照取得時間：「剛剛更新」「N 分鐘前更新」…
pub fn updated_text(fetched_at: SystemTime, now: SystemTime) -> String {
    let secs = now.duration_since(fetched_at).unwrap_or_default().as_secs();
    match secs {
        0..60 => "剛剛更新".into(),
        60..3600 => format!("{} 分鐘前更新", secs / 60),
        3600..86400 => format!("{} 小時前更新", secs / 3600),
        _ => format!("{} 天前更新", secs / 86400),
    }
}

/// 失敗原因文字（規格書第 7 節）
pub fn failure_text(f: &FetchFailure, provider_id: &str) -> String {
    let cli = provider_meta(provider_id).cli;
    match (f, provider_id) {
        (FetchFailure::NotLoggedIn, "openrouter") => "尚未設定 API key，請到設定 → Provider 輸入".into(),
        (FetchFailure::TokenExpired, "openrouter") => "API key 無效或已撤銷，請到設定 → Provider 重新輸入".into(),
        (FetchFailure::NotLoggedIn, "antigravity") => "找不到 Antigravity，請先安裝並登入".into(),
        _ => failure_text_generic(f, cli),
    }
}

fn failure_text_generic(f: &FetchFailure, cli: &str) -> String {
    match f {
        FetchFailure::NotLoggedIn => format!("找不到登入資訊，請先在終端機執行 {cli}"),
        FetchFailure::TokenExpired => format!("Token 已過期，請在終端機執行一次 {cli}"),
        FetchFailure::RateLimited { retry_at } => {
            format!("限流中，{} 自動重試", local_hm(*retry_at))
        }
        FetchFailure::Network => "無法連線，下次排程將重試".into(),
        FetchFailure::BadResponse => "回應格式改變".into(),
        FetchFailure::NotRunning => "Antigravity 未執行（也找不到 agy CLI），開啟 App 後會自動更新".into(),
        FetchFailure::AccessDenied => "無法讀取 Keychain（存取被拒），按「立即刷新」可重試".into(),
    }
}

/// 金額型額度文字：「本期已用 $4.33 / $10.00」；上限 0 表示無上限。
/// OpenRouter 的是整個帳戶的儲值，寫成「帳戶已用」以便和 Key 花費區分
pub fn credits_text(c: &Credits, provider_id: &str) -> String {
    let label = if provider_id == "openrouter" {
        "帳戶已用"
    } else {
        "本期已用"
    };
    if c.limit > 0.0 {
        format!("{label} ${:.2} / ${:.2}", c.used, c.limit)
    } else {
        format!("本月已用 ${:.2}（無上限）", c.used)
    }
}

/// 金額：「$1.23」；不到一分錢時多顯示幾位，避免看起來像沒用過
fn usd(v: f64) -> String {
    if v <= 0.0 {
        "$0.00".into()
    } else if v < 0.0001 {
        "<$0.0001".into()
    } else if v < 0.01 {
        format!("${v:.4}")
    } else {
        format!("${v:.2}")
    }
}

/// Key 花費摘要：「此 key 已用 $1.23 · 本月 $0.17」
pub fn key_spend_text(k: &KeySpend) -> String {
    format!("此 key 已用 {} · 本月 {}", usd(k.total), usd(k.month))
}

/// Key 花費明細（Provider 頁籤）
pub fn key_spend_detail(k: &KeySpend) -> String {
    format!("今日 {} · 本週 {}（UTC）", usd(k.day), usd(k.week))
}

/// 最快到期的重置在這段時間內到期時，以警示色提醒
const RESET_EXPIRY_SOON: Duration = Duration::from_secs(3 * 86400);

/// 保留重置摘要：「已存 2 次重置・最快 11/20（五）08:00 到期」「可套用 1 次重置（共 2 次）」；
/// 一次都沒有時為空字串
pub fn resets_text(r: &ResetBank, now: SystemTime) -> String {
    let left = r.remaining(now);
    let n = left.len();
    if n == 0 {
        return String::new();
    }
    let count = match r.usable {
        Some(u) if u >= n as u32 => format!("可套用 {n} 次重置"),
        Some(u) if u > 0 => format!("可套用 {u} 次重置（共 {n} 次）"),
        _ => format!("已存 {n} 次重置"),
    };
    match left[0] {
        Some(t) if n > 1 => format!("{count}・最快 {} 到期", absolute_time(t, now)),
        Some(t) => format!("{count}・{} 到期", absolute_time(t, now)),
        None => count,
    }
}

/// 最快到期的重置是否快到期了
pub fn resets_expiring_soon(r: &ResetBank, now: SystemTime) -> bool {
    r.remaining(now)
        .first()
        .copied()
        .flatten()
        .is_some_and(|t| t.duration_since(now).unwrap_or_default() <= RESET_EXPIRY_SOON)
}

/// 保留重置明細（Provider 頁籤）：每次重置一行到期時間，最後一行是套用方式
pub fn resets_detail(r: &ResetBank, provider_id: &str, now: SystemTime) -> String {
    let left = r.remaining(now);
    if left.is_empty() {
        return String::new();
    }
    let mut lines: Vec<String> = left
        .iter()
        .flatten()
        .map(|&t| {
            let d = t.duration_since(now).unwrap_or_default();
            format!("{} 到期（剩 {}）", absolute_time(t, now), duration_text(d))
        })
        .collect();
    let howto = match provider_id {
        "claude" => "在 claude.ai 或桌面版的「設定 → 用量」按 Reset for free 套用".to_string(),
        "codex" if r.usable.unwrap_or(0) > 0 => "在 codex 中輸入 /usage 套用".to_string(),
        "codex" => "撞到上限時，在 codex 中輸入 /usage 套用".to_string(),
        _ => String::new(),
    };
    if !howto.is_empty() {
        lines.push(howto);
    }
    lines.join("\n")
}

fn color(rgb: [u8; 3]) -> Color {
    Color::from_rgb_u8(rgb[0], rgb[1], rgb[2])
}

fn row(w: &UsageWindow, mode: DisplayMode, now: SystemTime) -> WindowRow {
    let remaining = remaining_percent(w, now);
    let (label, shown) = match mode {
        DisplayMode::Remaining => ("剩餘", remaining),
        DisplayMode::Used => ("已使用", 100.0 - remaining),
    };
    let p = if is_reset(w, now) { None } else { pace(w, now) };
    // 刻度：依時間比例此刻應剩（或應已使用）的位置
    let mark = p.as_ref().map_or(-1.0, |p| match mode {
        DisplayMode::Remaining => 100.0 - p.expected_used,
        DisplayMode::Used => p.expected_used,
    });
    WindowRow {
        name: w.name.as_str().into(),
        value_text: format!("{label} {shown:.0}%").into(),
        bar: shown as f32,
        mark: mark as f32,
        pace_text: p.as_ref().map(|p| pace_text(p, now)).unwrap_or_default().into(),
        reset_text: reset_text(w, now).into(),
        reset_abs: w
            .resets_at
            .filter(|t| *t > now)
            .map(|t| absolute_time(t, now))
            .unwrap_or_default()
            .into(),
        level: level_of(remaining),
    }
}

/// Provider 頁籤
pub fn tab(provider_id: &str) -> TabData {
    let meta = provider_meta(provider_id);
    TabData {
        name: meta.name.into(),
        accent: color(meta.accent),
    }
}

/// 憑證組的標題：「OpenRouter」或「OpenRouter · 工作」
pub fn title(s: &ProviderStatus) -> String {
    let name = provider_meta(s.provider_id).name;
    if s.group_name.is_empty() {
        name.to_string()
    } else {
        format!("{name} · {}", s.group_name)
    }
}

/// 有需要警示的失敗原因
pub fn has_warning(s: &ProviderStatus) -> bool {
    s.last_failure
        .as_ref()
        .is_some_and(|f| !matches!(f, FetchFailure::NotRunning))
}

/// 同帳戶的多組只留一份帳戶儲值（儲值餘額與帳戶已用），其餘只留 key 自己的資料。
/// 留在第一個沒有失敗原因的組，免得停在舊數字；都失敗時留在第一組
pub fn share_account_funds(statuses: &mut [ProviderStatus]) {
    let account = |s: &ProviderStatus| Some((s.provider_id, s.snapshot.as_ref()?.funds_account.clone()?));
    for i in 0..statuses.len() {
        let Some(key) = account(&statuses[i]) else { continue };
        let same = |s: &&ProviderStatus| account(s).as_ref() == Some(&key);
        let holder = statuses
            .iter()
            .filter(same)
            .find(|s| s.last_failure.is_none())
            .or_else(|| statuses.iter().find(same))
            .map(|s| s.group_id.clone());
        if holder.as_deref() != Some(statuses[i].group_id.as_str())
            && let Some(sn) = &mut statuses[i].snapshot
        {
            sn.credits = None;
            sn.windows.retain(|w| w.name != crate::providers::openrouter::BALANCE);
        }
    }
}

/// 同一個 Provider 的多組中，代表托盤的那一組：剩餘最少的；都沒有資料時為第一組
pub fn tightest_group<'a>(groups: &[&'a ProviderStatus], now: SystemTime) -> Option<&'a ProviderStatus> {
    groups
        .iter()
        .filter_map(|s| Some((*s, tightest(s.snapshot.as_ref()?, now)?.1)))
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(s, _)| s)
        .or_else(|| groups.first().copied())
}

/// 一個憑證組的卡片內容（總覽與 Provider 頁籤共用）
pub fn card(s: &ProviderStatus, mode: DisplayMode, now: SystemTime) -> CardData {
    let meta = provider_meta(s.provider_id);
    let snap = s.snapshot.as_ref();
    let resets = snap.and_then(|sn| sn.resets.as_ref());
    let spend = snap.and_then(|sn| sn.key_spend.as_ref());
    let rows: Vec<WindowRow> = snap
        .map(|sn| sn.windows.iter().map(|w| row(w, mode, now)).collect())
        .unwrap_or_default();
    // 「複製指令」只對執行一次 CLI 就能恢復的失敗原因提供
    let failure_cmd = match &s.last_failure {
        Some(FetchFailure::NotLoggedIn | FetchFailure::TokenExpired) => meta.cli,
        _ => "",
    };
    // Antigravity 未執行時不算失敗：只顯示提示，不顯示警示色
    let not_running = matches!(s.last_failure, Some(FetchFailure::NotRunning));
    let next_refresh = snap
        .and_then(|sn| (sn.fetched_at + s.refresh_interval).duration_since(now).ok())
        .map(|d| format!("約 {}後（{}）", duration_text(d), interval_text(s.refresh_interval)))
        .unwrap_or_else(|| interval_text(s.refresh_interval));
    CardData {
        id: s.group_id.as_str().into(),
        name: title(s).into(),
        mono: meta.mono.into(),
        accent: color(meta.accent),
        plan: snap.and_then(|sn| sn.plan.as_deref()).unwrap_or_default().into(),
        rows: ModelRc::new(VecModel::from(rows)),
        has_snapshot: snap.is_some(),
        updated_text: snap
            .map(|sn| updated_text(sn.fetched_at, now))
            .unwrap_or_default()
            .into(),
        stale: snap.is_some_and(|sn| is_stale(sn, s.refresh_interval, now)),
        failure: s
            .last_failure
            .as_ref()
            .filter(|_| !not_running)
            .map(|f| failure_text(f, s.provider_id))
            .unwrap_or_default()
            .into(),
        notice: if not_running {
            failure_text(&FetchFailure::NotRunning, s.provider_id).into()
        } else {
            Default::default()
        },
        failure_cmd: failure_cmd.into(),
        credits_text: snap
            .and_then(|sn| sn.credits.as_ref())
            .map(|c| credits_text(c, s.provider_id))
            .unwrap_or_default()
            .into(),
        spend_text: spend.map(key_spend_text).unwrap_or_default().into(),
        spend_detail: spend.map(key_spend_detail).unwrap_or_default().into(),
        resets_text: resets.map(|r| resets_text(r, now)).unwrap_or_default().into(),
        resets_detail: resets
            .map(|r| resets_detail(r, s.provider_id, now))
            .unwrap_or_default()
            .into(),
        resets_soon: resets.is_some_and(|r| resets_expiring_soon(r, now)),
        refreshing: s.refreshing,
        source: meta.source.into(),
        usage_url: meta.usage_url.into(),
        next_refresh: next_refresh.into(),
    }
}

/// 總覽最上方的一行：「最先用完：Codex 每週，剩 59%」
pub fn overview_headline(statuses: &[&ProviderStatus], now: SystemTime) -> String {
    statuses
        .iter()
        .filter_map(|s| {
            let (w, r) = tightest(s.snapshot.as_ref()?, now)?;
            Some((title(s), w, r))
        })
        .min_by(|a, b| a.2.total_cmp(&b.2))
        .map(|(name, w, r)| format!("最先用完：{name} {}，剩 {r:.0}%", w.name))
        .unwrap_or_default()
}

/// 托盤圖示的底色
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrayFill {
    Normal,
    Low,
    Critical,
}

/// 托盤圖示與 Tooltip 的內容
#[derive(Clone, Debug, PartialEq)]
pub struct TrayView {
    /// 圖示中央的文字：剩餘百分比、「滿」或「–」
    pub text: String,
    /// 外框顏色：焦點 Provider 的代表色；舊快照或沒有資料時為 None（灰色）
    pub rim: Option<[u8; 3]>,
    pub fill: TrayFill,
    /// 數字變淡（舊快照或沒有資料）
    pub dim: bool,
    /// 右下角警示標記
    pub warn: bool,
    pub tooltip: String,
}

/// 托盤圖示內容。`focus` 為焦點 Provider，`enabled` 為所有啟用的 Provider（依顯示順序）。
pub fn tray_view(focus: Option<&ProviderStatus>, enabled: &[&ProviderStatus], now: SystemTime) -> TrayView {
    let Some(focus) = focus else {
        return TrayView {
            text: "–".into(),
            rim: None,
            fill: TrayFill::Normal,
            dim: true,
            warn: false,
            tooltip: "AIBar：沒有啟用的 Provider".into(),
        };
    };
    let meta = provider_meta(focus.provider_id);
    let name = title(focus);
    // 同一個 Provider 的任一組失敗都要看得出來
    let warn = has_warning(focus)
        || enabled
            .iter()
            .any(|s| s.provider_id == focus.provider_id && has_warning(s));
    let snap = focus.snapshot.as_ref();
    let top = snap.and_then(|s| tightest(s, now));
    let stale = snap.is_none_or(|s| is_stale(s, focus.refresh_interval, now));

    let (text, fill) = match top {
        Some((_, r)) => (
            if r >= 99.5 {
                "滿".to_string()
            } else {
                format!("{r:.0}")
            },
            match level_of(r) {
                UsageLevel::Critical => TrayFill::Critical,
                UsageLevel::Low => TrayFill::Low,
                _ => TrayFill::Normal,
            },
        ),
        None => ("–".to_string(), TrayFill::Normal),
    };

    // 第一行：焦點 Provider 的摘要
    let mut lines = vec![match (top, &focus.last_failure) {
        (Some((w, r)), _) => {
            let reset = reset_text(w, now);
            if reset.is_empty() {
                format!("{name}・{} 剩 {r:.0}%", w.name)
            } else {
                format!("{name}・{} 剩 {r:.0}%・{reset}", w.name)
            }
        }
        (None, Some(f)) => format!("{name}・{}", failure_text(f, focus.provider_id)),
        (None, None) => format!("{name}・讀取中"),
    }];
    // 其後每個啟用 Provider 一行
    if enabled.len() > 1 {
        for s in enabled {
            let name = title(s);
            let line = match &s.snapshot {
                Some(sn) => sn
                    .windows
                    .iter()
                    .map(|w| format!("{} {:.0}%", w.name, remaining_percent(w, now)))
                    .collect::<Vec<_>>()
                    .join("・"),
                None => "無資料".into(),
            };
            lines.push(format!("{name}　{line}"));
        }
    }
    if let Some(s) = snap {
        lines.push(updated_text(s.fetched_at, now));
    }

    TrayView {
        text,
        rim: (!stale).then_some(meta.accent),
        fill,
        dim: stale,
        warn,
        tooltip: lines.join("\n"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOUR: u64 = 3600;

    fn at(now: SystemTime, secs: i64) -> SystemTime {
        if secs >= 0 {
            now + Duration::from_secs(secs as u64)
        } else {
            now - Duration::from_secs((-secs) as u64)
        }
    }

    fn win(name: &str, used: f64, resets_in: Option<i64>, len: Option<u64>, now: SystemTime) -> UsageWindow {
        UsageWindow {
            name: name.into(),
            used_percent: used,
            resets_at: resets_in.map(|s| at(now, s)),
            window_secs: len,
        }
    }

    fn snap(windows: Vec<UsageWindow>, age_secs: u64, now: SystemTime) -> Snapshot {
        Snapshot {
            plan: None,
            account_hash: String::new(),
            windows,
            credits: None,
            funds_account: None,
            key_spend: None,
            resets: None,
            fetched_at: now - Duration::from_secs(age_secs),
        }
    }

    fn status(id: &'static str, snap: Option<Snapshot>, fail: Option<FetchFailure>) -> ProviderStatus {
        ProviderStatus {
            provider_id: id,
            group_id: id.into(),
            group_name: String::new(),
            snapshot: snap,
            last_failure: fail,
            refreshing: false,
            refresh_interval: Duration::from_secs(5 * 60),
        }
    }

    #[test]
    fn 各_provider_的失敗文字() {
        assert!(failure_text(&FetchFailure::NotLoggedIn, "openrouter").contains("API key"));
        assert!(failure_text(&FetchFailure::TokenExpired, "codex").contains("codex"));
        assert!(failure_text(&FetchFailure::NotRunning, "antigravity").contains("未執行"));
        assert_eq!(
            credits_text(
                &Credits {
                    used: 4.333,
                    limit: 10.0
                },
                "commandcode"
            ),
            "本期已用 $4.33 / $10.00"
        );
        assert_eq!(
            credits_text(&Credits { used: 3.2, limit: 0.0 }, "commandcode"),
            "本月已用 $3.20（無上限）"
        );
        assert_eq!(
            credits_text(&Credits { used: 0.2, limit: 5.0 }, "openrouter"),
            "帳戶已用 $0.20 / $5.00"
        );
    }

    #[test]
    fn key_花費文字() {
        let k = KeySpend {
            total: 1.234,
            month: 0.0042,
            week: 0.00001,
            day: 0.0,
        };
        assert_eq!(key_spend_text(&k), "此 key 已用 $1.23 · 本月 $0.0042");
        assert_eq!(key_spend_detail(&k), "今日 $0.00 · 本週 <$0.0001（UTC）");
    }

    #[test]
    fn 保留重置文字() {
        let now = SystemTime::now();
        let in_days = |d: u64| Some(now + Duration::from_secs(d * 86400 + 30));
        let bank = |expires: Vec<Option<SystemTime>>, usable| ResetBank::new(expires, usable).unwrap();

        // 只有次數、沒有到期時間
        assert_eq!(resets_text(&bank(vec![None, None], Some(0)), now), "已存 2 次重置");
        assert_eq!(resets_text(&bank(vec![None, None], Some(2)), now), "可套用 2 次重置");
        assert_eq!(
            resets_text(&bank(vec![None, None], Some(1)), now),
            "可套用 1 次重置（共 2 次）"
        );

        // 有到期時間：摘要寫最快到期的那一次
        let b = bank(vec![in_days(20), in_days(10)], None);
        let soonest = absolute_time(in_days(10).unwrap(), now);
        assert_eq!(resets_text(&b, now), format!("已存 2 次重置・最快 {soonest} 到期"));
        assert!(!resets_expiring_soon(&b, now));
        let one = bank(vec![in_days(2)], None);
        assert!(resets_text(&one, now).starts_with("已存 1 次重置・"));
        assert!(resets_expiring_soon(&one, now));

        // 已過期的不算
        let expired = bank(vec![Some(now - Duration::from_secs(1))], None);
        assert_eq!(resets_text(&expired, now), "");
        assert_eq!(resets_detail(&expired, "codex", now), "");

        // 明細：每次一行，最後是套用方式
        let d = resets_detail(&b, "codex", now);
        let lines: Vec<&str> = d.lines().collect();
        assert_eq!(lines.len(), 3);
        assert!(lines[0].starts_with(&soonest) && lines[0].contains("（剩 10 天"));
        assert_eq!(lines[2], "撞到上限時，在 codex 中輸入 /usage 套用");
        assert!(resets_detail(&b, "claude", now).ends_with("按 Reset for free 套用"));
    }

    #[test]
    fn 剩餘門檻() {
        assert_eq!(level_of(30.0), UsageLevel::Normal);
        assert_eq!(level_of(29.9), UsageLevel::Low);
        assert_eq!(level_of(10.0), UsageLevel::Low);
        assert_eq!(level_of(9.9), UsageLevel::Critical);
    }

    #[test]
    fn 已重置視為全滿() {
        let now = SystemTime::now();
        assert_eq!(remaining_percent(&win("w", 80.0, Some(-1), None, now), now), 100.0);
        assert_eq!(remaining_percent(&win("w", 80.0, Some(60), None, now), now), 20.0);
    }

    #[test]
    fn 時間文字() {
        let now = SystemTime::now();
        let w = |s| win("w", 0.0, Some(s), None, now);
        assert_eq!(reset_text(&w(2 * 3600 + 13 * 60), now), "2 時 13 分後重置");
        assert_eq!(reset_text(&w(30), now), "1 分後重置");
        assert_eq!(reset_text(&w(3 * 86400 + 16 * 3600), now), "3 天 16 小時後重置");
        assert_eq!(reset_text(&w(-5), now), "已重置");
        assert_eq!(updated_text(now - Duration::from_secs(10), now), "剛剛更新");
        assert_eq!(updated_text(now - Duration::from_secs(5 * 60), now), "5 分鐘前更新");
    }

    #[test]
    fn 進度_少用可撐到重置() {
        let now = SystemTime::now();
        // 5 小時視窗剩 2 小時：已過 60%；用了 28% → 少用 32%
        let w = win("5 小時", 28.0, Some(2 * HOUR as i64), Some(5 * HOUR), now);
        let p = pace(&w, now).unwrap();
        assert!((p.expected_used - 60.0).abs() < 0.01);
        assert!((p.diff - 32.0).abs() < 0.01);
        assert_eq!(p.runout_at, None);
        assert_eq!(pace_text(&p, now), "比預期少用 32% · 可撐到重置");
    }

    #[test]
    fn 進度_多用會提早用完() {
        let now = SystemTime::now();
        // 已過 1 小時（20%）、用了 50% → 每小時 50%，再 1 小時用完，早於重置
        let w = win("5 小時", 50.0, Some(4 * HOUR as i64), Some(5 * HOUR), now);
        let p = pace(&w, now).unwrap();
        assert!((p.diff + 30.0).abs() < 0.01);
        assert_eq!(pace_text(&p, now), "比預期多用 30% · 約 1 時 0 分後用完");
    }

    #[test]
    fn 進度_資料不足時不顯示() {
        let now = SystemTime::now();
        assert!(pace(&win("w", 10.0, Some(3600), None, now), now).is_none());
        assert!(pace(&win("w", 10.0, None, Some(18000), now), now).is_none());
        assert!(pace(&win("w", 10.0, Some(-1), Some(18000), now), now).is_none());
    }

    #[test]
    fn 卡片的刻度與顯示方式() {
        let now = SystemTime::now();
        let s = status(
            "claude",
            Some(snap(
                vec![win("5 小時", 28.0, Some(2 * HOUR as i64), Some(5 * HOUR), now)],
                60,
                now,
            )),
            None,
        );
        let c = card(&s, DisplayMode::Remaining, now);
        let r = slint::Model::row_data(&c.rows, 0).unwrap();
        assert_eq!(r.value_text, "剩餘 72%");
        assert_eq!(r.bar, 72.0);
        assert!((r.mark - 40.0).abs() < 0.01);
        let c = card(&s, DisplayMode::Used, now);
        let r = slint::Model::row_data(&c.rows, 0).unwrap();
        assert_eq!(r.value_text, "已使用 28%");
        assert!((r.mark - 60.0).abs() < 0.01);
    }

    #[test]
    fn 總覽標題取剩最少() {
        let now = SystemTime::now();
        let a = status(
            "claude",
            Some(snap(vec![win("每週", 33.0, None, None, now)], 60, now)),
            None,
        );
        let b = status(
            "codex",
            Some(snap(vec![win("每週", 41.0, None, None, now)], 60, now)),
            None,
        );
        assert_eq!(overview_headline(&[&a, &b], now), "最先用完：Codex 每週，剩 59%");
    }

    #[test]
    fn 托盤顯示焦點的最少剩餘() {
        let now = SystemTime::now();
        let a = status(
            "codex",
            Some(snap(
                vec![
                    win("5 小時", 7.0, Some(3600), None, now),
                    win("每週", 41.0, None, None, now),
                ],
                60,
                now,
            )),
            None,
        );
        let t = tray_view(Some(&a), &[&a], now);
        assert_eq!(t.text, "59");
        assert_eq!(t.rim, Some(provider_meta("codex").accent));
        assert_eq!(t.fill, TrayFill::Normal);
        assert!(!t.dim && !t.warn);
        assert!(t.tooltip.starts_with("Codex・每週 剩 59%"));
    }

    #[test]
    fn 托盤低量轉色與滿() {
        let now = SystemTime::now();
        let low = status(
            "claude",
            Some(snap(vec![win("w", 80.0, None, None, now)], 0, now)),
            None,
        );
        assert_eq!(tray_view(Some(&low), &[&low], now).fill, TrayFill::Low);
        let crit = status(
            "claude",
            Some(snap(vec![win("w", 95.0, None, None, now)], 0, now)),
            None,
        );
        assert_eq!(tray_view(Some(&crit), &[&crit], now).fill, TrayFill::Critical);
        let full = status("claude", Some(snap(vec![win("w", 0.0, None, None, now)], 0, now)), None);
        assert_eq!(tray_view(Some(&full), &[&full], now).text, "滿");
    }

    #[test]
    fn 托盤舊快照轉灰並警示() {
        let now = SystemTime::now();
        let a = status(
            "claude",
            Some(snap(vec![win("w", 50.0, None, None, now)], 20 * 60, now)),
            Some(FetchFailure::TokenExpired),
        );
        let t = tray_view(Some(&a), &[&a], now);
        assert_eq!(t.rim, None);
        assert!(t.dim && t.warn);
        let none = status("codex", None, Some(FetchFailure::NotLoggedIn));
        assert_eq!(tray_view(Some(&none), &[&none], now).text, "–");
        let off = status("antigravity", None, Some(FetchFailure::NotRunning));
        assert!(!tray_view(Some(&off), &[&off], now).warn);
    }

    #[test]
    fn 同帳戶的儲值只留一份() {
        let now = SystemTime::now();
        let group = |id: &str, account: Option<&str>, fail| {
            let mut sn = snap(
                vec![
                    win("每月上限", 10.0, None, None, now),
                    win("儲值餘額", 20.0, None, None, now),
                ],
                0,
                now,
            );
            sn.credits = Some(Credits { used: 1.0, limit: 5.0 });
            sn.funds_account = account.map(Into::into);
            ProviderStatus {
                group_id: id.into(),
                ..status("openrouter", Some(sn), fail)
            }
        };
        // 留有儲值的組
        let kept = |mut v: Vec<ProviderStatus>| -> Vec<bool> {
            share_account_funds(&mut v);
            v.iter()
                .map(|s| {
                    let sn = s.snapshot.as_ref().unwrap();
                    assert_eq!(sn.windows[0].name, "每月上限");
                    assert_eq!(sn.credits.is_some(), sn.windows.len() == 2);
                    sn.credits.is_some()
                })
                .collect()
        };
        let a = group("a", Some("x"), None);
        let b = group("b", Some("x"), None);
        let other = group("c", Some("y"), None);
        assert_eq!(kept(vec![a.clone(), b.clone(), other]), [true, false, true]);
        // 第一組失敗時由第二組顯示；都失敗時留在第一組
        let failed = group("a", Some("x"), Some(FetchFailure::TokenExpired));
        assert_eq!(kept(vec![failed.clone(), b.clone()]), [false, true]);
        let failed_b = group("b", Some("x"), Some(FetchFailure::Network));
        assert_eq!(kept(vec![failed, failed_b]), [true, false]);
        // 帳戶不明時照樣顯示
        assert_eq!(kept(vec![group("a", None, None), group("b", None, None)]), [true, true]);
    }

    #[test]
    fn 多組的標題與托盤() {
        let now = SystemTime::now();
        let group = |id: &str, name: &str, used: f64, fail| ProviderStatus {
            group_id: id.into(),
            group_name: name.into(),
            ..status(
                "openrouter",
                Some(snap(vec![win("儲值餘額", used, None, None, now)], 0, now)),
                fail,
            )
        };
        let a = group("openrouter", "個人", 30.0, Some(FetchFailure::Network));
        let b = group("openrouter#x", "工作", 80.0, None);
        assert_eq!(title(&b), "OpenRouter · 工作");
        assert_eq!(title(&status("openrouter", None, None)), "OpenRouter");
        let focus = tightest_group(&[&a, &b], now).unwrap();
        assert_eq!(focus.group_id, "openrouter#x");
        let t = tray_view(Some(focus), &[&a, &b], now);
        assert_eq!(t.text, "20");
        assert!(t.tooltip.starts_with("OpenRouter · 工作・儲值餘額 剩 20%"));
        assert!(t.warn, "另一組失敗也要警示");
        assert!(overview_headline(&[&a, &b], now).contains("OpenRouter · 工作"));
        // 都沒有資料時取第一組
        let (c, d) = (status("codex", None, None), status("codex", None, None));
        assert!(std::ptr::eq(tightest_group(&[&c, &d], now).unwrap(), &c));
        assert!(tightest_group(&[], now).is_none());
    }
}
