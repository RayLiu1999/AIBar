//! 選單列圖示的內容：代表色外框的小電量格，百分比文字由 AppKit 畫在旁邊。

use crate::icon_art::{Canvas, circle, rounded_rect};
use crate::view::{TrayFill, TrayView};

type Rgb = [u8; 3];

const TILE_BG: Rgb = [0x1c, 0x1c, 0x1c];
const RIM_GRAY: Rgb = [0x6b, 0x6b, 0x6b];
const LOW: Rgb = [0xF2, 0xB5, 0x44];
const CRITICAL: Rgb = [0xE5, 0x53, 0x4B];

/// 圖示像素大小：顯示為 18pt，3 倍像素讓 Retina 清楚
pub const SIZE: u32 = 54;

/// 圖示旁的文字：「59」→「59%」、「滿」→「100%」、沒有資料為「–」
pub fn title(view: &TrayView) -> String {
    match remaining(view) {
        Some(r) => format!("{r:.0}%"),
        None => view.text.clone(),
    }
}

/// 剩餘百分比；沒有資料時為 None
fn remaining(view: &TrayView) -> Option<f32> {
    match view.text.as_str() {
        "滿" => Some(100.0),
        t => t.parse().ok(),
    }
}

/// 外框為代表色（舊快照為灰）；內部由下往上填滿剩餘量，低量轉黃、轉紅；失敗時右下角加黃點
pub fn draw(view: &TrayView) -> Canvas {
    let s = SIZE as f32;
    let mut canvas = Canvas::new(SIZE);
    let (rim_w, radius) = (s * 0.1, s * 0.24);
    let rim = view.rim.unwrap_or(RIM_GRAY);
    canvas.fill(rim, |x, y| rounded_rect(x, y, 0.0, 0.0, s, s, radius));
    let inner = radius - rim_w;
    canvas.fill(TILE_BG, |x, y| {
        rounded_rect(x, y, rim_w, rim_w, s - rim_w, s - rim_w, inner)
    });

    let level = match view.fill {
        TrayFill::Normal => rim,
        TrayFill::Low => LOW,
        TrayFill::Critical => CRITICAL,
    };
    if let Some(r) = remaining(view) {
        let gap = rim_w * 1.7;
        let height = (s - 2.0 * gap) * (r / 100.0).clamp(0.0, 1.0);
        // 幾乎用完時仍留一條看得見的底線
        let height = height.max(s * 0.08);
        let (top, bottom) = (s - gap - height, s - gap);
        let corner = (inner - (gap - rim_w)).max(1.0).min(height / 2.0);
        canvas.fill(level, |x, y| rounded_rect(x, y, gap, top, s - gap, bottom, corner));
    }
    if view.warn {
        let r = s * 0.16;
        let c = s - r;
        canvas.fill([0x20, 0x20, 0x20], |x, y| circle(x, y, c, c, r + s * 0.05));
        canvas.fill(LOW, |x, y| circle(x, y, c, c, r));
    }
    canvas
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
    fn 文字() {
        assert_eq!(title(&view("59", TrayFill::Normal, false)), "59%");
        assert_eq!(title(&view("滿", TrayFill::Normal, false)), "100%");
        assert_eq!(title(&view("–", TrayFill::Normal, false)), "–");
    }

    #[test]
    fn 各狀態的圖示互不相同() {
        let icons: Vec<Vec<u8>> = [
            view("59", TrayFill::Normal, false),
            view("24", TrayFill::Low, false),
            view("7", TrayFill::Critical, false),
            view("59", TrayFill::Normal, true),
            TrayView {
                rim: None,
                ..view("59", TrayFill::Normal, false)
            },
            view("–", TrayFill::Normal, false),
        ]
        .iter()
        .map(|v| draw(v).to_rgba())
        .collect();
        for (i, a) in icons.iter().enumerate() {
            assert_eq!(a.len(), (SIZE * SIZE * 4) as usize);
            assert!(icons[i + 1..].iter().all(|b| a != b));
        }
    }
}
