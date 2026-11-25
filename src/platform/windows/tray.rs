//! 系統匣圖示與右鍵選單（Slint 的 SystemTrayIcon）。

use std::rc::Rc;

use super::main_window::MainWindowHost;
use super::tray_icon;
use crate::Tray;
use crate::platform::TrayActions;
use crate::view::TrayView;

pub struct TrayHost(Tray);

impl TrayHost {
    /// 必須在建立任何視窗之前呼叫：Slint 1.18.1 的托盤只會嘗試建立一次，
    /// 若那時圖示還是空的就會失敗且不再重試（建立主視窗會觸發這次嘗試）
    pub fn new(initial: &TrayView) -> Result<Self, slint::PlatformError> {
        let tray = Tray::new()?;
        tray.set_tray_icon(tray_icon::make_tray(initial));
        Ok(Self(tray))
    }

    /// 接上左鍵（切換主視窗）與右鍵選單
    pub fn connect(&self, window: &Rc<MainWindowHost>, actions: TrayActions) {
        let weak = Rc::downgrade(window);
        self.0.on_left_clicked(move || {
            if let Some(window) = weak.upgrade() {
                window.toggle();
            }
        });
        self.0.on_refresh_now(actions.refresh);
        self.0.on_open_settings(actions.settings);
        self.0.on_show_about(actions.about);
        self.0.on_quit(actions.quit);
    }

    /// 更新圖示與 Tooltip
    pub fn set(&self, view: &TrayView) {
        self.0.set_tray_icon(tray_icon::make_tray(view));
        self.0.set_tray_tooltip(view.tooltip.as_str().into());
    }
}
