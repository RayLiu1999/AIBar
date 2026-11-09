// release 版不顯示主控台視窗；debug 版保留，方便看日誌
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! AIBar 進入點：建立托盤與主視窗，啟動背景排程器。
//!
//! `aibar --probe`：不開 UI，對每個 Provider 抓取一次並印出結果與回應結構（遮蔽字串值），
//! 用於驗證端點格式。

mod main_window;
mod model;
mod providers;
mod scheduler;
mod tray_icon;
mod view;

use std::cell::RefCell;
use std::rc::{Rc, Weak};
use std::sync::atomic::Ordering;
use std::time::{Duration, SystemTime};

use slint::ComponentHandle;

use main_window::MainWindowHost;
use model::ProviderStatus;
use scheduler::Scheduler;

slint::include_modules!();

/// 「更新於 N 分鐘前」與重置倒數的重新計算間隔
const UI_TICK: Duration = Duration::from_secs(30);
/// 開啟視窗時，快照比這舊就自動刷新。Claude 端點限流嚴格（相隔不到 1 分鐘就可能 429），不宜更短
const SHOW_REFRESH_AGE: Duration = Duration::from_secs(2 * 60);

struct App {
    tray: Tray,
    window: Rc<MainWindowHost>,
    scheduler: Scheduler,
    /// 排程器送來的最新狀態（含停用的 Provider）
    statuses: RefCell<Vec<ProviderStatus>>,
    about: RefCell<Option<About>>,
}

thread_local! {
    /// UI 執行緒上的 App，讓排程器執行緒送來的更新能找到它
    static APP: RefCell<Weak<App>> = RefCell::default();
}

impl App {
    fn is_enabled(&self, provider_id: &str) -> bool {
        match provider_id {
            "claude" => self.tray.get_claude_enabled(),
            "codex" => self.tray.get_codex_enabled(),
            _ => false,
        }
    }

    fn enabled_ids(&self) -> Vec<&'static str> {
        providers::all().iter().map(|p| p.id()).filter(|id| self.is_enabled(id)).collect()
    }

    fn set_statuses(&self, statuses: Vec<ProviderStatus>) {
        *self.statuses.borrow_mut() = statuses;
        self.render();
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

    fn on_enabled_changed(&self) {
        self.scheduler.set_enabled(self.enabled_ids());
        self.render();
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
    tray.set_tray_icon(tray_icon::make(view::TrayColor::Gray, false));

    // 首次偵測：憑證檔存在的 Provider 預設啟用（之後改由 config.json 記住）
    let all = providers::all();
    let enabled: Vec<&'static str> =
        all.iter().filter(|p| p.credentials_present()).map(|p| p.id()).collect();
    tray.set_claude_enabled(enabled.contains(&"claude"));
    tray.set_codex_enabled(enabled.contains(&"codex"));

    // 排程器在背景執行緒，結果送回 UI 執行緒交給 App
    let scheduler = scheduler::spawn(all, enabled, |statuses| {
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(app) = APP.with(|a| a.borrow().upgrade()) {
                app.set_statuses(statuses);
            }
        });
    });

    let window = MainWindowHost::new(
        with_app(|app| app.scheduler.refresh_now()),
        with_app(|app| app.scheduler.refresh_if_older(SHOW_REFRESH_AGE)),
    )?;
    let app = Rc::new(App {
        tray,
        window,
        scheduler,
        statuses: RefCell::default(),
        about: RefCell::default(),
    });
    APP.with(|a| *a.borrow_mut() = Rc::downgrade(&app));

    app.tray.on_left_clicked(with_app(|app| app.window.toggle()));
    app.tray.on_enabled_changed(with_app(|app| app.on_enabled_changed()));
    app.tray.on_refresh_now(with_app(|app| app.scheduler.refresh_now()));
    app.tray.on_show_about(with_app(|app| app.show_about()));
    app.tray.on_quit(|| {
        let _ = slint::quit_event_loop();
    });

    app.render();
    let tick = slint::Timer::default();
    tick.start(slint::TimerMode::Repeated, UI_TICK, with_app(|app| app.render()));

    // 托盤沒有視窗，關閉所有視窗也不能結束程式
    slint::run_event_loop_until_quit()?;
    Ok(())
}
