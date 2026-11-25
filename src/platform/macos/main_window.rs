//! 主視窗（工單 07 改為選單列面板）。

use std::rc::Rc;

use slint::{ComponentHandle, ModelRc, VecModel};

use crate::config::WindowGeometry;
use crate::{CardData, MainWindow};

pub fn after_show<T: ComponentHandle + 'static>(_component: &T) {}

pub struct MainWindowHost {
    window: MainWindow,
    cards: Rc<VecModel<CardData>>,
}

impl MainWindowHost {
    pub fn new(
        _saved: Option<WindowGeometry>,
        on_refresh: impl Fn() + 'static,
        _on_shown: impl Fn() + 'static,
        _on_hidden: impl Fn(WindowGeometry) + 'static,
    ) -> Result<Rc<Self>, slint::PlatformError> {
        let window = MainWindow::new()?;
        let cards = Rc::new(VecModel::default());
        window.set_cards(ModelRc::from(cards.clone()));
        window.on_refresh_all(on_refresh);
        Ok(Rc::new(Self { window, cards }))
    }

    pub fn set_cards(&self, cards: Vec<CardData>) {
        self.cards.set_vec(cards);
    }

    pub fn ui(&self) -> &MainWindow {
        &self.window
    }

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

    pub fn set_columns(&self, columns: bool) {
        self.window.set_columns(columns);
    }
}
