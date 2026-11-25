# 05 純重構：行程探測與 agy 執行

Type: task
Status: ready-for-human（等 Windows CI 與實機確認）
Blocked by: 03

對象：`antigravity.rs` 的行程列舉、命令列、監聽連接埠；`agy.rs` 的 Job Object、隱藏主控台與路徑尋找。解析函式不動。

- [x] `platform` 提供：依執行檔名篩選行程並讀命令列、某 pid 的監聽連接埠、可整組終止的子行程、agy 候選路徑
- [x] 既有解析測試不改內容即通過
- [ ] Windows 實機：Antigravity App 開著、關閉後改用 agy 兩條路徑；agy 結束後沒有殘留行程

## Comments

2026-11-25：在 Mac 以 `x86_64-pc-windows-msvc` 目標跑 fmt 與 clippy（`-D warnings`）通過，資源編譯器以空殼代替、未連結。共用單元測試 48 個以 stub 平台在 Mac 執行通過；DPAPI 與托盤繪圖共 4 個（其中 1 個 ignored）只能在 Windows 執行。
