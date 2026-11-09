//! AIBar 托盤原型：驗證 Slint 托盤的點擊、選單、動態圖示／Tooltip，
//! 以及彈出面板的定位與關閉行為。正式功能尚未實作。

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant};

use slint::winit_030::winit::event::WindowEvent;
use slint::winit_030::winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
use slint::winit_030::{EventResult, WinitWindowAccessor};
use slint::{ComponentHandle, Image, PhysicalPosition, Rgba8Pixel, SharedPixelBuffer};
use windows::Win32::Foundation::{HWND, POINT, RECT};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint,
};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::System::Threading::GetCurrentProcessId;
use windows::Win32::UI::Shell::{NOTIFYICONIDENTIFIER, Shell_NotifyIconGetRect};
use windows::Win32::UI::WindowsAndMessaging::{
    FindWindowExW, GetCursorPos, GetWindowThreadProcessId, HWND_MESSAGE, SetForegroundWindow,
};
use windows::core::{PCWSTR, w};

slint::include_modules!();

/// 面板邏輯尺寸（需與 panel.slint 一致）
const PANEL_W: f32 = 320.0;
const PANEL_H: f32 = 160.0;
/// 面板與工作區邊緣的間距（邏輯像素）
const PANEL_MARGIN: f32 = 12.0;
/// 面板因失焦關閉後，這段時間內的托盤點擊視為「關閉」而非「重新開啟」
const REOPEN_GUARD: Duration = Duration::from_millis(300);

/// 原型用的四種顏色：綠、黃、紅、灰
const COLORS: [(&str, [u8; 3]); 4] = [
    ("綠", [0x22, 0xc5, 0x5e]),
    ("黃", [0xea, 0xb3, 0x08]),
    ("紅", [0xef, 0x44, 0x44]),
    ("灰", [0x9c, 0xa3, 0xaf]),
];

/// 產生 32×32 的圓形托盤圖示；`warn` 為真時在右下角加上警示點
fn make_icon(rgb: [u8; 3], warn: bool) -> Image {
    const N: u32 = 32;
    let mut buf = SharedPixelBuffer::<Rgba8Pixel>::new(N, N);
    let c = (N as f32 - 1.0) / 2.0;
    for (i, px) in buf.make_mut_slice().iter_mut().enumerate() {
        let x = (i as u32 % N) as f32;
        let y = (i as u32 / N) as f32;
        let in_circle = ((x - c).powi(2) + (y - c).powi(2)).sqrt() <= c;
        let in_badge = warn && ((x - 25.0).powi(2) + (y - 25.0).powi(2)).sqrt() <= 6.5;
        *px = if in_badge {
            Rgba8Pixel { r: 0xff, g: 0xff, b: 0xff, a: 0xff }
        } else if in_circle {
            Rgba8Pixel { r: rgb[0], g: rgb[1], b: rgb[2], a: 0xff }
        } else {
            Rgba8Pixel { r: 0, g: 0, b: 0, a: 0 }
        };
    }
    Image::from_rgba8(buf)
}

/// Slint 托盤內部隱藏視窗的類別名稱與 uID（見 i-slint-core 的 system_tray/windows.rs）。
/// Slint 沒有公開這兩個值，若 Slint 改版而失效，會退回以游標位置定位。
const SLINT_TRAY_CLASS: PCWSTR = w!("SlintSystemTrayWindow");
const SLINT_TRAY_UID: u32 = 1;

/// 取得本程式托盤圖示在螢幕上的矩形（實體像素）；
/// 圖示在「^」隱藏區時，回傳的是它在彈出框中的位置。
fn tray_icon_rect() -> Option<RECT> {
    let pid = unsafe { GetCurrentProcessId() };
    let mut after: Option<HWND> = None;
    // 其他 Slint 程式也可能有同類別的視窗，只取屬於本程式的
    loop {
        let hwnd =
            unsafe { FindWindowExW(Some(HWND_MESSAGE), after, SLINT_TRAY_CLASS, PCWSTR::null()) }
                .ok()?;
        let mut owner = 0u32;
        unsafe { GetWindowThreadProcessId(hwnd, Some(&mut owner)) };
        if owner == pid {
            let id = NOTIFYICONIDENTIFIER {
                cbSize: size_of::<NOTIFYICONIDENTIFIER>() as u32,
                hWnd: hwnd,
                uID: SLINT_TRAY_UID,
                ..Default::default()
            };
            return unsafe { Shell_NotifyIconGetRect(&id) }.ok();
        }
        after = Some(hwnd);
    }
}

/// 面板定位所需的螢幕資訊（皆為實體像素）
struct ScreenInfo {
    /// 托盤圖示的矩形；取不到時以游標位置代替（寬高為 0）
    anchor: RECT,
    /// anchor 是否來自真正的圖示位置
    from_icon: bool,
    monitor: RECT,
    work: RECT,
    scale: f32,
}

fn screen_for_tray() -> Option<ScreenInfo> {
    let (anchor, from_icon) = match tray_icon_rect() {
        Some(r) => (r, true),
        None => {
            let mut p = POINT::default();
            unsafe { GetCursorPos(&mut p) }.ok()?;
            (RECT { left: p.x, top: p.y, right: p.x, bottom: p.y }, false)
        }
    };
    let center = POINT { x: (anchor.left + anchor.right) / 2, y: (anchor.top + anchor.bottom) / 2 };
    let hmon = unsafe { MonitorFromPoint(center, MONITOR_DEFAULTTONEAREST) };
    let mut mi = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
    if !unsafe { GetMonitorInfoW(hmon, &mut mi) }.as_bool() {
        return None;
    }
    let (mut dx, mut dy) = (96u32, 96u32);
    let scale = match unsafe { GetDpiForMonitor(hmon, MDT_EFFECTIVE_DPI, &mut dx, &mut dy) } {
        Ok(()) => dx as f32 / 96.0,
        Err(_) => 1.0,
    };
    Some(ScreenInfo { anchor, from_icon, monitor: mi.rcMonitor, work: mi.rcWork, scale })
}

/// 計算面板左上角（實體像素）：以托盤圖示為錨點，放在圖示朝螢幕內側的那一邊，
/// 且不超出工作區。圖示在工作列上時面板貼齊工作列；圖示在「^」彈出框裡時，
/// 面板會退到圖示之外，不會擋住圖示。
fn panel_position(s: &ScreenInfo) -> PhysicalPosition {
    let w = (PANEL_W * s.scale) as i32;
    let h = (PANEL_H * s.scale) as i32;
    let m = (PANEL_MARGIN * s.scale) as i32;
    let (a, wk, mon) = (s.anchor, s.work, s.monitor);
    let clamp_x = |x: i32| x.clamp(wk.left + m, (wk.right - w - m).max(wk.left + m));
    let clamp_y = |y: i32| y.clamp(wk.top + m, (wk.bottom - h - m).max(wk.top + m));
    let cx = (a.left + a.right) / 2;
    let cy = (a.top + a.bottom) / 2;

    let (x, y) = if wk.top > mon.top {
        // 工作列在上：面板在圖示下方
        (clamp_x(cx - w / 2), (a.bottom + m).max(wk.top + m))
    } else if wk.left > mon.left {
        // 工作列在左：面板在圖示右方
        ((a.right + m).max(wk.left + m), clamp_y(cy - h / 2))
    } else if wk.right < mon.right {
        // 工作列在右：面板在圖示左方
        ((a.left - w - m).min(wk.right - w - m), clamp_y(cy - h / 2))
    } else {
        // 工作列在下（預設）：面板在圖示上方
        (clamp_x(cx - w / 2), (a.top - h - m).min(wk.bottom - h - m))
    };
    PhysicalPosition::new(x, y)
}

/// 讓面板成為前景視窗並取得鍵盤焦點。
/// 托盤點擊後 Shell 會授予本程式前景權限，此時呼叫 `SetForegroundWindow` 才會成功。
fn bring_to_front(panel: &Panel) {
    let ok = panel.window().with_winit_window(|w| {
        let Ok(handle) = w.window_handle() else { return false };
        let RawWindowHandle::Win32(h) = handle.as_raw() else { return false };
        let hwnd = HWND(h.hwnd.get() as *mut core::ffi::c_void);
        unsafe { SetForegroundWindow(hwnd) }.as_bool()
    });
    eprintln!("[面板] 取得前景焦點：{ok:?}");
}

/// 面板狀態：關閉時整個釋放元件，以驗證閒置記憶體
#[derive(Default)]
struct PanelState {
    panel: RefCell<Option<Panel>>,
    closed_at: Cell<Option<Instant>>,
}

impl PanelState {
    fn close(&self) {
        if let Some(p) = self.panel.borrow_mut().take() {
            let _ = p.hide();
            self.closed_at.set(Some(Instant::now()));
            eprintln!("[面板] 已關閉並釋放");
        }
    }

    fn toggle(self: &Rc<Self>) {
        if self.panel.borrow().is_some() {
            self.close();
            return;
        }
        // 點托盤會先讓面板失焦而關閉，接著才收到 clicked；此時不要再打開
        if self.closed_at.get().is_some_and(|t| t.elapsed() < REOPEN_GUARD) {
            eprintln!("[面板] 剛因失焦關閉，忽略這次點擊");
            return;
        }
        if let Err(e) = self.open() {
            eprintln!("[面板] 開啟失敗：{e}");
        }
    }

    fn open(self: &Rc<Self>) -> Result<(), slint::PlatformError> {
        let panel = Panel::new()?;
        let info = screen_for_tray();
        let text = match &info {
            Some(s) => format!(
                "{} L{} T{} R{} B{}\n工作區 L{} T{} R{} B{}\n縮放 {:.2}",
                if s.from_icon { "圖示" } else { "游標（取不到圖示）" },
                s.anchor.left, s.anchor.top, s.anchor.right, s.anchor.bottom,
                s.work.left, s.work.top, s.work.right, s.work.bottom, s.scale
            ),
            None => "無法取得游標或螢幕資訊".into(),
        };
        eprintln!("[面板] 開啟：{}", text.replace('\n', "；"));
        panel.set_info(text.into());
        if let Some(s) = &info {
            panel.window().set_position(panel_position(s));
        }

        let weak = Rc::downgrade(self);
        panel.on_close_requested(move || {
            if let Some(st) = weak.upgrade() {
                st.close();
            }
        });

        // 失焦（點擊面板外）時關閉；延到事件處理結束後再釋放，避免在自身回呼中被 drop
        let weak = Rc::downgrade(self);
        panel.window().on_winit_window_event(move |_, ev| {
            if let WindowEvent::Focused(false) = ev {
                let weak = weak.clone();
                slint::Timer::single_shot(Duration::ZERO, move || {
                    if let Some(st) = weak.upgrade() {
                        st.close();
                    }
                });
            }
            EventResult::Propagate
        });

        panel.show()?;
        // 視窗要到下一輪事件迴圈才真正建立，所以延後再搶前景焦點；
        // 面板沒有焦點時，Esc 與「點外面關閉」都不會生效
        let weak_panel = panel.as_weak();
        slint::Timer::single_shot(Duration::from_millis(30), move || {
            if let Some(p) = weak_panel.upgrade() {
                bring_to_front(&p);
            }
        });
        *self.panel.borrow_mut() = Some(panel);
        Ok(())
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    slint::BackendSelector::new().backend_name("winit".into()).renderer_name("software".into()).select()?;

    let tray = Tray::new()?;
    let panel = Rc::new(PanelState::default());
    let about: Rc<RefCell<Option<About>>> = Rc::default();

    // 動態圖示：每 3 秒輪換顏色與警示標記，同時更新 Tooltip
    let step = Rc::new(Cell::new(0usize));
    let apply = {
        let tray = tray.as_weak();
        let step = step.clone();
        move || {
            let Some(tray) = tray.upgrade() else { return };
            let i = step.get();
            let (name, rgb) = COLORS[(i / 2) % COLORS.len()];
            let warn = i % 2 == 1;
            tray.set_tray_icon(make_icon(rgb, warn));
            tray.set_tray_tooltip(
                format!("AIBar 原型\n圖示：{name}{}\nClaude {}", if warn { "＋警示" } else { "" },
                    if tray.get_claude_enabled() { "啟用" } else { "停用" }).into(),
            );
        }
    };
    apply();
    let timer = slint::Timer::default();
    {
        let apply = apply.clone();
        let step = step.clone();
        timer.start(slint::TimerMode::Repeated, Duration::from_secs(3), move || {
            step.set(step.get() + 1);
            apply();
        });
    }

    {
        let panel = panel.clone();
        tray.on_left_clicked(move || {
            eprintln!("[托盤] 左鍵點擊");
            panel.toggle();
        });
    }
    tray.on_refresh_now(move || {
        eprintln!("[選單] 立即刷新");
        step.set(step.get() + 1);
        apply();
    });
    {
        let about = about.clone();
        tray.on_show_about(move || {
            eprintln!("[選單] 關於");
            let mut slot = about.borrow_mut();
            if slot.is_none() {
                *slot = About::new().ok();
            }
            if let Some(a) = slot.as_ref() {
                let _ = a.show();
            }
        });
    }
    tray.on_quit(|| {
        eprintln!("[選單] 結束");
        let _ = slint::quit_event_loop();
    });

    // 托盤沒有視窗，關閉所有視窗也不能結束程式
    slint::run_event_loop_until_quit()?;
    drop(tray);
    Ok(())
}
