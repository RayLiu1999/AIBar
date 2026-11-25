//! 判斷是否從 `.app` 執行。通知與登入項目的 API 在 `.app` 以外呼叫會失敗，甚至直接當掉。

use objc2_foundation::NSBundle;

pub fn is_app() -> bool {
    let bundle = NSBundle::mainBundle();
    bundle.bundleIdentifier().is_some() && bundle.bundlePath().to_string().ends_with(".app")
}
