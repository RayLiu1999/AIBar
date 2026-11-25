# 04 純重構：視窗與托盤

Type: task
Status: ready-for-human（等 Windows CI 與實機確認）
Blocked by: 03

對象：`main_window.rs`（`work_area`、`initial_position`、`on_some_monitor`、`bring_to_front`）、`tray_icon.rs`（`tray_icon_size`、`text_mask`、`apply_app_icon`）。

- [x] `POINT`／`RECT`／`HWND` 不出現在 `platform/` 以外
- [x] `tray_icon.rs` 整檔原樣搬移（只有檔案位置改變），未在 Windows 執行 `dump_icons`
- [ ] Windows 實機：多螢幕、縮放 100%／150%、拔除螢幕後置中、並排切換

## Comments

2026-11-25：面板決定後，`main_window.rs`、`tray_icon.rs` 整檔歸 Windows，不抽共用介面；新增 `TrayHost`、`after_show`。Windows 目標的 fmt、clippy 通過。
