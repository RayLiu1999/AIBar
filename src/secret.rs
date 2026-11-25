//! 機密：各 Provider 的 API key（規格書第 8 節）。
//!
//! 內容是 `{"<憑證組 id>": "<API key>"}` 的 JSON（第一組的 id 就是 Provider id），整份交給作業系統的加密儲存保管
//! （見 `platform::secret_store`）。機密只在記憶體中使用，絕不寫入日誌。

use std::collections::BTreeMap;
use std::sync::Mutex;

use crate::platform;

/// 讀寫互斥（排程器執行緒讀、UI 執行緒寫）
static LOCK: Mutex<()> = Mutex::new(());

fn load_all() -> BTreeMap<String, String> {
    platform::secret_store::load()
        .and_then(|plain| serde_json::from_slice(&plain).ok())
        .unwrap_or_default()
}

/// 讀取某個憑證組的機密；沒有時為 None
pub fn get(id: &str) -> Option<String> {
    let _guard = LOCK.lock();
    load_all().remove(id).filter(|s| !s.is_empty())
}

/// 某個 Provider 存有機密的憑證組
pub fn ids(provider: &str) -> Vec<String> {
    let _guard = LOCK.lock();
    let prefix = format!("{provider}#");
    load_all()
        .into_keys()
        .filter(|id| id == provider || id.starts_with(&prefix))
        .collect()
}

/// 是否已存有某個憑證組的機密（不解出內容給呼叫端）
pub fn has(id: &str) -> bool {
    get(id).is_some()
}

/// 設定或清除（`None`）某個憑證組的機密
pub fn set(id: &str, value: Option<&str>) -> bool {
    let _guard = LOCK.lock();
    let mut all = load_all();
    match value.map(str::trim).filter(|v| !v.is_empty()) {
        Some(v) => {
            all.insert(id.to_string(), v.to_string());
        }
        None => {
            all.remove(id);
        }
    }
    let plain = serde_json::to_vec(&all).unwrap_or_default();
    platform::secret_store::store(&plain)
}
