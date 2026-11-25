//! Windows 實作（Win32 與 WinRT）。

pub mod autostart;
mod paths;
mod system;

pub use paths::{app_dir, home_dir};
pub use system::{acquire_single_instance, copy_to_clipboard, open_url};
