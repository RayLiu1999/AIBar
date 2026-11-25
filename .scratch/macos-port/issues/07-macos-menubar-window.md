# 07 macOS 選單列與面板

Type: task
Status: ready-for-human（等實際操作確認）
Blocked by: 01, 06

依 D1–D3、D9 與 01 的結論實作。

- [x] 選單列為代表色圖示＋剩餘百分比；低量、舊快照、失敗原因仍可從圖示分辨
- [x] 左鍵彈出面板；再點、點外面、Esc 收起；27 使用 expanded interface session，收起時一併 `cancel`
- [x] 面板開啟時觸發「開啟視窗」的刷新規則（規格書第 9 節）
- [ ] 面板不超出圖示所在螢幕；Retina 與一般螢幕混用時位置正確
- [x] `MainWindow` 加 `popover` 屬性：無邊框、不顯示並排切換、加入「結束」；Windows 的外觀不變
- [ ] 開啟設定視窗時面板收起；設定視窗可正常取得焦點、Cmd+W 關閉
- [x] `settings.slint` 第 250、409、429、434 行等 Windows 字樣依平台顯示
- [ ] 中文粗體正確

## Comments

2026-11-25：已實作，已勾選者為程式完成，尚未由使用者實際操作正式版確認。中文粗體未解：Slint 找不到 PingFang TC（畫面與預設相同），Heiti TC 外觀更差，維持預設。
重要：若右鍵沒有事件，關於頁仍可由「設定」進入。
