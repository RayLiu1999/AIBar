# AIBar

Windows 系統匣 AI 用量工具（Rust + Slint）。完整規格見 `docs/spec.md`。

## Agent skills

### Issue tracker

Issues 以本機 Markdown 存於 `.scratch/<feature>/issues/`。See `docs/agents/issue-tracker.md`.

### Triage labels

使用預設五個標籤（needs-triage、needs-info、ready-for-agent、ready-for-human、wontfix），記在 issue 檔的 `Status:` 行。See `docs/agents/triage-labels.md`.

### Domain docs

Single-context：根目錄 `CONTEXT.md` + `docs/adr/`。See `docs/agents/domain.md`.
