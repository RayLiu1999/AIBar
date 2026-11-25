//! `%APPDATA%\AiBar\config.json` 的讀寫（規格書第 8 節）。
//! 使用者不需要編輯這個檔案，所有設定都在設定視窗操作。

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::model::GroupSpec;
use crate::platform::app_dir;

/// 所有已知的 Provider（預設順序）。尚未實作的也列出，設定視窗會顯示為「即將推出」。
pub const KNOWN_PROVIDERS: [&str; 5] = ["claude", "codex", "antigravity", "commandcode", "openrouter"];

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProviderEntry {
    pub id: String,
    /// 有 `groups` 時等於「任一組啟用」，供舊版讀取
    pub enabled: bool,
    /// 憑證組，陣列順序即顯示順序；空的表示只有一組未命名的
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub groups: Vec<GroupEntry>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GroupEntry {
    pub id: String,
    #[serde(default)]
    pub name: String,
    pub enabled: bool,
}

/// 憑證組名稱的長度上限（字元）
const MAX_GROUP_NAME: usize = 20;

/// 整理使用者輸入的名稱：去掉前後空白與換行，限制長度
pub fn clean_group_name(name: &str) -> String {
    let name: String = name.chars().filter(|c| !c.is_control()).collect();
    name.trim()
        .chars()
        .take(MAX_GROUP_NAME)
        .collect::<String>()
        .trim_end()
        .to_string()
}

impl ProviderEntry {
    /// 所有憑證組；沒有設定時為一組未命名的，id 就是 Provider id
    pub fn all_groups(&self) -> Vec<GroupEntry> {
        if self.groups.is_empty() {
            return vec![GroupEntry {
                id: self.id.clone(),
                name: String::new(),
                enabled: self.enabled,
            }];
        }
        self.groups.clone()
    }

    /// 移除不屬於此 Provider 或重複的組，並整理名稱
    fn normalize_groups(&mut self) {
        let mut seen: Vec<String> = Vec::new();
        let prefix = format!("{}#", self.id);
        let own = self.id.clone();
        self.groups.retain(|g| {
            let keep = (g.id == own || g.id.len() > prefix.len() && g.id.starts_with(&prefix)) && !seen.contains(&g.id);
            seen.push(g.id.clone());
            keep
        });
        for g in &mut self.groups {
            g.name = clean_group_name(&g.name);
        }
        self.sync_enabled();
    }

    fn sync_enabled(&mut self) {
        if !self.groups.is_empty() {
            self.enabled = self.groups.iter().any(|g| g.enabled);
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Display {
    #[default]
    Remaining,
    Used,
}

/// 總覽的排列方式
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OverviewLayout {
    /// 卡片由上往下排成一欄
    #[default]
    List,
    /// 卡片左右並排，視窗寬度不夠時換列
    Columns,
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
    pub overview_layout: OverviewLayout,
    pub window: Option<WindowGeometry>,
    /// 額度快用完時是否發 Toast 通知
    pub notifications: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            providers: KNOWN_PROVIDERS
                .iter()
                .map(|id| ProviderEntry {
                    id: id.to_string(),
                    enabled: false,
                    groups: Vec::new(),
                })
                .collect(),
            display: Display::Remaining,
            selected_tab: "overview".into(),
            overview_layout: OverviewLayout::List,
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
        for p in &mut self.providers {
            p.normalize_groups();
        }
        for id in KNOWN_PROVIDERS {
            if !self.providers.iter().any(|p| p.id == id) {
                self.providers.push(ProviderEntry {
                    id: id.into(),
                    enabled: detected.contains(&id),
                    groups: Vec::new(),
                });
            }
        }
    }

    #[cfg(test)]
    pub fn is_enabled(&self, id: &str) -> bool {
        self.providers.iter().any(|p| p.id == id && p.enabled)
    }

    pub fn set_enabled(&mut self, id: &str, enabled: bool) {
        if let Some(p) = self.providers.iter_mut().find(|p| p.id == id) {
            p.enabled = enabled;
            for g in &mut p.groups {
                g.enabled = enabled;
            }
        }
    }

    /// 所有憑證組，依 Provider 與組的顯示順序
    pub fn group_specs(&self, available: &[&'static str]) -> Vec<GroupSpec> {
        self.providers
            .iter()
            .filter_map(|p| Some((*available.iter().find(|id| **id == p.id)?, p)))
            .flat_map(|(provider_id, p)| {
                p.all_groups().into_iter().map(move |g| GroupSpec {
                    provider_id,
                    id: g.id,
                    name: g.name,
                    enabled: g.enabled,
                })
            })
            .collect()
    }

    /// 新增一組並啟用。`keep_first` 為 false 時，原本隱含的第一組沒有憑證，直接由新的取代
    pub fn add_group(&mut self, provider: &str, id: &str, name: &str, keep_first: bool) {
        let Some(p) = self.providers.iter_mut().find(|p| p.id == provider) else {
            return;
        };
        if p.groups.is_empty() && keep_first {
            p.groups = p.all_groups();
        }
        if p.groups.iter().all(|g| g.id != id) {
            p.groups.push(GroupEntry {
                id: id.into(),
                name: clean_group_name(name),
                enabled: true,
            });
        }
        p.sync_enabled();
    }

    /// 對指定的組做修改；隱含的第一組會先寫入設定
    fn edit_group(&mut self, id: &str, f: impl FnOnce(&mut GroupEntry)) {
        let Some(p) = self
            .providers
            .iter_mut()
            .find(|p| p.all_groups().iter().any(|g| g.id == id))
        else {
            return;
        };
        p.groups = p.all_groups();
        if let Some(g) = p.groups.iter_mut().find(|g| g.id == id) {
            f(g);
        }
        p.sync_enabled();
    }

    pub fn rename_group(&mut self, id: &str, name: &str) {
        let name = clean_group_name(name);
        self.edit_group(id, |g| g.name = name);
    }

    pub fn set_group_enabled(&mut self, id: &str, enabled: bool) {
        self.edit_group(id, |g| g.enabled = enabled);
    }

    /// 移除一組；最後一組移除後 Provider 停用
    pub fn remove_group(&mut self, id: &str) {
        for p in &mut self.providers {
            if p.all_groups().iter().any(|g| g.id == id) {
                p.groups.retain(|g| g.id != id);
                p.enabled = p.groups.iter().any(|g| g.enabled);
            }
        }
    }

    /// 顯示順序中的位置；未知的 Provider 排最後
    #[cfg(test)]
    pub fn order_of(&self, id: &str) -> usize {
        self.providers.iter().position(|p| p.id == id).unwrap_or(usize::MAX)
    }

    /// 把 Provider 往前（負）或往後（正）移動一格
    pub fn move_provider(&mut self, id: &str, delta: i32) {
        let Some(i) = self.providers.iter().position(|p| p.id == id) else {
            return;
        };
        let j = i as i64 + delta as i64;
        if j >= 0 && (j as usize) < self.providers.len() {
            self.providers.swap(i, j as usize);
        }
    }
}

fn path() -> Option<PathBuf> {
    app_dir().map(|d| d.join("config.json"))
}

/// 寫入 `app_dir()` 下的檔案：先寫暫存檔再改名，避免寫到一半當機留下損毀的檔案
pub fn write_atomic(file_name: &str, contents: impl AsRef<[u8]>) -> std::io::Result<()> {
    let dir = app_dir().ok_or_else(|| std::io::Error::other("找不到 AIBar 的資料夾"))?;
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
        assert_eq!(cfg.overview_layout, OverviewLayout::List, "舊設定檔沒有此欄位時為清單");
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
        let cfg = Config {
            display: Display::Used,
            overview_layout: OverviewLayout::Columns,
            ..Config::default()
        };
        let text = serde_json::to_string(&cfg).unwrap();
        assert!(text.contains(r#""display":"used""#));
        assert!(text.contains(r#""overview_layout":"columns""#));
        assert_eq!(serde_json::from_str::<Config>(&text).unwrap(), cfg);
    }

    #[test]
    fn 憑證組_舊設定視為一組() {
        let cfg: Config = serde_json::from_str(r#"{"providers":[{"id":"openrouter","enabled":true}]}"#).unwrap();
        let specs = cfg.group_specs(&["openrouter"]);
        assert_eq!(specs.len(), 1);
        assert_eq!(
            (specs[0].id.as_str(), specs[0].name.as_str(), specs[0].enabled),
            ("openrouter", "", true)
        );
        // 沒有多組時，檔案格式與舊版相同
        assert!(!serde_json::to_string(&cfg).unwrap().contains("groups"));
    }

    #[test]
    fn 憑證組_新增改名停用移除() {
        let mut cfg = Config::default();
        cfg.set_enabled("openrouter", true);
        // 已有第一組：保留並附加
        cfg.add_group("openrouter", "openrouter#a", "  工作\n ", true);
        let ids = |c: &Config| -> Vec<(String, String, bool)> {
            c.group_specs(&["openrouter"])
                .into_iter()
                .map(|g| (g.id, g.name, g.enabled))
                .collect()
        };
        assert_eq!(
            ids(&cfg),
            [
                ("openrouter".into(), "".into(), true),
                ("openrouter#a".into(), "工作".into(), true)
            ]
        );
        cfg.rename_group("openrouter", "個人");
        cfg.set_group_enabled("openrouter#a", false);
        assert_eq!(ids(&cfg)[0].1, "個人");
        assert!(cfg.is_enabled("openrouter"));
        cfg.set_group_enabled("openrouter", false);
        assert!(!cfg.is_enabled("openrouter"), "全部停用時 Provider 也停用");
        cfg.set_group_enabled("openrouter#a", true);
        cfg.remove_group("openrouter");
        assert_eq!(ids(&cfg), [("openrouter#a".into(), "工作".into(), true)]);
        cfg.remove_group("openrouter#a");
        assert!(!cfg.is_enabled("openrouter"));
        assert_eq!(ids(&cfg), [("openrouter".into(), "".into(), false)]);
        // 第一組沒有憑證：由新的取代
        cfg.add_group("openrouter", "openrouter", "個人", false);
        assert_eq!(ids(&cfg), [("openrouter".into(), "個人".into(), true)]);
        assert_eq!(
            serde_json::from_str::<Config>(&serde_json::to_string(&cfg).unwrap()).unwrap(),
            cfg
        );
    }

    #[test]
    fn 憑證組_清理不合法的內容() {
        let mut cfg: Config = serde_json::from_str(
            r#"{"providers":[{"id":"openrouter","enabled":false,"groups":[
                {"id":"openrouter#a","name":"a","enabled":true},
                {"id":"openrouter#a","name":"重複","enabled":true},
                {"id":"openrouter#","name":"空","enabled":true},
                {"id":"claude#x","name":"別家","enabled":true}]}]}"#,
        )
        .unwrap();
        cfg.normalize(&[]);
        let p = cfg.providers.iter().find(|p| p.id == "openrouter").unwrap();
        assert_eq!(p.groups.len(), 1);
        assert!(p.enabled, "enabled 應與各組一致");
        assert_eq!(clean_group_name(&"長".repeat(50)).chars().count(), MAX_GROUP_NAME);
    }
}
