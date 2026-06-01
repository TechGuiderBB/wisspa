# Decisions

Minor decisions not specified in `WISSPA_PRD.md`. Each entry: what was decided, why, and which phase introduced it.

---

## Phase 1

### 1. Test hotkey = `Cmd+Shift+Space`
The `fn` key (PRD §4.2 default for Dictation) isn't reliably registerable via `tauri-plugin-global-shortcut` because macOS special-cases it outside the normal modifier set. For Phase 1 smoke testing we use a safe combo. Real `fn` handling is deferred to Phase 3 when the Hotkeys tab needs a proper key-capture component anyway.

### 2. Audio transport = base64 over `invoke`
Frontend converts the recorded `Blob` to a base64 string and passes it to a Rust command. Avoids serialising binary in Tauri events. ~33% size overhead is acceptable for short voice clips (well under 1 MB typical).

### 3. Audio format = `audio/webm; codecs=opus`
Native MediaRecorder format in WKWebView on macOS. Groq Whisper accepts webm directly.

### 4. API keys (Phase 1 only) = `.env` via `dotenvy`
Quickest unblock for end-to-end testing. Keychain integration arrives in Phase 3 per the PRD build order.

### 5. Clipboard save/restore window = 200ms
After issuing simulated `Cmd+V`, wait 200ms before restoring previous clipboard. Long enough for the target app to consume the paste, short enough not to feel laggy.

### 6. Hidden runtime window
A single invisible window hosts the React app so MediaRecorder has a webview to live in. The menu bar tray icon and visible Settings window arrive in Phase 2/3.

### 7. App bundle identifier = `com.techguider.wisspa`
TechGuider-namespaced bundle identifier.

### 8. Frontend package name = `wisspa` (lowercase)
npm requires lowercase package names; product name in Tauri config stays `Wisspa`.

### 9. Cmd+V dispatch via AppleScript, not enigo (Phase 1)
The PRD specifies `enigo` for keystroke simulation. On macOS, even with Accessibility permission granted (`AXIsProcessTrusted() == true`), enigo's `CGEventPost`-based dispatch hard-aborts the host process — the abort bypasses Rust's `catch_unwind` and takes the whole binary down right after the keystroke is posted. AppleScript via `osascript -e 'tell application "System Events" to keystroke "v" using command down'` is robust, well-documented, and the macOS-blessed path. enigo is kept in `Cargo.toml` for the Phase 4 keystroke action type (where it dispatches its own process, isolating any abort).

### 10. macOS accessory app from launch
`setActivationPolicy(Accessory)` is set in `setup()` so there's no dock icon and the app never steals focus on launch / rebuild.

### 11. Runtime window must stay visible (WKWebView JS throttling)
macOS WKWebView throttles JS in fully hidden or off-screen windows, which broke MediaRecorder and the `process_audio` invoke path in Phase 2. The runtime window therefore stays **on screen** at top-center, sized 220×56, transparent, undecorated, `focus: false`. It renders a dim "Wisspa" pill (gray dot, no flash). The recording overlay (`alwaysOnTop: true`, same size + position) sits directly above it and renders the active recording state (red flashing dot + "Wisspa"). Result: the user sees a single pill that flips between idle and recording states; the runtime's WebView is never paused.

### 12. Recording UI = single-pill stack (overlay-on-top-of-runtime)
Two windows at the same top-center position: runtime (always visible, idle pill) underneath, overlay (toggled on hotkey, recording pill) on top. Eliminates the bottom-right "spare" pill earlier prototypes had. Simpler UX, single visual focal point.

### 13. Recording sessions + cancellation model (issue #31)
Every hotkey press mints a monotonic **session id** (`session.rs`) that is threaded press → `process_audio` → mode runners → `inject_text`. Replaces the old single `CANCEL_EPOCH` counter, which only the prompt-preview wait consumed — STT, the LLM call, and injection kept running after Esc and pasted into whatever field was focused by then, and overlapping recordings could race the clipboard.

**Overlap policy: latest-wins (drop-prior).** Starting a new recording supersedes any older in-flight one, so only the newest recording ever injects. This matches the dictation tool's intent (the thing you just said is the thing you want) and avoids a queue that would paste stale text seconds later. Both cancel and supersede are expressed by one `cancelled_through` watermark: a session is aborted iff `session <= cancelled_through`. `begin()` raises the watermark to the prior session; `cancel_active()` (Esc) raises it to the current session.

**Cancellation aborts the work, not just the result.** A `tokio::sync::watch` channel broadcasts watermark changes; STT and the LLM call run inside `tokio::select!` against an `aborted(session)` future, so Esc (or a newer recording) drops the request future and cancels the in-flight reqwest call rather than letting it finish and discarding the output. Injection is deliberately **not** raced — it has side effects (clipboard). Instead it takes a global `tokio::sync::Mutex` (single-flight, so two completions can't interleave the clipboard read/write/restore) and re-checks `is_aborted` after acquiring the lock, returning the `CANCELLED_MARKER` without pasting if the session is stale.

The mode+session pair is delivered to the frontend in a single `wisspa://start-recording` payload (previously mode and start were two separate events — a race), and the frontend echoes the session id back through `process_audio`. A `session` of 0 means an older frontend with no session plumbing and is treated as never-cancelled (legacy passthrough).
