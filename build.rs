//! 建置腳本：編譯 Slint UI，並把 App 圖示與檔案資訊嵌入 exe。

// 部分繪圖函式只有程式本體會用到
#[allow(dead_code)]
#[path = "src/icon_art.rs"]
mod icon_art;

use std::path::Path;

/// .ico 內含的尺寸（檔案總管、工作列、Alt+Tab 各取所需）
const ICON_SIZES: [u32; 6] = [16, 24, 32, 48, 64, 256];

/// 以 32 位元 BMP 格式寫出多尺寸 .ico（透明度由 alpha 通道提供，AND 遮罩全為 0）
fn write_ico(path: &Path) -> std::io::Result<()> {
    let images: Vec<(u32, Vec<u8>)> = ICON_SIZES
        .iter()
        .map(|&size| {
            let rgba = icon_art::app_icon(size).to_rgba();
            let mask_row = (size as usize).div_ceil(32) * 4;
            let mut data = Vec::new();
            // BITMAPINFOHEADER：高度為兩倍（色彩＋遮罩）
            data.extend_from_slice(&40u32.to_le_bytes());
            data.extend_from_slice(&(size as i32).to_le_bytes());
            data.extend_from_slice(&(size as i32 * 2).to_le_bytes());
            data.extend_from_slice(&1u16.to_le_bytes());
            data.extend_from_slice(&32u16.to_le_bytes());
            data.extend_from_slice(&[0u8; 24]);
            // BGRA，由下而上
            for y in (0..size as usize).rev() {
                for x in 0..size as usize {
                    let i = (y * size as usize + x) * 4;
                    data.extend_from_slice(&[rgba[i + 2], rgba[i + 1], rgba[i], rgba[i + 3]]);
                }
            }
            data.extend(std::iter::repeat_n(0u8, mask_row * size as usize));
            (size, data)
        })
        .collect();

    let mut ico = Vec::new();
    ico.extend_from_slice(&[0, 0, 1, 0]);
    ico.extend_from_slice(&(images.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * images.len() as u32;
    for (size, data) in &images {
        let dim = if *size >= 256 { 0 } else { *size as u8 };
        ico.extend_from_slice(&[dim, dim, 0, 0]);
        ico.extend_from_slice(&1u16.to_le_bytes());
        ico.extend_from_slice(&32u16.to_le_bytes());
        ico.extend_from_slice(&(data.len() as u32).to_le_bytes());
        ico.extend_from_slice(&offset.to_le_bytes());
        offset += data.len() as u32;
    }
    for (_, data) in &images {
        ico.extend_from_slice(data);
    }
    std::fs::write(path, ico)
}

fn main() {
    // 編譯 UI；app.slint 匯出托盤、主視窗與設定視窗
    slint_build::compile("ui/app.slint").expect("Slint 編譯失敗");

    println!("cargo:rerun-if-changed=src/icon_art.rs");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let out = std::env::var("OUT_DIR").expect("缺少 OUT_DIR");
        let ico = Path::new(&out).join("aibar.ico");
        write_ico(&ico).expect("無法產生 aibar.ico");

        // 檔案資訊：工作管理員「啟動應用程式」與檔案總管顯示的名稱
        let mut res = winresource::WindowsResource::new();
        res.set_icon(ico.to_str().expect("路徑含非 UTF-8 字元"))
            .set("FileDescription", "AIBar")
            .set("ProductName", "AIBar")
            .set("CompanyName", "Ray Liu")
            .set("LegalCopyright", "© 2026 Ray Liu");
        res.compile().expect("無法嵌入 Windows 資源（需要 Windows SDK 的 rc.exe）");
    }
}
