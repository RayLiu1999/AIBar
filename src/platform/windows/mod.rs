//! Windows 實作（Win32 與 WinRT）。

pub mod autostart;
mod paths;
pub mod process;
pub mod process_tree;
pub mod secret_store;
mod system;
pub mod toast;
mod tray;
mod tray_icon;

pub use paths::{AGY_EXE, agy_dirs, antigravity_dir, app_dir, home_dir};
pub use system::{acquire_single_instance, copy_to_clipboard, open_url};
pub use tray::TrayHost;
pub use tray_icon::apply_app_icon;
