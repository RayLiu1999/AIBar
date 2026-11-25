// release 版不顯示主控台視窗；debug 版保留，方便看日誌
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! AIBar 進入點：建立托盤、主視窗與設定視窗，啟動背景排程器。
//!
//! `aibar --probe`：不開 UI，對每個 Provider 抓取一次並印出結果與回應結構（遮蔽字串值），
//! 用於驗證端點格式。

#[macro_use]
mod applog;
mod config;
mod icon_art;
mod model;
mod notify;
mod platform;
mod providers;
mod scheduler;
mod secret;
mod snapshot_store;
mod view;

use std::cell::RefCell;
use std::rc::{Rc, Weak};
use std::sync::atomic::Ordering;
use std::time::{Duration, SystemTime};

use slint::{ComponentHandle, ModelRc, VecModel};

use config::{Config, OverviewLayout};
use model::{FetchFailure, GroupSpec, ProviderStatus};
use platform::{MainWindowHost, TrayActions, TrayHost};
use scheduler::Scheduler;
use view::DisplayMode;

slint::include_modules!();

/// 「更新於 N 分鐘前」與重置倒數的重新計算間隔
const UI_TICK: Duration = Duration::from_secs(30);
/// 設定視窗的分頁
const SETTINGS_PAGE_ABOUT: i32 = 3;
const SAVE_FAILED: &str = "無法儲存 API key，詳見日誌";

struct App {
    tray: TrayHost,
    window: Rc<MainWindowHost>,
    settings: RefCell<Option<SettingsWindow>>,
    scheduler: Scheduler,
    config: RefCell<Config>,
    /// 已實作的 Provider id
    available: Vec<&'static str>,
    /// 排程器送來的最新狀態（含停用的 Provider）
    statuses: RefCell<Vec<ProviderStatus>>,
    notifier: RefCell<notify::Notifier>,
    /// 設定視窗目前顯示的 Provider 與憑證組，用來判斷是否需要重建
    settings_rows: RefCell<Vec<(ProviderSetting, Vec<GroupSetting>)>>,
}

thread_local! {
    /// UI 執行緒上的 App，讓排程器執行緒送來的更新能找到它
    static APP: RefCell<Weak<App>> = RefCell::default();
}

impl App {
    fn save_config(&self) {
        config::save(&self.config.borrow());
    }

    /// 已實作的 Provider 的所有憑證組（含停用的），依顯示順序
    fn groups(&self) -> Vec<GroupSpec> {
        self.config.borrow().group_specs(&self.available)
    }

    /// 至少有一組啟用的 Provider id，依顯示順序
    fn enabled_ids(&self) -> Vec<&'static str> {
        let mut ids: Vec<&'static str> = Vec::new();
        for g in self.groups().into_iter().filter(|g| g.enabled) {
            if !ids.contains(&g.provider_id) {
                ids.push(g.provider_id);
            }
        }
        ids
    }

    /// 啟用的憑證組的狀態，依顯示順序；同帳戶的儲值只留一份
    fn enabled_statuses(&self) -> Vec<ProviderStatus> {
        let statuses = self.statuses.borrow();
        let mut enabled: Vec<ProviderStatus> = self
            .groups()
            .iter()
            .filter(|g| g.enabled)
            .filter_map(|g| statuses.iter().find(|s| s.group_id == g.id).cloned())
            .collect();
        view::share_account_funds(&mut enabled);
        enabled
    }

    fn set_statuses(&self, statuses: Vec<ProviderStatus>) {
        *self.statuses.borrow_mut() = statuses;
        self.render();
        self.render_settings();
        self.check_notifications();
    }

    /// 額度跨過 80%／95% 時發通知（設定中可關閉）
    fn check_notifications(&self) {
        if !self.config.borrow().notifications {
            return;
        }
        let statuses = self.enabled_statuses();
        let enabled: Vec<&ProviderStatus> = statuses.iter().collect();
        for alert in self.notifier.borrow_mut().check(&enabled, SystemTime::now()) {
            log!("[通知] {}", alert.title);
            platform::toast::show(&alert.title, &alert.body);
        }
    }

    /// 依目前狀態更新托盤圖示、Tooltip、頁籤與卡片
    fn render(&self) {
        let now = SystemTime::now();
        let ids = self.enabled_ids();
        let statuses = self.enabled_statuses();
        let enabled: Vec<&ProviderStatus> = statuses.iter().collect();

        // 選到的 Provider 已停用時退回總覽
        let selected = self.config.borrow().selected_tab.clone();
        let tab_index = ids.iter().position(|id| *id == selected).map(|i| i + 1);
        // 焦點 Provider：總覽時為排序第一個，否則為該頁籤；有多組時以剩餘最少的一組代表
        let focus_id = ids.get(tab_index.unwrap_or(1) - 1).copied().unwrap_or_default();
        let focus_groups: Vec<&ProviderStatus> =
            enabled.iter().copied().filter(|s| s.provider_id == focus_id).collect();
        let focus = view::tightest_group(&focus_groups, now);

        let tray = view::tray_view(focus, &enabled, now);
        self.tray.set(&tray);

        let display = self.config.borrow().display;
        let ui = self.window.ui();
        ui.set_headline(view::overview_headline(&enabled, now).into());
        ui.set_selected_tab(tab_index.unwrap_or(0) as i32);
        ui.set_tabs(ModelRc::new(VecModel::from(
            ids.iter().map(|id| view::tab(id)).collect::<Vec<_>>(),
        )));
        // Provider 頁籤列出該 Provider 的每一組
        let detail: Vec<CardData> = match tab_index {
            Some(_) => focus_groups.iter().map(|s| view::card(s, display, now)).collect(),
            None => Vec::new(),
        };
        ui.set_detail_cards(ModelRc::new(VecModel::from(detail)));
        self.window
            .set_cards(enabled.iter().map(|s| view::card(s, display, now)).collect());
    }

    /// 頁籤切換：0 為總覽，n 為第 n 個頁籤的 Provider
    fn select_tab(&self, index: i32) {
        let id = match self.enabled_ids().get((index - 1).max(0) as usize) {
            Some(id) if index > 0 => id.to_string(),
            _ => "overview".to_string(),
        };
        self.config.borrow_mut().selected_tab = id;
        self.save_config();
        self.render();
    }

    /// 啟用狀態改變：通知排程器、重畫、存檔
    fn apply_enabled(&self) {
        self.scheduler.set_groups(self.groups());
        self.save_config();
        self.render();
        self.render_settings();
    }

    /// 設定視窗中每個 Provider 的偵測狀態文字
    fn provider_status_text(&self, id: &str) -> String {
        let meta = view::provider_meta(id);
        let present = providers::all().iter().any(|p| p.id() == id && p.credentials_present());
        let plan = self
            .statuses
            .borrow()
            .iter()
            .find(|s| s.provider_id == id)
            .and_then(|s| s.snapshot.as_ref()?.plan.clone())
            .map(|p| format!(" · {p}"))
            .unwrap_or_default();
        match (id, present) {
            ("openrouter", _) => match self.stored_groups(id) {
                0 => "尚未設定 API key：按「新增一組」貼上".into(),
                n => format!("已設定 {n} 組 API key"),
            },
            ("antigravity", true) => format!("已安裝 Antigravity，App 開啟時讀取{plan}"),
            ("antigravity", false) => "找不到 Antigravity".into(),
            (_, true) => format!("已偵測到 {} 登入{plan}", tool_name(id)),
            (_, false) => format!("找不到 {} 登入，請先在終端機執行 {}", tool_name(id), meta.cli),
        }
    }

    /// 已存有 API key 的組數
    fn stored_groups(&self, provider: &str) -> usize {
        self.groups()
            .iter()
            .filter(|g| g.provider_id == provider && secret::has(&g.id))
            .count()
    }

    /// 設定視窗中某個 Provider 的憑證組
    fn group_rows(&self, p: &config::ProviderEntry) -> Vec<GroupSetting> {
        let statuses = self.statuses.borrow();
        let groups = p.all_groups();
        let many = groups.len() > 1;
        groups
            .into_iter()
            // 隱含的第一組沒有 API key 時不列出
            .filter(|g| !p.groups.is_empty() || secret::has(&g.id))
            .map(|g| {
                let status = statuses.iter().find(|s| s.group_id == g.id);
                let failure = status.and_then(|s| s.last_failure.as_ref());
                let plan = status.and_then(|s| s.snapshot.as_ref()?.plan.as_deref());
                GroupSetting {
                    title: match (g.name.is_empty(), many) {
                        (false, _) => g.name.as_str().into(),
                        (true, true) => "未命名".into(),
                        (true, false) => "API key".into(),
                    },
                    status: match (secret::has(&g.id), failure, plan) {
                        (false, ..) => "讀不到 API key，請按「編輯」重新輸入".into(),
                        (_, Some(FetchFailure::TokenExpired), _) if g.enabled => {
                            "API key 無效或已撤銷，請按「編輯」重新輸入".into()
                        }
                        (_, Some(f), _) if g.enabled => view::failure_text(f, &p.id).into(),
                        (_, _, Some(plan)) => format!("已儲存 · {plan}").into(),
                        _ => "已儲存".into(),
                    },
                    name: g.name.into(),
                    id: g.id.into(),
                    enabled: g.enabled,
                }
            })
            .collect()
    }

    /// 新增（`group` 為空）或修改一組憑證；成功時關閉編輯表單，失敗時顯示原因
    fn save_group(&self, provider: &str, group: &str, name: &str, key: &str) {
        let key = key.trim();
        let result = if group.is_empty() {
            self.add_group(provider, name, key)
        } else {
            self.edit_group(group, name, key)
        };
        if let Some(w) = self.settings.borrow().as_ref() {
            match result {
                Ok(()) => w.invoke_edit("".into(), "".into()),
                Err(reason) => w.set_edit_error(reason.into()),
            }
        }
        self.render_settings();
    }

    fn add_group(&self, provider: &str, name: &str, key: &str) -> Result<(), &'static str> {
        if key.is_empty() {
            return Err("請貼上 API key");
        }
        let existing: Vec<GroupSpec> = self
            .groups()
            .into_iter()
            .filter(|g| g.provider_id == provider)
            .collect();
        let Some(available) = self.available.iter().find(|id| **id == provider) else {
            return Err(SAVE_FAILED);
        };
        let has_first = secret::has(provider);
        let implicit = existing.len() == 1 && existing[0].id == *available && existing[0].name.is_empty();
        // 還沒有任何憑證時沿用 Provider id，與單組的舊資料相容
        let id = if implicit && !has_first {
            provider.to_string()
        } else {
            new_group_id(provider)
        };
        let mut name = config::clean_group_name(name);
        if name.is_empty() && id != provider {
            name = format!("第 {} 組", existing.len() + 1);
        }
        if !secret::set(&id, Some(key)) {
            return Err(SAVE_FAILED);
        }
        // 沿用的組可能還留著舊憑證的快照；要在啟用之前清掉
        self.scheduler.reset(&id);
        self.config.borrow_mut().add_group(provider, &id, &name, has_first);
        self.apply_enabled();
        Ok(())
    }

    /// 改名；`key` 不為空時一併更換 API key
    fn edit_group(&self, group: &str, name: &str, key: &str) -> Result<(), &'static str> {
        if !key.is_empty() {
            if !secret::set(group, Some(key)) {
                return Err(SAVE_FAILED);
            }
            self.scheduler.reset(group);
        }
        self.config.borrow_mut().rename_group(group, name);
        self.apply_enabled();
        Ok(())
    }

    fn delete_group(&self, group: &str) {
        if !secret::set(group, None) {
            if let Some(w) = self.settings.borrow().as_ref() {
                w.set_edit_error("無法刪除 API key，詳見日誌".into());
            }
            return;
        }
        self.config.borrow_mut().remove_group(group);
        self.apply_enabled();
        // 最後一組移除後會剩下同 id 的隱含組，清掉它的快照
        self.scheduler.reset(group);
        if let Some(w) = self.settings.borrow().as_ref() {
            w.invoke_edit("".into(), "".into());
        }
    }

    /// 更新設定視窗的內容（視窗未建立時略過）
    fn render_settings(&self) {
        let settings = self.settings.borrow();
        let Some(w) = settings.as_ref() else { return };
        let cfg = self.config.borrow().clone();
        let rows: Vec<(ProviderSetting, Vec<GroupSetting>)> = cfg
            .providers
            .iter()
            .map(|p| {
                let meta = view::provider_meta(&p.id);
                let available = self.available.contains(&p.id.as_str());
                let row = ProviderSetting {
                    id: p.id.as_str().into(),
                    name: meta.name.into(),
                    mono: meta.mono.into(),
                    accent: slint::Color::from_rgb_u8(meta.accent[0], meta.accent[1], meta.accent[2]),
                    status: self.provider_status_text(&p.id).into(),
                    enabled: p.enabled && available,
                    available,
                    needs_key: p.id == providers::openrouter::ID,
                    groups: ModelRc::default(),
                };
                let groups = if row.needs_key { self.group_rows(p) } else { Vec::new() };
                (row, groups)
            })
            .collect();
        // 內容沒變就不重建，避免輸入框失去焦點
        if *self.settings_rows.borrow() != rows {
            let model: Vec<ProviderSetting> = rows
                .iter()
                .map(|(row, groups)| ProviderSetting {
                    groups: ModelRc::new(VecModel::from(groups.clone())),
                    ..row.clone()
                })
                .collect();
            w.set_providers(ModelRc::new(VecModel::from(model)));
            *self.settings_rows.borrow_mut() = rows;
        }
        w.set_display_index(match cfg.display {
            DisplayMode::Remaining => 0,
            DisplayMode::Used => 1,
        });
        w.set_autostart(platform::autostart::is_enabled());
        w.set_notifications(cfg.notifications);
    }

    fn open_settings(&self, page: Option<i32>) {
        if self.settings.borrow().is_none() {
            match SettingsWindow::new() {
                Ok(w) => {
                    w.set_version(env!("CARGO_PKG_VERSION").into());
                    w.set_macos(platform::IS_MACOS);
                    w.on_autostart_toggled(|on| {
                        platform::autostart::set_enabled(on);
                        with_app(|app| app.render_settings())();
                    });
                    w.on_display_changed(|index| {
                        with_app(move |app| {
                            app.config.borrow_mut().display = if index == 1 {
                                DisplayMode::Used
                            } else {
                                DisplayMode::Remaining
                            };
                            app.save_config();
                            app.render();
                        })();
                    });
                    w.on_provider_toggled(|id, on| {
                        with_app(move |app| {
                            app.config.borrow_mut().set_enabled(&id, on);
                            app.apply_enabled();
                        })();
                    });
                    w.on_provider_moved(|id, delta| {
                        with_app(move |app| {
                            app.config.borrow_mut().move_provider(&id, delta);
                            app.apply_enabled();
                        })();
                    });
                    w.on_group_saved(|provider, group, name, key| {
                        let args = [provider, group, name, key].map(|s| s.to_string());
                        defer(move |app| {
                            let [provider, group, name, key] = &args;
                            app.save_group(provider, group, name, key);
                        });
                    });
                    w.on_group_toggled(|group, on| {
                        defer(move |app| {
                            app.config.borrow_mut().set_group_enabled(&group, on);
                            app.apply_enabled();
                        });
                    });
                    w.on_group_deleted(|group| defer(move |app| app.delete_group(&group)));
                    w.on_notifications_toggled(|on| {
                        with_app(move |app| {
                            app.config.borrow_mut().notifications = on;
                            app.save_config();
                        })();
                    });
                    *self.settings.borrow_mut() = Some(w);
                }
                Err(e) => {
                    log!("[設定] 無法建立設定視窗：{e}");
                    return;
                }
            }
        }
        self.render_settings();
        if let Some(w) = self.settings.borrow().as_ref() {
            if let Some(page) = page {
                w.set_page(page);
            }
            // 上次沒存的輸入（可能含 API key）不留著
            if !w.window().is_visible() {
                w.invoke_edit("".into(), "".into());
            }
            let _ = w.show();
            platform::after_show(w);
        }
    }
}

/// 新憑證組的 id；以時間產生，刪除後不會重複使用
fn new_group_id(provider: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    format!("{provider}#{nanos:x}")
}

/// 設定視窗中用來描述登入來源的工具名稱
fn tool_name(id: &str) -> &'static str {
    match id {
        "claude" => "Claude Code",
        "codex" => "Codex CLI",
        "commandcode" => "Command Code CLI",
        _ => "CLI",
    }
}

/// 產生只在 App 還存在時才執行的回呼，避免回呼與 App 互相持有造成循環參照
fn with_app(f: impl Fn(Rc<App>) + 'static) -> impl Fn() + 'static {
    move || {
        if let Some(app) = APP.with(|a| a.borrow().upgrade()) {
            f(app);
        }
    }
}

/// 在下一輪事件迴圈執行：設定列的回呼當中不可重建該列
fn defer(f: impl Fn(Rc<App>) + 'static) {
    slint::Timer::single_shot(Duration::ZERO, with_app(f));
}

/// `--probe [id]`：對每個（或指定的）Provider 抓取一次並印出結果
fn probe(only: Option<&str>) {
    providers::PROBE.store(true, Ordering::Relaxed);
    // 有多組 API key 的 Provider 逐組查詢
    let targets = providers::all()
        .into_iter()
        .filter(|p| only.is_none_or(|id| p.id() == id))
        .flat_map(|p| {
            let mut ids = secret::ids(p.id());
            if ids.is_empty() {
                ids.push(p.id().to_string());
            }
            ids.into_iter()
                .filter_map(move |id| Some((providers::for_group(p.id(), &id)?, id)))
                .collect::<Vec<_>>()
        });
    for (p, id) in targets {
        eprintln!(
            "== {id}（憑證{}）",
            if p.credentials_present() { "存在" } else { "不存在" }
        );
        match p.fetch() {
            Ok(s) => {
                eprintln!("方案：{}", s.plan.as_deref().unwrap_or("（無）"));
                for w in &s.windows {
                    eprintln!(
                        "  {}：{}%，{}",
                        w.name,
                        w.used_percent,
                        view::reset_text(w, SystemTime::now())
                    );
                }
                if let Some(k) = &s.key_spend {
                    eprintln!("  {} · {}", view::key_spend_text(k), view::key_spend_detail(k));
                }
                if let Some(r) = &s.resets {
                    let now = SystemTime::now();
                    eprintln!("  {}", view::resets_text(r, now));
                    for line in view::resets_detail(r, p.id(), now).lines() {
                        eprintln!("    {line}");
                    }
                }
            }
            Err(e) => eprintln!("失敗：{e:?}"),
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    platform::prepare();
    let args: Vec<String> = std::env::args().collect();
    if let Some(i) = args.iter().position(|a| a == "--probe") {
        probe(args.get(i + 1).map(String::as_str));
        return Ok(());
    }
    // `--notify-test`：發一則測試通知，確認系統的通知設定沒有擋住 AIBar
    if args.iter().any(|a| a == "--notify-test") {
        platform::toast::init();
        platform::toast::show_test("AIBar 測試通知", "看到這則通知表示額度提醒可以正常運作");
        return Ok(());
    }

    // 已有一份在執行就直接結束，避免托盤出現兩個圖示、兩份排程同時打端點
    if !platform::acquire_single_instance() {
        return Ok(());
    }
    log!("AIBar {} 啟動", env!("CARGO_PKG_VERSION"));
    platform::toast::init();
    platform::autostart::refresh();

    slint::BackendSelector::new()
        .backend_name("winit".into())
        .renderer_name("software".into())
        .select()?;

    // 必須在建立任何視窗之前（原因見 TrayHost::new）
    let tray = TrayHost::new(&view::tray_view(None, &[], SystemTime::now()))?;

    // 讀取設定；首次啟動時憑證檔存在的 Provider 預設啟用
    let all = providers::all();
    let available: Vec<&'static str> = all.iter().map(|p| p.id()).collect();
    let detected: Vec<&str> = all.iter().filter(|p| p.credentials_present()).map(|p| p.id()).collect();
    let cfg = config::load(&detected);
    let groups = cfg.group_specs(&available);

    // 上次存下的快照；帳號已更換的捨棄
    let snapshots = snapshot_store::keep_matching(snapshot_store::load(), |id| {
        let g = groups.iter().find(|g| g.id == id)?;
        providers::for_group(g.provider_id, &g.id)?.current_account_hash()
    });

    // 排程器在背景執行緒，結果送回 UI 執行緒交給 App
    let factory = Box::new(|g: &GroupSpec| providers::for_group(g.provider_id, &g.id));
    let scheduler = scheduler::spawn(factory, groups.clone(), snapshots, |statuses| {
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(app) = APP.with(|a| a.borrow().upgrade()) {
                app.set_statuses(statuses);
            }
        });
    });

    let window = MainWindowHost::new(
        cfg.window,
        with_app(|app| app.scheduler.refresh_now()),
        with_app(|app| app.scheduler.refresh_on_show()),
        |geometry| {
            with_app(move |app| {
                app.config.borrow_mut().window = Some(geometry);
                app.save_config();
            })();
        },
    )?;
    let app = Rc::new(App {
        tray,
        window,
        settings: RefCell::default(),
        scheduler,
        config: RefCell::new(cfg),
        available,
        statuses: RefCell::default(),
        notifier: RefCell::new(notify::Notifier::load()),
        settings_rows: RefCell::default(),
    });
    APP.with(|a| *a.borrow_mut() = Rc::downgrade(&app));

    let ui = app.window.ui();
    ui.on_tab_selected(|i| with_app(move |app| app.select_tab(i))());
    ui.on_copy_text(|text| {
        platform::copy_to_clipboard(&text);
    });
    ui.on_open_url(|url| platform::open_url(&url));
    ui.on_open_settings(with_app(|app| app.open_settings(None)));
    ui.set_columns(app.config.borrow().overview_layout == OverviewLayout::Columns);
    ui.on_columns_toggled(|on| {
        with_app(move |app| {
            app.config.borrow_mut().overview_layout = if on {
                OverviewLayout::Columns
            } else {
                OverviewLayout::List
            };
            app.save_config();
            app.window.set_columns(on);
        })()
    });

    app.tray.connect(
        &app.window,
        TrayActions {
            refresh: Box::new(with_app(|app| app.scheduler.refresh_now())),
            settings: Box::new(with_app(|app| app.open_settings(None))),
            about: Box::new(with_app(|app| app.open_settings(Some(SETTINGS_PAGE_ABOUT)))),
            quit: Box::new(|| {
                let _ = slint::quit_event_loop();
            }),
        },
    );

    app.render();
    let tick = slint::Timer::default();
    tick.start(slint::TimerMode::Repeated, UI_TICK, with_app(|app| app.render()));

    // 托盤沒有視窗，關閉所有視窗也不能結束程式
    slint::run_event_loop_until_quit()?;

    // 結束前記下視窗位置（視窗開著時）
    if app.window.ui().window().is_visible() {
        app.config.borrow_mut().window = Some(app.window.geometry());
        app.save_config();
    }
    Ok(())
}
