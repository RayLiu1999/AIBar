//! 主視窗：托盤左鍵切換顯示／隱藏，按 X 或 Esc 隱藏回托盤。
//! 視窗只建立一次，隱藏時保留位置與大小；位置與大小另存入 config.json，重開程式後沿用。

use std::cell::Cell;
use std::rc::{Rc, Weak};
use std::time::{Duration, Instant};

use slint::winit_030::winit::event::WindowEvent;
use slint::winit_030::winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
use slint::winit_030::{EventResult, WinitWindowAccessor};
use slint::{
    CloseRequestResponse, ComponentHandle, Image, ModelRc, PhysicalPosition, PhysicalSize, VecModel,
};
use windows::Win32::Foundation::{HWND, POINT};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITOR_DEFAULTTONULL, MONITORINFO,
    MonitorFromPoint,
};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::WindowsAndMessaging::{GetCursorPos, SetForegroundWindow};

use crate::config::WindowGeometry;
use crate::{CardData, MainWindow};

/// 第一次開啟時的視窗大小（邏輯像素，需與 main_window.slint 的 preferred 尺寸一致）
const INITIAL_W: f32 = 460.0;
const INITIAL_H: f32 = 640.0;
/// 點托盤會讓視窗先失焦；這段時間內失焦的視窗，仍視為點擊當下在最前面
const FOCUS_GRACE: Duration = Duration::from_millis(400);
/// 視窗顯示後延遲這麼久再搶前景（視窗要到下一輪事件迴圈才真正出現）
const FOCUS_DELAY: Duration = Duration::from_millis(30);

/// 游標所在螢幕工作區正中央的視窗左上角位置（實體像素）
fn initial_position() -> Option<PhysicalPosition> {
    let mut p = POINT::default();
    unsafe { GetCursorPos(&mut p) }.ok()?;
    let hmon = unsafe { MonitorFromPoint(p, MONITOR_DEFAULTTONEAREST) };
    let mut mi = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
    if !unsafe { GetMonitorInfoW(hmon, &mut mi) }.as_bool() {
        return None;
    }
    let (mut dx, mut dy) = (96u32, 96u32);
    let scale = match unsafe { GetDpiForMonitor(hmon, MDT_EFFECTIVE_DPI, &mut dx, &mut dy) } {
        Ok(()) => dx as f32 / 96.0,
        Err(_) => 1.0,
    };
    let wk = mi.rcWork;
    // 一般視窗含標題列與邊框，外框比內容區大一些，預留 48 邏輯像素
    let w = ((INITIAL_W + 16.0) * scale) as i32;
    let h = ((INITIAL_H + 48.0) * scale) as i32;
    let x = wk.left + ((wk.right - wk.left) - w) / 2;
    let y = wk.top + ((wk.bottom - wk.top) - h) / 2;
    Some(PhysicalPosition::new(x.max(wk.left), y.max(wk.top)))
}

/// 存下的位置是否仍在某個螢幕上（螢幕可能已拔除或改變排列）
fn on_some_monitor(g: &WindowGeometry) -> bool {
    let center = POINT { x: g.x + g.width as i32 / 2, y: g.y + 20 };
    !unsafe { MonitorFromPoint(center, MONITOR_DEFAULTTONULL) }.is_invalid()
}

/// 讓視窗成為前景視窗。托盤點擊後 Shell 會授予本程式前景權限，此時才會成功。
fn bring_to_front(window: &slint::Window) {
    window.with_winit_window(|w| {
        let Ok(handle) = w.window_handle() else { return };
        let RawWindowHandle::Win32(h) = handle.as_raw() else { return };
        let hwnd = HWND(h.hwnd.get() as *mut core::ffi::c_void);
        if !unsafe { SetForegroundWindow(hwnd) }.as_bool() {
            eprintln!("[視窗] 無法取得前景焦點");
        }
    });
}

pub struct MainWindowHost {
    window: MainWindow,
    /// 是否已放到初始位置（只在第一次開啟時定位，之後保留使用者拖到的位置）
    placed: Cell<bool>,
    focused: Cell<bool>,
    focus_lost_at: Cell<Option<Instant>>,
    cards: Rc<VecModel<CardData>>,
    /// 上次存下的位置與大小，第一次顯示時套用
    saved: Cell<Option<WindowGeometry>>,
    on_shown: Box<dyn Fn()>,
    on_hidden: Box<dyn Fn(WindowGeometry)>,
}

impl MainWindowHost {
    /// `on_refresh`：使用者按下視窗上的「立即刷新」；`on_shown`：視窗從隱藏變為顯示；
    /// `on_hidden`：視窗隱藏，附上當時的位置與大小
    pub fn new(
        saved: Option<WindowGeometry>,
        on_refresh: impl Fn() + 'static,
        on_shown: impl Fn() + 'static,
        on_hidden: impl Fn(WindowGeometry) + 'static,
    ) -> Result<Rc<Self>, slint::PlatformError> {
        let window = MainWindow::new()?;
        let cards = Rc::new(VecModel::default());
        window.set_cards(ModelRc::from(cards.clone()));
        window.on_refresh_all(on_refresh);

        Ok(Rc::new_cyclic(|weak: &Weak<Self>| {
            let w = weak.clone();
            window.on_hide_requested(move || {
                if let Some(host) = w.upgrade() {
                    host.hide();
                }
            });
            // 按 X：只隱藏，不結束程式
            let w = weak.clone();
            window.window().on_close_requested(move || {
                if let Some(host) = w.upgrade() {
                    (host.on_hidden)(host.geometry());
                }
                CloseRequestResponse::HideWindow
            });
            // 記錄焦點變化，用來判斷托盤點擊時視窗是否在最前面
            let w = weak.clone();
            window.window().on_winit_window_event(move |_, ev| {
                if let WindowEvent::Focused(focused) = ev
                    && let Some(host) = w.upgrade()
                {
                    host.focused.set(*focused);
                    if !*focused {
                        host.focus_lost_at.set(Some(Instant::now()));
                    }
                }
                EventResult::Propagate
            });
            Self {
                window,
                placed: Cell::new(false),
                focused: Cell::new(false),
                focus_lost_at: Cell::default(),
                cards,
                saved: Cell::new(saved),
                on_shown: Box::new(on_shown),
                on_hidden: Box::new(on_hidden),
            }
        }))
    }

    pub fn set_cards(&self, cards: Vec<CardData>) {
        self.cards.set_vec(cards);
    }

    pub fn set_icon(&self, icon: Image) {
        self.window.set_window_icon(icon);
    }

    /// Slint 視窗本身，用於設定其他屬性與回呼
    pub fn ui(&self) -> &MainWindow {
        &self.window
    }

    /// 目前的位置與大小（實體像素）
    pub fn geometry(&self) -> WindowGeometry {
        let pos = self.window.window().position();
        let size = self.window.window().size();
        WindowGeometry { x: pos.x, y: pos.y, width: size.width, height: size.height }
    }

    fn hide(&self) {
        (self.on_hidden)(self.geometry());
        let _ = self.window.hide();
    }

    /// 托盤左鍵：視窗在最前面就隱藏，否則顯示並移到最前面
    pub fn toggle(self: &Rc<Self>) {
        let was_front = self.focused.get()
            || self.focus_lost_at.get().is_some_and(|t| t.elapsed() < FOCUS_GRACE);
        if self.window.window().is_visible() && was_front {
            self.hide();
        } else {
            self.show();
        }
    }

    fn show(self: &Rc<Self>) {
        let was_visible = self.window.window().is_visible();
        if !self.placed.replace(true) {
            match self.saved.take().filter(on_some_monitor) {
                Some(g) => {
                    self.window.window().set_position(PhysicalPosition::new(g.x, g.y));
                    self.window.window().set_size(PhysicalSize::new(g.width, g.height));
                }
                None => {
                    if let Some(pos) = initial_position() {
                        self.window.window().set_position(pos);
                    }
                }
            }
        }
        if let Err(e) = self.window.show() {
            eprintln!("[視窗] 顯示失敗：{e}");
            return;
        }
        if !was_visible {
            (self.on_shown)();
        }
        let weak = Rc::downgrade(self);
        slint::Timer::single_shot(FOCUS_DELAY, move || {
            if let Some(host) = weak.upgrade() {
                bring_to_front(host.window.window());
            }
        });
    }
}
