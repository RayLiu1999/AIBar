//! Windows 系統功能：單一執行個體、複製到剪貼簿、以預設瀏覽器開啟網址。

use windows::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError, GlobalFree, HANDLE, HGLOBAL};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
};
use windows::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock};
use windows::Win32::System::Ole::CF_UNICODETEXT;
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
use windows::core::{HSTRING, w};

/// 取得單一執行個體鎖（具名 Mutex，範圍為目前登入工作階段）。
/// 已有另一份 AIBar 持有時回傳 false。鎖在程式結束時由系統釋放。
pub fn acquire_single_instance() -> bool {
    match unsafe { CreateMutexW(None, false, w!("Local\\AIBar.SingleInstance")) } {
        // 刻意不關閉 handle：持有到程式結束
        Ok(_handle) => (unsafe { GetLastError() }) != ERROR_ALREADY_EXISTS,
        // 建立失敗時不擋住啟動
        Err(_) => true,
    }
}

/// 把文字複製到剪貼簿；失敗時回傳 false
pub fn copy_to_clipboard(text: &str) -> bool {
    let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    let bytes = wide.len() * size_of::<u16>();
    unsafe {
        if OpenClipboard(None).is_err() {
            return false;
        }
        let ok = (|| -> Option<()> {
            EmptyClipboard().ok()?;
            let mem: HGLOBAL = GlobalAlloc(GMEM_MOVEABLE, bytes).ok()?;
            let ptr = GlobalLock(mem) as *mut u16;
            if ptr.is_null() {
                let _ = GlobalFree(Some(mem));
                return None;
            }
            std::ptr::copy_nonoverlapping(wide.as_ptr(), ptr, wide.len());
            let _ = GlobalUnlock(mem);
            // 成功後記憶體歸剪貼簿所有，不可再釋放
            if SetClipboardData(CF_UNICODETEXT.0 as u32, Some(HANDLE(mem.0))).is_err() {
                let _ = GlobalFree(Some(mem));
                return None;
            }
            Some(())
        })()
        .is_some();
        let _ = CloseClipboard();
        ok
    }
}

/// 以預設瀏覽器開啟網址（只接受 https）
pub fn open_url(url: &str) {
    if !url.starts_with("https://") {
        return;
    }
    unsafe {
        ShellExecuteW(None, w!("open"), &HSTRING::from(url), None, None, SW_SHOWNORMAL);
    }
}
