//! Windows Toast 通知（規格書第 7 節）：額度視窗的已使用量跨過 80% 與 95% 時各通知一次，
//! 該視窗重置後才會再次通知。
//!
//! 免安裝程式要發 Toast，需要一個 AppUserModelID：在
//! `HKCU\Software\Classes\AppUserModelId\<AUMID>` 登錄顯示名稱與圖示（圖示寫成 PNG 放在
//! `%APPDATA%\AiBar\icon.png`），再以 `SetCurrentProcessExplicitAppUserModelID` 讓本程式使用它。
//! 已通知紀錄存於 `%APPDATA%\AiBar\notified.json`，重開機不重複通知。

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use windows::Data::Xml::Dom::XmlDocument;
use windows::UI::Notifications::{ToastNotification, ToastNotificationManager};
use windows::Win32::System::Registry::{HKEY_CURRENT_USER, REG_SZ, RegSetKeyValueW};
use windows::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID;
use windows::core::{HSTRING, PCWSTR};

use crate::config::{app_dir, write_atomic, write_atomic_bytes};
use crate::icon_art;
use crate::model::ProviderStatus;
use crate::view;

const AUMID: &str = "RayLiu.AIBar";
const STATE_FILE: &str = "notified.json";
/// 已使用量門檻（%）：跨過時通知
const THRESHOLDS: [f64; 2] = [80.0, 95.0];

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn set_reg_string(key: &str, name: &str, value: &str) {
    let (k, n, v) = (wide(key), wide(name), wide(value));
    let _ = unsafe {
        RegSetKeyValueW(
            HKEY_CURRENT_USER,
            PCWSTR(k.as_ptr()),
            PCWSTR(n.as_ptr()),
            REG_SZ.0,
            Some(v.as_ptr() as *const core::ffi::c_void),
            (v.len() * 2) as u32,
        )
    };
}

/// 登錄 AppUserModelID 並套用到本程式（啟動時呼叫一次）
pub fn init() {
    let icon = app_dir().map(|d| d.join("icon.png"));
    if let Some(path) = &icon {
        let _ = write_atomic_bytes("icon.png", &icon_art::png(&icon_art::app_icon(256)));
        let key = format!(r"Software\Classes\AppUserModelId\{AUMID}");
        set_reg_string(&key, "DisplayName", "AIBar");
        set_reg_string(&key, "IconUri", &path.display().to_string());
    }
    if let Err(e) = unsafe { SetCurrentProcessExplicitAppUserModelID(&HSTRING::from(AUMID)) } {
        log!("[通知] 無法設定 AppUserModelID：{e}");
    }
}

fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// 顯示一則 Toast
pub fn show(title: &str, body: &str) {
    let xml = format!(
        "<toast><visual><binding template=\"ToastGeneric\"><text>{}</text><text>{}</text></binding></visual></toast>",
        escape_xml(title),
        escape_xml(body)
    );
    let result = (|| -> windows::core::Result<()> {
        let doc = XmlDocument::new()?;
        doc.LoadXml(&HSTRING::from(xml))?;
        let toast = ToastNotification::CreateToastNotification(&doc)?;
        ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(AUMID))?.Show(&toast)
    })();
    if let Err(e) = result {
        log!("[通知] 無法顯示 Toast：{e}");
    }
}

/// 一則待發的通知
#[derive(Debug, PartialEq)]
pub struct Alert {
    pub title: String,
    pub body: String,
}

/// 已通知紀錄：鍵為「Provider|額度視窗|重置時間」，值為已通知到的門檻數（1 = 80%、2 = 95%）
#[derive(Default)]
pub struct Notifier {
    sent: HashMap<String, usize>,
}

impl Notifier {
    pub fn load() -> Self {
        let sent = app_dir()
            .and_then(|d| std::fs::read_to_string(d.join(STATE_FILE)).ok())
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default();
        Self { sent }
    }

    fn save(&self) {
        // 單元測試不寫入使用者真正的紀錄檔
        if cfg!(test) {
            return;
        }
        let text = serde_json::to_string(&self.sent).unwrap_or_default();
        if let Err(e) = write_atomic(STATE_FILE, &text) {
            log!("[通知] 無法寫入 {STATE_FILE}：{e}");
        }
    }

    /// 比對目前狀態，回傳需要發出的通知並更新紀錄。重置時間已過的紀錄會被清掉。
    pub fn check(&mut self, statuses: &[&ProviderStatus], now: SystemTime) -> Vec<Alert> {
        let mut alerts = Vec::new();
        let mut changed = false;
        let now_secs = now.duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);

        // 清掉已重置視窗的紀錄（鍵的最後一段是重置時間，0 表示沒有重置時間）
        let before = self.sent.len();
        self.sent.retain(|key, _| {
            let reset: u64 = key.rsplit('|').next().and_then(|s| s.parse().ok()).unwrap_or(0);
            reset == 0 || reset > now_secs
        });
        changed |= self.sent.len() != before;

        for s in statuses {
            let Some(snap) = &s.snapshot else { continue };
            let name = view::provider_meta(s.provider_id).name;
            for w in &snap.windows {
                let used = view::used_percent(w, now);
                let level = THRESHOLDS.iter().filter(|t| used >= **t).count();
                if level == 0 {
                    continue;
                }
                let reset = w
                    .resets_at
                    .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                    .map_or(0, |d| d.as_secs());
                let key = format!("{}|{}|{reset}", s.provider_id, w.name);
                let sent = self.sent.get(&key).copied().unwrap_or(0);
                if level > sent {
                    self.sent.insert(key, level);
                    changed = true;
                    let reset_text = view::reset_text(w, now);
                    alerts.push(Alert {
                        title: format!("{name} {} 已用 {used:.0}%", w.name),
                        body: if reset_text.is_empty() {
                            format!("剩餘 {:.0}%", 100.0 - used)
                        } else {
                            format!("剩餘 {:.0}%，{reset_text}", 100.0 - used)
                        },
                    });
                }
            }
        }
        if changed {
            self.save();
        }
        alerts
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Snapshot, UsageWindow};
    use std::time::Duration;

    fn status(used: f64, reset_in: u64, now: SystemTime) -> ProviderStatus {
        ProviderStatus {
            provider_id: "codex",
            snapshot: Some(Snapshot {
                plan: None,
                account_hash: String::new(),
                windows: vec![UsageWindow {
                    name: "每週".into(),
                    used_percent: used,
                    resets_at: Some(now + Duration::from_secs(reset_in)),
                    window_secs: None,
                }],
                credits: None,
                fetched_at: now,
            }),
            last_failure: None,
            refreshing: false,
            refresh_interval: Duration::from_secs(300),
        }
    }

    fn check(n: &mut Notifier, s: &ProviderStatus, now: SystemTime) -> Vec<Alert> {
        n.check(&[s], now)
    }

    #[test]
    fn 門檻各通知一次() {
        let now = UNIX_EPOCH + Duration::from_secs(1_800_000_000);
        let mut n = Notifier::default();
        assert!(check(&mut n, &status(70.0, 3600, now), now).is_empty());
        let a = check(&mut n, &status(82.0, 3600, now), now);
        assert_eq!(a.len(), 1);
        assert_eq!(a[0].title, "Codex 每週 已用 82%");
        assert!(check(&mut n, &status(90.0, 3600, now), now).is_empty());
        assert_eq!(check(&mut n, &status(96.0, 3600, now), now).len(), 1);
        assert!(check(&mut n, &status(99.0, 3600, now), now).is_empty());
    }

    #[test]
    fn 重置後再通知() {
        let now = UNIX_EPOCH + Duration::from_secs(1_800_000_000);
        let mut n = Notifier::default();
        assert_eq!(check(&mut n, &status(85.0, 60, now), now).len(), 1);
        // 一小時後：舊視窗已重置，新視窗（新的重置時間）又到 85%
        let later = now + Duration::from_secs(3600);
        assert_eq!(check(&mut n, &status(85.0, 3600, later), later).len(), 1);
        assert_eq!(n.sent.len(), 1, "已重置視窗的紀錄應被清掉");
    }

    #[test]
    fn 跳過門檻只通知一次() {
        let now = UNIX_EPOCH + Duration::from_secs(1_800_000_000);
        let mut n = Notifier::default();
        assert_eq!(check(&mut n, &status(97.0, 3600, now), now).len(), 1);
    }
}
