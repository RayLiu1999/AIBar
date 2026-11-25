//! 機密的保存：登入 Keychain 的一個 generic password 項目，內容為整份機密。
//!
//! 每次執行最多讀一次 Keychain，之後用記憶體中的內容：App 的簽章改變時系統會詢問使用者，
//! 若每次刷新都讀，拒絕後會反覆跳出授權視窗。

use std::sync::Mutex;

use security_framework::passwords::{delete_generic_password, get_generic_password, set_generic_password};

const SERVICE: &str = "com.rayliu.aibar";
const ACCOUNT: &str = "secrets";
/// errSecItemNotFound
const NOT_FOUND: i32 = -25300;

static CACHE: Mutex<Cache> = Mutex::new(Cache::Unread);

enum Cache {
    Unread,
    Read(Option<Vec<u8>>),
}

/// Keychain 的存取；錯誤為 OSStatus
trait Backend {
    fn get(&self) -> Result<Option<Vec<u8>>, i32>;
    fn set(&self, data: &[u8]) -> Result<(), i32>;
    fn delete(&self) -> Result<(), i32>;
}

struct Keychain {
    service: &'static str,
}

impl Backend for Keychain {
    fn get(&self) -> Result<Option<Vec<u8>>, i32> {
        match get_generic_password(self.service, ACCOUNT) {
            Ok(data) => Ok(Some(data)),
            Err(e) if e.code() == NOT_FOUND => Ok(None),
            Err(e) => Err(e.code()),
        }
    }

    fn set(&self, data: &[u8]) -> Result<(), i32> {
        set_generic_password(self.service, ACCOUNT, data).map_err(|e| e.code())
    }

    fn delete(&self) -> Result<(), i32> {
        match delete_generic_password(self.service, ACCOUNT) {
            Ok(()) => Ok(()),
            Err(e) if e.code() == NOT_FOUND => Ok(()),
            Err(e) => Err(e.code()),
        }
    }
}

/// 讀出保存的內容；沒有或無法存取時為 None
pub fn load() -> Option<Vec<u8>> {
    load_from(&Keychain { service: SERVICE }, &CACHE)
}

/// 保存內容（覆蓋舊的）；沒有任何機密時刪除項目
pub fn store(plain: &[u8]) -> bool {
    store_to(&Keychain { service: SERVICE }, &CACHE, plain)
}

fn load_from(backend: &impl Backend, cache: &Mutex<Cache>) -> Option<Vec<u8>> {
    let mut cache = cache.lock().ok()?;
    if let Cache::Read(data) = &*cache {
        return data.clone();
    }
    let data = backend.get().unwrap_or_else(|code| {
        log!("[機密] 無法讀取 Keychain：{code}");
        None
    });
    *cache = Cache::Read(data.clone());
    data
}

fn store_to(backend: &impl Backend, cache: &Mutex<Cache>, plain: &[u8]) -> bool {
    let Ok(mut cache) = cache.lock() else { return false };
    let empty = plain.is_empty() || plain == b"{}";
    let result = if empty { backend.delete() } else { backend.set(plain) };
    match result {
        Ok(()) => {
            *cache = Cache::Read((!empty).then(|| plain.to_vec()));
            true
        }
        Err(code) => {
            log!("[機密] 無法寫入 Keychain：{code}");
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};

    #[derive(Default)]
    struct Fake {
        data: RefCell<Option<Vec<u8>>>,
        reads: Cell<u32>,
        denied: Cell<bool>,
    }

    impl Backend for Fake {
        fn get(&self) -> Result<Option<Vec<u8>>, i32> {
            self.reads.set(self.reads.get() + 1);
            if self.denied.get() {
                return Err(-128);
            }
            Ok(self.data.borrow().clone())
        }
        fn set(&self, data: &[u8]) -> Result<(), i32> {
            if self.denied.get() {
                return Err(-128);
            }
            *self.data.borrow_mut() = Some(data.to_vec());
            Ok(())
        }
        fn delete(&self) -> Result<(), i32> {
            *self.data.borrow_mut() = None;
            Ok(())
        }
    }

    #[test]
    fn 存取與清除() {
        let (fake, cache) = (Fake::default(), Mutex::new(Cache::Unread));
        assert_eq!(load_from(&fake, &cache), None);
        assert!(store_to(&fake, &cache, br#"{"openrouter":"k"}"#));
        assert_eq!(load_from(&fake, &cache).unwrap(), br#"{"openrouter":"k"}"#);
        // 清除最後一個機密時刪除整個項目
        assert!(store_to(&fake, &cache, b"{}"));
        assert!(fake.data.borrow().is_none());
        assert_eq!(load_from(&fake, &cache), None);
    }

    #[test]
    fn 每次執行只讀一次() {
        let (fake, cache) = (Fake::default(), Mutex::new(Cache::Unread));
        fake.denied.set(true);
        for _ in 0..3 {
            assert_eq!(load_from(&fake, &cache), None);
        }
        assert_eq!(fake.reads.get(), 1, "被拒後不可反覆詢問");
        // 寫入失敗要回報，且不改變已知內容
        assert!(!store_to(&fake, &cache, br#"{"openrouter":"k"}"#));
        assert_eq!(load_from(&fake, &cache), None);
    }

    /// 手動檢查用：以測試專用的項目實際存取登入 Keychain，結束時刪除
    /// （`cargo test keychain_實際存取 -- --ignored`）
    #[test]
    #[ignore]
    fn keychain_實際存取() {
        let keychain = Keychain {
            service: "com.rayliu.aibar.test",
        };
        keychain.delete().unwrap();
        assert_eq!(keychain.get(), Ok(None));
        keychain.set(b"one").unwrap();
        keychain.set(b"two").unwrap();
        let read = keychain.get();
        keychain.delete().unwrap();
        assert_eq!(read, Ok(Some(b"two".to_vec())));
        assert_eq!(keychain.get(), Ok(None));
    }
}
