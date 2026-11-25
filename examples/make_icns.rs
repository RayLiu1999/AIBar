//! 產生 macOS 的 .icns：`cargo run --example make_icns -- <輸出路徑>`。
//! 圖示與 Windows 的 exe 圖示共用 `icon_art::app_icon`。

#[allow(dead_code)]
#[path = "../src/icon_art.rs"]
mod icon_art;

/// icns 的類型代碼與像素大小（內容為 PNG）
const ENTRIES: [(&[u8; 4], u32); 8] = [
    (b"ic11", 32),
    (b"ic12", 64),
    (b"ic07", 128),
    (b"ic13", 256),
    (b"ic08", 256),
    (b"ic14", 512),
    (b"ic09", 512),
    (b"ic10", 1024),
];

fn main() -> std::io::Result<()> {
    let out = std::env::args().nth(1).expect("用法：make_icns <輸出路徑>");
    let mut body = Vec::new();
    for (kind, size) in ENTRIES {
        let png = icon_art::png(&icon_art::app_icon(size));
        body.extend_from_slice(kind);
        body.extend_from_slice(&(png.len() as u32 + 8).to_be_bytes());
        body.extend_from_slice(&png);
    }
    let mut icns = b"icns".to_vec();
    icns.extend_from_slice(&(body.len() as u32 + 8).to_be_bytes());
    icns.extend_from_slice(&body);
    std::fs::write(out, icns)
}
