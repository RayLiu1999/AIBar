# 01 選單列面板原型

Type: prototype
Status: needs-info（等使用者實際點擊）
Blocked by: 無

要回答的問題：macOS 27 上，原生 `NSStatusItem`＋Slint 無邊框面板能否做出 D1、D2 的互動？原型未合併。

已完成（自動測試，2026-11-25）：

- [x] 27 的 expanded interface API 存在並可設定 delegate
- [x] 面板貼在圖示正下方、水平置中（Retina 主螢幕）；第一次顯示 17 ms，之後 2 ms
- [x] `LSUIElement` 生效：activation policy 為 accessory
- [x] 面板能取得焦點；失焦事件會送到
- [x] 軟體渲染器正確顯示繁中
- [x] 尚未開過面板：記憶體 22 MB，閒置 30 秒 CPU 時間約 10 ms；開過再隱藏：39 MB

實際點擊（2026-11-25，依事件紀錄）：

- [x] 左鍵彈出、失焦收起：10 次 session 皆成對開關，沒有收起後立刻重開（最短間隔約 0.35 秒，為使用者再次點擊）
- [x] 失焦一定先於 session 結束送達；重複收起不會出錯

待確認：

- [ ] Esc 收起（紀錄中尚無）；再點一次圖示與點外面的手感
- [ ] 右鍵是否有事件（紀錄中沒有任何 action 事件，尚不知是否按過右鍵）
- [ ] 在非 Retina 螢幕點圖示，位置與清晰度
- [ ] 全螢幕 App 與其他桌面空間
- [ ] 圓角、陰影、深淺色選單列的外觀

正式實作要注意：

- 子元件繼承 `MainWindow` 時父元件屬性不會匯出到 Rust；只用 winit hook 去邊框會被 Slint 改回來。要在 `MainWindow` 加屬性控制 `no-frame`。
- 程式呼叫 `performClick` 只會觸發 action，不會開始 session，不能用來自動測試 27 的點擊。
- 中文粗體沒有生效（數字有），要指定字型（例如 PingFang TC）再比較。

## Comments
