//! Windows 實作（Win32 與 WinRT）。

mod system;

pub use system::{acquire_single_instance, copy_to_clipboard, open_url};
