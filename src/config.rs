//! `%APPDATA%\AiBar\config.json` 的讀寫（規格書第 8 節）。
//! 使用者不需要編輯這個檔案，所有設定都在設定視窗操作。

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// 所有已知的 Provider（預設順序）。尚未實作的也列出，設定視窗會顯示為「即將推出」。
pub const KNOWN_PROVIDERS: [&str; 5] = ["claude", "codex", "antigravity", "commandcode", "openrouter"];

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProviderEntry {
    pub id: String,
    pub enabled: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Display {
    #[default]
    Remaining,
    Used,
}

/// 主視窗的位置與大小（實體像素）
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct WindowGeometry {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// 陣列順序即顯示順序
    pub providers: Vec<ProviderEntry>,
    pub display: Display,
    /// "overview" 或 Provider id
    pub selected_tab: String,
    pub window: Option<WindowGeometry>,
    /// 額度快用完時是否發 Toast 通知
    pub notifications: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            providers: KNOWN_PROVIDERS
                .iter()
                .map(|id| ProviderEntry { id: id.to_string(), enabled: false })
                .collect(),
            display: Display::Remaining,
            selected_tab: "overview".into(),
            window: None,
            notifications: true,
        }
    }
}

impl Config {
    /// 補上缺少的已知 Provider（附加在最後；偵測得到登入憑證的預設啟用）、移除未知與重複的項目
    fn normalize(&mut self, detected: &[&str]) {
        let mut seen = Vec::new();
        self.providers.retain(|p| {
            let keep = KNOWN_PROVIDERS.contains(&p.id.as_str()) && !seen.contains(&p.id);
            seen.push(p.id.clone());
            keep
        });
        for id in KNOWN_PROVIDERS {
            if !self.providers.iter().any(|p| p.id == id) {
                self.providers.push(ProviderEntry { id: id.into(), enabled: detected.contains(&id) });
            }
        }
    }

    pub fn is_enabled(&self, id: &str) -> bool {
        self.providers.iter().any(|p| p.id == id && p.enabled)
    }

    pub fn set_enabled(&mut self, id: &str, enabled: bool) {
        if let Some(p) = self.providers.iter_mut().find(|p| p.id == id) {
            p.enabled = enabled;
        }
    }

    /// 顯示順序中的位置；未知的 Provider 排最後
    pub fn order_of(&self, id: &str) -> usize {
        self.providers.iter().position(|p| p.id == id).unwrap_or(usize::MAX)
    }

    /// 把 Provider 往前（負）或往後（正）移動一格
    pub fn move_provider(&mut self, id: &str, delta: i32) {
        let Some(i) = self.providers.iter().position(|p| p.id == id) else { return };
        let j = i as i64 + delta as i64;
        if j >= 0 && (j as usize) < self.providers.len() {
            self.providers.swap(i, j as usize);
        }
    }
}

/// AIBar 自己的資料夾：`%APPDATA%\AiBar`
pub fn app_dir() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(|d| PathBuf::from(d).join("AiBar"))
}

fn path() -> Option<PathBuf> {
    app_dir().map(|d| d.join("config.json"))
}

/// 寫入 `app_dir()` 下的檔案：先寫暫存檔再改名，避免寫到一半當機留下損毀的檔案
pub fn write_atomic(file_name: &str, contents: &str) -> std::io::Result<()> {
    let dir = app_dir().ok_or_else(|| std::io::Error::other("找不到 APPDATA"))?;
    std::fs::create_dir_all(&dir)?;
    let tmp = dir.join(format!("{file_name}.tmp"));
    std::fs::write(&tmp, contents)?;
    std::fs::rename(&tmp, dir.join(file_name))
}

/// 同 `write_atomic`，寫入二進位內容
pub fn write_atomic_bytes(file_name: &str, contents: &[u8]) -> std::io::Result<()> {
    let dir = app_dir().ok_or_else(|| std::io::Error::other("找不到 APPDATA"))?;
    std::fs::create_dir_all(&dir)?;
    let tmp = dir.join(format!("{file_name}.tmp"));
    std::fs::write(&tmp, contents)?;
    std::fs::rename(&tmp, dir.join(file_name))
}

/// 讀取設定。檔案不存在時為首次啟動：`detected` 中的 Provider 預設啟用，並立即寫入檔案。
/// 檔案損毀時以預設值取代（同樣依 `detected` 啟用）。
pub fn load(detected: &[&str]) -> Config {
    let parsed = path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .map(|text| serde_json::from_str::<Config>(&text));
    match parsed {
        Some(Ok(mut cfg)) => {
            let before = cfg.providers.clone();
            cfg.normalize(detected);
            if cfg.providers != before {
                save(&cfg);
            }
            cfg
        }
        other => {
            if matches!(other, Some(Err(_))) {
                log!("[設定] config.json 格式錯誤，改用預設值");
            }
            let mut cfg = Config::default();
            for id in detected {
                cfg.set_enabled(id, true);
            }
            save(&cfg);
            cfg
        }
    }
}

/// 寫入設定
pub fn save(cfg: &Config) {
    let text = serde_json::to_string_pretty(cfg).unwrap_or_default();
    if let Err(e) = write_atomic("config.json", &text) {
        log!("[設定] 無法寫入 config.json：{e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 補齊與清理_provider() {
        let mut cfg: Config = serde_json::from_str(
            r#"{"providers":[{"id":"codex","enabled":true},{"id":"bogus","enabled":true},{"id":"codex","enabled":false}]}"#,
        )
        .unwrap();
        cfg.normalize(&["openrouter"]);
        let ids: Vec<&str> = cfg.providers.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids, ["codex", "claude", "antigravity", "commandcode", "openrouter"]);
        assert!(cfg.is_enabled("codex"));
        assert!(!cfg.is_enabled("claude"));
        assert!(cfg.is_enabled("openrouter"), "新加入且偵測得到的 Provider 預設啟用");
        assert_eq!(cfg.display, Display::Remaining);
        assert_eq!(cfg.selected_tab, "overview");
    }

    #[test]
    fn 移動順序() {
        let mut cfg = Config::default();
        cfg.move_provider("codex", -1);
        assert_eq!(cfg.order_of("codex"), 0);
        cfg.move_provider("codex", -1);
        assert_eq!(cfg.order_of("codex"), 0);
        cfg.move_provider("openrouter", 1);
        assert_eq!(cfg.order_of("openrouter"), 4);
    }

    #[test]
    fn 序列化格式() {
        let mut cfg = Config::default();
        cfg.display = Display::Used;
        let text = serde_json::to_string(&cfg).unwrap();
        assert!(text.contains(r#""display":"used""#));
        assert_eq!(serde_json::from_str::<Config>(&text).unwrap(), cfg);
    }
}
