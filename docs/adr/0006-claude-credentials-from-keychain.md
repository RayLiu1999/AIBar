# macOS 以 security 指令唯讀 Keychain 中的 Claude 憑證

macOS 的 Claude Code 把登入憑證存在 Keychain（`Claude Code-credentials`），沒有 `.credentials.json`。AIBar 先找檔案，沒有時以系統的 `/usr/bin/security` 讀取該項目，延續 ADR-0001：只讀、不換發、不寫入。同一項目還存有其他服務的 token，只取用 `claudeAiOauth`。

透過 `security` 而不直接呼叫 Security framework，是因為 Keychain 的授權綁定讀取者的簽章：直接讀取時，每次更新或重新簽章 AIBar 都可能再次跳出授權視窗。代價是依賴外部指令的結束碼，而且無法在授權視窗中顯示 AIBar 的名稱。

讀取被拒、Keychain 鎖定或 60 秒內沒有回應時，視為「存取被拒」並停止自動重試，只有手動刷新或停用後重新啟用才再讀，避免背景刷新反覆跳出授權視窗。設定了 `CLAUDE_CONFIG_DIR` 時，Keychain 的項目可能屬於別的帳號，因此只讀該目錄的檔案。

## Considered Options

- 直接呼叫 Security framework：不必啟動外部程式，但授權綁在 AIBar 的簽章上。
- 要求使用者自行貼上 token：token 約 8 小時過期，不可行。
- 以 `security` 唯讀（採用）。
