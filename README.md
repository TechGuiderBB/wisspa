# Wisspa

> System-wide AI voice tool for macOS. Hold a hotkey, speak, and Wisspa types — cleaned, structured, and aware of the app you're in.

**Status:** v0.3.0 — all seven phases of the PRD implemented. macOS 13+ on Apple Silicon.

---

## What it does

Three modes, three hotkeys:

| Mode | Default hotkey | What happens |
|---|---|---|
| **Dictation** | `Cmd+Shift+Space` | Speech → Groq Whisper → Claude Haiku cleanup (filler-word removal, punctuation, app-aware tone) → pasted into the focused field |
| **Action** | `Cmd+Shift+A` | Speech → match against the editable YAML action registry → run the matched action (shell / AppleScript / open URL / open app / keystroke) |
| **Prompt** | `Cmd+Shift+P` | Speech (+ optional selected text) → Claude Sonnet rewrites into a structured AI prompt formatted for the focused AI tool (Claude / ChatGPT / Cursor / Gemini) → pasted |
| **Cancel** | `Esc` | Aborts the current recording, no API call |

A small "Wisspa" pill lives at the top-center of the monitor your cursor is on; it flashes red while you're recording.

---

## Quick start

### Prerequisites

- macOS 13+ on Apple Silicon
- [Rust](https://rustup.rs) toolchain
- Node 20+ and [pnpm](https://pnpm.io)
- Xcode Command Line Tools (`xcode-select --install`)
- A Groq API key ([console.groq.com/keys](https://console.groq.com/keys))
- An Anthropic API key ([console.anthropic.com](https://console.anthropic.com/settings/keys))

### Run from source

```bash
git clone https://github.com/TechGuiderBB/wisspa.git
cd wisspa
pnpm install
pnpm tauri dev
```

On first launch the onboarding wizard walks you through:

1. Microphone permission
2. Accessibility permission (for synthetic `Cmd+V`)
3. Screen Recording (optional, for screenshot actions)
4. Automation / System Events
5. API key entry (saved to macOS Keychain)
6. Hotkey overview
7. Test dictation

### Build a production `.app`

```bash
pnpm tauri build
```

Output: `src-tauri/target/release/bundle/dmg/Wisspa_0.3.0_aarch64.dmg`

Drag `Wisspa.app` into `/Applications/`. If Gatekeeper complains the first time, run:

```bash
xattr -d com.apple.quarantine /Applications/Wisspa.app
```

---

## How action mode works

Actions live as YAML files in:

```
~/Library/Application Support/com.techguider.wisspa/actions/
```

Wisspa hot-reloads on any change — add, edit, or remove files there and the new registry is live immediately. Schema:

```yaml
id: screenshot
name: "Interactive Screenshot to Clipboard"
description: "..."
triggers:
  - "screenshot"
  - "take a screenshot"
type: shell            # shell | applescript | open_url | open_app | keystroke
command: "screencapture -i -c"
requires_permissions:
  - screen_recording
destructive: false
success_feedback: "Screenshot captured"
failure_feedback: "Screenshot failed"
enabled: true
```

Placeholders supported in `command`:

- `{query}` — transcript after the trigger phrase
- `{clipboard}` — current clipboard text
- `{selected_text}` — current selection (via clipboard)
- `{active_app}` — frontmost app name

Matching is two-pass: exact phrase (longest trigger wins) then fuzzy (Levenshtein ≤ 3). Shell commands are validated on load — `sudo`, `rm -rf`, `dd`, piped curl/wget, etc. are rejected.

Ships with 14 default actions: `screenshot`, `screenshot_to_file`, `start_screen_recording`, `copy_selection`, `paste`, `new_note`, `open_app`, `search_google`, `search_github`, `search_youtube`, `clear_clipboard`, `lock_screen`, `show_desktop`, `mute_audio`.

---

## How prompt mode works

When you hold the prompt hotkey:

1. Speech → Groq Whisper transcript.
2. Detect the focused AI app (or use your manual override from Settings).
3. (Optional) Capture currently-selected text via clipboard save → simulated `Cmd+C` → read → restore.
4. Claude Sonnet 4.6 rewrites the transcript into a structured prompt formatted for that AI tool:
   - **Claude / Claude Code** → XML tags (`<context>`, `<task>`, `<constraints>`, `<output_format>`)
   - **ChatGPT** → Markdown headings
   - **Cursor / VS Code** → terse, inline-friendly
   - **Gemini** → Markdown + numbered steps
   - **Unknown** → generic Markdown
5. Preview toast counts down (configurable in Settings), then the rewritten prompt is pasted.

---

## Settings

Click the menu-bar microphone icon → **Open Settings…** Seven tabs:

- **General** — launch on login, overlay visibility, recording mode (press-and-hold / toggle), sound, theme.
- **API Keys** — Groq + Anthropic, stored in macOS Keychain. Test buttons hit each provider's `/models` endpoint.
- **Hotkeys** — click a row, press your desired combo. Single keys like `F18` / `F19` work. Reset to defaults available.
- **Actions** — registry overview and link to the YAML folder.
- **Prompt Mode** — include selected text, show preview, preview timeout, manual app override.
- **History** — the last 100 dictations / actions / prompts, exportable as CSV, clearable.
- **About** — live permissions diagnostic with deep links into System Settings.

---

## Architecture

```
Tauri 2 app
├── Rust backend
│   ├── hotkeys.rs     · tauri-plugin-global-shortcut, live reassignment
│   ├── stt.rs         · Groq Whisper multipart upload
│   ├── llm.rs         · Anthropic Messages API (Haiku + Sonnet)
│   ├── injector.rs    · clipboard write + AppleScript Cmd+V
│   ├── app_detector   · AppleScript `System Events` frontmost app
│   ├── selection.rs   · Cmd+C trick + clipboard save/restore
│   ├── actions/       · YAML registry, file watcher, matcher, executor
│   ├── modes/         · dictation, action, prompt pipelines
│   ├── permissions.rs · AX trust, screen-rec preflight, automation probe
│   ├── keychain.rs    · macOS Keychain via the `keyring` crate
│   ├── settings_store · JSON-on-disk per §6.1
│   ├── history.rs     · SQLite via `rusqlite` per §6.2
│   └── tray.rs        · menu-bar icon + menu
└── React frontend (Vite)
    ├── App.tsx                       · hash router → runtime / overlay / settings / onboarding
    ├── components/RecordingOverlay   · flashing-red Wisspa pill (top-center)
    ├── components/settings/*Tab      · seven settings tabs
    ├── pages/Settings.tsx            · sidebar nav + tab host
    ├── pages/Onboarding.tsx          · first-launch 8-step wizard
    └── lib/                          · audio (MediaRecorder), Tauri bindings, types
```

Three windows always exist:

- **main** (runtime, 220×56, always-on-top, all-spaces) — hosts the WebView for MediaRecorder and renders the idle pill. WKWebView throttles JS in hidden windows, hence the visible-but-unobtrusive pill.
- **overlay** (220×56, always-on-top, all-spaces, hidden until recording) — stacks on top of `main` to show the recording state.
- **settings** / **onboarding** — opened on demand.

---

## Troubleshooting

**Nothing pastes after dictating.**
Accessibility permission. Settings → About → Accessibility row should say *Granted*. If not, click *Open settings* and add Wisspa to the list.

**Keychain keeps asking for the password.**
Every Rust rebuild creates a fresh unsigned binary that macOS treats as a new app. In dev, Wisspa reads keys from `.env` first and only consults Keychain if env is empty — so put your keys in `.env` while iterating. Production builds (signed) prompt once for *Always Allow*.

**Hotkey reassignment doesn't seem to register.**
The Hotkeys tab temporarily unregisters all global shortcuts during capture so the webview can receive the key event. If you cancel without entering a combo, press `Esc` while capturing to clean up.

**Prompt mode "selected text" capture misses.**
Some apps (Slack, Notion, certain browser tabs) take longer than the default 280 ms to write to the clipboard after a synthetic `Cmd+C`. Reliable for native apps and most editors; flaky for some web targets.

**The Wisspa pill is on the wrong monitor.**
It anchors to the primary monitor. Set your preferred display as primary in System Settings → Displays → Arrange.

---

## Logs

Dev: stdout of `pnpm tauri dev`.

Production: `Console.app` → filter by `wisspa`. Most log lines are `INFO`-level.

---

## License

Wisspa is open source under the [MIT License](LICENSE). Copyright © 2026 TechGuider.

---

## Acceptance criteria (PRD §11) status

- [x] Hotkey-driven dictation pastes cleaned text into any focused field.
- [x] Action hotkey + "screenshot" triggers the interactive screenshot tool.
- [x] Action hotkey + "search Google for …" opens Google with the query.
- [x] Prompt hotkey produces a structured prompt formatted for the active AI tool.
- [x] Cancel hotkey aborts a recording without an API call.
- [x] All 7 settings tabs render and persist changes to `settings.json`.
- [x] API keys saved to Keychain (env override in dev).
- [x] Hotkey reassignment applies live without restart.
- [x] Test buttons verify Groq + Anthropic keys.
- [x] 14 default actions ship in `default-actions/` and seed on first launch.
- [x] YAML edits in the actions dir hot-reload without restart.
- [x] Invalid YAML logs a warning and is skipped.
- [x] Shell allowlist rejects `sudo`, `rm -rf`, `dd`, piped curl/wget, etc.
- [x] First-launch wizard walks through permissions + keys + hotkey overview + test dictation.
- [x] Permissions diagnostic panel in About tab with re-grant deep links.
- [x] Network failure during STT → toast, no crash.
- [x] Empty audio → no API call, silent dismiss.
- [x] Permission denied → clear error path.
- [x] History stored in SQLite, exportable, clearable.

Open / not gated for v1:

- [ ] Performance budget formally measured (median ≤ 800 ms hotkey release → text inserted).
- [ ] Revoked-permission banner in the main window (diagnostic panel covers it indirectly).
- [ ] Custom menu-bar template icon (current icon is the bundle icon).
- [ ] Edit-before-insert UI for prompt mode (currently shows a preview toast then injects).
- [ ] In-GUI action editor (the registry hot-reloads; edit YAML files directly for now).
