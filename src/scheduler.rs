//! 刷新排程與退避（規格書第 9 節）。
//!
//! 以憑證組為單位排程：同一個 Provider 的每一組各自刷新與退避。
//! 排程器跑在背景執行緒；每次抓取再各開一條執行緒，讓各組並行且彼此獨立。
//! 狀態有變化時，把所有組的 ProviderStatus 交給 `on_update`（由呼叫端送回 UI 執行緒）。

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread;
use std::time::{Duration, SystemTime};

use crate::model::{FetchFailure, GroupSpec, ProviderStatus, Snapshot};
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
    /// 所有憑證組（含停用的）；不在其中的移除
    SetGroups(Vec<GroupSpec>),
    /// 某一組的憑證已更換：捨棄舊資料並立即刷新
    Reset(String),
    /// 抓取結果；世代與目前不符（憑證已更換）時捨棄
    Done(String, u64, Box<Result<Snapshot, FetchFailure>>),
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

    /// 更新憑證組：新增的建立、不在其中的移除，並套用名稱與啟用狀態
    pub fn set_groups(&self, groups: Vec<GroupSpec>) {
        let _ = self.tx.send(Msg::SetGroups(groups));
    }

    /// 某一組的憑證已更換
    pub fn reset(&self, group_id: &str) {
        let _ = self.tx.send(Msg::Reset(group_id.into()));
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
    /// 憑證存取被拒：只接受手動刷新
    manual_only: bool,
    /// 憑證更換時遞增，用來辨認過時的抓取結果
    generation: u64,
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

impl Slot {
    fn new(group: &GroupSpec, provider: Arc<dyn UsageProvider>, snapshot: Option<Snapshot>, now: SystemTime) -> Self {
        let interval = provider.refresh_interval();
        let next_due = snapshot
            .as_ref()
            .map(|s| s.fetched_at + interval)
            .filter(|due| *due > now && *due <= now + interval)
            .unwrap_or(now);
        Slot {
            status: ProviderStatus {
                provider_id: group.provider_id,
                group_id: group.id.clone(),
                group_name: group.name.clone(),
                snapshot,
                last_failure: None,
                refreshing: false,
                refresh_interval: interval,
            },
            enabled: group.enabled,
            next_due,
            base_interval: interval,
            interval,
            backoff_until: None,
            last_attempt: None,
            manual_only: false,
            generation: 0,
            provider,
        }
    }
}

/// 由憑證組建立 Provider
type Factory = Box<dyn Fn(&GroupSpec) -> Option<Arc<dyn UsageProvider>> + Send>;

/// 啟動排程器。`snapshots` 為上次存下的快照（鍵為組的 id）：未滿一個刷新週期的等到期再刷新
/// （避免頻繁重開程式時打到限流），其餘立即刷新。
pub fn spawn(
    factory: Factory,
    groups: Vec<GroupSpec>,
    mut snapshots: HashMap<String, Snapshot>,
    on_update: impl Fn(Vec<ProviderStatus>) + Send + 'static,
) -> Scheduler {
    let (tx, rx) = mpsc::channel();
    let now = SystemTime::now();
    let slots = groups
        .iter()
        .filter_map(|g| Some(Slot::new(g, factory(g)?, snapshots.remove(&g.id), now)))
        .collect();

    let worker_tx = tx.clone();
    thread::Builder::new()
        .name("scheduler".into())
        .spawn(move || run(slots, factory, rx, worker_tx, on_update))
        .expect("無法建立排程執行緒");
    Scheduler { tx }
}

fn run(
    mut slots: Vec<Slot>,
    factory: Factory,
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
            if slot.enabled && !slot.manual_only && !slot.status.refreshing && slot.next_due <= now {
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
            .filter(|s| s.enabled && !s.manual_only && !s.status.refreshing)
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
                    let fresh = slot.snapshot_age(now).is_some_and(|age| age < slot.base_interval / 2);
                    if !slot.in_backoff(now) && !slot.manual_only && !fresh {
                        start(slot, &tx);
                    }
                }
                publish(&slots);
            }
            Ok(Msg::SetGroups(groups)) => {
                let before = slots.len();
                set_groups(&mut slots, &factory, &groups, SystemTime::now());
                publish(&slots);
                if slots.len() < before {
                    save(&slots);
                }
            }
            Ok(Msg::Reset(id)) => {
                if let Some(slot) = slots.iter_mut().find(|s| s.status.group_id == id) {
                    reset(slot, SystemTime::now());
                }
                publish(&slots);
                save(&slots);
            }
            Ok(Msg::Done(id, generation, result)) => {
                let success = result.is_ok();
                let slot = slots
                    .iter_mut()
                    .find(|s| s.status.group_id == id && s.generation == generation);
                let Some(slot) = slot else { continue };
                finish(slot, *result);
                publish(&slots);
                if success {
                    save(&slots);
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
}

/// 把所有快照寫入檔案
fn save(slots: &[Slot]) {
    // 單元測試不寫入使用者真正的快照
    if cfg!(test) {
        return;
    }
    snapshot_store::save(
        slots
            .iter()
            .filter_map(|s| Some((s.status.group_id.as_str(), s.status.snapshot.as_ref()?))),
    );
}

fn set_groups(slots: &mut Vec<Slot>, factory: &Factory, groups: &[GroupSpec], now: SystemTime) {
    slots.retain(|s| groups.iter().any(|g| g.id == s.status.group_id));
    for g in groups {
        let Some(slot) = slots.iter_mut().find(|s| s.status.group_id == g.id) else {
            if let Some(provider) = factory(g) {
                slots.push(Slot::new(g, provider, None, now));
            }
            continue;
        };
        slot.status.group_name = g.name.clone();
        // 剛啟用的盡快刷新（仍遵守退避）
        if g.enabled && !slot.enabled && !slot.in_backoff(now) {
            slot.next_due = now;
        }
        // 停用後重新啟用視為使用者要求重試
        if !g.enabled {
            slot.manual_only = false;
        }
        slot.enabled = g.enabled;
    }
}

/// 憑證已更換：舊的快照、失敗原因與退避都不再適用
fn reset(slot: &mut Slot, now: SystemTime) {
    slot.generation += 1;
    slot.status.snapshot = None;
    slot.status.last_failure = None;
    slot.status.refreshing = false;
    slot.interval = slot.base_interval;
    slot.backoff_until = None;
    slot.last_attempt = None;
    slot.manual_only = false;
    slot.next_due = now;
}

/// 在新執行緒中抓取；同一 Provider 前一次尚未結束時不會呼叫到這裡
fn start(slot: &mut Slot, tx: &Sender<Msg>) {
    slot.status.refreshing = true;
    slot.last_attempt = Some(SystemTime::now());
    let provider = slot.provider.clone();
    let (id, generation) = (slot.status.group_id.clone(), slot.generation);
    let tx = tx.clone();
    thread::spawn(move || {
        let result = provider.fetch();
        let _ = tx.send(Msg::Done(id, generation, Box::new(result)));
    });
}

/// 套用抓取結果。失敗時保留既有快照，只更新失敗原因。
fn finish(slot: &mut Slot, result: Result<Snapshot, FetchFailure>) {
    let now = SystemTime::now();
    slot.status.refreshing = false;
    slot.manual_only = matches!(result, Err(FetchFailure::AccessDenied));
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
            log!(
                "[{}] 限流，退避 {} 分鐘",
                slot.status.group_id,
                slot.interval.as_secs() / 60
            );
        }
        Err(failure) => {
            slot.status.last_failure = Some(failure);
            slot.next_due = now + slot.interval;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// 依序回傳預先放好的結果；用完後都是存取被拒
    struct Fake {
        results: Mutex<Vec<(Duration, Result<f64, FetchFailure>)>>,
    }

    impl UsageProvider for Fake {
        fn id(&self) -> &'static str {
            "openrouter"
        }
        fn credentials_present(&self) -> bool {
            true
        }
        fn current_account_hash(&self) -> Option<String> {
            None
        }
        fn fetch(&self) -> Result<Snapshot, FetchFailure> {
            let next = {
                let mut r = self.results.lock().unwrap();
                if r.is_empty() { None } else { Some(r.remove(0)) }
            };
            let (delay, result) = next.unwrap_or((Duration::ZERO, Err(FetchFailure::AccessDenied)));
            thread::sleep(delay);
            result.map(|used| Snapshot {
                plan: None,
                account_hash: String::new(),
                windows: vec![crate::model::UsageWindow {
                    name: "w".into(),
                    used_percent: used,
                    resets_at: None,
                    window_secs: None,
                }],
                credits: None,
                funds_account: None,
                key_spend: None,
                resets: None,
                fetched_at: SystemTime::now(),
            })
        }
    }

    fn group(id: &str, name: &str, enabled: bool) -> GroupSpec {
        GroupSpec {
            provider_id: "openrouter",
            id: id.into(),
            name: name.into(),
            enabled,
        }
    }

    type Results = Vec<(Duration, Result<f64, FetchFailure>)>;

    /// 每一組各有自己的結果；回傳排程器與狀態的接收端
    fn start_with(
        results: Vec<(&'static str, Results)>,
        groups: Vec<GroupSpec>,
    ) -> (Scheduler, Receiver<Vec<ProviderStatus>>) {
        let map: HashMap<&str, Arc<Fake>> = results
            .into_iter()
            .map(|(id, r)| (id, Arc::new(Fake { results: Mutex::new(r) })))
            .collect();
        let (tx, rx) = mpsc::channel();
        let factory: Factory = Box::new(move |g| {
            let fake: Arc<dyn UsageProvider> = map.get(g.id.as_str())?.clone();
            Some(fake)
        });
        let scheduler = spawn(factory, groups, HashMap::new(), move |s| {
            let _ = tx.send(s);
        });
        (scheduler, rx)
    }

    /// 等到符合條件的狀態
    fn wait(
        rx: &Receiver<Vec<ProviderStatus>>,
        what: &str,
        f: impl Fn(&[ProviderStatus]) -> bool,
    ) -> Vec<ProviderStatus> {
        loop {
            let s = rx
                .recv_timeout(Duration::from_secs(5))
                .unwrap_or_else(|_| panic!("等不到：{what}"));
            if f(&s) {
                return s;
            }
        }
    }

    fn used(s: &ProviderStatus) -> Option<f64> {
        Some(s.snapshot.as_ref()?.windows[0].used_percent)
    }

    #[test]
    fn 存取被拒後只有手動刷新才重試() {
        let (scheduler, rx) = start_with(vec![("openrouter", vec![])], vec![group("openrouter", "", true)]);
        wait(&rx, "啟動時自動刷新一次", |s| {
            !s[0].refreshing && s[0].last_failure.is_some()
        });
        // 開啟視窗不重試
        scheduler.refresh_on_show();
        wait(&rx, "開啟視窗", |s| !s[0].refreshing);
        assert!(rx.recv_timeout(Duration::from_millis(200)).is_err(), "不應再次抓取");
        // 停用再啟用視為要求重試
        scheduler.set_groups(vec![group("openrouter", "", false)]);
        scheduler.set_groups(vec![group("openrouter", "", true)]);
        wait(&rx, "重新啟用後重試", |s| s[0].refreshing);
    }

    #[test]
    fn 各組獨立且可增減() {
        let ok = |v| vec![(Duration::ZERO, Ok(v))];
        let (scheduler, rx) = start_with(
            vec![
                ("openrouter", ok(10.0)),
                ("openrouter#a", vec![(Duration::ZERO, Err(FetchFailure::Network))]),
                ("openrouter#b", ok(30.0)),
            ],
            vec![group("openrouter", "個人", true), group("openrouter#a", "工作", true)],
        );
        let s = wait(&rx, "兩組都抓完", |s| {
            used(&s[0]).is_some() && s[1].last_failure.is_some()
        });
        assert_eq!(used(&s[0]), Some(10.0));
        assert!(s[0].last_failure.is_none(), "一組失敗不影響另一組");
        assert_eq!(s[1].group_name, "工作");
        // 新增 b、移除 a、改名第一組
        scheduler.set_groups(vec![group("openrouter", "改名", true), group("openrouter#b", "", true)]);
        let s = wait(&rx, "新增的組抓完", |s| s.len() == 2 && used(&s[1]).is_some());
        assert_eq!((s[0].group_name.as_str(), used(&s[0])), ("改名", Some(10.0)));
        assert_eq!((s[1].group_id.as_str(), used(&s[1])), ("openrouter#b", Some(30.0)));
    }

    #[test]
    fn 更換憑證後捨棄舊結果() {
        let slow = Duration::from_millis(300);
        let (scheduler, rx) = start_with(
            vec![("openrouter", vec![(slow, Ok(11.0)), (Duration::ZERO, Ok(22.0))])],
            vec![group("openrouter", "", true)],
        );
        wait(&rx, "第一次抓取進行中", |s| s[0].refreshing);
        scheduler.reset("openrouter");
        wait(&rx, "新憑證的結果", |s| used(&s[0]) == Some(22.0));
        // 舊憑證較慢的結果到達後不可覆蓋
        thread::sleep(slow * 2);
        assert!(rx.try_iter().all(|s| used(&s[0]) == Some(22.0)));
    }
}
