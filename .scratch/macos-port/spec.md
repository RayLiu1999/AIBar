# AIBar macOS 版規劃

Status: needs-info（等待工單 01 的實際點擊測試）
建立：2026-11-24；2026-11-25 改為選單列彈出面板，並填入工單 01、02 的實測結果

## 目標

在同一個 repo、同一份程式碼支援 macOS，Windows 版行為不變。
不處理與其他用量工具同時執行造成的限流；實測 Claude 前應先結束同類工具。

**驗收標準**

- Windows 版：CI（fmt、clippy、測試）通過，並在 Windows 實機確認托盤、視窗、通知、開機自啟、OpenRouter key 與現版一致。
- macOS 版：Claude、Codex、Command Code、OpenRouter、Antigravity（agy CLI）的數字與官方一致。
- 背景刷新不會反覆跳出 Keychain 授權視窗；使用者拒絕一次後不再自動詢問。
- 點選單列圖示，面板貼著圖示下方彈出；再點一次、點面板外面或按 Esc 收起。
- 沒有 Dock 圖示；視窗隱藏、閒置時 CPU 接近 0，記憶體實測後記入規格書。

**不做**：SwiftUI 改寫、Widget、Intel Mac、Mac App Store、自動更新、Linux。

## 已查證的事實

| 項目 | 結果 | 依據 |
| --- | --- | --- |
| Slint 托盤支援 macOS | 支援，用 `NSStatusItem` | `i-slint-core` 1.18.1 `items/system_tray/appkit.rs` |
| 左鍵與選單不能並存 | 有 `Menu` 時左鍵只會彈出選單，`clicked` 不觸發；沒有 `Menu` 時左鍵觸發 `clicked`，右鍵沒有反應 | 同上，及 Slint 官方文件 |
| 圖示旁可放文字 | `title` 屬性會顯示在圖示旁 | 同上 |
| 圖示尺寸 | 高度固定 18pt、保留原始像素；不是 template image，會保留顏色 | 同上 |
| Dock 圖示 | winit 0.30.13 未指定 activation policy 時不覆寫，`LSUIElement` 應會生效 | `platform_impl/macos/{event_loop,app_state}.rs` |
| Claude 憑證 | macOS 的 Claude Code 沒有 `.credentials.json`，憑證在 Keychain 的 `Claude Code-credentials`；查中繼資料不會跳授權 | 實測 |
| Claude 帳號識別 | `~/.claude.json` 有 `organizationUuid`、`lastReleaseNotesSeen` | 實測 |
| Codex、Command Code | `auth.json` 的路徑與欄位和 Windows 相同 | 實測（只看欄位名） |
| Antigravity | agy CLI 可用（1.1.11 以上） | 實測 |

## 風險

1. **選單列點擊（待實際點擊確認）。** Apple 開發者論壇 [thread 832823](https://developer.apple.com/forums/thread/832823) 回報 macOS 27
   改變了 `NSStatusItem` 的點擊。27 SDK 的 `NSStatusItem.h` 已提供正式做法 `NSStatusItemExpandedInterfaceDelegate`（27.0 起），
   並註明應取代 target/action 的手動切換。Slint 1.18.1 的托盤仍用 target/action，所以 macOS 不使用 Slint 的托盤。
2. **Keychain 授權視窗。** 讀取一次沒有跳窗；token 換發後是否仍不跳窗待觀察（工單 02）。
3. **簽章。** ad-hoc 簽章沒有穩定的 designated requirement，重新建置後 Keychain 與通知授權可能失效；改用固定憑證簽章。

## 決策

| # | 決策 | 理由 | 備案 |
| --- | --- | --- | --- |
| D1 | 原生 `NSStatusItem`：27 起用 `NSStatusItemExpandedInterfaceDelegate`，13–26 用 target/action | 27 的正式做法；公開 API 就能取得圖示座標 | 點擊測試失敗時改回只用 target/action |
| D2 | 點圖示彈出無邊框面板，失焦、Esc、再次點擊收起。內容沿用 `MainWindow`，以 `popover` 屬性切換 `no-frame` | ADR-0002 否決面板的原因都是 Windows 上的限制（依賴 Slint 內部類別名稱、搶焦點、座標），macOS 沒有這些問題 | — |
| D3 | 選單列用「代表色圖示＋文字（如 59%）」，由 AppKit 顯示文字 | 不必畫字；原型已可顯示 | — |
| D4 | Claude 憑證先讀檔，沒有再以 `/usr/bin/security` 讀 Keychain；只讀、不換發（符合 ADR-0001），且只取 `claudeAiOauth` | 實測不跳窗；同一筆內容還有 `mcpOAuth`，不可碰 | Security framework |
| D5 | OpenRouter key 存 Keychain（`security-framework`，已在 `Cargo.lock`） | 不自製加密 | — |
| D6 | 開機自啟用 `SMAppService`、通知用 `UNUserNotificationCenter`，都只在 `.app` 內啟用 | 系統正式 API；未打包時直接呼叫通知 API 可能當掉 | 自啟改用 LaunchAgent |
| D7 | 最低 macOS 13、只出 arm64；簽章身分由環境變數指定 | `SMAppService` 需 13 以上；公證需 Developer ID | 要公開發布時另開工單 |
| D8 | 不拆 workspace，平台程式放 `src/platform/{windows,macos}/` | 單一執行檔，拆 crate 沒有好處 | — |
| D9 | 面板固定大小、不記位置；總覽只用清單排列，不顯示並排切換。設定仍是一般視窗，「結束」「關於」從面板進入 | 面板貼著圖示，位置與並排加寬沒有意義 | — |

## 目標結構

```
src/
├── platform/
│   ├── mod.rs            // 依 cfg(target_os) 轉出同名函式；不用 trait，兩平台簽章一致即可
│   ├── windows/          // 現有 Win32／WinRT 程式原樣搬入
│   │   └── {system,paths,autostart,secret_store,toast,process,process_tree,main_window,tray,tray_icon}.rs
│   └── macos/            // 同名函式的 macOS 實作，另有 status_item（選單列）與 panel（面板）
├── notify.rs             // 只留門檻判斷與已通知紀錄（共用）
├── secret.rs             // 只留「讀／寫／清除某 Provider 的機密」介面
└── providers/            // 共用；行程列舉、憑證來源、行程樹終止由 platform 提供
```

原則：`platform/` 以外不出現 `windows::`、`objc2` 或 `#[cfg(target_os)]`（憑證路徑等單行差異也收進 `platform`）。

## 階段與工單

| 階段 | 工單 | 分支 | 完成條件 |
| --- | --- | --- | --- |
| 1 驗證 | 01 選單列面板原型、02 Keychain 讀取 | `spike/macos`（不合併） | 結論寫回工單，D1、D4 定案 |
| 2 純重構 | 03 系統功能、04 視窗與托盤、05 行程探測 | `refactor/platform` | Windows CI 通過且實機無差異，先合併 |
| 3 macOS | 06 可編譯與基礎、07 選單列與面板、08 Claude、09 OpenRouter、10 打包、11 自啟與通知、12 agy、13 Antigravity App | `feat/macos` | 各工單驗收條件 |
| 4 收尾 | 14 文件、15 CI 與發布 | `feat/macos` | README、規格書、ADR 與程式一致 |

可用的最小版本是 01–08 加 10：選單列、Claude、Codex、Command Code。
