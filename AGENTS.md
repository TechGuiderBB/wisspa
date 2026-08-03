# AGENTS.md — Wisspa (macOS app)

> Working memory for the Wisspa Tauri app repo. Read this first before making changes in this codebase.
> `CLAUDE.md` points here — there is one canonical guide.

---

## 1. What this repo is

The macOS desktop app. Tauri 2 (Rust backend, React/Vite/TypeScript frontend). Bundle identifier `com.techguider.wisspa`. macOS 13+ on Apple Silicon only. Open source under the MIT License.

System-wide AI voice tool: hold a hotkey, speak, Wisspa does one of three things based on which hotkey was pressed:

| Mode | Default hotkey | What happens |
|---|---|---|
| **Dictation** | `⌘⇧Space` | Speech → Groq Whisper → Claude Haiku cleanup with app-aware tone → pasted into focused field |
| **Action** | `⌘⇧A` | Speech → match against editable YAML action registry → run shell / AppleScript / open URL / open app / keystroke |
| **Prompt** | `⌘⇧P` | Speech (+ optional selected text) → Claude Sonnet rewrites into a structured prompt formatted for the focused AI tool (Claude / ChatGPT / Cursor / Gemini) → pasted |
| **Cancel** | `Esc` | Aborts the current recording, no API call |

The differentiator is **Prompt Mode** — most voice tools solve dictation; Wisspa solves voice-to-good-prompt.

**Status:** v0.3.0 public preview, heading toward v1.0 general release. The app is BYO-key: users supply their own Groq + Anthropic keys (stored in macOS Keychain); the app itself ships with no credentials and talks only to `api.groq.com` and `api.anthropic.com`.

---

## 2. Tech stack

| Layer | Choice | Pinned at |
|---|---|---|
| Desktop framework | Tauri 2 (`macos-private-api`, `tray-icon`) | `src-tauri/Cargo.toml` |
| Backend language | Rust 2021 edition | |
| Frontend | React 18 + Vite 6 + TypeScript 5 | `package.json` |
| Frontend styling | Tailwind CSS 3 (still on `tailwind.config.js`, JS not TS) | |
| Frontend state | Zustand 5 | |
| STT | Groq API, model `whisper-large-v3-turbo` | `src-tauri/src/stt.rs` |
| LLM (cleanup) | Anthropic Messages API, `claude-haiku-4-5-20251001`, `max_tokens: 2048`, `temperature: 0.2` | `src-tauri/src/llm.rs` |
| LLM (prompt mode) | Anthropic Messages API, `claude-sonnet-4-6`, `max_tokens: 4096`, `temperature: 0.4` | `src-tauri/src/llm.rs` |
| Audio capture | Browser `MediaRecorder` (`audio/webm;codecs=opus`), base64 over Tauri `invoke` | `src/lib/audio.ts` |
| Global hotkeys | `tauri-plugin-global-shortcut` | |
| Local storage | JSON via `tauri-plugin-store` for settings; SQLite via `rusqlite` (bundled) for history | |
| Shell execution | `tauri-plugin-shell` with custom allowlist validation | `src-tauri/src/actions/registry.rs` |
| Clipboard | `tauri-plugin-clipboard-manager` | |
| Text injection | AppleScript `osascript -e 'tell application "System Events" to keystroke "v" using command down'` — deliberately **not** `enigo` (see `DECISIONS.md` item 9) | `src-tauri/src/injector.rs` |
| HTTP client | `reqwest` 0.12 with `rustls-tls`, `json`, `multipart` | |
| Async runtime | `tokio` 1 (`full` features) | |
| Keychain | `keyring` 3 with `apple-native` | `src-tauri/src/keychain.rs` |
| YAML parsing | `serde_yaml` 0.9 | `src-tauri/src/actions/registry.rs` |
| Fuzzy matching | `strsim` 0.11 (Levenshtein) | `src-tauri/src/actions/matcher.rs` |
| File watching | `notify` 6 + `notify-debouncer-mini` 0.4 — for action-registry hot-reload | |
| Auto-updater | `tauri-plugin-updater` 2.10.1 (endpoints point at this repo's GitHub Releases) | |
| Launch-on-login | `tauri-plugin-autostart` 2.5.1 | |

---

## 3. Folder structure

```
wisspa/
├── src-tauri/                       # Rust backend
│   ├── Cargo.toml
│   ├── tauri.conf.json              # Window definitions, identifier, bundling, updater
│   ├── Info.plist                   # macOS usage descriptions (Mic / Apple Events / Screen Capture)
│   ├── entitlements.plist
│   ├── capabilities/default.json    # Tauri 2 capability acl
│   ├── icons/                       # All platform icon sizes
│   └── src/
│       ├── main.rs                  # Entry point + window positioning + setup
│       ├── lib.rs
│       ├── commands.rs              # All Tauri commands exposed to the frontend (process_audio lives here)
│       ├── hotkeys.rs               # Global shortcut registration, live reassignment
│       ├── audio.rs                 # Audio bridge to frontend
│       ├── stt.rs                   # Groq Whisper multipart upload + hallucination filter
│       ├── llm.rs                   # Anthropic Messages API (Haiku + Sonnet)
│       ├── injector.rs              # Clipboard write + AppleScript Cmd+V
│       ├── selection.rs             # Read selected text via Cmd+C trick (280 ms wait)
│       ├── app_detector.rs          # Frontmost app via AppleScript + press-time snapshot
│       ├── permissions.rs           # AX trust, screen-rec preflight, automation probe
│       ├── keychain.rs              # API key storage in macOS Keychain
│       ├── settings_store.rs        # Settings JSON on disk
│       ├── history.rs               # SQLite history logger
│       ├── sounds.rs                # Start/stop recording sounds
│       ├── toast.rs                 # Native notification toasts
│       ├── tray.rs                  # Menu-bar icon + menu
│       ├── actions/
│       │   ├── registry.rs          # YAML loader, hot-reload watcher, shell allowlist validation
│       │   ├── matcher.rs           # Exact (longest trigger wins) → fuzzy (Levenshtein ≤ 3)
│       │   └── executor.rs          # Runs actions
│       ├── modes/
│       │   ├── dictation.rs
│       │   ├── action.rs
│       │   └── prompt.rs
│       └── prompts/
│           ├── haiku_cleanup.md     # Loaded via include_str!() into llm.rs
│           └── sonnet_prompt.md     # Loaded via include_str!() into llm.rs
├── src/                             # React frontend
│   ├── App.tsx                      # Hash router → runtime / overlay / settings / onboarding
│   ├── components/
│   │   ├── RecordingOverlay.tsx
│   │   └── settings/                # Settings tabs (General, API Keys, Hotkeys, Actions, Prompt Mode, Vocab, Corrections, History, About)
│   ├── pages/
│   │   ├── Onboarding.tsx           # 8-step first-launch wizard
│   │   └── Settings.tsx
│   ├── lib/
│   │   ├── audio.ts                 # MediaRecorder wrapper
│   │   ├── settings.ts              # Settings client mirroring Rust SettingsStore schema
│   │   └── tauri.ts
│   └── store/
│       └── recording.ts             # Zustand recording state
├── default-actions/                 # 14 default YAML actions shipped with installer
├── .github/workflows/
│   ├── release.yml                  # Build + sign + publish
│   └── security.yml                 # gitleaks + cargo audit + tests
├── DECISIONS.md                     # Implementation choices + reasoning
├── WISSPA_PRD.md                    # Original product PRD (partially out of date; code wins)
├── CONTRIBUTING.md
├── SECURITY.md
└── README.md
```

---

## 4. The runtime windows

Defined in `src-tauri/tauri.conf.json`. Critical to understand because the pill UX depends on the layout.

| Label | Size | Visible at idle | alwaysOnTop | Purpose |
|---|---|---|---|---|
| `main` | 220×56 | Yes (transparent, undecorated, `focus: false`) | No | Hosts the WebView for MediaRecorder and the idle pill. **WKWebView throttles JS in hidden windows — this window MUST stay visible.** See `DECISIONS.md` item 11. |
| `overlay` | 220×56 | No (toggled by hotkey) | Yes | Stacks on top of `main` to show recording state (flashing red dot). |
| `settings` | 820×600 | No (opened on demand) | No | The settings GUI. |
| `onboarding` | 720×560 | No (opened on first launch) | No | First-launch 8-step wizard. |

The pill follows the monitor the cursor is on (see `main.rs`).

---

## 5. The pipeline (every mode)

1. Hotkey **press** in `hotkeys.rs` → emits `wisspa://recording-mode` with mode name + `wisspa://start-recording`. Also calls `app_detector::snapshot_target_app_now()` so the user's *intended* frontmost app is captured at press time.
2. Frontend (`App.tsx`) starts `MediaRecorder` (`audio/webm;codecs=opus` preferred). Overlay window is shown by Rust.
3. Hotkey **release** → frontend stops `MediaRecorder`, runs silence guard, calls Rust `process_audio` (base64 audio + mode).
4. Rust decodes → Groq Whisper STT → Whisper-hallucination filter → routes by mode.
5. **Dictation:** Haiku cleanup with `{ACTIVE_APP_NAME}` in system prompt → inject via clipboard + `Cmd+V`.
6. **Action:** match against YAML registry (exact then fuzzy via `strsim::levenshtein` ≤ 3) → execute via `actions/executor.rs`.
7. **Prompt:** resolve target app (manual override → press-time snapshot → live AppleScript → `"Generic"`) → optionally capture selection → Sonnet rewrite → inject.
8. Result logged to SQLite history and surfaced as a native toast.

---

## 6. Action types

Five action types, declared in YAML, validated on load:

| `type` | What `command` is | Example |
|---|---|---|
| `shell` | A shell command. **Allowlist rejects** `sudo`, `rm -rf`, `dd`, piped `curl`/`wget`. | `screencapture -i -c` |
| `applescript` | An AppleScript string run via `osascript`. | `set volume with output muted` |
| `open_url` | URL template; supports `{query}`. | `https://github.com/search?q={query}` |
| `open_app` | App name; supports `{query}` for "open {query}". | `Cursor` |
| `keystroke` | Key combo converted to AppleScript System Events keystroke (`combo_to_applescript`). | `cmd+shift+5` |

Placeholders in `command`: `{query}`, `{clipboard}`, `{selected_text}`, `{active_app}`.

---

## 7. Install & run

**Prerequisites:**
- macOS 13+ on Apple Silicon
- Rust toolchain (rustup)
- Node 20+ and pnpm
- Xcode Command Line Tools (`xcode-select --install`)
- Groq API key + Anthropic API key

**Dev:**

```bash
git clone https://github.com/TechGuiderBB/wisspa.git
cd wisspa
pnpm install
# Put keys in .env for dev — keychain prompts every rebuild otherwise (see Gotchas)
echo "GROQ_API_KEY=..." > .env
echo "ANTHROPIC_API_KEY=..." >> .env
pnpm tauri dev
```

**Production build:**

```bash
pnpm tauri build
# Output: src-tauri/target/release/bundle/dmg/Wisspa_<version>_aarch64.dmg
```

If Gatekeeper complains after install:

```bash
xattr -d com.apple.quarantine /Applications/Wisspa.app
```

---

## 8. Storage locations

| What | Where |
|---|---|
| Settings JSON | `~/Library/Application Support/com.techguider.wisspa/settings.json` |
| History SQLite | `~/Library/Application Support/com.techguider.wisspa/history.db` |
| Action YAML files (user) | `~/Library/Application Support/com.techguider.wisspa/actions/` |
| API keys | macOS Keychain (entries `com.techguider.wisspa.groq` and `…anthropic`) |
| Default actions (seeded on first launch) | `default-actions/` in this repo |

## Required macOS permissions

| Permission | Why | Optional? |
|---|---|---|
| Microphone | Audio capture | No |
| Accessibility | Synthetic `Cmd+V` paste | No |
| Screen Recording | Screenshot actions only | Yes |
| Automation / Apple Events | Frontmost-app detection + AppleScript actions | No |

Usage descriptions live in `src-tauri/Info.plist`. About tab has a live diagnostic panel with deep links into System Settings.

---

## 9. Testing

Rust unit tests live in `#[cfg(test)]` modules across `src-tauri/src/` (llm, session, prompt_review, executor, clipboard, registry, app_detector, learning, vocab_import, redact, pending). Run them with:

```bash
cd src-tauri && cargo test
```

Frontend typechecking: `pnpm build` (runs `tsc` + Vite build).

CI:

- **`security.yml`** — `gitleaks` (full-history secret scanning) + `cargo audit` + `cargo test` + frontend typecheck. On push, on PR, and weekly cron.
- **`release.yml`** — build + sign + publish to GitHub Releases.

**Manual QA path:** the acceptance criteria in `README.md` and `WISSPA_PRD.md §11` are canonical. Includes real-app smoke tests (Cursor, Claude desktop, Slack, Gmail in Chrome, Notes, Obsidian).

---

## 10. Conventions

- Prompts (Haiku cleanup, Sonnet rewrite) live as `.md` files under `src-tauri/src/prompts/` and are loaded via `include_str!()` in `llm.rs`. **Edit those files — don't put prompt strings in Rust source.**
- YAML action files are source of truth for the registry. Rust parses and validates them on load, hot-reloads on file changes.
- Settings schema is **mirrored on both sides**: `src-tauri/src/settings_store.rs` (Rust) and `src/lib/settings.ts` (TS). Changes need both.
- The `.env` file is dev-only and is git-ignored. Production reads from Keychain.
- AppleScript is preferred over `enigo` for synthetic keystrokes: `enigo`'s `CGEventPost` aborts the host process even with Accessibility granted (`DECISIONS.md` item 9).
- Committed docs stay free of personal names, machine-local paths, and business/commercial details — this is a public repo.

---

## 11. Gotchas (read before changing the app)

1. **Don't hide the `main` window.** WKWebView throttles JS in fully hidden windows, which breaks `MediaRecorder` and the audio path. The runtime window must stay visible — it's the dim pill at top-center (`DECISIONS.md` item 11).
2. **Keychain prompts every rebuild in dev.** Each Rust rebuild creates a fresh unsigned binary that macOS treats as a new app. Put keys in `.env` while iterating; Keychain takes over in signed production builds.
3. **Destructive action confirmation goes through the tray menu, not a toast.** The voice trigger does NOT execute a `destructive: true` action — it stores it as pending and surfaces `Confirm: <name>` + `Cancel pending action` items in the tray. 15-second auto-cancel timeout. The pending state replaces (not queues) when a second destructive trigger fires. See `actions/pending.rs` and `actions/executor.rs::execute`.
4. **Selected-text capture (Prompt Mode) uses a 280 ms post-`Cmd+C` wait.** Reliable for native apps and most editors; flaky for Slack, Notion, some browser tabs (`selection.rs`).
5. **Hotkey reassignment temporarily unregisters all global shortcuts during capture** so the webview can receive the raw key event. Press `Esc` to cancel cleanly if you abort.
6. **The pill follows the cursor's monitor** in multi-display setups.
7. **`enigo` aborts the host process on macOS.** If you're tempted to switch `Cmd+V` injection back to `enigo` — don't. The abort bypasses `catch_unwind`. AppleScript via `osascript` is the macOS-blessed path.

---

## 12. Behavioural rules

- Do what has been asked; nothing more, nothing less
- ALWAYS prefer editing an existing file to creating a new one
- NEVER proactively create documentation files (`*.md`) unless requested
- ALWAYS read a file before editing it
- NEVER commit secrets, credentials, or `.env` files
- NEVER commit Groq or Anthropic API keys — they belong in `.env` (dev) or Keychain (prod)
- TypeScript strict mode; handle loading, error, and empty states in UI work

---

## 13. When you start a session in this repo

1. Read this file
2. Read the relevant PRD section (`WISSPA_PRD.md`; note it drifts — code is authoritative)
3. Do the work; run `cargo test` and `pnpm build` before opening a PR

---

## 14. References

| Doc | Purpose |
|---|---|
| `README.md` | App overview, modes, hotkeys, troubleshooting |
| `WISSPA_PRD.md` | Product PRD — reconciled against v0.1.0 code on 2026-05-16. Useful product context; code is authoritative where they disagree. |
| `DECISIONS.md` | Implementation choices and the reasoning behind them |
| `CHANGELOG.md` | Release history |
| `CONTRIBUTING.md` | Dev setup, test commands, PR process |
| `SECURITY.md` | Vulnerability disclosure |
