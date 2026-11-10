# AIBar

在 Windows 系統匣顯示各 AI 訂閱服務的額度用量與重置時間，讓使用者不必開網頁或 CLI 就知道還剩多少額度。

## Language

**Provider**：
一個提供額度的 AI 服務（Claude、Codex、Antigravity、Command Code、OpenRouter），是用量資料的來源單位。
_Avoid_: 服務、平台、帳號

**啟用**：
使用者在介面上打開某個 Provider 的狀態；只有啟用的 Provider 會顯示卡片、讀取憑證並連線，停用的完全不碰。
_Avoid_: 顯示、勾選、開啟

**方案（Plan）**：
使用者在某個 Provider 訂閱的等級，例如 Claude 的 Max 5x、Codex 的 Plus。
_Avoid_: 訂閱、等級、tier

**額度視窗（Usage Window）**：
Provider 的一段計量期間與其用量百分比，例如「5 小時」「每週」；一個 Provider 可有多個。
_Avoid_: 限額、配額、limit

**剩餘**：
額度視窗尚未使用的百分比（100 減已使用）。介面預設以剩餘呈現，可在設定改為已使用。
_Avoid_: 餘額、可用量

**進度（Pace）**：
額度視窗的已使用比例與「已經過時間占視窗長度的比例」相比，用來判斷照目前速度能否撐到重置。
_Avoid_: 速率、燃燒率

**焦點 Provider**：
托盤圖示目前代表的 Provider：主視窗在總覽時為排序第一的啟用 Provider，在某個 Provider 頁籤時為該 Provider。
_Avoid_: 目前 Provider、主要 Provider

**金額型額度**：
以美元計的額度（本期已用與上限），例如 Command Code 的月額度、OpenRouter 的儲值餘額；與額度視窗並存顯示。
_Avoid_: 點數、餘額（「餘額」只用於 OpenRouter 的儲值）

**保留重置**：
服務發給使用者、可先存起來、需要時再手動套用的額度重置。Codex 在 CLI 輸入 `/usage` 套用；Claude 在 claude.ai 或桌面版「設定 → 用量」按 Reset for free 套用。每一次各有自己的到期時間；分成「已存」的次數與此刻「可套用」的次數（Codex 沒撞到上限時通常無法套用）。
_Avoid_: 重置點數、reset credits

**快照（Snapshot）**：
某個 Provider 最後一次**成功**取得的用量資料，連同取得時間。抓取失敗不會覆蓋快照。
_Avoid_: 資料、結果

**舊快照**：
取得時間已超過該 Provider 3 個背景刷新間隔的快照；仍會顯示，但不再視為可信的現況。
_Avoid_: 過期資料、Stale（「過期」保留給 token）

**已重置**：
額度視窗的重置時間已過，快照上的用量不再成立，視為 0%。
_Avoid_: 歸零、清空

**刷新（Refresh）**：
向 Provider 重新抓取用量的一次嘗試；成功時產生新的快照，失敗時只留下失敗原因。
_Avoid_: 更新、同步、輪詢

**失敗原因**：
最近一次刷新沒有成功的理由（如 token 過期、網路逾時、回應格式改變），與快照並存顯示。
_Avoid_: 錯誤狀態
