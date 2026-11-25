# 12 macOS 的 agy CLI

Type: task
Status: resolved
Blocked by: 06

- [x] 從 Finder 啟動的 App 沒有 shell 的 PATH，需另外檢查 `~/.local/bin`、`/opt/homebrew/bin`、`/usr/local/bin`
- [x] 子行程放進獨立 process group，結束或逾時後確認沒有殘留
- [ ] 確認 `BROWSER` 在 macOS 能擋住登入視窗；不能就另找方法
- [x] 以 agy 實測

## Comments

2026-11-25：開發時從終端機啟動會經由 PATH 找到 agy 並成功取得用量（約 3.6 秒，無殘留行程）；從 Finder 啟動尚找不到，待本工單處理。

2026-11-25：乾淨環境變數下成功取得用量（約 3.4 秒），沒有殘留行程。`BROWSER` 改指向 `/usr/bin/true`；未登入時是否會跳出登入視窗沒有實測（需登出帳號）。
