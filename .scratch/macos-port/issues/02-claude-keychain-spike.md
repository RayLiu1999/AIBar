# 02 讀取 Claude Code 的 Keychain 憑證

Type: prototype
Status: ready-for-human
Blocked by: 無

要回答的問題：AIBar 能否讀到憑證而不反覆打擾使用者？會讀到真實 token，只印欄位名稱，不得印出或寫檔。

- [x] 從終端機執行 `/usr/bin/security find-generic-password -s "Claude Code-credentials" -w`：結束碼 0、約 10 ms、未跳窗
- [x] 從 `.app`（已簽章、乾淨環境變數）執行：成功，10 秒內完成，HTTP 200
- [x] 內容是否為 `{"claudeAiOauth": {...}}`，欄位與 `.credentials.json` 相同（`accessToken`、`expiresAt`、`subscriptionType`、`rateLimitTier`）：是，另有 `organizationUuid` 等；最上層還有 `mcpOAuth`，不可讀取或輸出
- [ ] 設定 `CLAUDE_CONFIG_DIR` 時 service 名稱是否不同；同名項目有多筆時如何選（`-a`）
- [ ] Claude Code 換發 token 後是否要重新授權（至少跨一次約 8 小時的換發）
- [x] 項目不存在：結束碼 44
- [ ] 使用者按「拒絕」、Keychain 鎖定、逾時三種情況的結束碼
- [ ] 對照：以 `security-framework` 直接讀取的授權行為

## Comments
