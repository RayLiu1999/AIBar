//! 開機自啟：以系統的登入項目（`SMAppService.mainAppService`）為唯一來源。
//! 使用者在「系統設定 → 一般 → 登入項目」停用後，這裡也會顯示為關閉。

use objc2_service_management::{SMAppService, SMAppServiceStatus};

use super::bundle;

pub fn is_enabled() -> bool {
    bundle::is_app() && unsafe { SMAppService::mainAppService().status() } == SMAppServiceStatus::Enabled
}

/// 開啟或關閉開機自啟。需要使用者在系統設定核准時，開啟該設定頁
pub fn set_enabled(enabled: bool) -> bool {
    if !bundle::is_app() {
        log!("[開機自啟] 只有從 AIBar.app 執行時可以設定");
        return false;
    }
    unsafe {
        let service = SMAppService::mainAppService();
        let result = if enabled {
            service.registerAndReturnError()
        } else {
            service.unregisterAndReturnError()
        };
        if let Err(e) = &result {
            log!("[開機自啟] 設定失敗：{}", e.code());
        }
        if enabled && service.status() == SMAppServiceStatus::RequiresApproval {
            SMAppService::openSystemSettingsLoginItems();
        }
        result.is_ok()
    }
}

/// 系統會追蹤 App 的位置，不需要更新
pub fn refresh() {}
