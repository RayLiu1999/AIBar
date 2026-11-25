//! 主面板：點選單列圖示時貼著圖示下方彈出；失焦、Esc 或再點一次圖示收起。
//! 面板只建立一次，大小固定、不記位置。

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};
use std::time::{Duration, Instant};

use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_app_kit::{
    NSApplication, NSColor, NSPopUpMenuWindowLevel, NSView, NSWindow, NSWindowCollectionBehavior, NSWindowStyleMask,
};
use objc2_foundation::NSPoint;
use slint::winit_030::winit::event::{ElementState, WindowEvent};
use slint::winit_030::winit::keyboard::{KeyCode, PhysicalKey};
use slint::winit_030::winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
use slint::winit_030::{EventResult, WinitWindowAccessor};
use slint::{CloseRequestResponse, ComponentHandle, ModelRc, VecModel};

use super::tray::StatusItem;
use crate::config::WindowGeometry;
use crate::{CardData, MainWindow};

/// 面板與圖示的距離、與螢幕左右邊緣的最小距離
const GAP: f64 = 4.0;
const MARGIN: f64 = 8.0;
const CORNER_RADIUS: f64 = 12.0;
/// 點圖示會讓面板先失焦而收起；這段時間內的同一次點擊不可又打開（macOS 26 以前）
const REOPEN_GUARD: Duration = Duration::from_millis(300);
/// 視窗要到下一輪事件迴圈才真正出現
const FOCUS_DELAY: Duration = Duration::from_millis(30);

fn ns_window(window: &slint::Window) -> Option<Retained<NSWindow>> {
    window
        .with_winit_window(|w| {
            let RawWindowHandle::AppKit(h) = w.window_handle().ok()?.as_raw() else {
                return None;
            };
            let view = unsafe { Retained::retain(h.ns_view.as_ptr().cast::<NSView>()) }?;
            view.window()
        })
        .flatten()
}

/// 讓本程式成為使用中的程式，並讓視窗取得鍵盤焦點
fn activate(window: &NSWindow) {
    let Some(mtm) = MainThreadMarker::new() else { return };
    // activate() 需 macOS 14；本程式支援 13，且沒有 Dock 圖示時仍需強制取得焦點
    #[allow(deprecated)]
    NSApplication::sharedApplication(mtm).activateIgnoringOtherApps(true);
    window.makeKeyAndOrderFront(None);
}

/// 一般視窗（設定視窗）顯示後的處理：移到最前面，並支援 Cmd+W 關閉
pub fn after_show<T: ComponentHandle + 'static>(component: &T) {
    let command = Cell::new(false);
    let weak = component.as_weak();
    component.window().on_winit_window_event(move |_, event| match event {
        WindowEvent::ModifiersChanged(m) => {
            command.set(m.state().super_key());
            EventResult::Propagate
        }
        WindowEvent::KeyboardInput { event, .. }
            if command.get()
                && event.state == ElementState::Pressed
                && event.physical_key == PhysicalKey::Code(KeyCode::KeyW) =>
        {
            if let Some(c) = weak.upgrade() {
                let _ = c.hide();
            }
            EventResult::PreventDefault
        }
        _ => EventResult::Propagate,
    });
    let weak = component.as_weak();
    slint::Timer::single_shot(FOCUS_DELAY, move || {
        if let Some(window) = weak.upgrade().and_then(|c| ns_window(c.window())) {
            activate(&window);
        }
    });
}

pub struct MainWindowHost {
    window: MainWindow,
    cards: Rc<VecModel<CardData>>,
    item: RefCell<Weak<StatusItem>>,
    hidden_at: Cell<Option<Instant>>,
    on_shown: Box<dyn Fn()>,
}

impl MainWindowHost {
    /// 面板不記位置，因此不使用 `saved` 與 `on_hidden`
    pub fn new(
        _saved: Option<WindowGeometry>,
        on_refresh: impl Fn() + 'static,
        on_shown: impl Fn() + 'static,
        _on_hidden: impl Fn(WindowGeometry) + 'static,
    ) -> Result<Rc<Self>, slint::PlatformError> {
        let window = MainWindow::new()?;
        window.set_popover(true);
        let cards = Rc::new(VecModel::default());
        window.set_cards(ModelRc::from(cards.clone()));
        window.on_refresh_all(on_refresh);
        window.on_quit_requested(|| {
            let _ = slint::quit_event_loop();
        });

        Ok(Rc::new_cyclic(|weak: &Weak<Self>| {
            let w = weak.clone();
            window.on_hide_requested(move || {
                if let Some(host) = w.upgrade() {
                    host.hide();
                }
            });
            let w = weak.clone();
            window.window().on_close_requested(move || {
                if let Some(host) = w.upgrade() {
                    host.hide();
                }
                CloseRequestResponse::KeepWindowShown
            });
            let w = weak.clone();
            window.window().on_winit_window_event(move |_, event| {
                if matches!(event, WindowEvent::Focused(false)) {
                    // 不在事件處理當中隱藏視窗
                    let w = w.clone();
                    slint::Timer::single_shot(Duration::ZERO, move || {
                        if let Some(host) = w.upgrade() {
                            host.hide();
                        }
                    });
                }
                EventResult::Propagate
            });
            Self {
                window,
                cards,
                item: RefCell::default(),
                hidden_at: Cell::default(),
                on_shown: Box::new(on_shown),
            }
        }))
    }

    /// 接上選單列圖示，用來定位
    pub(super) fn attach(&self, item: Weak<StatusItem>) {
        *self.item.borrow_mut() = item;
    }

    pub fn set_cards(&self, cards: Vec<CardData>) {
        self.cards.set_vec(cards);
    }

    /// Slint 視窗本身，用於設定其他屬性與回呼
    pub fn ui(&self) -> &MainWindow {
        &self.window
    }

    /// 目前的位置與大小（實體像素）
    pub fn geometry(&self) -> WindowGeometry {
        let pos = self.window.window().position();
        let size = self.window.window().size();
        WindowGeometry {
            x: pos.x,
            y: pos.y,
            width: size.width,
            height: size.height,
        }
    }

    /// 面板固定為清單排列
    pub fn set_columns(&self, _columns: bool) {
        self.window.set_columns(false);
    }

    /// 點圖示切換（macOS 26 以前）
    pub(super) fn toggle(&self) {
        let just_hidden = self.hidden_at.get().is_some_and(|t| t.elapsed() < REOPEN_GUARD);
        if self.window.window().is_visible() {
            self.hide();
        } else if !just_hidden {
            self.show();
        }
    }

    pub(super) fn show(&self) {
        let was_visible = self.window.window().is_visible();
        self.window.set_columns(false);
        if let Err(e) = self.window.show() {
            log!("[面板] 顯示失敗：{e}");
            return;
        }
        match ns_window(self.window.window()) {
            Some(window) => {
                style(&window);
                self.place(&window);
                activate(&window);
            }
            None => log!("[面板] 取不到 NSWindow"),
        }
        if !was_visible {
            (self.on_shown)();
        }
    }

    pub(super) fn hide(&self) {
        if !self.window.window().is_visible() {
            return;
        }
        self.hidden_at.set(Some(Instant::now()));
        let _ = self.window.hide();
        let item = self.item.borrow().upgrade();
        if let Some(item) = item {
            item.cancel_session();
        }
        return_focus();
    }

    /// 置於圖示正下方並水平置中，不超出圖示所在的螢幕
    fn place(&self, window: &NSWindow) {
        let Some((icon, screen)) = self.item.borrow().upgrade().and_then(|i| i.anchor()) else {
            log!("[面板] 取不到圖示位置");
            return;
        };
        let width = window.frame().size.width;
        let right = screen.origin.x + screen.size.width - width - MARGIN;
        let x = (icon.origin.x + (icon.size.width - width) / 2.0)
            .min(right)
            .max(screen.origin.x + MARGIN);
        window.setFrameTopLeftPoint(NSPoint::new(x, icon.origin.y - GAP));
        window.invalidateShadow();
    }
}

/// 面板外觀：浮在其他視窗上、可出現在全螢幕 App 上、圓角、固定大小
fn style(window: &NSWindow) {
    window.setStyleMask(window.styleMask() & !NSWindowStyleMask::Resizable);
    window.setLevel(NSPopUpMenuWindowLevel);
    window.setCollectionBehavior(
        NSWindowCollectionBehavior::MoveToActiveSpace
            | NSWindowCollectionBehavior::FullScreenAuxiliary
            | NSWindowCollectionBehavior::Transient,
    );
    window.setMovable(false);
    window.setHasShadow(true);
    window.setOpaque(false);
    window.setBackgroundColor(Some(&NSColor::clearColor()));
    if let Some(view) = window.contentView() {
        view.setWantsLayer(true);
        if let Some(layer) = view.layer() {
            layer.setCornerRadius(CORNER_RADIUS);
            layer.setMasksToBounds(true);
        }
    }
}

/// 面板收起後把焦點還給原本的程式；設定視窗開著時不動
fn return_focus() {
    let Some(mtm) = MainThreadMarker::new() else { return };
    let app = NSApplication::sharedApplication(mtm);
    let other_visible = app.windows().iter().any(|w| w.isVisible() && w.canBecomeMainWindow());
    if app.isActive() && !other_visible {
        app.hide(None);
    }
}
