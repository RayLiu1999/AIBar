//! 圖示的基本繪圖與 App 圖示。只用標準函式庫，
//! 讓 build.rs（產生 exe 的 .ico）與程式本體（視窗圖示）共用同一份畫法。

/// 圓角矩形的覆蓋率（0–1），以有號距離場計算，邊緣自然抗鋸齒
pub fn rounded_rect(x: f32, y: f32, left: f32, top: f32, right: f32, bottom: f32, r: f32) -> f32 {
    let (cx, cy) = ((left + right) / 2.0, (top + bottom) / 2.0);
    let (hw, hh) = ((right - left) / 2.0, (bottom - top) / 2.0);
    let qx = (x - cx).abs() - hw + r;
    let qy = (y - cy).abs() - hh + r;
    let outside = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt();
    let d = outside + qx.max(qy).min(0.0) - r;
    (0.5 - d).clamp(0.0, 1.0)
}

/// 圓的覆蓋率（0–1）
pub fn circle(x: f32, y: f32, cx: f32, cy: f32, r: f32) -> f32 {
    let d = ((x - cx).powi(2) + (y - cy).powi(2)).sqrt() - r;
    (0.5 - d).clamp(0.0, 1.0)
}

/// RGBA 畫布（直接透明度，非預乘；各通道 0–1）
pub struct Canvas {
    pub size: u32,
    pub px: Vec<[f32; 4]>,
}

impl Canvas {
    pub fn new(size: u32) -> Self {
        Self { size, px: vec![[0.0; 4]; (size * size) as usize] }
    }

    /// 以覆蓋率 `cov`（0–1）把顏色疊到第 `i` 個像素上（source-over）
    pub fn blend(&mut self, i: usize, rgb: [u8; 3], cov: f32) {
        if cov <= 0.0 {
            return;
        }
        let d = &mut self.px[i];
        let src = [rgb[0] as f32 / 255.0, rgb[1] as f32 / 255.0, rgb[2] as f32 / 255.0];
        let out_a = cov + d[3] * (1.0 - cov);
        for c in 0..3 {
            d[c] = (src[c] * cov + d[c] * d[3] * (1.0 - cov)) / out_a.max(1e-6);
        }
        d[3] = out_a;
    }

    /// 對每個像素中心呼叫 `f(x, y) -> 覆蓋率` 並疊上顏色
    pub fn fill(&mut self, rgb: [u8; 3], f: impl Fn(f32, f32) -> f32) {
        for i in 0..self.px.len() {
            let x = (i as u32 % self.size) as f32 + 0.5;
            let y = (i as u32 / self.size) as f32 + 0.5;
            self.blend(i, rgb, f(x, y));
        }
    }

    /// 轉成 RGBA 位元組（由上而下）
    pub fn to_rgba(&self) -> Vec<u8> {
        let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        self.px.iter().flat_map(|p| [q(p[0]), q(p[1]), q(p[2]), q(p[3])]).collect()
    }
}

/// App 圖示：深色圓角方塊內三根由低到高、分別為 Provider 代表色的長條
pub fn app_icon(size: u32) -> Canvas {
    let s = size as f32;
    let k = s / 32.0; // 以 32×32 的設計稿座標縮放
    let mut canvas = Canvas::new(size);
    canvas.fill([0x26, 0x30, 0x3b], |x, y| rounded_rect(x, y, 0.0, 0.0, s, s, 8.0 * k));
    let bars: [(f32, f32, [u8; 3]); 3] = [
        (7.0, 17.0, [0xE0, 0x8A, 0x67]),
        (13.75, 12.0, [0x4C, 0xB5, 0xC1]),
        (20.5, 7.0, [0x7B, 0xC8, 0x8A]),
    ];
    for (left, top, color) in bars {
        canvas.fill(color, |x, y| {
            rounded_rect(x, y, left * k, top * k, (left + 4.5) * k, 25.0 * k, 2.0 * k)
        });
    }
    canvas
}
