# AIBar

**繁體中文** | [English](README.en.md)

[![CI](https://github.com/RayLiu1999/AIBar/actions/workflows/ci.yml/badge.svg)](https://github.com/RayLiu1999/AIBar/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/RayLiu1999/AIBar)](https://github.com/RayLiu1999/AIBar/releases/latest)

Windows 系統匣的 AI 訂閱用量小工具。不用開網頁或 CLI，看一眼托盤就知道 Claude、Codex 等服務的額度還剩多少、何時重置。

- 托盤圖示直接寫著「還剩幾 %」，外框顏色代表是哪一家
- 主視窗以頁籤列出各家的額度視窗、進度條、重置倒數，以及「照目前速度能不能撐到重置」
- 免輸入密碼：借用各 CLI 已登入的憑證，只讀不寫
- 單一 exe（約 8 MB），免安裝 runtime；視窗隱藏時記憶體約 30 MB、閒置 CPU 為 0

靈感來自 macOS 的 [CodexBar](https://github.com/steipete/CodexBar)，範圍刻意縮小，專為 Windows 打造。

## 支援的服務

| 服務 | 顯示的額度 | 資料來源 |
| --- | --- | --- |
| Claude（訂閱） | 5 小時、每週（含指定模型），與保留重置次數及到期時間 | Claude Code 的登入憑證 `~/.claude/.credentials.json` |
| Codex（訂閱） | 5 小時、每週，與保留重置次數及到期時間 | Codex CLI 的登入憑證 `~/.codex/auth.json` |
| Antigravity | 各模型組的 5 小時、每週 | 執行中的 Antigravity App（本機 language server，不連外網）；App 沒開時改用已登入的 agy CLI |
| Command Code | 5 小時、每週、每月，與本期花費 | Command Code CLI 的登入憑證 `~/.commandcode/auth.json` |
| OpenRouter | Key 的花費上限、儲值餘額 | 在設定視窗貼上的 API key（以 Windows DPAPI 加密保存） |

只要對應的 CLI 登入過一次，第一次啟動時就會自動啟用該服務。Token 過期時 AIBar **不會**自行換發（避免把你的 CLI 登出），只會提示你在終端機執行一次對應的 CLI。

## 使用方式

- **托盤圖示**：數字是「焦點服務」剩最少的那個額度還剩幾 %。主視窗停在「總覽」時，焦點是排序第一的服務；切到某個服務的頁籤，托盤就換成那一家。剩不到 30% 底色轉黃、不到 10% 轉紅；資料太久沒更新會轉灰；出錯時右下角加黃點。滑鼠停在圖示上可看到各家摘要。
- **左鍵**：開關主視窗。視窗可拖動、縮放，會記住位置；按 X 或 Esc 只會隱藏回托盤。
- **右鍵**：立即刷新、設定、關於、結束。
- **進度刻度**：進度條上的白色刻度標出「依時間比例此刻應該剩多少」，下方寫「比預期少用 32% · 可撐到重置」或「約 1 天 4 小時後用完」。
- **設定**：開機自啟、顯示剩餘或已使用、各服務的啟用開關與排序、OpenRouter API key、額度通知。
- **通知**：任一額度已用超過 80% 與 95% 時各發一則 Windows 通知，重置後才會再通知。

刷新頻率：背景每 10 分鐘；打開視窗時資料超過 5 分鐘會自動刷新，也可以手動刷新。Claude 的用量端點限流以帳號計，Mac 上的 CodexBar 或 Claude Code 的 `/usage` 也會算進去，不宜更頻繁。

## 安裝

### 下載（建議）

1. 到 [Releases](https://github.com/RayLiu1999/AIBar/releases/latest) 下載 `AIBar-<版本>-windows-x64.exe`（或內含 README、LICENSE 的 zip）。
2. 放到固定位置，例如 `%LOCALAPPDATA%\Programs\AIBar\`，然後直接執行，不需要安裝任何 runtime（支援 Windows 10／11 x64）。
3. 第一次執行時 Windows SmartScreen 可能顯示「Windows 已保護您的電腦」，因為 exe 沒有程式碼簽章；按「其他資訊」→「仍要執行」即可。
4. 在設定中打開「開機時自動啟動」即可常駐。開機自啟記的是 exe 的路徑，所以之後別再搬動它；更新時直接用新版覆蓋同一個檔案（先從托盤右鍵「結束」）。

可用 Release 附的 `SHA256SUMS.txt` 核對下載的檔案。

### 自行建置

1. 安裝 [Rust](https://rustup.rs/)（stable，目標 `x86_64-pc-windows-msvc`）。
2. 安裝 Visual Studio Build Tools（或 Visual Studio），勾選「使用 C++ 的桌面開發」，內含 Windows SDK。
3. 建置：

   ```powershell
   git clone https://github.com/RayLiu1999/AIBar.git
   cd AIBar
   cargo build --release
   ```

4. `target\release\aibar.exe` 就是成品，之後同上面「下載」的步驟 2–4。

> 托盤圖示預設可能收在工作列的「^」裡，可以拖到工作列上常駐顯示。

## 資料與隱私

- AIBar 只**讀取** CLI 的憑證檔，絕不修改；token 只存在記憶體中。
- 自己寫入的檔案都在 `%APPDATA%\AiBar\`：

  | 檔案 | 內容 |
  | --- | --- |
  | `config.json` | 啟用與排序、顯示方式、視窗位置等設定 |
  | `snapshots.json` | 各服務最後一次取得的用量（不含 token 與 email，帳號只存雜湊） |
  | `secrets.bin` | OpenRouter API key，以 DPAPI 加密，只有同一個 Windows 使用者解得開 |
  | `notified.json` | 已發過的通知，避免重複 |
  | `aibar.log` | 日誌（只記狀態碼與耗時，不記 token 或回應內容），超過 1 MB 自動輪替 |

- 網路請求只連向各服務的官方網域；Antigravity 只連本機 `127.0.0.1`。沒有任何分析或回報。

## 疑難排解

| 狀況 | 處理 |
| --- | --- |
| Claude 顯示「限流中」 | 端點以帳號限流，等提示的時間會自動重試。若同時在別台電腦執行 CodexBar 等工具，可拉長它們的刷新間隔 |
| 顯示「Token 已過期」 | 在終端機執行一次對應的 CLI（`claude`、`codex`），下一次刷新就會恢復 |
| Antigravity 顯示「未執行」 | 開啟 Antigravity App，或安裝並登入 agy CLI，就會自動更新；這不算錯誤 |
| 沒有收到通知 | 執行 `aibar.exe --notify-test` 發一則測試通知，並確認 Windows「設定 → 系統 → 通知」中 AIBar 沒有被關閉 |
| 數字和官方不同 | 執行 `aibar.exe --probe claude`（或其他服務 id）檢視端點回應結構（字串值會遮蔽） |

## 開發

- 技術：Rust + [Slint](https://slint.dev/)（UI 與系統托盤）、`ureq`（HTTP）、`windows` crate（Win32 與 WinRT）。
- 規格書：[`docs/spec.md`](docs/spec.md)；名詞定義：[`CONTEXT.md`](CONTEXT.md)；架構決策：[`docs/adr/`](docs/adr/)。
- 測試：`cargo test`；CI（GitHub Actions）另檢查 `cargo fmt --check` 與 `cargo clippy -- -D warnings`。
- 發布：更新 `Cargo.toml` 的 `version` 並 commit 後，推送同版本的標籤（例如 `git tag -a v0.2.0 -m "v0.2.0"`、`git push origin v0.2.0`），Release workflow 會自動建置並附上 exe、zip 與檢查碼。
- 每個服務的端點細節集中在 `src/providers/<服務>.rs`，端點改版時只需修改一個檔案。

## 注意事項

除 OpenRouter 外，AIBar 使用的都是各服務**非公開**的用量端點，可能隨時改版或不被官方支援；請以低頻率、個人用途使用。本專案與 Anthropic、OpenAI、Google、Command Code、OpenRouter 皆無關聯。

## 授權

本專案以 [MIT License](LICENSE) 授權。UI 使用 Slint 的 Royalty-free 授權，設定視窗的「關於」頁依其要求顯示 Slint 標示。
