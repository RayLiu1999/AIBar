# Command Code 借用 CLI 的 API key，不用手動貼 cookie；DPAPI 改給 OpenRouter

規格原本打算讓使用者從瀏覽器複製 Command Code 的 cookie 貼進 AIBar，再以 DPAPI 加密保存，因為當時以為只有帳務網頁才有用量資料。實作時發現 Command Code CLI 登入後會在 `%USERPROFILE%\.commandcode\auth.json` 存一把 API key，CLI 的 `/usage` 指令就是用這把 key 呼叫 `api.commandcode.ai` 的 `/alpha/billing/credits` 與 `/alpha/billing/subscriptions`。因此改成和 Claude、Codex 一樣借用 CLI 的本機憑證：使用者不必複製 cookie，cookie 過期的問題也不存在。代價是依賴標示為 alpha 的非公開端點。

DPAPI 加密儲存（`secrets.bin`）仍然保留，改用於 OpenRouter：OpenRouter 沒有本機 CLI 憑證可借，只能由使用者在設定視窗貼上 API key。

## Considered Options

- 手動貼 cookie（原規劃）：使用者要開瀏覽器開發者工具複製，cookie 過期就得重貼，體驗差。
- 自動讀取瀏覽器 cookie：Windows 上 Chromium cookie 以 App-Bound Encryption 加密，工程黑洞，規格已排除。
- 借用 CLI 的 API key（採用）：零設定，與其他 Provider 一致；端點改版時集中在 `providers/command_code.rs` 修正。
