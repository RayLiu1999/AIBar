fn main() {
    // 編譯 UI；app.slint 匯出托盤與面板兩個頂層元件
    slint_build::compile("ui/app.slint").expect("Slint 編譯失敗");
}
