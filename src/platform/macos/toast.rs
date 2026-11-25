//! 系統通知（`UNUserNotificationCenter`）。只有從 `.app` 執行時可用。
//! 第一次要發通知時才請求授權；使用者拒絕後由系統略過。

use std::sync::mpsc;
use std::time::Duration;

use block2::{DynBlock, RcBlock};
use objc2::rc::Retained;
use objc2::runtime::{Bool, NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{AllocAnyThread, define_class, msg_send};
use objc2_foundation::{NSError, NSString};
use objc2_user_notifications::{
    UNAuthorizationOptions, UNMutableNotificationContent, UNNotification, UNNotificationPresentationOptions,
    UNNotificationRequest, UNUserNotificationCenter, UNUserNotificationCenterDelegate,
};

use super::bundle;

/// 測試通知等待使用者回應授權的時間
const TEST_TIMEOUT: Duration = Duration::from_secs(60);

define_class!(
    #[unsafe(super = NSObject)]
    struct Delegate;

    unsafe impl NSObjectProtocol for Delegate {}

    unsafe impl UNUserNotificationCenterDelegate for Delegate {
        /// 面板開著時（本程式在最前面）也照常顯示
        #[unsafe(method(userNotificationCenter:willPresentNotification:withCompletionHandler:))]
        fn will_present(
            &self,
            _center: &UNUserNotificationCenter,
            _notification: &UNNotification,
            completion: &DynBlock<dyn Fn(UNNotificationPresentationOptions)>,
        ) {
            completion.call((UNNotificationPresentationOptions::Banner | UNNotificationPresentationOptions::List,));
        }
    }
);

/// 啟動時呼叫一次
pub fn init() {
    if !bundle::is_app() {
        return;
    }
    let delegate: Retained<Delegate> = unsafe { msg_send![Delegate::alloc(), init] };
    UNUserNotificationCenter::currentNotificationCenter().setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    // 通知中心只弱參照 delegate，刻意持有到程式結束
    std::mem::forget(delegate);
}

/// 顯示一則通知
pub fn show(title: &str, body: &str) {
    send(title, body, |_| {});
}

/// 發一則測試通知並等待結果
pub fn show_test(title: &str, body: &str) {
    let (tx, rx) = mpsc::channel();
    send(title, body, move |result| {
        let _ = tx.send(result);
    });
    match rx.recv_timeout(TEST_TIMEOUT) {
        Ok(Ok(())) => {
            eprintln!("已送出測試通知");
            // 讓系統有時間顯示
            std::thread::sleep(Duration::from_secs(1));
        }
        Ok(Err(reason)) => eprintln!("無法送出：{reason}"),
        Err(_) => eprintln!("等待授權逾時"),
    }
}

fn send(title: &str, body: &str, done: impl Fn(Result<(), String>) + Send + Sync + Clone + 'static) {
    if !bundle::is_app() {
        log!("[通知] 只有從 AIBar.app 執行時可以發通知");
        done(Err("不是從 AIBar.app 執行".into()));
        return;
    }
    let (title, body) = (title.to_string(), body.to_string());
    let on_authorized = RcBlock::new(move |granted: Bool, _error: *mut NSError| {
        if !granted.as_bool() {
            log!("[通知] 未獲授權");
            done(Err("未獲授權，請到「系統設定 → 通知」開啟 AIBar".into()));
            return;
        }
        let content = UNMutableNotificationContent::new();
        content.setTitle(&NSString::from_str(&title));
        content.setBody(&NSString::from_str(&body));
        // 沒有觸發條件：立即顯示；識別碼不同，通知不會互相取代
        let id = objc2_foundation::NSUUID::UUID().UUIDString();
        let request = UNNotificationRequest::requestWithIdentifier_content_trigger(&id, &content, None);
        let done = done.clone();
        let on_added = RcBlock::new(move |error: *mut NSError| {
            let error = unsafe { error.as_ref() };
            if let Some(e) = error {
                log!("[通知] 無法顯示：{}", e.code());
            }
            done(error.map_or(Ok(()), |e| Err(format!("錯誤 {}", e.code()))));
        });
        UNUserNotificationCenter::currentNotificationCenter()
            .addNotificationRequest_withCompletionHandler(&request, Some(&on_added));
    });
    UNUserNotificationCenter::currentNotificationCenter()
        .requestAuthorizationWithOptions_completionHandler(UNAuthorizationOptions::Alert, &on_authorized);
}
