// release 版不顯示主控台視窗；debug 版保留，方便看日誌
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! AIBar 進入點。目前托盤與主視窗顯示的是 demo 假資料，尚未接上真正的 Provider。

mod demo;
mod main_window;
mod model;
mod tray_icon;
mod view;

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};
use std::time::{Duration, SystemTime};

use slint::ComponentHandle;

use main_window::MainWindowHost;
use model::ProviderStatus;

slint::include_modules!();

/// 「更新於 N 分鐘前」與重置倒數的重新計算間隔
const UI_TICK: Duration = Duration::from_secs(30);
/// demo：模擬一次刷新所需時間
const DEMO_REFRESH_TIME: Duration = Duration::from_millis(1200);

struct App {
    tray: Tray,
    window: Rc<MainWindowHost>,
    statuses: RefCell<Vec<ProviderStatus>>,
    /// demo：目前顯示的假資料情境
    scenario: Cell<usize>,
    about: RefCell<Option<About>>,
}

impl App {
    fn is_enabled(&self, provider_id: &str) -> bool {
        match provider_id {
            "claude" => self.tray.get_claude_enabled(),
            "codex" => self.tray.get_codex_enabled(),
            _ => false,
        }
    }

    /// 依目前狀態更新托盤圖示、Tooltip、視窗圖示與卡片
    fn render(&self) {
        let now = SystemTime::now();
        let statuses = self.statuses.borrow();
        let enabled: Vec<&ProviderStatus> =
            statuses.iter().filter(|s| self.is_enabled(s.provider_id)).collect();

        let summary = view::tray_summary(&enabled, now);
        let icon = tray_icon::make(summary.color, summary.warn);
        self.tray.set_tray_icon(icon.clone());
        self.tray.set_tray_tooltip(summary.tooltip.into());
        self.window.set_icon(icon);
        self.window.set_cards(enabled.iter().map(|s| view::card(s, now)).collect());
    }

    /// demo：標記刷新中，稍後切換到下一個假資料情境
    fn refresh_all(self: &Rc<Self>) {
        {
            let mut statuses = self.statuses.borrow_mut();
            // 前一次刷新尚未結束時不重疊
            if statuses.iter().any(|s| s.refreshing) {
                return;
            }
            for s in statuses.iter_mut().filter(|s| self.is_enabled(s.provider_id)) {
                s.refreshing = true;
            }
        }
        self.render();

        let weak = Rc::downgrade(self);
        slint::Timer::single_shot(DEMO_REFRESH_TIME, move || {
            let Some(app) = weak.upgrade() else { return };
            let next = (app.scenario.get() + 1) % demo::SCENARIO_COUNT;
            app.scenario.set(next);
            *app.statuses.borrow_mut() = demo::scenario(next, SystemTime::now());
            app.render();
        });
    }

    fn show_about(&self) {
        let mut slot = self.about.borrow_mut();
        if slot.is_none() {
            *slot = About::new().ok();
        }
        if let Some(about) = slot.as_ref() {
            let _ = about.show();
        }
    }
}

/// 產生只在 App 還存在時才執行的回呼，避免回呼與 App 互相持有造成循環參照
fn with_app(weak: &Weak<App>, f: impl Fn(Rc<App>) + 'static) -> impl Fn() + 'static {
    let weak = weak.clone();
    move || {
        if let Some(app) = weak.upgrade() {
            f(app);
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    slint::BackendSelector::new()
        .backend_name("winit".into())
        .renderer_name("software".into())
        .select()?;

    let tray = Tray::new()?;
    // 必須在建立任何視窗之前就設定圖示：Slint 1.18.1 的托盤只會嘗試建立一次，
    // 若那時圖示還是空的就會失敗且不再重試（建立主視窗會觸發這次嘗試）
    tray.set_tray_icon(tray_icon::make(view::TrayColor::Gray, false));
    // 視窗的刷新按鈕要呼叫 App，但 App 又持有視窗；先建立視窗、回呼透過 cell 延後綁定
    let app_ref: Rc<RefCell<Weak<App>>> = Rc::default();
    let window = MainWindowHost::new({
        let app_ref = app_ref.clone();
        move || {
            if let Some(app) = app_ref.borrow().upgrade() {
                app.refresh_all();
            }
        }
    })?;
    let app = Rc::new(App {
        tray,
        window,
        statuses: RefCell::new(demo::scenario(0, SystemTime::now())),
        scenario: Cell::new(0),
        about: RefCell::default(),
    });
    *app_ref.borrow_mut() = Rc::downgrade(&app);
    let weak = Rc::downgrade(&app);

    app.tray.on_left_clicked(with_app(&weak, |app| app.window.toggle()));
    app.tray.on_enabled_changed(with_app(&weak, |app| app.render()));
    app.tray.on_refresh_now(with_app(&weak, |app| app.refresh_all()));
    app.tray.on_show_about(with_app(&weak, |app| app.show_about()));
    app.tray.on_quit(|| {
        let _ = slint::quit_event_loop();
    });

    app.render();
    let tick = slint::Timer::default();
    tick.start(slint::TimerMode::Repeated, UI_TICK, with_app(&weak, |app| app.render()));

    // 托盤沒有視窗，關閉所有視窗也不能結束程式
    slint::run_event_loop_until_quit()?;
    Ok(())
}
