# 11 開機自啟與通知

Type: task
Status: ready-for-human（等實際操作）
Blocked by: 10

- [ ] `SMAppService.mainApp`：狀態以系統為準，使用者在「系統設定」停用後 AIBar 顯示為關閉
- [x] 啟動時不自動重新註冊（Windows 版會更新 exe 路徑，macOS 不需要）
- [x] `UNUserNotificationCenter`：不在 `.app` 內時直接略過；第一次需要通知時才請求授權
- [ ] `--notify-test` 可用；80%、95% 各一次且重開不重複

## Comments

2026-11-25：已實作。未打包執行不會當掉；已打包的 `--notify-test` 等待授權 60 秒逾時，原因未明（可能無人回應提示，或命令列直接執行時不跳出）。登入項目未實際註冊過。
