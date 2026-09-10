# Changelog

All notable changes to Wisspa are documented here. The format is loosely based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html) ahead of the v1.0 cut.

## [0.4.2] — 2026-09-10

### Fixed
- **Capture failures are no longer silent** (#81). A recording that came back with zero bytes was discarded by a bare `return` — no log line, no history row, no on-screen feedback. Eight consecutive dictation failures on 7 September left no evidence anywhere, which is why that incident was never root-caused. Empty captures are now logged with full diagnostics (duration, chunk count, warm-stream flag, mic track state), written to history as a failure, and flashed on the pill. Holds under 600 ms are treated as accidental hotkey brushes and stay console-only, so the new report keeps its signal value.
- **Warm mic streams are now health-checked before reuse** (#81). `startRecording` promoted a pre-warmed stream on `readyState` alone while the cold path also required the track not be muted. A macOS mic track another process has grabbed stays `live` but goes `muted`, and MediaRecorder on a muted track emits no data at all. Both paths now share one check.

## [0.4.1] — 2026-08-06

### Changed
- **Wisspa is fully free and open source.** The planned Supporter License is gone — the License settings tab and all validation plumbing are removed (#78). Signed, notarised builds with automatic updates are free for everyone.
- **Release builds are now Developer-ID signed and Apple-notarised** (and stapled) — first launch opens with no Gatekeeper warning (#77).

### Security
- Rotated the updater signing keypair. **Auto-update from 0.4.0 will fail signature verification** — download 0.4.1 manually once from Releases (or wisspa.app/download). No user data or settings are affected.

## [0.4.0] — 2026-08-03

First open-source release (MIT License). Bundles the public-launch hardening pass and a full reliability/quality program across the voice pipeline.

### Added
- **Command Mode** (#66): select text anywhere, hold `⌘⇧C`, speak an instruction ("make this formal", "translate to French") — the selection is rewritten in place. Works with push-to-talk and toggle recording.
- **Dictation edit-before-insert** (#67): opt-in review window for dictation, reusing Prompt Mode's review gate.
- **Usage metering** (#67): Anthropic input/output tokens per history row (summed across multi-call runs) with a per-row display and a running total in the History tab.
- **Per-app profiles** (#65): per-app tone + vocabulary for dictation (new Profiles settings tab). Profile vocab takes precedence over global entries and feeds the STT hint first.
- **Wider browser context** (#65): Dia added to the scriptable-tab list; Firefox/Zen/Orion get title-only routing context.
- **Prompt personalisation** (#64): optional free-text user profile injected as standing preferences into Prompt Mode; **adaptive two-pass refinement** (complex transcripts get a critique-and-revise second pass; toggleable, on by default).
- **Eval harness** (#62): 30 property-scored fixtures (routing, formatting, degenerate input, injection attempts) run against the real rewrite path via `cargo test -- --ignored` (see `eval/README.md`).
- **Mic input device selection** and **automatic update check on launch** (#63; toast only when an update exists, respects quiet notifications).
- **Toggle recording mode** (#59): press to start, press again to stop. `show_overlay` now honoured.
- **STT model + language selection, max recording length slider** (#59): turbo vs full `whisper-large-v3` vs English-only distil; auto-detect or pinned language; 10–120 s cap.
- **History search, mode filter, re-inject, and 1000-row pruning** (#52).
- **Recording overlay feedback** (#53): live input level meter, processing elapsed timer, esc-to-cancel hint, wider route chip.
- **Pipeline resilience** (#55): one retry on transient Groq/Anthropic failures (429 honours Retry-After, capped), Prompt Mode falls back to the raw transcript instead of losing the utterance, STT failures are recorded in history.
- **Dictation-complete sound toggle** (#50).
- OSS scaffolding: SECURITY.md, CONTRIBUTING.md, issue templates; `cargo test` + frontend typecheck in CI; all Actions pinned to commit SHAs (#56).

### Changed
- **Prompt-rewrite system prompt v2** (#60): insufficient-intent passthrough (noise never becomes a fabricated task), a silent quality bar, few-shot exemplars, and real input documentation. Plus an output preamble guard with warn-level telemetry.
- **Confidence-based silence gating** (#58): Groq `verbose_json` segment confidence (`no_speech_prob` / `avg_logprob`) replaces the phrase denylist that discarded real dictations like "thank you"; fixes the inverted mic-sensitivity multiplier and lowers the byte-rate floor (quiet speakers).
- **Latency** (#57): Anthropic prompt caching on the static system prompts; event-driven clipboard restore (pasteboard changeCount polling) replaces the fixed 250 ms post-paste sleep.
- **Vocabulary hint capped** at ~800 chars to stay inside Whisper's prompt window (#61).
- Removed dead settings (`theme`, unused LLM model fields) and the unused `enigo` dependency (#59, #51).

### Fixed
- Esc no longer plays a cancel sound system-wide when nothing is recording (#51).
- Clipboard is restored even when app activation or Cmd+V dispatch fails mid-injection; selection capture no longer clobbers non-text clipboards (#51).
- Preview countdown no longer stalls invisibly under quiet notifications; history latency metric now includes STT time (#51).
- `{query}` in `open_url` actions is percent-encoded (#54).
- Settings sidebar version is dynamic; updater endpoint points at this repo (#51, launch prep).
- TS Settings mirror gains `word_corrections` (latent reset risk) (#68).

### Security
- Voice transcripts are now delimited as untrusted input in Prompt Mode (`<transcript_untrusted>`), and the dictation system-prompt app name is sanitised — closing the last two prompt-injection gaps (#54).
- Real CSP replaces `"csp": null`; devtools capability removed from production (#54).
- History rewritten before publication to remove internal planning docs and personal data; gitleaks (full history) + cargo audit + semgrep all enforced green in CI.

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

[0.4.0]: https://github.com/TechGuiderBB/wisspa/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/TechGuiderBB/wisspa/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/TechGuiderBB/wisspa/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/TechGuiderBB/wisspa/releases/tag/v0.1.0
