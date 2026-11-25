# 08 macOS 的 Claude 憑證

Type: task
Status: ready-for-human（等與 Claude Code `/usage` 對照、token 換發後觀察）
Blocked by: 02, 06

- [x] 先讀 `.credentials.json`，沒有再讀 Keychain；`credentials_present` 只查中繼資料，不讀內容
- [x] 子行程有逾時；token 只在記憶體，不進參數、日誌、檔案
- [x] 新增失敗原因「Keychain 存取被拒」：不再自動詢問，只有手動刷新才重試；同步 `CONTEXT.md` 與失敗原因文字
- [x] 授權邏輯可用假的讀取器做單元測試
- [ ] 結束同類工具後實測，數字與 Claude Code `/usage` 一致

## Comments

2026-11-25：查詢成功（HTTP 200、方案與 5 小時／每週額度），尚未逐項與 `/usage` 對照。設定 `CLAUDE_CONFIG_DIR` 時只讀檔案。
