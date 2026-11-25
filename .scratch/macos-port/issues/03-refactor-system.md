# 03 純重構：系統功能搬入 platform/windows

Type: task
Status: ready-for-human（等 Windows CI 與實機確認）
Blocked by: 無

只搬移，不改行為。對象：`platform.rs`、`autostart.rs`、`secret.rs` 的 DPAPI、`notify.rs` 的 Toast 與 AUMID、`config.rs::app_dir`、`providers/mod.rs::home_dir`。

- [x] `windows` 移到僅限 Windows 的相依；`build.rs` 在非 Windows 主機可編譯。`winresource` 刻意不動：建置相依的 cfg 看主機而非目標
- [x] `Notifier` 的門檻邏輯留在 `notify.rs`，既有測試不改內容即通過
- [x] `%APPDATA%\AiBar` 的檔名、格式不變，現有使用者的設定與 `secrets.bin` 可直接沿用
- [x] 每種功能一個 commit
- [ ] Windows CI 通過
- [ ] Windows 實機：通知（`--notify-test`）、開機自啟、既有 OpenRouter key、剪貼簿與開網址、重複啟動

## Comments

2026-11-25：在 Mac 以 `x86_64-pc-windows-msvc` 目標跑 fmt 與 clippy（`-D warnings`）通過，資源編譯器以空殼代替、未連結。共用單元測試 48 個以 stub 平台在 Mac 執行通過；DPAPI 與托盤繪圖共 4 個（其中 1 個 ignored）只能在 Windows 執行。
