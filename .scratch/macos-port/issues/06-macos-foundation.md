# 06 macOS 可編譯與基礎功能

Type: task
Status: resolved（CI 待 push 後確認）
Blocked by: 04, 05

- [x] 資料夾 `~/Library/Application Support/AIBar`，權限 0700；家目錄用 `HOME`
- [x] 單一執行個體用檔案鎖；剪貼簿用 `pbcopy`；開網址用 `/usr/bin/open`（仍只接受 https）
- [x] 未完成的功能回報「不支援」而不是 panic
- [x] `cargo test`、`cargo clippy -- -D warnings` 在 macOS 通過；`--probe codex`、`--probe commandcode` 取得正確資料
- [x] CI 加入 macOS job（與 Windows 並行）

## Comments
