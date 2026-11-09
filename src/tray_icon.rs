//! 產生托盤圖示：依顏色畫實心圓，需要時在右下角加上警示點。
//! 以程式產生，不需要另外準備 8 個圖檔。

use slint::{Image, Rgba8Pixel, SharedPixelBuffer};

use crate::view::TrayColor;

const SIZE: u32 = 32;

fn rgb(color: TrayColor) -> [u8; 3] {
    match color {
        TrayColor::Green => [0x22, 0xc5, 0x5e],
        TrayColor::Yellow => [0xea, 0xb3, 0x08],
        TrayColor::Red => [0xef, 0x44, 0x44],
        TrayColor::Gray => [0x9c, 0xa3, 0xaf],
    }
}

/// 點 (x, y) 對半徑 r 的圓的覆蓋率（0–1），用於邊緣抗鋸齒
fn coverage(x: f32, y: f32, cx: f32, cy: f32, r: f32) -> f32 {
    let d = ((x - cx).powi(2) + (y - cy).powi(2)).sqrt();
    (r - d + 0.5).clamp(0.0, 1.0)
}

pub fn make(color: TrayColor, warn: bool) -> Image {
    let [r, g, b] = rgb(color);
    let mut buf = SharedPixelBuffer::<Rgba8Pixel>::new(SIZE, SIZE);
    let c = SIZE as f32 / 2.0;
    for (i, px) in buf.make_mut_slice().iter_mut().enumerate() {
        // 取像素中心
        let x = (i as u32 % SIZE) as f32 + 0.5;
        let y = (i as u32 / SIZE) as f32 + 0.5;
        let body = coverage(x, y, c, c, c - 1.0);
        // 警示點：白色實心圓，外圈留一圈透明，讓它在任何顏色上都清楚
        let (bx, by) = (24.5, 24.5);
        let ring = if warn { coverage(x, y, bx, by, 8.0) } else { 0.0 };
        let badge = if warn { coverage(x, y, bx, by, 6.0) } else { 0.0 };

        let alpha = body * (1.0 - ring) + badge;
        let (pr, pg, pb) = if badge > 0.0 { (0xff, 0xff, 0xff) } else { (r, g, b) };
        *px = Rgba8Pixel { r: pr, g: pg, b: pb, a: (alpha.clamp(0.0, 1.0) * 255.0) as u8 };
    }
    Image::from_rgba8(buf)
}
