# AIBar

[繁體中文](README.md) | **English**

A Windows system-tray widget for AI subscription usage. See how much of your Claude, Codex and other quotas is left, and when they reset, without opening a website or a CLI.

- The tray icon shows the remaining percentage; its border color tells you which service it is
- A tabbed main window lists every usage window with progress bars, reset countdowns, and whether your current pace lasts until the reset
- No passwords: it reuses the credentials your CLIs already have, read-only
- A single exe (about 8 MB), no runtime to install; about 30 MB of memory while hidden and 0 idle CPU

Inspired by [CodexBar](https://github.com/steipete/CodexBar) for macOS, deliberately smaller in scope and built for Windows.

## Supported services

| Service | Quotas shown | Data source |
| --- | --- | --- |
| Claude (subscription) | 5-hour, weekly (including per-model), banked resets and their expiry | Claude Code credentials `~/.claude/.credentials.json` |
| Codex (subscription) | 5-hour, weekly, banked resets and their expiry | Codex CLI credentials `~/.codex/auth.json` |
| Antigravity | 5-hour and weekly per model group | The running Antigravity app (local language server, no internet access) |
| Command Code | 5-hour, weekly, monthly, and spend this period | Command Code CLI credentials `~/.commandcode/auth.json` |
| OpenRouter | Key spending limit, credit balance | An API key pasted in Settings (stored encrypted with Windows DPAPI) |

Any service whose CLI you have logged in to is enabled automatically on first launch. When a token expires, AIBar does **not** refresh it itself (that could log your CLI out); it asks you to run the CLI once in a terminal.

## Usage

- **Tray icon**: the number is the remaining percentage of the tightest quota of the *focused service*. On the Overview tab the focused service is the first one in your order; switching to a service's tab makes the tray follow it. Below 30% the tile turns yellow, below 10% red; stale data turns it gray; an error adds a yellow dot. Hover for a summary of every service.
- **Left click**: show or hide the main window. It can be moved and resized and remembers its position; X or Esc only hides it to the tray.
- **Right click**: Refresh now, Settings, About, Quit.
- **Pace marker**: the white tick on a progress bar marks how much should be left at this point in the window. The line below says, for example, "32% under pace · lasts until reset" or "runs out in about 1 day 4 hours".
- **Settings**: start with Windows, show remaining or used, enable and order services, OpenRouter API key, quota notifications.
- **Notifications**: a Windows notification when any quota passes 80% and 95% used, once each until it resets.

Refresh rate: Claude every 10 minutes (its usage endpoint is rate-limited per account, and CodexBar on a Mac or Claude Code's `/usage` count too), others every 5 minutes. Opening the window refreshes stale data, and you can refresh manually.

> The UI text is currently in Traditional Chinese.

## Installation

You currently need to build it yourself.

1. Install [Rust](https://rustup.rs/) (stable, target `x86_64-pc-windows-msvc`).
2. Install Visual Studio Build Tools (or Visual Studio) with "Desktop development with C++", which includes the Windows SDK.
3. Build:

   ```powershell
   git clone <repo-url> AIBar
   cd AIBar
   cargo build --release
   ```

4. Copy `target\release\aibar.exe` somewhere permanent (for example `%LOCALAPPDATA%\Programs\AIBar\`) and run it. Turn on "Start with Windows" in Settings to keep it running.

> The tray icon may start hidden under the "^" overflow; drag it onto the taskbar to keep it visible.

## Data and privacy

- AIBar only **reads** CLI credential files and never modifies them; tokens are kept in memory only.
- Everything it writes lives in `%APPDATA%\AiBar\`:

  | File | Contents |
  | --- | --- |
  | `config.json` | Enabled services and order, display mode, window position and other settings |
  | `snapshots.json` | The last usage data fetched per service (no tokens or emails; accounts stored as a hash) |
  | `secrets.bin` | The OpenRouter API key, encrypted with DPAPI so only the same Windows user can decrypt it |
  | `notified.json` | Notifications already sent, to avoid repeats |
  | `aibar.log` | Log (status codes and timings only, never tokens or response bodies), rotated at 1 MB |

- Network requests go only to each service's official domain; Antigravity is reached on `127.0.0.1` only. No analytics or telemetry.

## Troubleshooting

| Problem | What to do |
| --- | --- |
| Claude shows "rate limited" | The endpoint is limited per account; AIBar retries at the time shown. If CodexBar or a similar tool runs on another machine, lengthen its refresh interval |
| "Token expired" | Run the matching CLI once in a terminal (`claude`, `codex`); the next refresh recovers |
| Antigravity shows "not running" | Open the Antigravity app and it updates automatically; this is not an error |
| No notifications | Run `aibar.exe --notify-test` to send a test notification, and check that AIBar is allowed under Windows Settings → System → Notifications |
| Numbers differ from the official page | Run `aibar.exe --probe claude` (or another service id) to print the endpoint's response structure with string values masked |

## Development

- Stack: Rust + [Slint](https://slint.dev/) (UI and system tray), `ureq` (HTTP), the `windows` crate (Win32 and WinRT).
- Spec: [`docs/spec.md`](docs/spec.md); glossary: [`CONTEXT.md`](CONTEXT.md); architecture decisions: [`docs/adr/`](docs/adr/) (all in Traditional Chinese).
- Tests: `cargo test`.
- Each service's endpoint details live in `src/providers/<service>.rs`, so an endpoint change touches one file.

## Disclaimer

Except for OpenRouter, AIBar relies on **unofficial** usage endpoints that may change at any time or may not be supported by the providers. Use it for personal purposes at a low polling rate. This project is not affiliated with Anthropic, OpenAI, Google, Command Code or OpenRouter.

## License

Released under the [MIT License](LICENSE). The UI uses Slint under its Royalty-free license; the About page in Settings shows the Slint attribution it requires.
