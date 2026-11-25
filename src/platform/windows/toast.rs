//! Windows Toast 通知。
//!
//! 免安裝程式要發 Toast，需要一個 AppUserModelID：在
//! `HKCU\Software\Classes\AppUserModelId\<AUMID>` 登錄顯示名稱與圖示（圖示寫成 PNG 放在
//! `%APPDATA%\AiBar\icon.png`），再以 `SetCurrentProcessExplicitAppUserModelID` 讓本程式使用它。

use windows::Data::Xml::Dom::XmlDocument;
use windows::UI::Notifications::{ToastNotification, ToastNotificationManager};
use windows::Win32::System::Registry::{HKEY_CURRENT_USER, REG_SZ, RegSetKeyValueW};
use windows::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID;
use windows::core::{HSTRING, PCWSTR};

use super::app_dir;
use crate::config::write_atomic_bytes;
use crate::icon_art;

const AUMID: &str = "RayLiu.AIBar";

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
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
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
