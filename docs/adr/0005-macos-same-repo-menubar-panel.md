# macOS 與 Windows 同一個 repo；macOS 採選單列彈出面板

AIBar 加入 macOS 版。Provider、排程、快照與介面內容與平台無關，而用量端點多半非公開、隨時可能改版，因此不另開專案：系統 API 集中在 `src/platform/{windows,macos}/`，兩邊提供同名的函式與型別，其餘模組不直接呼叫系統 API，也不拆 Cargo workspace。

macOS 的主畫面是貼著選單列圖示的彈出面板，內容沿用同一個 `MainWindow`。ADR-0002 否決彈出面板的原因（拿不到圖示座標、要搶前景焦點、點圖示收起後又立刻重開）都來自 Windows 的托盤與 Slint 的封裝，在 macOS 用原生 `NSStatusItem` 都有公開的做法，因此 ADR-0002 只適用於 Windows。

macOS 不使用 Slint 的 `SystemTrayIcon`：它以 target/action 處理點擊，掛上選單後就收不到左鍵，也拿不到圖示座標；macOS 27 起官方要求改用 `NSStatusItemExpandedInterfaceDelegate`。代價是多一份 AppKit 程式碼，且 macOS 26 以前要退回 target/action。

## Considered Options

- 另開 macOS 專案：平台程式最單純，但端點改版要修兩次，介面與排程會逐漸分岔。
- SwiftUI 重寫介面：最貼近系統，但介面要維護兩份，且與 CodexBar 高度重疊。
- macOS 也用一般視窗：共用最多，但不符合選單列工具的使用習慣。
- 同 repo、共用介面、原生選單列圖示（採用）。
