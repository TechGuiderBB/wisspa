# Wisspa — Product Requirements Document

> **Working name: Wisspa.** This is a placeholder — global find/replace before launch.
> **Owner:** TechGuider
> **Status:** v1.0 spec — intended for single-pass Claude Code implementation
> **Target platform (v1):** macOS 13+ (Apple Silicon)
> **Last updated:** May 2026

---

## 1. Overview

### 1.1 Vision

Wisspa is a system-wide AI voice tool for macOS. The user presses a hotkey, speaks, and Wisspa does one of three things based on which mode is active:

1. **Dictation Mode** — transcribes speech, cleans it up with an LLM, and types the result into whatever app is focused.
2. **Action Mode** — interprets speech as a command and executes a registered action (e.g., "screenshot", "open Cursor", "search GitHub for `tauri-plugin-store`").
3. **Prompt Mode** — rewrites rough spoken intent into a structured, high-quality prompt formatted for the AI tool currently in focus (Claude, ChatGPT, Cursor, Gemini).

The product replaces typing for 80% of text input and replaces ad-hoc prompt-writing with structured, target-aware prompts.

### 1.2 Problem statement

Existing voice tools (Wispr Flow, Glaido, Superwhisper) solve dictation but stop there. None of them turn speech into structured prompts, and none expose a user-extensible action registry. As LLMs become the dominant interface for knowledge work, the bottleneck shifts from typing speed to prompt quality. Wisspa closes that gap.

### 1.3 Target user (v1)

Solo founders, developers, and consultants who already work with Claude / ChatGPT / Cursor daily, are comfortable with macOS permission prompts, and want a power-user tool — not a mass-market dictation app.

---

## 2. Goals & Non-Goals

### 2.1 Goals (v1)

- Sub-500ms perceived latency for dictation (hotkey release → text appearing).
- Three modes (Dictation, Action, Prompt) accessible via configurable hotkeys.
- User-editable Action Registry shipped with 10+ default actions.
- Prompt Mode that adapts output format to the active AI app.
- Settings GUI for hotkeys, API keys, action management, and mode behaviour.
- Fully native macOS feel (menu bar app, native notifications, proper permission handling).
- Local-first config and history — no remote backend required for v1.

### 2.2 Non-Goals (v1)

- Windows / Linux support.
- iOS / Android support.
- Cross-device sync.
- Multiple users / teams / SSO.
- Custom-trained STT models.
- Local-only STT (whisper.cpp) — v1 uses Groq Whisper API.
- Voice activity detection / always-on listening — push-to-talk only.
- Reading text under the cursor via Accessibility APIs (clipboard/selection only in v1).
- Multi-language support — English (en-US, en-AU, en-GB) only in v1.

---

## 3. Tech Stack

| Layer | Choice | Version | Notes |
|---|---|---|---|
| Desktop framework | Tauri | 2.x (latest stable) | Rust core + WebView frontend |
| Frontend | React + Vite + TypeScript | React 18, Vite 5, TS 5.x | Standard, well-supported stack |
| Styling | Tailwind CSS | 3.x | Plus shadcn/ui components |
| State | Zustand | latest | Lightweight, no boilerplate |
| STT | Groq API | `whisper-large-v3-turbo` | ~200ms median latency |
| LLM (cleanup) | Anthropic API | `claude-haiku-4-5-20251001` | Fast, cheap, sufficient for cleanup |
| LLM (prompt mode) | Anthropic API | `claude-sonnet-4-6` | Quality matters more than latency here |
| Audio capture | Browser MediaRecorder (in webview) | — | Simpler than `cpal`; revisit in v2 |
| Global hotkeys | `tauri-plugin-global-shortcut` | latest | |
| Local storage | `tauri-plugin-store` | latest | JSON files in app data dir |
| Shell execution | `tauri-plugin-shell` | latest | Sandboxed allowlist |
| Clipboard | `tauri-plugin-clipboard-manager` | latest | |
| Text injection | `enigo` (Rust crate) | latest | For Cmd+V simulation |
| HTTP client (Rust) | `reqwest` | 0.12+ | For API calls |
| Notifications | `tauri-plugin-notification` | latest | |

---

## 4. High-Level Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│ Tauri App (Wisspa)                                                 │
│                                                                 │
│ ┌─────────────────┐         ┌────────────────────────────────┐  │
│ │ Rust Backend    │◀───────▶│ React Frontend (Webview)       │  │
│ │                 │  events │                                │  │
│ │ • Hotkey daemon │         │ • Menu bar UI                  │  │
│ │ • Audio bridge  │         │ • Settings window              │  │
│ │ • API clients   │         │ • Recording indicator overlay  │  │
│ │ • Action runner │         │ • MediaRecorder audio capture  │  │
│ │ • Text injector │         │ • Mode state                   │  │
│ │ • Config store  │         │                                │  │
│ └─────────────────┘         └────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────────┘
                  │
                  ▼
        ┌─────────────────────┐
        │ External services   │
        │ • Groq API (STT)    │
        │ • Anthropic API     │
        └─────────────────────┘
```

### 4.1 Core pipeline

```
1. User presses configured hotkey
2. Rust hotkey handler → emits "start-recording" event to frontend
3. Frontend starts MediaRecorder, shows recording indicator overlay
4. User releases hotkey (or presses again, depending on toggle mode)
5. Frontend stops MediaRecorder → emits audio blob to Rust
6. Rust uploads audio to Groq Whisper → receives raw transcript
7. Rust calls active app detector (AppleScript) → gets frontmost app name
8. Mode router branches:
     ├─ Dictation: send transcript + app context to Haiku for cleanup
     ├─ Action:    pattern-match transcript against Action Registry, execute
     └─ Prompt:    send transcript + app context to Sonnet, get structured prompt
9. Rust injects final text via clipboard + simulated Cmd+V
10. Recording indicator dismisses; toast notification shows result preview
```

### 4.2 Mode routing

Mode is determined at hotkey-press time. Each mode has its own hotkey (user-configurable). No automatic intent detection in v1 — explicit hotkeys eliminate false positives.

**Default hotkeys:**

| Mode | Default | Behaviour |
|---|---|---|
| Dictation | `fn` (hold) or `fn fn` (double-tap to toggle) | Standard dictation |
| Action | `fn + Shift` (hold) | Voice command execution |
| Prompt | `fn + Option` (hold) | Voice → structured prompt |
| Cancel current recording | `Esc` | Discards audio, no API call |

All hotkeys are reassignable in Settings.

---

## 5. Feature Specifications

### 5.1 Feature: Dictation Mode

**Trigger:** Press-and-hold the configured dictation hotkey while speaking.

**Behaviour:**
1. On press, recording overlay appears (small floating pill near the mic icon in the menu bar, with a live waveform).
2. On release, audio is sent to Groq Whisper for transcription.
3. Raw transcript is sent to Claude Haiku for cleanup with the system prompt below.
4. Cleaned text is injected into the focused text field via clipboard + Cmd+V simulation.
5. Toast notification shows a preview of what was inserted with a 3-second timeout.

**Haiku system prompt for dictation cleanup:**

```
You are a dictation post-processor. The user spoke into a microphone and the speech was transcribed by a STT engine. Your job is to clean up that transcript so it reads as polished written text.

Apply these transformations:
- Remove filler words: "um", "uh", "like" (when used as filler), "you know", "I mean".
- Add correct punctuation and capitalization.
- Resolve self-corrections: "Let's meet Tuesday — no wait, Wednesday" → "Let's meet Wednesday".
- Format obvious lists, numbered steps, and code formatting when intent is clear.
- Fix transcription errors using context (e.g., "to" vs "two" vs "too").
- Preserve the user's voice, tone, and word choice. Do NOT paraphrase or rewrite for style.
- Do NOT add content. Do NOT expand abbreviations the user used intentionally.

The user is currently focused on the app: {ACTIVE_APP_NAME}.
Adapt tone subtly based on context:
- Email apps (Gmail, Mail, Superhuman): polished, complete sentences.
- Chat apps (Slack, Discord, iMessage): casual, can keep contractions and short sentences.
- Code editors (Cursor, VS Code, Xcode): preserve technical terminology exactly; format code-like content with backticks.
- Notes apps (Obsidian, Notion, Apple Notes): clean prose, structure with bullets if list intent is clear.
- Default: clean professional prose.

Return ONLY the cleaned text. No preamble, no quotes, no explanation.
```

**API params (Haiku):**
- `max_tokens: 2048`
- `temperature: 0.2`
- `model: claude-haiku-4-5-20251001`

**Edge cases:**
- Empty transcript → no injection, silent dismiss.
- API error → fallback to raw Groq transcript, show warning toast.
- Transcript >2000 chars → still process, but warn in toast.

---

### 5.2 Feature: Action Mode

**Trigger:** Press-and-hold the configured action hotkey while speaking the command.

**Behaviour:**
1. Audio captured and sent to Groq Whisper (no Haiku cleanup needed — actions match against raw text).
2. Transcript is matched against the Action Registry using:
   - **Exact phrase match** (first priority): trigger phrases match transcript verbatim (case-insensitive, ignoring punctuation).
   - **Fuzzy match** (second priority): Levenshtein distance ≤ 3 against any trigger phrase.
   - **No match** → toast: "No action matched. Did you mean: [top 2 suggestions]?"
3. Matched action is validated (permission check, destructive flag).
4. If `destructive: true`, show confirmation toast with 3-second timeout to cancel.
5. Action executes. Success/failure toast shown.

#### 5.2.1 Action Registry schema

Actions are stored as YAML files in `~/Library/Application Support/Wisspa/actions/`. Each file = one action. Wisspa watches this directory and hot-reloads on change.

```yaml
# ~/Library/Application Support/Wisspa/actions/screenshot.yaml
id: screenshot
name: "Interactive Screenshot to Clipboard"
description: "Triggers macOS interactive screenshot, copies result to clipboard"
triggers:
  - "screenshot"
  - "take a screenshot"
  - "screen grab"
  - "capture screen"
type: shell
command: "screencapture -i -c"
working_dir: null
requires_permissions:
  - screen_recording
destructive: false
success_feedback: "Screenshot captured to clipboard"
failure_feedback: "Screenshot failed"
enabled: true
```

**Action `type` values supported in v1:**
- `shell` — runs a shell command. `command` field is the command string.
- `applescript` — runs AppleScript. `command` field is the script.
- `open_url` — opens URL in default browser. `command` field is URL template (supports `{query}` placeholder).
- `open_app` — opens app by name. `command` field is app name.
- `keystroke` — types a keystroke combo via enigo. `command` field is e.g. `cmd+shift+4`.

**Reserved placeholders in `command`:**
- `{query}` — populated with the remainder of the transcript after the trigger phrase. E.g., trigger "search GitHub for" + transcript "search GitHub for tauri-plugin-store" → query = "tauri-plugin-store".
- `{clipboard}` — current clipboard text.
- `{selected_text}` — currently selected text (read via Cmd+C simulation).
- `{active_app}` — name of frontmost app.

#### 5.2.2 v1 Default Actions (ship with installer)

| ID | Triggers | Type | Command |
|---|---|---|---|
| `screenshot` | "screenshot", "take a screenshot" | shell | `screencapture -i -c` |
| `screenshot_to_file` | "screenshot to file", "save screenshot" | shell | `screencapture -i ~/Desktop/screenshot-$(date +%s).png` |
| `start_screen_recording` | "start screen recording", "record screen" | keystroke | `cmd+shift+5` |
| `copy_selection` | "copy that", "copy this" | keystroke | `cmd+c` |
| `paste` | "paste", "paste it" | keystroke | `cmd+v` |
| `new_note` | "new note", "make a note" | shell | `echo "{query}" >> ~/Documents/voice-notes.md` |
| `open_app` | "open {query}" | open_app | `{query}` |
| `search_google` | "search Google for", "google" | open_url | `https://www.google.com/search?q={query}` |
| `search_github` | "search GitHub for" | open_url | `https://github.com/search?q={query}&type=repositories` |
| `search_youtube` | "search YouTube for" | open_url | `https://www.youtube.com/results?search_query={query}` |
| `clear_clipboard` | "clear clipboard" | shell | `pbcopy < /dev/null` |
| `lock_screen` | "lock screen", "lock my mac" | keystroke | `ctrl+cmd+q` |
| `show_desktop` | "show desktop" | keystroke | `fn+f11` |
| `mute_audio` | "mute", "mute audio" | applescript | `set volume with output muted` |

#### 5.2.3 Rust executor signature

```rust
// src-tauri/src/actions/executor.rs

pub struct ActionExecutor {
    registry: ActionRegistry,
    shell_allowlist: Vec<String>,
}

#[derive(Debug)]
pub enum ActionResult {
    Success { feedback: String },
    Failure { reason: String },
    Cancelled,
    PermissionDenied { permission: String },
}

impl ActionExecutor {
    pub async fn execute(
        &self,
        transcript: &str,
        context: &ExecutionContext,
    ) -> ActionResult {
        // 1. Match transcript against registry
        let matched = self.registry.find_match(transcript)?;

        // 2. Check permissions
        for perm in &matched.requires_permissions {
            if !self.has_permission(perm) {
                return ActionResult::PermissionDenied { permission: perm.clone() };
            }
        }

        // 3. Confirmation if destructive
        if matched.destructive {
            let confirmed = self.request_confirmation(&matched).await;
            if !confirmed { return ActionResult::Cancelled; }
        }

        // 4. Resolve placeholders ({query}, {clipboard}, etc.)
        let resolved_command = self.resolve_placeholders(&matched.command, transcript, context);

        // 5. Dispatch to executor for action type
        match matched.action_type {
            ActionType::Shell => self.run_shell(&resolved_command).await,
            ActionType::AppleScript => self.run_applescript(&resolved_command).await,
            ActionType::OpenUrl => self.open_url(&resolved_command).await,
            ActionType::OpenApp => self.open_app(&resolved_command).await,
            ActionType::Keystroke => self.send_keystroke(&resolved_command).await,
        }
    }
}
```

**Security constraints:**
- Shell commands must NOT use `sudo`, `rm -rf`, `dd`, or piped curl/wget execution. Validate on registry load.
- Shell commands run with the user's normal permissions, no elevation.
- All custom actions added by the user via the GUI must pass validation before being saved.

---

### 5.3 Feature: Prompt Mode

**Trigger:** Press-and-hold the configured prompt hotkey while speaking the intent.

**Behaviour:**
1. Audio captured → Groq Whisper → raw transcript.
2. Active app detected (AppleScript: `tell application "System Events" to get name of first application process whose frontmost is true`).
3. If user has text selected (read via clipboard with save/restore), that becomes `<context>` in the rewritten prompt.
4. Transcript + active app + (optional) selected text → Claude Sonnet 4.6 with the prompt-rewriter system prompt.
5. Rewritten prompt is injected via clipboard + Cmd+V.
6. Toast shows: "Prompt generated — [first 50 chars]..." with an "Edit before inserting" option (5-second timeout).

#### 5.3.1 Sonnet system prompt (Prompt Mode)

```
You are an expert prompt engineer. The user spoke a rough description of what they want an AI to do. Rewrite it as a structured prompt that will get a high-quality response from the target AI.

# Inputs
- User's spoken intent: {TRANSCRIPT}
- Target AI app: {ACTIVE_APP}   (e.g., Claude, ChatGPT, Cursor, Gemini, or generic)
- Selected text (optional context): {SELECTED_TEXT}

# Core principles
1. **Match complexity to task.** A simple request ("summarise this email in two sentences") gets a simple prompt — do NOT inflate it with role declarations, XML tags, or step-by-step scaffolding. Heavy structure for heavy tasks only.
2. **Preserve the user's intent exactly.** Do not add tasks the user didn't ask for. Do not change scope.
3. **Use the right format for the target AI.**
   - **Claude / Claude Code:** XML tags for sectioning (`<context>`, `<task>`, `<constraints>`, `<output_format>`). Claude is trained to attend to these.
   - **ChatGPT / GPT-4 / GPT-5:** Markdown headings (`## Context`, `## Task`, `## Output format`). Avoid XML.
   - **Cursor / VS Code Copilot:** Inline-friendly, terse. Reference files/symbols where mentioned. Keep under 3 paragraphs unless complexity demands more.
   - **Gemini:** Markdown + clear numbered steps work best.
   - **Unknown / generic:** Default to Markdown headings.
4. **Sections to include only when warranted by the task:**
   - Role / persona (only if the task is specialised — legal review, code review, etc.)
   - Context (always, if selected text is provided)
   - Task (always)
   - Constraints (only if there are real boundaries — length, format, language, what to avoid)
   - Output format (when the user expects a specific shape — JSON, table, code, bullet list)
   - Examples (only if the user provided them in their speech, or if the task is unusual and one would clarify)
5. **Preserve user voice in casual contexts.** If the target is Cursor mid-coding-flow, keep the prompt one or two sentences. Don't force enterprise structure onto a quick fix.

# Output
Return ONLY the rewritten prompt. No preamble, no explanation, no quotes around it.
```

**API params (Sonnet):**
- `max_tokens: 4096`
- `temperature: 0.4`
- `model: claude-sonnet-4-6`

#### 5.3.2 Active app → format mapping

The mapping in 5.3.1 is implemented as a Rust lookup table:

```rust
fn target_format_for(app_name: &str) -> PromptFormat {
    let lower = app_name.to_lowercase();
    if lower.contains("claude") || lower.contains("cursor") && lower.contains("claude") {
        PromptFormat::ClaudeXml
    } else if lower.contains("chatgpt") || lower.contains("openai") {
        PromptFormat::ChatGptMarkdown
    } else if lower.contains("cursor") || lower.contains("code") {
        PromptFormat::CursorInline
    } else if lower.contains("gemini") || lower.contains("bard") {
        PromptFormat::GeminiMarkdown
    } else {
        PromptFormat::GenericMarkdown
    }
}
```

This value is passed in as `{ACTIVE_APP}` so Sonnet knows what format to emit.

---

### 5.4 Feature: Settings GUI

A single Settings window accessible from the menu bar icon. Built as a React app inside the Tauri webview.

**Tabs:**

1. **General**
   - Launch on login (toggle)
   - Show recording overlay (toggle)
   - Default mode behaviour: press-and-hold vs toggle (radio)
   - Sound on start/stop recording (toggle + volume slider)
   - Theme: System / Light / Dark

2. **API Keys**
   - Groq API key (password input, stored in macOS Keychain via `tauri-plugin-keychain` or similar)
   - Anthropic API key (same)
   - "Test connection" button per key

3. **Hotkeys**
   - Dictation hotkey (key capture input)
   - Action hotkey (key capture input)
   - Prompt hotkey (key capture input)
   - Cancel hotkey (default Esc, reassignable)
   - Validation: prevent conflicting/system-reserved combos
   - Reset to defaults button

4. **Actions**
   - List of all actions in registry (table: ID, name, triggers, type, enabled toggle)
   - Buttons: Add Action, Edit, Delete, Duplicate, Import YAML, Export YAML, Reveal in Finder
   - Add/Edit dialog: form for all action fields with live validation
   - Search/filter bar

5. **Prompt Mode**
   - Always include selected text as context (toggle, default ON)
   - Show preview before inserting (toggle, default ON)
   - Preview timeout (slider, 3-10 seconds, default 5)
   - Override target app format manually (dropdown — useful when active app detection fails)

6. **History**
   - List of last 100 dictations / actions / prompts (timestamp, mode, snippet)
   - Stored locally in `~/Library/Application Support/Wisspa/history.db` (SQLite via `rusqlite`)
   - Clear history button
   - Export to CSV button

7. **About**
   - Version
   - GitHub link
   - License
   - Permissions diagnostic panel (shows status of Microphone, Accessibility, Screen Recording, Automation)

---

### 5.5 Feature: Onboarding & Permissions

On first launch, walk user through:

1. **Welcome screen** — short product pitch, "Get started" button.
2. **Microphone permission** — explanatory copy, button triggers system prompt.
3. **Accessibility permission** — explanatory copy, button opens System Settings → Privacy & Security → Accessibility, with deep link if possible.
4. **Screen Recording permission** (optional, only if user wants screenshot actions) — explanatory copy, button opens System Settings.
5. **Automation permission** — explained when first AppleScript action runs; cannot be pre-granted.
6. **API key entry** — Groq + Anthropic; show "Get a key" links to each provider.
7. **Hotkey confirmation** — show defaults, let user reassign.
8. **Test dictation** — final step, prompts user to do a test dictation into a textarea inside the onboarding window.

Each permission state is checked on every launch; if any are revoked, show a banner in the main window.

---

## 6. Data Schemas

### 6.1 Settings (`~/Library/Application Support/Wisspa/settings.json`)

```json
{
  "version": 1,
  "general": {
    "launch_on_login": false,
    "show_overlay": true,
    "recording_mode": "press_and_hold",
    "play_sounds": true,
    "sound_volume": 0.5,
    "theme": "system"
  },
  "hotkeys": {
    "dictation": "fn",
    "action": "fn+shift",
    "prompt": "fn+option",
    "cancel": "escape"
  },
  "prompt_mode": {
    "include_selected_text": true,
    "show_preview": true,
    "preview_timeout_seconds": 5,
    "manual_app_override": null
  },
  "stt": {
    "provider": "groq",
    "model": "whisper-large-v3-turbo",
    "language": "en"
  },
  "cleanup_llm": {
    "provider": "anthropic",
    "model": "claude-haiku-4-5-20251001"
  },
  "prompt_llm": {
    "provider": "anthropic",
    "model": "claude-sonnet-4-6"
  }
}
```

### 6.2 History (`~/Library/Application Support/Wisspa/history.db` — SQLite)

```sql
CREATE TABLE history (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  timestamp INTEGER NOT NULL,        -- Unix epoch ms
  mode TEXT NOT NULL,                -- 'dictation' | 'action' | 'prompt'
  active_app TEXT,
  raw_transcript TEXT NOT NULL,
  output TEXT,                       -- Cleaned text / action result / generated prompt
  action_id TEXT,                    -- NULL unless mode = 'action'
  duration_ms INTEGER,
  status TEXT NOT NULL               -- 'success' | 'failure' | 'cancelled'
);

CREATE INDEX idx_history_timestamp ON history(timestamp DESC);
```

### 6.3 Action file (YAML) — see §5.2.1

API keys are stored in **macOS Keychain**, never in `settings.json`.

---

## 7. API Contracts

### 7.1 Groq Whisper STT

```
POST https://api.groq.com/openai/v1/audio/transcriptions
Authorization: Bearer {GROQ_API_KEY}
Content-Type: multipart/form-data

Form fields:
- file: audio blob (webm or m4a, ≤25MB)
- model: "whisper-large-v3-turbo"
- response_format: "json"
- language: "en"
- temperature: 0

Response:
{ "text": "..." }
```

Errors: surface HTTP error code and message to the user via toast; do not block UI.

### 7.2 Anthropic Messages API

```
POST https://api.anthropic.com/v1/messages
x-api-key: {ANTHROPIC_API_KEY}
anthropic-version: 2023-06-01
Content-Type: application/json

Body (Haiku cleanup):
{
  "model": "claude-haiku-4-5-20251001",
  "max_tokens": 2048,
  "temperature": 0.2,
  "system": "{HAIKU_SYSTEM_PROMPT}",
  "messages": [{ "role": "user", "content": "{RAW_TRANSCRIPT}" }]
}

Body (Sonnet prompt mode):
{
  "model": "claude-sonnet-4-6",
  "max_tokens": 4096,
  "temperature": 0.4,
  "system": "{SONNET_SYSTEM_PROMPT}",
  "messages": [{
    "role": "user",
    "content": "Active app: {ACTIVE_APP}\n\nSelected text (if any):\n{SELECTED_TEXT}\n\nUser intent:\n{TRANSCRIPT}"
  }]
}
```

Errors: surface; do not retry automatically beyond 1 retry on 5xx.

---

## 8. Project Structure

```
wisspa/
├── src-tauri/                    # Rust backend
│   ├── Cargo.toml
│   ├── tauri.conf.json
│   ├── build.rs
│   └── src/
│       ├── main.rs               # Entry point, setup
│       ├── lib.rs                # Re-exports
│       ├── hotkeys.rs            # Global hotkey registration
│       ├── audio.rs              # Audio capture bridge to frontend
│       ├── stt.rs                # Groq client
│       ├── llm.rs                # Anthropic client (Haiku + Sonnet)
│       ├── injector.rs           # Clipboard + Cmd+V injection via enigo
│       ├── app_detector.rs       # Frontmost app detection via AppleScript
│       ├── actions/
│       │   ├── mod.rs
│       │   ├── registry.rs       # YAML loader, hot-reload
│       │   ├── executor.rs       # Runs actions
│       │   └── matcher.rs        # Exact + fuzzy matching
│       ├── modes/
│       │   ├── mod.rs
│       │   ├── dictation.rs
│       │   ├── action.rs
│       │   └── prompt.rs
│       ├── permissions.rs        # macOS permission checks
│       ├── keychain.rs           # API key storage
│       ├── history.rs            # SQLite logger
│       ├── prompts/
│       │   ├── haiku_cleanup.md  # Loaded at build/runtime
│       │   └── sonnet_prompt.md
│       └── commands.rs           # Tauri command handlers exposed to frontend
├── src/                          # React frontend
│   ├── main.tsx
│   ├── App.tsx
│   ├── components/
│   │   ├── RecordingOverlay.tsx  # Floating mic indicator
│   │   ├── Toast.tsx
│   │   ├── ui/                   # shadcn components
│   │   └── settings/
│   │       ├── GeneralTab.tsx
│   │       ├── ApiKeysTab.tsx
│   │       ├── HotkeysTab.tsx
│   │       ├── ActionsTab.tsx
│   │       ├── PromptModeTab.tsx
│   │       ├── HistoryTab.tsx
│   │       └── AboutTab.tsx
│   ├── pages/
│   │   ├── Onboarding.tsx
│   │   └── Settings.tsx
│   ├── store/
│   │   ├── settings.ts           # Zustand store
│   │   ├── recording.ts
│   │   └── history.ts
│   ├── lib/
│   │   ├── audio.ts              # MediaRecorder wrapper
│   │   ├── tauri.ts              # Tauri command bindings
│   │   └── hotkey-input.ts       # Hotkey capture component logic
│   └── styles/
│       └── globals.css
├── default-actions/              # Shipped YAML actions, copied to user dir on first run
│   ├── screenshot.yaml
│   ├── search_google.yaml
│   └── ...
├── package.json
├── vite.config.ts
├── tailwind.config.js
├── tsconfig.json
└── README.md
```

---

## 9. Visual Design

- Menu bar icon: simple mic glyph, animates while recording.
- Recording overlay: small pill (~200×40px) anchored to top-center of screen, semi-transparent, with live waveform. Dismisses with fade.
- Settings window: 800×600, native macOS chrome, tabbed sidebar layout (like Raycast settings).
- Toasts: top-right, stack vertically, auto-dismiss with progress bar.
- Theme: respect system light/dark mode by default. Use Tailwind's `dark:` variants. Accent colour: a confident blue (`#3b82f6`) — easy to change later.
- Typography: SF Pro (system), 14px body, 12px secondary.
- Spacing: 8px grid throughout.

---

## 10. Build & Run

```bash
# Prerequisites: Rust (rustup), Node 20+, Xcode CLT, pnpm
git clone <repo>
cd wisspa
pnpm install
pnpm tauri dev          # Dev mode
pnpm tauri build        # Production build → dmg + app bundle in src-tauri/target/release/bundle/
```

The user installs the `.dmg`, drags Wisspa.app to Applications, and launches. Production builds will ship signed and notarised; for unsigned dev builds, `xattr -d com.apple.quarantine /Applications/Wisspa.app` clears the Gatekeeper flag.

---

## 11. Acceptance Criteria (Definition of Done for v1)

The build is complete when **all** of the following are true:

### 11.1 Core flows

- [ ] User can press the dictation hotkey, speak, release, and see cleaned text appear in any focused text field (tested in: Cursor, Claude desktop, Slack, Gmail in Chrome, Notes, Obsidian).
- [ ] User can press the action hotkey and say "screenshot" and the interactive screenshot tool activates.
- [ ] User can press the action hotkey and say "search Google for tauri plugins" and Google opens in the browser with that query.
- [ ] User can press the prompt hotkey, say "review this code for security issues", and a properly formatted prompt is inserted (XML tags if focused on Claude; Markdown if ChatGPT).
- [ ] User can press the cancel hotkey while recording and the recording aborts without an API call.

### 11.2 Settings GUI

- [ ] All 7 tabs render and persist changes to `settings.json`.
- [ ] API keys saved to Keychain, not the JSON file.
- [ ] Hotkey reassignment works and reflects immediately without restart.
- [ ] Adding a new action via the GUI saves a valid YAML file in the actions dir.
- [ ] Test buttons for Groq + Anthropic correctly verify keys.

### 11.3 Action Registry

- [ ] All 14 default actions ship with the installer.
- [ ] Editing a YAML file in `~/Library/Application Support/Wisspa/actions/` is picked up by Wisspa without restart.
- [ ] Invalid YAML produces a clear error in the Actions tab.
- [ ] Shell allowlist rejects `sudo`, `rm -rf`, etc.

### 11.4 Onboarding

- [ ] First-launch wizard guides through all 4 permissions + API keys + hotkey confirmation + test dictation.
- [ ] Revoked permissions show a banner with a "Re-grant" deep link.

### 11.5 Performance

- [ ] Hotkey release → text inserted: median ≤ 800ms for a 3-second dictation (depends on network; measured on home Wi-Fi).
- [ ] App startup: ≤ 1 second to menu bar icon ready.
- [ ] Memory footprint at idle: ≤ 150 MB.

### 11.6 Reliability

- [ ] Network failure during STT → graceful toast, no crash.
- [ ] Empty audio → no API call, silent dismiss.
- [ ] Permissions denied → clear error message pointing to System Settings.

---

## 12. Out of Scope (Explicitly NOT in v1)

- Windows / Linux builds
- iOS / Android apps
- Local STT (whisper.cpp)
- Custom vocabulary / personal dictionary learning
- Snippets (voice shortcuts for canned text)
- Multi-language dictation (en only)
- Cross-device sync
- Team / multi-user features
- Streaming STT (record-then-send only)
- Reading text around cursor via Accessibility API (clipboard/selection only)
- Voice activity detection / always-on
- Command Mode (Wispr-style highlight-and-rewrite)
- Auto-updates (manual reinstall for now)
- Telemetry / analytics

---

## 13. v2 Candidate Features (roadmap notes — NOT for v1)

- Local STT fallback (whisper.cpp with Metal acceleration) for full privacy mode.
- Streaming STT for sub-200ms perceived latency.
- Personal dictionary that learns corrections over time.
- Voice snippets — "intro snippet" expands to your standard email opener.
- Command Mode — highlight text, hold a hotkey, say "make this more formal".
- MCP integration — actions can call MCP tools (e.g., "create a Jira ticket for this").
- Prompt library — save generated prompts as named templates, recallable by voice.
- Workflow chaining — "prompt mode then send to Claude": generates prompt AND dispatches via Anthropic API, pastes response.
- Active-text-field context reading via Accessibility API.
- iOS companion app with WhatsApp-style PTT capture.
- Windows + Linux support.

---

## 14. Open Questions for Claude Code

Claude Code should make sensible defaults for the following and call them out in the README:

1. Recording sound effect choice (use a soft system sound or ship a custom one).
2. Exact menu bar icon SVG (suggest a minimal microphone glyph, ~16×16).
3. Whether to bundle the SQLite library (use `rusqlite` with `bundled` feature for portability).
4. Whether to use `tauri-plugin-autostart` for launch-on-login (yes — install it).

---

## 15. References & Inspiration

- **Wispr Flow** (https://wisprflow.ai) — feature reference for dictation UX, Command Mode pattern.
- **Glaido** (https://glaido.com) — context-aware dictation, single-hotkey simplicity.
- **Superwhisper** — fully-local STT comparison point for v2.
- **VoiceInk** (open-source) — example of similar architecture in Swift.
- **Tauri 2 docs** (https://tauri.app) — plugin and permission patterns.
- **Anthropic API docs** (https://docs.claude.com) — Claude model IDs and Messages API contract.
- **Groq API docs** (https://console.groq.com/docs) — Whisper endpoint reference.

---

## END OF PRD
