//! 繪製圖示：托盤數字圖示（規格書第 7 節）與 App 圖示。
//!
//! 托盤圖示依目前 DPI 的小圖示尺寸直接繪製，不縮放點陣，文字才會清楚。
//! 文字用 GDI 以灰階抗鋸齒畫成遮罩（白字黑底），再當作透明度合成到圖示上。

use slint::{Image, Rgba8Pixel, SharedPixelBuffer};
use windows::Win32::Foundation::{COLORREF, RECT};
use windows::Win32::Graphics::Gdi::{
    ANTIALIASED_QUALITY, BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CLIP_DEFAULT_PRECIS,
    CreateCompatibleDC, CreateDIBSection, CreateFontW, DEFAULT_CHARSET, DIB_RGB_COLORS,
    DT_CENTER, DT_NOPREFIX, DT_SINGLELINE, DT_VCENTER, DeleteDC, DeleteObject, DrawTextW,
    FF_SWISS, FW_BOLD, GdiFlush, OUT_DEFAULT_PRECIS, SelectObject, SetBkMode, SetTextColor,
    TRANSPARENT, VARIABLE_PITCH,
};
use windows::Win32::UI::HiDpi::{GetDpiForSystem, GetSystemMetricsForDpi};
use windows::Win32::UI::WindowsAndMessaging::SM_CXSMICON;
use windows::core::HSTRING;

use crate::icon_art::{self, Canvas, circle, rounded_rect};
use crate::view::{TrayFill, TrayView};

type Rgb = [u8; 3];

const TILE_BG: Rgb = [0x1c, 0x1c, 0x1c];
const RIM_GRAY: Rgb = [0x6b, 0x6b, 0x6b];
const LOW_BG: Rgb = [0xF2, 0xB5, 0x44];
const CRITICAL_BG: Rgb = [0xE5, 0x53, 0x4B];
const WARN: Rgb = [0xF2, 0xB5, 0x44];

/// 目前 DPI 下托盤小圖示的邊長（像素）
pub fn tray_icon_size() -> u32 {
    let size = unsafe { GetSystemMetricsForDpi(SM_CXSMICON, GetDpiForSystem()) };
    if size > 0 { size as u32 } else { 16 }
}

fn into_image(canvas: Canvas) -> Image {
    let rgba = canvas.to_rgba();
    let mut buf = SharedPixelBuffer::<Rgba8Pixel>::new(canvas.size, canvas.size);
    for (dst, p) in buf.make_mut_slice().iter_mut().zip(rgba.chunks_exact(4)) {
        *dst = Rgba8Pixel { r: p[0], g: p[1], b: p[2], a: p[3] };
    }
    Image::from_rgba8(buf)
}

/// 用 GDI 畫出文字遮罩（每像素 0–255 的覆蓋率），文字置中
fn text_mask(text: &str, size: u32, em_px: i32, face: &str) -> Option<Vec<u8>> {
    let n = size as i32;
    unsafe {
        let hdc = CreateCompatibleDC(None);
        if hdc.is_invalid() {
            return None;
        }
        let bi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: n,
                biHeight: -n, // 由上而下
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
        let Ok(bmp) = CreateDIBSection(Some(hdc), &bi, DIB_RGB_COLORS, &mut bits, None, 0) else {
            let _ = DeleteDC(hdc);
            return None;
        };
        let old_bmp = SelectObject(hdc, bmp.into());
        let font = CreateFontW(
            -em_px,
            0,
            0,
            0,
            FW_BOLD.0 as i32,
            0,
            0,
            0,
            DEFAULT_CHARSET,
            OUT_DEFAULT_PRECIS,
            CLIP_DEFAULT_PRECIS,
            ANTIALIASED_QUALITY,
            (VARIABLE_PITCH.0 | FF_SWISS.0) as u32,
            &HSTRING::from(face),
        );
        let old_font = SelectObject(hdc, font.into());
        SetTextColor(hdc, COLORREF(0x00FF_FFFF));
        SetBkMode(hdc, TRANSPARENT);
        let mut rc = RECT { left: 0, top: 0, right: n, bottom: n };
        let mut wide: Vec<u16> = text.encode_utf16().collect();
        DrawTextW(hdc, &mut wide, &mut rc, DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);
        let _ = GdiFlush();

        // DIB 為 BGRA；白字的任一通道即覆蓋率
        let raw = std::slice::from_raw_parts(bits as *const u8, (size * size * 4) as usize);
        let mask = raw.chunks_exact(4).map(|p| p[0].max(p[1]).max(p[2])).collect();

        SelectObject(hdc, old_font);
        SelectObject(hdc, old_bmp);
        let _ = DeleteObject(font.into());
        let _ = DeleteObject(bmp.into());
        let _ = DeleteDC(hdc);
        Some(mask)
    }
}

/// 托盤數字圖示，尺寸為目前 DPI 的托盤小圖示大小
pub fn make_tray(view: &TrayView) -> Image {
    make_tray_sized(view, tray_icon_size())
}

pub fn make_tray_sized(view: &TrayView, size: u32) -> Image {
    let s = size as f32;
    let mut canvas = Canvas::new(size);
    let radius = (s * 0.22).max(3.0);
    let rim = (s * 0.1).max(1.5);

    let bg = match view.fill {
        TrayFill::Normal => TILE_BG,
        TrayFill::Low => LOW_BG,
        TrayFill::Critical => CRITICAL_BG,
    };
    let fg: Rgb = match (view.fill, view.dim) {
        (TrayFill::Low, _) => [0x2b, 0x1f, 0x05],
        (_, true) => [0xa8, 0xa8, 0xa8],
        _ => [0xff, 0xff, 0xff],
    };

    // 外框（整塊）→ 內部底色
    canvas.fill(view.rim.unwrap_or(RIM_GRAY), |x, y| rounded_rect(x, y, 0.0, 0.0, s, s, radius));
    canvas.fill(bg, |x, y| {
        rounded_rect(x, y, rim, rim, s - rim, s - rim, (radius - rim).max(1.0))
    });

    // 文字：數字用 Segoe UI，「滿」用正黑體；兩位數要塞進內框，字級較小
    let chars = view.text.chars().count();
    let (face, em) = if view.text.is_ascii() {
        ("Segoe UI", if chars >= 2 { s * 0.62 } else { s * 0.72 })
    } else {
        ("Microsoft JhengHei UI", s * 0.6)
    };
    if let Some(mask) = text_mask(&view.text, size, em.round() as i32, face) {
        for (i, m) in mask.into_iter().enumerate() {
            canvas.blend(i, fg, m as f32 / 255.0);
        }
    }

    // 警示標記：右下角黃點，壓在外框角落以免蓋住數字；外圍一圈深色讓它在任何底色上都看得出來
    if view.warn {
        let r = (s * 0.15).max(2.2);
        let (cx, cy) = (s - r - 0.3, s - r - 0.3);
        canvas.fill([0x20, 0x20, 0x20], |x, y| circle(x, y, cx, cy, r + s * 0.06));
        canvas.fill(WARN, |x, y| circle(x, y, cx, cy, r));
    }
    into_image(canvas)
}

/// App 圖示（視窗與工作列）；畫法與 exe 圖示共用 icon_art
pub fn make_app_icon(size: u32) -> Image {
    into_image(icon_art::app_icon(size))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view(text: &str, fill: TrayFill, warn: bool) -> TrayView {
        TrayView {
            text: text.into(),
            rim: Some([0x4C, 0xB5, 0xC1]),
            fill,
            dim: false,
            warn,
            tooltip: String::new(),
        }
    }

    #[test]
    fn 文字遮罩有內容() {
        let mask = text_mask("59", 20, 12, "Segoe UI").unwrap();
        assert!(mask.iter().any(|&m| m > 200), "應畫出文字");
        let mask = text_mask("滿", 20, 12, "Microsoft JhengHei UI").unwrap();
        assert!(mask.iter().any(|&m| m > 200), "應畫出中文字");
    }

    #[test]
    fn 圖示尺寸正確() {
        for size in [16, 20, 24, 32] {
            let img = make_tray_sized(&view("59", TrayFill::Low, true), size);
            assert_eq!(img.size().width, size);
        }
        assert_eq!(make_app_icon(64).size().width, 64);
    }

    /// 手動檢查用：把各種狀態的圖示輸出成 RGBA 原始檔
    /// （`cargo test dump_icons -- --ignored`，輸出到 target/icons/）
    #[test]
    #[ignore]
    fn dump_icons() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/icons");
        std::fs::create_dir_all(&dir).unwrap();
        let cases = [
            ("normal", view("59", TrayFill::Normal, false)),
            ("low", view("24", TrayFill::Low, false)),
            ("critical", view("7", TrayFill::Critical, false)),
            ("full", view("滿", TrayFill::Normal, false)),
            (
                "stale_warn",
                TrayView { rim: None, dim: true, ..view("59", TrayFill::Normal, true) },
            ),
        ];
        for size in [16u32, 20, 24, 32] {
            for (name, v) in &cases {
                let img = make_tray_sized(v, size);
                let buf = img.to_rgba8().unwrap();
                std::fs::write(dir.join(format!("{name}_{size}.rgba")), buf.as_bytes()).unwrap();
            }
        }
        let buf = make_app_icon(64).to_rgba8().unwrap();
        std::fs::write(dir.join("app_64.rgba"), buf.as_bytes()).unwrap();
    }
}
