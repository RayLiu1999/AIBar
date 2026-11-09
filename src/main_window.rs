//! 主視窗：托盤左鍵切換顯示／隱藏，按 X 或 Esc 隱藏回托盤。
//! 視窗只建立一次，隱藏時保留位置與大小。

use std::cell::Cell;
use std::rc::{Rc, Weak};
use std::time::{Duration, Instant};

use slint::winit_030::winit::event::WindowEvent;
use slint::winit_030::winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
use slint::winit_030::{EventResult, WinitWindowAccessor};
use slint::{ComponentHandle, Image, ModelRc, PhysicalPosition, VecModel};
use windows::Win32::Foundation::{HWND, POINT};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint,
};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::WindowsAndMessaging::{GetCursorPos, SetForegroundWindow};

use crate::{CardData, MainWindow};

/// 第一次開啟時的視窗大小（邏輯像素，需與 main_window.slint 的 preferred 尺寸一致）
const INITIAL_W: f32 = 420.0;
const INITIAL_H: f32 = 360.0;
/// 第一次開啟時與工作區右下角的間距（邏輯像素）
const MARGIN: f32 = 12.0;
/// 點托盤會讓視窗先失焦；這段時間內失焦的視窗，仍視為點擊當下在最前面
const FOCUS_GRACE: Duration = Duration::from_millis(400);
/// 視窗顯示後延遲這麼久再搶前景（視窗要到下一輪事件迴圈才真正出現）
const FOCUS_DELAY: Duration = Duration::from_millis(30);

/// 游標所在螢幕工作區右下角的視窗左上角位置（實體像素）
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
    let m = (MARGIN * scale) as i32;
    Some(PhysicalPosition::new((wk.right - w - m).max(wk.left), (wk.bottom - h - m).max(wk.top)))
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
}

impl MainWindowHost {
    /// `on_refresh`：使用者按下視窗上的「立即刷新」
    pub fn new(on_refresh: impl Fn() + 'static) -> Result<Rc<Self>, slint::PlatformError> {
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
            }
        }))
    }

    pub fn set_cards(&self, cards: Vec<CardData>) {
        self.cards.set_vec(cards);
    }

    pub fn set_icon(&self, icon: Image) {
        self.window.set_window_icon(icon);
    }

    fn hide(&self) {
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
        if !self.placed.replace(true)
            && let Some(pos) = initial_position()
        {
            self.window.window().set_position(pos);
        }
        if let Err(e) = self.window.show() {
            eprintln!("[視窗] 顯示失敗：{e}");
            return;
        }
        let weak = Rc::downgrade(self);
        slint::Timer::single_shot(FOCUS_DELAY, move || {
            if let Some(host) = weak.upgrade() {
                bring_to_front(host.window.window());
            }
        });
    }
}
