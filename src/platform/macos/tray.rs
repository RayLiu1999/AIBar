//! 選單列圖示（原生 `NSStatusItem`）。
//!
//! 不用 Slint 的 SystemTrayIcon：它以 target/action 處理點擊，而 macOS 27 起點圖示彈出視窗
//! 應使用 `NSStatusItemExpandedInterfaceDelegate`；也拿不到圖示的座標。
//! macOS 26 以前沒有這個 API，退回 target/action。

use std::cell::RefCell;
use std::rc::{Rc, Weak};
use std::time::Duration;

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{
    AllocAnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, class, define_class, extern_protocol, msg_send, sel,
};
use objc2_app_kit::{
    NSApplication, NSEventMask, NSEventType, NSImage, NSMenu, NSMenuItem, NSStatusBar, NSStatusItem,
    NSVariableStatusItemLength,
};
use objc2_foundation::{NSData, NSPoint, NSRect, NSSize, NSString};

use super::main_window::MainWindowHost;
use super::status_icon;
use crate::icon_art;
use crate::platform::TrayActions;
use crate::view::TrayView;

extern_protocol!(
    /// macOS 27 SDK 的 `NSStatusItemExpandedInterfaceDelegate`（objc2-app-kit 0.3.2 還沒有）
    /// 方法簽章必須與 SDK 標頭一致。
    #[allow(clippy::missing_safety_doc)]
    #[name = "NSStatusItemExpandedInterfaceDelegate"]
    unsafe trait ExpandedInterfaceDelegate: NSObjectProtocol {
        #[unsafe(method(statusItem:didBeginExpandedInterfaceSession:))]
        fn did_begin(&self, item: &NSStatusItem, session: &AnyObject);
        #[unsafe(method(statusItemDidEndExpandedInterfaceSession:animated:))]
        fn did_end(&self, item: &NSStatusItem, animated: bool);
    }
);

struct HandlerState {
    window: Weak<MainWindowHost>,
    actions: TrayActions,
}

define_class!(
    #[unsafe(super = NSObject)]
    #[thread_kind = MainThreadOnly]
    #[ivars = HandlerState]
    struct Handler;

    unsafe impl NSObjectProtocol for Handler {}

    unsafe impl ExpandedInterfaceDelegate for Handler {
        #[unsafe(method(statusItem:didBeginExpandedInterfaceSession:))]
        fn session_began(&self, _item: &NSStatusItem, _session: &AnyObject) {
            if let Some(window) = self.ivars().window.upgrade() {
                window.show();
            }
        }

        #[unsafe(method(statusItemDidEndExpandedInterfaceSession:animated:))]
        fn session_ended(&self, _item: &NSStatusItem, _animated: bool) {
            if let Some(window) = self.ivars().window.upgrade() {
                window.hide();
            }
        }
    }

    impl Handler {
        #[unsafe(method(clicked:))]
        fn clicked(&self, sender: &AnyObject) {
            let event = NSApplication::sharedApplication(self.mtm()).currentEvent();
            if event.is_some_and(|e| e.r#type() == NSEventType::RightMouseUp) {
                self.pop_up_menu(sender);
            } else if !has_expanded_interface()
                && let Some(window) = self.ivars().window.upgrade()
            {
                window.toggle();
            }
        }

        #[unsafe(method(refresh:))]
        fn refresh(&self, _sender: &AnyObject) {
            (self.ivars().actions.refresh)();
        }

        #[unsafe(method(settings:))]
        fn settings(&self, _sender: &AnyObject) {
            (self.ivars().actions.settings)();
        }

        #[unsafe(method(about:))]
        fn about(&self, _sender: &AnyObject) {
            (self.ivars().actions.about)();
        }

        #[unsafe(method(quit:))]
        fn quit(&self, _sender: &AnyObject) {
            (self.ivars().actions.quit)();
        }
    }
);

impl Handler {
    fn new(mtm: MainThreadMarker, state: HandlerState) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(state);
        unsafe { msg_send![super(this), init] }
    }

    /// 右鍵選單；系統不一定會把右鍵送給選單列圖示，因此所有功能在面板上也都有
    fn pop_up_menu(&self, button: &AnyObject) {
        let menu = NSMenu::new(self.mtm());
        let items = [
            ("立即刷新", sel!(refresh:)),
            ("設定…", sel!(settings:)),
            ("關於", sel!(about:)),
            ("結束", sel!(quit:)),
        ];
        for (title, action) in items {
            if title == "結束" {
                menu.addItem(&NSMenuItem::separatorItem(self.mtm()));
            }
            let item = NSMenuItem::new(self.mtm());
            item.setTitle(&NSString::from_str(title));
            unsafe {
                item.setTarget(Some(self));
                item.setAction(Some(action));
            }
            menu.addItem(&item);
        }
        let Some(view) = button.downcast_ref::<objc2_app_kit::NSView>() else {
            return;
        };
        let below = NSPoint::new(0.0, view.bounds().size.height + 4.0);
        menu.popUpMenuPositioningItem_atLocation_inView(None, below, Some(view));
    }
}

/// 是否有 macOS 27 的 expanded interface API
fn has_expanded_interface() -> bool {
    class!(NSStatusItem)
        .instance_method(sel!(setExpandedInterfaceDelegate:))
        .is_some()
}

/// 已放上選單列的圖示
pub struct StatusItem {
    item: Retained<NSStatusItem>,
    /// `NSStatusItem` 與按鈕只弱參照 delegate 與 target，由這裡持有
    _handler: Retained<Handler>,
    mtm: MainThreadMarker,
}

impl StatusItem {
    fn new(mtm: MainThreadMarker, state: HandlerState) -> Self {
        let item = NSStatusBar::systemStatusBar().statusItemWithLength(NSVariableStatusItemLength);
        let handler = Handler::new(mtm, state);
        if let Some(button) = item.button(mtm) {
            unsafe {
                button.setTarget(Some(&handler));
                button.setAction(Some(sel!(clicked:)));
            }
            button.sendActionOn(NSEventMask::LeftMouseUp | NSEventMask::RightMouseUp);
            button.setImagePosition(objc2_app_kit::NSCellImagePosition::ImageLeading);
        }
        if has_expanded_interface() {
            let delegate: &ProtocolObject<dyn ExpandedInterfaceDelegate> = ProtocolObject::from_ref(&*handler);
            let _: () = unsafe { msg_send![&*item, setExpandedInterfaceDelegate: delegate] };
        }
        Self {
            item,
            _handler: handler,
            mtm,
        }
    }

    fn set(&self, view: &TrayView) {
        let Some(button) = self.item.button(self.mtm) else {
            return;
        };
        let png = icon_art::png(&status_icon::draw(view));
        if let Some(image) = NSImage::initWithData(NSImage::alloc(), &NSData::with_bytes(&png)) {
            image.setSize(NSSize::new(18.0, 18.0));
            button.setImage(Some(&image));
        }
        let title = status_icon::title(view);
        button.setTitle(&NSString::from_str(&format!(" {title}")));
        button.setAppearsDisabled(view.dim);
        button.setToolTip(Some(&NSString::from_str(&view.tooltip)));
    }

    /// 圖示在螢幕上的範圍，與所在螢幕的可用區（AppKit 座標，原點在左下）
    pub fn anchor(&self) -> Option<(NSRect, NSRect)> {
        let button = self.item.button(self.mtm)?;
        let window = button.window()?;
        let rect = window.convertRectToScreen(button.convertRect_toView(button.bounds(), None));
        Some((rect, window.screen()?.visibleFrame()))
    }

    /// 面板自行收起時通知系統；沒有進行中的 session 則不做事
    pub fn cancel_session(&self) {
        if !has_expanded_interface() {
            return;
        }
        let session: Option<Retained<AnyObject>> = unsafe { msg_send![&*self.item, expandedInterfaceSession] };
        if let Some(session) = session {
            let _: () = unsafe { msg_send![&*session, cancel] };
        }
    }
}

#[derive(Default)]
struct Inner {
    item: RefCell<Option<Rc<StatusItem>>>,
    /// 圖示放上之前的最新內容
    pending: RefCell<Option<TrayView>>,
}

pub struct TrayHost(Rc<Inner>);

impl TrayHost {
    pub fn new(_initial: &TrayView) -> Result<Self, slint::PlatformError> {
        Ok(Self(Rc::default()))
    }

    /// 事件迴圈開始後放上圖示，並接上面板與選單
    pub fn connect(&self, window: &Rc<MainWindowHost>, actions: TrayActions) {
        let inner = self.0.clone();
        let weak = Rc::downgrade(window);
        slint::Timer::single_shot(Duration::ZERO, move || {
            let Some(mtm) = MainThreadMarker::new() else { return };
            let Some(window) = weak.upgrade() else { return };
            let item = Rc::new(StatusItem::new(mtm, HandlerState { window: weak, actions }));
            if let Some(view) = inner.pending.take() {
                item.set(&view);
            }
            window.attach(Rc::downgrade(&item));
            *inner.item.borrow_mut() = Some(item);
        });
    }

    /// 更新圖示、文字與 Tooltip
    pub fn set(&self, view: &TrayView) {
        match self.0.item.borrow().as_ref() {
            Some(item) => item.set(view),
            None => *self.0.pending.borrow_mut() = Some(view.clone()),
        }
    }
}
