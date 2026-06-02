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

### 13. Default actions bundled via resource map, not a glob (issue #34)
`seed_defaults_if_empty` (`actions/registry.rs`) copies the 14 shipped YAML files into the user's actions dir on first launch. It looks in two places: `CARGO_MANIFEST_DIR/../default-actions` (dev) and `resource_dir()/default-actions` (packaged). The bundle config had no `resources` entry, so packaged `.dmg` installs shipped **zero** default actions — verified by building `v0.1.0` and inspecting `Wisspa.app/Contents/Resources/`, which held only `icon.icns`.

Fix: `bundle.resources` in `tauri.conf.json` set to the **map form**
```json
"resources": { "../default-actions": "default-actions" }
```
not the array/glob form (`["../default-actions/**"]`). Reason: Tauri places `../`-prefixed resources under a `_up_/` folder to preserve the relative path, which would land the files at `Resources/_up_/default-actions/` and miss the `resource_dir().join("default-actions")` lookup. The map form pins the destination to `Resources/default-actions/` directly. Verified post-fix against both the built `.app` and the mounted `.dmg`: 14 YAMLs present at the expected path, no `_up_` folder.

The copy loop was extracted into a unit-testable `copy_yaml_files(src, dest)` helper that now also honours the long-documented "only copy files that don't yet exist" contract (previously relied on the empty-dir gate alone). A release smoke check was added to `LAUNCH.md` so this can't silently regress.

### 14. Recording sessions + cancellation model (issue #31)
Every hotkey press mints a monotonic **session id** (`session.rs`) that is threaded press → `process_audio` → mode runners → `inject_text`. Replaces the old single `CANCEL_EPOCH` counter, which only the prompt-preview wait consumed — STT, the LLM call, and injection kept running after Esc and pasted into whatever field was focused by then, and overlapping recordings could race the clipboard.

**Overlap policy: latest-wins (drop-prior).** Starting a new recording supersedes any older in-flight one, so only the newest recording ever injects. This matches the dictation tool's intent (the thing you just said is the thing you want) and avoids a queue that would paste stale text seconds later. Both cancel and supersede are expressed by one `cancelled_through` watermark: a session is aborted iff `session <= cancelled_through`. `begin()` raises the watermark to the prior session; `cancel_active()` (Esc) raises it to the current session.

**Cancellation aborts the work, not just the result.** A `tokio::sync::watch` channel broadcasts watermark changes; STT and the LLM call run inside `tokio::select!` against an `aborted(session)` future, so Esc (or a newer recording) drops the request future and cancels the in-flight reqwest call rather than letting it finish and discarding the output. Injection is deliberately **not** raced — it has side effects (clipboard). Instead it takes a global `tokio::sync::Mutex` (single-flight, so two completions can't interleave the clipboard read/write/restore) and re-checks `is_aborted` after acquiring the lock, returning the `CANCELLED_MARKER` without pasting if the session is stale.

The mode+session pair is delivered to the frontend in a single `wisspa://start-recording` payload (previously mode and start were two separate events — a race), and the frontend echoes the session id back through `process_audio`. A `session` of 0 means an older frontend with no session plumbing and is treated as never-cancelled (legacy passthrough). The matching `wisspa://stop-recording` payload carries the same mode+session so the release binds to the recording its own press began.

### 15. Lossless clipboard snapshot/restore via NSPasteboard (issue #32)
Dictation injects by writing to the clipboard, pasting `Cmd+V`, then restoring the previous clipboard. The old restore path used `tauri-plugin-clipboard-manager`'s `read_text()`, which returns `Err` for any non-text clipboard (image, file reference, RTF). That `Err` was treated as "nothing to restore", so dictating while a screenshot or copied file was on the clipboard **silently destroyed it**.

Chose **option 1/3 from the issue: snapshot every pasteboard flavor and restore them all** (`clipboard.rs`), via direct `NSPasteboard` access (`objc2-app-kit`). Rejected option 2 (detect non-text and skip the paste) because it kills the dictation the user asked for.

Key constraints:
- **Owned Rust data, not Cocoa objects, across `.await`.** The snapshot converts each flavor to `(String, Vec<u8>)` immediately. `inject_text` is an async fn on a multi-threaded runtime, so its future must be `Send`; holding `Retained<NSData>` across the activate/paste/sleep awaits would break that and risk autorelease lifetime bugs.
- **All ObjC work inside `autoreleasepool`.** objc2 0.6 exposes the `NSPasteboard` methods we use as safe (no `MainThreadMarker`, no raw pointers), so no `unsafe` blocks are needed.
- **Promised/lazy flavors fail open.** A provider that vends data on demand returns no bytes for `dataForType:`; we can't reproduce the promise, so those flavors are skipped and the count is logged (never silently claimed as "all flavors restored").
- **Empty clipboard:** leave the injected text in place (unchanged prior behaviour) rather than clearing.

This swaps the implementation *inside* the single-flight inject mutex from #14, so the snapshot→write→paste→restore window is already serialized — two pastes can't interleave and corrupt the clipboard. Verified with a round-trip unit test against a private `pasteboardWithUniqueName` (text + binary bytes incl. 0x00/0xFF), so the test never touches the real system clipboard. `objc2`, `objc2-app-kit`, `objc2-foundation` were already in the tree transitively via Tauri; promoting them to direct deps adds exactly one crate to the lockfile — `objc2-core-video`, a non-optional dependency of `objc2-app-kit` that no feature flag removes.

### 16. File logging + redaction policy (issue #33)
`env_logger` wrote to stderr, which macOS Launch Services redirects to `/dev/null` for a double-clicked/autostarted app — so a shipped build produced **no** diagnostic artifact. Replaced with a small synchronous `log::Log` backend (`logging.rs`) writing to `~/Library/Logs/Wisspa/wisspa.log`, rotating at 5 MB and keeping 3 files.

**Why hand-rolled, not `tracing-appender`/`fern`:** `tracing-appender` only rotates on time, not size, so it can't meet "5 MB, keep 3". A synchronous backend also has no background flush-guard to keep alive (an async-appender footgun), and — being a plain `log` backend — every existing `log::info!`/`warn!`/`error!` call keeps working with no migration. It also mirrors to stderr so `tauri dev` still shows logs.

**Redaction policy:** transcripts, LLM output, and clipboard/selection values are never written verbatim at info/warn. `redact::redact()` returns `<redacted chars=N sha256=xxxxxxxx>` — a length plus a short SHA-256 prefix, enough to correlate the same content across lines without exposing it. Applied at every such log site (`commands.rs` transcript + hallucination, `dictation.rs` divergence, `executor.rs` resolved command). A **Verbose logging** toggle (Settings → General, default OFF, mirrored in `settings_store.rs` + `settings.ts`) flips `redact()` to log content verbatim when the user is capturing a bug; it is read at startup and on each `process_audio` call so it takes effect without a restart.

**Export Diagnostics** (About tab → `export_diagnostics` command) bundles the already-redacted log files plus app version, accessibility state and hotkey config into a `.zip`. It deliberately excludes `history.db` (transcripts) and the raw `settings.json` (vocabulary, paths, future licence state). `zip` uses the `stored` method only — no compression backend — so no new crates (`zopfli`/`zlib-rs`) enter the cargo-audit surface; `sha2` and `zip` were already in the tree transitively. `env_logger` was removed.
