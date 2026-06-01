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

### 13. File logging + redaction policy (issue #33)
`env_logger` wrote to stderr, which macOS Launch Services redirects to `/dev/null` for a double-clicked/autostarted app — so a shipped build produced **no** diagnostic artifact. Replaced with a small synchronous `log::Log` backend (`logging.rs`) writing to `~/Library/Logs/Wisspa/wisspa.log`, rotating at 5 MB and keeping 3 files.

**Why hand-rolled, not `tracing-appender`/`fern`:** `tracing-appender` only rotates on time, not size, so it can't meet "5 MB, keep 3". A synchronous backend also has no background flush-guard to keep alive (an async-appender footgun), and — being a plain `log` backend — every existing `log::info!`/`warn!`/`error!` call keeps working with no migration. It also mirrors to stderr so `tauri dev` still shows logs.

**Redaction policy:** transcripts, LLM output, and clipboard/selection values are never written verbatim at info/warn. `redact::redact()` returns `<redacted chars=N sha256=xxxxxxxx>` — a length plus a short SHA-256 prefix, enough to correlate the same content across lines without exposing it. Applied at every such log site (`commands.rs` transcript + hallucination, `dictation.rs` divergence, `executor.rs` resolved command). A **Verbose logging** toggle (Settings → General, default OFF, mirrored in `settings_store.rs` + `settings.ts`) flips `redact()` to log content verbatim when the user is capturing a bug; it is read at startup and on each `process_audio` call so it takes effect without a restart.

**Export Diagnostics** (About tab → `export_diagnostics` command) bundles the already-redacted log files plus app version, accessibility state and hotkey config into a `.zip`. It deliberately excludes `history.db` (transcripts) and the raw `settings.json` (vocabulary, paths, future licence state). `zip` uses the `stored` method only — no compression backend — so no new crates (`zopfli`/`zlib-rs`) enter the cargo-audit surface; `sha2` and `zip` were already in the tree transitively. `env_logger` was removed.
