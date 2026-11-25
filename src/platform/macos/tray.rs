//! 選單列圖示（工單 07 實作）。

use std::rc::Rc;

use super::main_window::MainWindowHost;
use crate::platform::TrayActions;
use crate::view::TrayView;

pub struct TrayHost;

impl TrayHost {
    pub fn new(_initial: &TrayView) -> Result<Self, slint::PlatformError> {
        Ok(Self)
    }

    pub fn connect(&self, _window: &Rc<MainWindowHost>, _actions: TrayActions) {}

    pub fn set(&self, _view: &TrayView) {}
}
