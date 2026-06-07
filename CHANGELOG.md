# Changelog

All notable changes to Wisspa are documented here. The format is loosely based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html) ahead of the v1.0 cut.

## [0.3.0] — 2026-06-04

### Added
- **File logger with rotation and a Settings → Privacy toggle** (#39). Logs go to `~/Library/Logs/Wisspa/wisspa.log`, rotate at the configured size, and run at info level by default. Verbose mode (opt-in) captures redacted transcript metadata for support diagnostics.
- **Export Diagnostics** button in the About tab (#39). Bundles the most recent redacted log entries plus app version, permission state, and hotkey config into a single `.zip` the user can attach to a bug report.
- Default actions are now seeded into packaged DMG installs (#35). First-launch on a fresh install now ships all 14 voice commands instead of an empty Actions tab.

### Changed
- **Prompt Mode treats selected text as untrusted** (#36). Selected text from a focused app is now wrapped in a `<selected_text_untrusted>` delimiter block in the Sonnet user message. The system prompt's Step 0.5 instruction was extended to cover this block alongside `<browser_context_untrusted>`. Defends against prompt-injection vectors in selected webpage / email / Slack text.
- **Recording session id threaded through the pipeline** (#37). Press-time session id flows through `process_audio`, mode runners, and `inject_text`. Cancel (Esc) now aborts the in-flight backend pipeline rather than only suppressing UI updates; overlapping recordings no longer race the clipboard restore.
- **Clipboard flavours preserved across dictation** (#38). Images, files, and other non-text clipboard content now survive a dictation paste. Replaces the previous text-only snapshot/restore that silently destroyed non-text content.
- **Logging redaction policy** (#39). Full transcripts, LLM outputs, and shell-command resolution values are no longer logged at info level. Length + content hash only. Verbose logging requires explicit opt-in.

### Fixed
- Production binaries now produce a log artefact. Previously `env_logger` wrote to stderr, which macOS Launch Services redirects to `/dev/null` — so a user reporting a bug had no diagnostic to send (#33, #39).

### Security
- Selected text is no longer treated as trusted LLM instructions (#30 / #36). Combined with the browser-context hardening from #28, all data inputs flowing into the Sonnet rewrite are now delimited and explicitly marked as content.

## [0.2.0] — 2026-05-26

### Added
- **Prompt Mode reads active browser tab to branch on destination** (#28). The press-time snapshot now captures the active tab URL and title for known browsers (Chrome family, Safari) in addition to the app name. Sonnet's system prompt branches on AI-tool URLs (claude.ai, chatgpt.com, etc.) producing a structured prompt vs non-AI URLs (gmail.com, linkedin.com, notion.so, etc.) producing the finished content directly. Falls back to the existing behaviour when Automation for the browser is denied or no window is open. The browser context is wrapped in a `<browser_context_untrusted>` block in the user message; URLs are sanitised to scheme+host before being passed to the model.
- **Quiet notifications toggle** in General settings (#27). Suppresses the success toast that fires after a recording is pasted.

### Changed
- **Pill marked as LSUIElement so it stops disappearing** (#29). The app's process is now an agent app (no Dock icon, no menu bar focus). Eliminates the pill-disappears-when-another-app-takes-focus edge case.
- **Paste lands in the target app, pill follows the cursor** (#26). `activate_app` switched from `tell application "X" to activate` (per-app Apple Events + silently skipped TCC prompts) to `tell application "System Events" to tell process "X" to set frontmost to true` (single existing grant, works for any visible process). The pill now anchors to the cursor's display.

## [0.1.0] — 2026-05-14

Initial public preview. Three-mode voice tool (Dictation, Action, Prompt) with a system-wide hotkey, Whisper STT via Groq, and Anthropic LLM for cleanup + prompt rewriting.

[0.3.0]: https://github.com/TechGuiderBB/wisspa/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/TechGuiderBB/wisspa/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/TechGuiderBB/wisspa/releases/tag/v0.1.0
