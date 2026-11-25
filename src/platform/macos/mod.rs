//! macOS 實作（AppKit 與系統內建工具）。

pub mod autostart;
mod bundle;
pub mod claude_keychain;
mod main_window;
mod paths;
pub mod process;
pub mod process_tree;
pub mod secret_store;
mod status_icon;
mod system;
pub mod toast;
mod tray;

pub use main_window::{MainWindowHost, after_show};
pub use paths::{AGY_EXE, agy_dirs, antigravity_dir, app_dir, home_dir, no_browser};
pub use system::{acquire_single_instance, copy_to_clipboard, open_url, prepare};
pub use tray::TrayHost;

/// 供介面用語判斷平台
pub const IS_MACOS: bool = true;
