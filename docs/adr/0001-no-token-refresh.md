# 不自行 refresh CLI 的 OAuth token

Claude 的 access token 約 8 小時過期，AIBar 仍刻意不使用 refresh token 自行換發，只在每次刷新時重讀 CLI 的憑證檔，過期時保留快照並提示使用者執行一次 CLI。原因是若 OAuth 伺服器採 refresh token rotation，AIBar 換發後 CLI 手上的 refresh token 會失效、導致使用者被登出；代價是長時間未開 CLI 時看到的是舊快照。

## Considered Options

- 自行 refresh、只存記憶體不寫檔：可能讓 CLI 登出，違反「不破壞 CLI 登入狀態」原則。
- 過期時在背景執行 CLI 觸發 refresh：需找不耗額度又會 refresh 的指令，CLI 改版即失效。
