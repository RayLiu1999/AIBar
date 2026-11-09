// release 版不顯示主控台視窗；debug 版保留，方便看日誌
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! AIBar 進入點：建立托盤、主視窗與設定視窗，啟動背景排程器。
//!
//! `aibar --probe`：不開 UI，對每個 Provider 抓取一次並印出結果與回應結構（遮蔽字串值），
//! 用於驗證端點格式。

mod autostart;
mod config;
mod icon_art;
mod main_window;
mod model;
mod platform;
mod providers;
mod scheduler;
mod snapshot_store;
mod tray_icon;
mod view;

use std::cell::RefCell;
use std::rc::{Rc, Weak};
use std::sync::atomic::Ordering;
use std::time::{Duration, SystemTime};

use slint::{ComponentHandle, ModelRc, VecModel};

use config::Config;
use main_window::MainWindowHost;
use model::ProviderStatus;
use scheduler::Scheduler;
use view::DisplayMode;

slint::include_modules!();

/// 「更新於 N 分鐘前」與重置倒數的重新計算間隔
const UI_TICK: Duration = Duration::from_secs(30);
/// 設定視窗的分頁
const SETTINGS_PAGE_ABOUT: i32 = 2;

struct App {
    tray: Tray,
    window: Rc<MainWindowHost>,
    settings: RefCell<Option<SettingsWindow>>,
    scheduler: Scheduler,
    config: RefCell<Config>,
    /// 已實作的 Provider id
    available: Vec<&'static str>,
    /// 排程器送來的最新狀態（含停用的 Provider）
    statuses: RefCell<Vec<ProviderStatus>>,
}

thread_local! {
    /// UI 執行緒上的 App，讓排程器執行緒送來的更新能找到它
    static APP: RefCell<Weak<App>> = RefCell::default();
}

impl App {
    fn save_config(&self) {
        config::save(&self.config.borrow());
    }

    /// 已啟用且已實作的 Provider id，依顯示順序
    fn enabled_ids(&self) -> Vec<&'static str> {
        let cfg = self.config.borrow();
        let mut ids: Vec<&'static str> =
            self.available.iter().copied().filter(|id| cfg.is_enabled(id)).collect();
        ids.sort_by_key(|id| cfg.order_of(id));
        ids
    }

    fn set_statuses(&self, statuses: Vec<ProviderStatus>) {
        *self.statuses.borrow_mut() = statuses;
        self.render();
        self.render_settings();
    }

    /// 依目前狀態更新托盤圖示、Tooltip、頁籤與卡片
    fn render(&self) {
        let now = SystemTime::now();
        let ids = self.enabled_ids();
        let statuses = self.statuses.borrow();
        // 啟用的 Provider，依顯示順序
        let enabled: Vec<&ProviderStatus> = ids
            .iter()
            .filter_map(|id| statuses.iter().find(|s| s.provider_id == *id))
            .collect();

        // 選到的 Provider 已停用時退回總覽
        let selected = self.config.borrow().selected_tab.clone();
        let tab_index = enabled.iter().position(|s| s.provider_id == selected).map(|i| i + 1);
        // 焦點 Provider：總覽時為排序第一個，否則為該頁籤
        let focus = match tab_index {
            Some(i) => enabled.get(i - 1).copied(),
            None => enabled.first().copied(),
        };

        let tray = view::tray_view(focus, &enabled, now);
        self.tray.set_tray_icon(tray_icon::make_tray(&tray));
        self.tray.set_tray_tooltip(tray.tooltip.into());

        let display = self.config.borrow().display;
        let ui = self.window.ui();
        ui.set_headline(view::overview_headline(&enabled, now).into());
        ui.set_selected_tab(tab_index.unwrap_or(0) as i32);
        self.window.set_cards(enabled.iter().map(|s| view::card(s, display, now)).collect());
    }

    /// 頁籤切換：0 為總覽，n 為第 n 個啟用的 Provider
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
        self.scheduler.set_enabled(self.enabled_ids());
        self.save_config();
        self.render();
        self.render_settings();
    }

    /// 設定視窗中每個 Provider 的偵測狀態文字
    fn provider_status_text(&self, id: &str) -> String {
        let meta = view::provider_meta(id);
        match id {
            "antigravity" => return "v1 推出".into(),
            "commandcode" => return "v2 推出（需貼上 cookie）".into(),
            _ => {}
        }
        let tool = if id == "claude" { "Claude Code" } else { "Codex CLI" };
        let present = providers::all().iter().any(|p| p.id() == id && p.credentials_present());
        if !present {
            return format!("找不到 {tool} 登入，請先在終端機執行 {}", meta.cli);
        }
        let plan = self
            .statuses
            .borrow()
            .iter()
            .find(|s| s.provider_id == id)
            .and_then(|s| s.snapshot.as_ref()?.plan.clone());
        match plan {
            Some(plan) => format!("已偵測到 {tool} 登入 · {plan}"),
            None => format!("已偵測到 {tool} 登入"),
        }
    }

    /// 更新設定視窗的內容（視窗未建立時略過）
    fn render_settings(&self) {
        let settings = self.settings.borrow();
        let Some(w) = settings.as_ref() else { return };
        let cfg = self.config.borrow().clone();
        let rows: Vec<ProviderSetting> = cfg
            .providers
            .iter()
            .map(|p| {
                let meta = view::provider_meta(&p.id);
                let available = self.available.contains(&p.id.as_str());
                ProviderSetting {
                    id: p.id.as_str().into(),
                    name: meta.name.into(),
                    mono: meta.mono.into(),
                    accent: slint::Color::from_rgb_u8(meta.accent[0], meta.accent[1], meta.accent[2]),
                    status: self.provider_status_text(&p.id).into(),
                    enabled: p.enabled && available,
                    available,
                }
            })
            .collect();
        w.set_providers(ModelRc::new(VecModel::from(rows)));
        w.set_display_index(match cfg.display {
            DisplayMode::Remaining => 0,
            DisplayMode::Used => 1,
        });
        w.set_autostart(autostart::is_enabled());
    }

    fn open_settings(&self, page: Option<i32>) {
        if self.settings.borrow().is_none() {
            match SettingsWindow::new() {
                Ok(w) => {
                    w.set_version(env!("CARGO_PKG_VERSION").into());
                    w.on_autostart_toggled(|on| {
                        autostart::set_enabled(on);
                        with_app(|app| app.render_settings())();
                    });
                    w.on_display_changed(|index| {
                        with_app(move |app| {
                            app.config.borrow_mut().display =
                                if index == 1 { DisplayMode::Used } else { DisplayMode::Remaining };
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
                    *self.settings.borrow_mut() = Some(w);
                }
                Err(e) => {
                    eprintln!("[設定] 無法建立設定視窗：{e}");
                    return;
                }
            }
        }
        self.render_settings();
        if let Some(w) = self.settings.borrow().as_ref() {
            if let Some(page) = page {
                w.set_page(page);
            }
            let _ = w.show();
            tray_icon::apply_app_icon(w.window());
        }
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

/// `--probe`：對每個 Provider 抓取一次並印出結果
fn probe() {
    providers::PROBE.store(true, Ordering::Relaxed);
    for p in providers::all() {
        eprintln!("== {}（憑證檔{}）", p.id(), if p.credentials_present() { "存在" } else { "不存在" });
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
            }
            Err(e) => eprintln!("失敗：{e:?}"),
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::args().any(|a| a == "--probe") {
        probe();
        return Ok(());
    }

    slint::BackendSelector::new()
        .backend_name("winit".into())
        .renderer_name("software".into())
        .select()?;

    let tray = Tray::new()?;
    // 必須在建立任何視窗之前就設定圖示：Slint 1.18.1 的托盤只會嘗試建立一次，
    // 若那時圖示還是空的就會失敗且不再重試（建立主視窗會觸發這次嘗試）
    tray.set_tray_icon(tray_icon::make_tray(&view::tray_view(None, &[], SystemTime::now())));

    // 讀取設定；首次啟動時憑證檔存在的 Provider 預設啟用
    let all = providers::all();
    let available: Vec<&'static str> = all.iter().map(|p| p.id()).collect();
    let detected: Vec<&str> =
        all.iter().filter(|p| p.credentials_present()).map(|p| p.id()).collect();
    let cfg = config::load(&detected);
    let enabled: Vec<&'static str> =
        available.iter().copied().filter(|id| cfg.is_enabled(id)).collect();

    // 上次存下的快照；帳號已更換的捨棄
    let snapshots = snapshot_store::keep_matching(snapshot_store::load(), |id| {
        all.iter().find(|p| p.id() == id)?.current_account_hash()
    });

    // 排程器在背景執行緒，結果送回 UI 執行緒交給 App
    let scheduler = scheduler::spawn(all, enabled, snapshots, |statuses| {
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
    });
    APP.with(|a| *a.borrow_mut() = Rc::downgrade(&app));

    let ui = app.window.ui();
    ui.on_tab_selected(|i| with_app(move |app| app.select_tab(i))());
    ui.on_copy_text(|text| {
        platform::copy_to_clipboard(&text);
    });
    ui.on_open_url(|url| platform::open_url(&url));
    ui.on_open_settings(with_app(|app| app.open_settings(None)));

    app.tray.on_left_clicked(with_app(|app| app.window.toggle()));
    app.tray.on_refresh_now(with_app(|app| app.scheduler.refresh_now()));
    app.tray.on_open_settings(with_app(|app| app.open_settings(None)));
    app.tray.on_show_about(with_app(|app| app.open_settings(Some(SETTINGS_PAGE_ABOUT))));
    app.tray.on_quit(|| {
        let _ = slint::quit_event_loop();
    });

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
