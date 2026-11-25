# 10 打包 .app 與簽章

Type: task
Status: resolved（通知授權留待工單 11 驗證）
Blocked by: 07

- [x] 新增 `justfile`：`just bundle-macos` 產生 `dist/AIBar.app`（`Info.plist` 含 `LSUIElement`、`LSMinimumSystemVersion` 13.0、版本取自 `Cargo.toml`）
- [x] `.icns` 由 `icon_art` 產生，不另存圖檔；`dist/` 加入 `.gitignore`
- [x] 簽章身分由環境變數指定，不寫進 repo；`codesign --verify --strict` 通過
- [ ] 重新建置並簽章後，Keychain 與通知授權不需重新同意

## Comments

2026-11-25：簽章成功，待命 23 MB、閒置 30 秒 CPU 約 20 ms，執行檔 7.7 MB。
