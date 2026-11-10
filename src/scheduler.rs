//! 刷新排程與退避（規格書第 9 節）。
//!
//! 排程器跑在背景執行緒；每次抓取再各開一條執行緒，讓 Provider 並行且彼此獨立。
//! 狀態有變化時，把所有 Provider 的 ProviderStatus 交給 `on_update`（由呼叫端送回 UI 執行緒）。

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread;
use std::time::{Duration, SystemTime};

use crate::model::{FetchFailure, ProviderStatus, Snapshot};
use crate::providers::UsageProvider;
use crate::snapshot_store;

/// 預設背景刷新間隔（Provider 可覆寫，見 `UsageProvider::refresh_interval`）。
/// 開啟視窗時資料超過半個間隔就會刷新，背景刷新只需讓托盤與通知大致即時，因此統一 10 分鐘。
pub const DEFAULT_REFRESH_INTERVAL: Duration = Duration::from_secs(10 * 60);
/// 手動刷新的下限：同一 Provider 距上次送出未滿此時間就略過，避免連按觸發限流
const MANUAL_MIN_GAP: Duration = Duration::from_secs(60);
/// 429 退避上限
const MAX_BACKOFF: Duration = Duration::from_secs(30 * 60);
/// 排程器至少每隔這麼久醒來一次，以牆上時鐘檢查是否到期。
/// Windows 的等待逾時在睡眠期間不計時，定期檢查可讓從睡眠恢復後最慢 1 分鐘內補上刷新。
const MAX_SLEEP: Duration = Duration::from_secs(60);

enum Msg {
    /// 手動刷新（忽略退避，但受 MANUAL_MIN_GAP 限制）
    RefreshNow,
    /// 開啟視窗：快照超過該 Provider 刷新間隔一半且不在退避中才刷新
    RefreshOnShow,
    SetEnabled(Vec<&'static str>),
    Done(&'static str, Result<Snapshot, FetchFailure>),
}

/// 給 UI 端使用的控制介面
#[derive(Clone)]
pub struct Scheduler {
    tx: Sender<Msg>,
}

impl Scheduler {
    /// 手動刷新：立刻刷新全部啟用的 Provider，不受退避限制；
    /// 但同一 Provider 1 分鐘內只送一次
    pub fn refresh_now(&self) {
        let _ = self.tx.send(Msg::RefreshNow);
    }

    /// 開啟視窗時：刷新快照超過該 Provider 刷新間隔一半的 Provider（遵守退避）
    pub fn refresh_on_show(&self) {
        let _ = self.tx.send(Msg::RefreshOnShow);
    }

    pub fn set_enabled(&self, ids: Vec<&'static str>) {
        let _ = self.tx.send(Msg::SetEnabled(ids));
    }
}

struct Slot {
    provider: Arc<dyn UsageProvider>,
    status: ProviderStatus,
    enabled: bool,
    /// 下次背景刷新時間（牆上時鐘）
    next_due: SystemTime,
    /// 此 Provider 的基本刷新間隔
    base_interval: Duration,
    /// 目前的刷新間隔；429 時加倍，成功後恢復為基本間隔
    interval: Duration,
    /// 退避中：此時間前不做自動刷新
    backoff_until: Option<SystemTime>,
    /// 上次送出請求的時間
    last_attempt: Option<SystemTime>,
}

impl Slot {
    fn in_backoff(&self, now: SystemTime) -> bool {
        self.backoff_until.is_some_and(|t| now < t)
    }

    /// 快照的年齡；沒有快照時為 None
    fn snapshot_age(&self, now: SystemTime) -> Option<Duration> {
        let fetched = self.status.snapshot.as_ref()?.fetched_at;
        Some(now.duration_since(fetched).unwrap_or_default())
    }
}

/// 啟動排程器。`snapshots` 為上次存下的快照：未滿一個刷新週期的等到期再刷新
/// （避免頻繁重開程式時打到限流），其餘立即刷新。
pub fn spawn(
    providers: Vec<Arc<dyn UsageProvider>>,
    enabled: Vec<&'static str>,
    mut snapshots: HashMap<String, Snapshot>,
    on_update: impl Fn(Vec<ProviderStatus>) + Send + 'static,
) -> Scheduler {
    let (tx, rx) = mpsc::channel();
    let now = SystemTime::now();
    let slots = providers
        .into_iter()
        .map(|p| {
            let interval = p.refresh_interval();
            let snapshot = snapshots.remove(p.id());
            let next_due = snapshot
                .as_ref()
                .map(|s| s.fetched_at + interval)
                .filter(|due| *due > now && *due <= now + interval)
                .unwrap_or(now);
            Slot {
                status: ProviderStatus {
                    provider_id: p.id(),
                    snapshot,
                    last_failure: None,
                    refreshing: false,
                    refresh_interval: interval,
                },
                enabled: enabled.contains(&p.id()),
                next_due,
                base_interval: interval,
                interval,
                backoff_until: None,
                last_attempt: None,
                provider: p,
            }
        })
        .collect();

    let worker_tx = tx.clone();
    thread::Builder::new()
        .name("scheduler".into())
        .spawn(move || run(slots, rx, worker_tx, on_update))
        .expect("無法建立排程執行緒");
    Scheduler { tx }
}

fn run(
    mut slots: Vec<Slot>,
    rx: Receiver<Msg>,
    tx: Sender<Msg>,
    on_update: impl Fn(Vec<ProviderStatus>),
) {
    let publish = |slots: &[Slot]| on_update(slots.iter().map(|s| s.status.clone()).collect());
    publish(&slots);

    loop {
        // 先處理到期的背景刷新
        let now = SystemTime::now();
        let mut changed = false;
        for slot in slots.iter_mut() {
            if slot.enabled && !slot.status.refreshing && slot.next_due <= now {
                start(slot, &tx);
                changed = true;
            }
        }
        if changed {
            publish(&slots);
        }

        // 睡到下一個到期時間（最多 MAX_SLEEP），期間處理指令與抓取結果
        let wait = slots
            .iter()
            .filter(|s| s.enabled && !s.status.refreshing)
            .filter_map(|s| s.next_due.duration_since(now).ok())
            .min()
            .unwrap_or(MAX_SLEEP)
            .min(MAX_SLEEP);

        match rx.recv_timeout(wait) {
            Ok(Msg::RefreshNow) => {
                let now = SystemTime::now();
                for slot in slots.iter_mut().filter(|s| s.enabled && !s.status.refreshing) {
                    let too_soon = slot
                        .last_attempt
                        .is_some_and(|t| now.duration_since(t).is_ok_and(|d| d < MANUAL_MIN_GAP));
                    if !too_soon {
                        start(slot, &tx);
                    }
                }
                publish(&slots);
            }
            Ok(Msg::RefreshOnShow) => {
                let now = SystemTime::now();
                for slot in slots.iter_mut().filter(|s| s.enabled && !s.status.refreshing) {
                    let fresh =
                        slot.snapshot_age(now).is_some_and(|age| age < slot.base_interval / 2);
                    if !slot.in_backoff(now) && !fresh {
                        start(slot, &tx);
                    }
                }
                publish(&slots);
            }
            Ok(Msg::SetEnabled(ids)) => {
                let now = SystemTime::now();
                for slot in slots.iter_mut() {
                    let enable = ids.contains(&slot.status.provider_id);
                    // 剛啟用的 Provider 盡快刷新（仍遵守退避）
                    if enable && !slot.enabled && !slot.in_backoff(now) {
                        slot.next_due = now;
                    }
                    slot.enabled = enable;
                }
                publish(&slots);
            }
            Ok(Msg::Done(id, result)) => {
                let success = result.is_ok();
                if let Some(slot) = slots.iter_mut().find(|s| s.status.provider_id == id) {
                    finish(slot, result);
                }
                publish(&slots);
                // 成功時把所有快照寫入檔案
                if success {
                    snapshot_store::save(slots.iter().filter_map(|s| {
                        Some((s.status.provider_id, s.status.snapshot.as_ref()?))
                    }));
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
}

/// 在新執行緒中抓取；同一 Provider 前一次尚未結束時不會呼叫到這裡
fn start(slot: &mut Slot, tx: &Sender<Msg>) {
    slot.status.refreshing = true;
    slot.last_attempt = Some(SystemTime::now());
    let provider = slot.provider.clone();
    let tx = tx.clone();
    thread::spawn(move || {
        let result = provider.fetch();
        let _ = tx.send(Msg::Done(provider.id(), result));
    });
}

/// 套用抓取結果。失敗時保留既有快照，只更新失敗原因。
fn finish(slot: &mut Slot, result: Result<Snapshot, FetchFailure>) {
    let now = SystemTime::now();
    slot.status.refreshing = false;
    match result {
        Ok(mut snapshot) => {
            // 這次的來源沒有方案名稱時（例如 Antigravity 改從 agy CLI 取得），沿用上一份快照的
            if snapshot.plan.is_none() {
                snapshot.plan = slot.status.snapshot.as_ref().and_then(|s| s.plan.clone());
            }
            slot.status.snapshot = Some(snapshot);
            slot.status.last_failure = None;
            slot.interval = slot.base_interval;
            slot.backoff_until = None;
            slot.next_due = now + slot.base_interval;
        }
        Err(FetchFailure::RateLimited { retry_at }) => {
            // 間隔加倍（上限 30 分鐘）；伺服器指定的 Retry-After 更晚時以它為準
            slot.interval = (slot.interval * 2).min(MAX_BACKOFF);
            let until = (now + slot.interval).max(retry_at);
            slot.backoff_until = Some(until);
            slot.next_due = until;
            slot.status.last_failure = Some(FetchFailure::RateLimited { retry_at: until });
            log!("[{}] 限流，退避 {} 分鐘", slot.status.provider_id, slot.interval.as_secs() / 60);
        }
        Err(failure) => {
            slot.status.last_failure = Some(failure);
            slot.next_due = now + slot.interval;
        }
    }
}
