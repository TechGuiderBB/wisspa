# Design: Fix clipped first words on recording start

Date: 2026-05-21
Status: Draft — awaiting review

## Problem

When a user presses a recording hotkey and starts speaking immediately, the
first one or two words are missing from the transcript.

## Root cause

`src/lib/audio.ts::startRecording()` calls `acquireHealthyStream()` on every
hotkey press, which runs `navigator.mediaDevices.getUserMedia({ audio: true })`
from cold (`audio.ts:122`). Cold `getUserMedia` on macOS takes roughly
200-800 ms to return a live stream. `MediaRecorder` then has its own small
spin-up before the first audio frame. `onstop` fully tears the stream down
(`audio.ts:153`), so the next press starts cold again. Any speech in that
startup window is lost.

Separately, the "recording started" chime (`Cue::Start`, `sounds.rs`) and the
overlay are fired in `hotkeys.rs` on hotkey **press** — before capture is
actually live — so they do not tell the user when it is genuinely safe to
speak.

## Goals

- No words lost when the user speaks immediately on hotkey press.
- The macOS orange mic indicator only appears when the user is clearly
  invoking Wisspa. It is never on "in the background".
- The audible chime is optional; the visual cue is always present.

## Non-goals (explicitly out of scope)

- A silent always-on microphone. macOS reserves indicator-free mic access for
  OS components (e.g. "Hey Siri"); no third-party app can do it. Any open mic
  session lights the orange dot, always.
- Interactive / animated recording pill, thinking-bubble during Prompt Mode
  processing. Recorded as future work below.

## Design

Two independent layers. Layer 1 fixes the data loss and ships first. Layer 2
removes the startup delay and is opt-in.

### Layer 1 — Ready-cue (no new permission)

Move the "you may speak now" signal from hotkey-press to the moment capture is
genuinely live.

- `hotkeys.rs` press handler: keep `show_overlay`, but the overlay renders a
  dimmed "warming" state. Do **not** play `Cue::Start` here.
- `audio.ts::startRecording()`: subscribe to `MediaRecorder.onstart`. When it
  fires, capture is confirmed live — emit a new event `wisspa://recording-armed`
  to Rust.
- Rust handles `wisspa://recording-armed`: switch the overlay to its "armed"
  state (the existing red flashing dot) and play the ready chime if enabled.
- The user learns the rhythm: press, brief beat, the pill turns red (and
  chimes, if on), speak. Nothing is lost because the user keys off the cue.

Tradeoff: a ~0.2-0.8 s wait before the cue appears, since the mic still starts
cold. Layer 2 removes that wait.

### Layer 2 — Pre-warm on the hotkey's first key (opt-in)

When the modifier portion of a recording hotkey is held, warm the mic so that
completing the combo starts capture instantly.

- New Rust module `src-tauri/src/prearm.rs`: a macOS modifier monitor.
  **Implemented** by polling the current modifier state via
  `CGEventSourceFlagsState` on a ~40 ms timer — this reads current state
  only, not the keystroke stream, so it needs no Input Monitoring permission.
  (A `CGEventTap` was considered first but rejected for exactly that
  permission cost.)
- On app start, compute the set of modifier combinations used by the three
  recording hotkeys (dictation / action / prompt) from settings. The cancel
  hotkey is excluded.
- When a held modifier set matches a recording hotkey's modifiers, emit
  `wisspa://prewarm-mic`. The frontend `audio.ts::warmMic()` acquires and
  **retains** a healthy stream in a module-level slot.
- When the full hotkey fires, `startRecording()` consumes the warm stream
  instead of a cold `getUserMedia`. Capture is near-instant.
- If the modifiers are released without completing the combo, emit
  `wisspa://prewarm-cancel` after a short idle (~1 s); `audio.ts` releases the
  warm stream and the orange dot clears.
- A max-warm timeout (~4-5 s) releases the warm stream even if the modifiers
  stay held, to cap accidental mic-open time.
- The orange dot appears the instant the modifiers go down. This is honest:
  the mic genuinely is open. It is brief and tied to the user reaching for the
  Wisspa key.

Opt-in: gated by a new setting `general.fast_recording_start` (default
`false`). As implemented (modifier-state polling) it needs no new macOS
permission. The setting applies live — `save_settings` restarts the monitor
via a generation counter, so no app restart is required.

## Files changed

| File | Change |
|---|---|
| `src/lib/audio.ts` | Retained warm-stream slot; `warmMic()` / `releaseWarmStream()`; `startRecording()` consumes warm stream with cold fallback; `onstart` → emit `recording-armed`; health-check warm stream before use |
| `src-tauri/src/hotkeys.rs` | Stop playing `Cue::Start` on press; overlay shows "warming" state on press |
| `src-tauri/src/sounds.rs` | `Cue::Start` playback gated by new `ready_chime` setting in addition to `play_sounds` |
| `src-tauri/src/commands.rs` | New command(s) / event wiring for `recording-armed`, `prewarm-mic`, `prewarm-cancel` |
| `src-tauri/src/prearm.rs` | **New** — macOS modifier monitor, recording-hotkey modifier-set matching |
| `src-tauri/src/main.rs` | Register `prearm` module / monitor on setup |
| `src-tauri/src/settings_store.rs` | `General`: add `ready_chime: bool` (default `true`), `fast_recording_start: bool` (default `false`), with serde defaults |
| `src/lib/settings.ts` | Mirror the two new `general` fields |
| `src/components/RecordingOverlay.tsx` | Two visual states: "warming" (dimmed) and "armed" (red flash) |
| `src/App.tsx` | Listen for `prewarm-mic` / `prewarm-cancel`; call `warmMic()` / `releaseWarmStream()` |
| `src/components/settings/GeneralTab.tsx` | Controls for `ready_chime` and `fast_recording_start` (with permission guidance) |
| `src-tauri/Info.plist` | Input Monitoring usage description, if Layer 2 needs it |

## Data flow

```
Layer 1 (always):
  hotkey press → show overlay (warming) → emit start-recording
    → audio.ts startRecording() → MediaRecorder.onstart
    → emit recording-armed → overlay armed + ready chime → user speaks

Layer 2 (opt-in, when enabled + permission granted):
  modifiers down → prearm.rs match → emit prewarm-mic
    → audio.ts warmMic() (orange dot on)
  full hotkey → startRecording() reuses warm stream → onstart near-instant
  modifiers up without combo → (idle ~1 s) → prewarm-cancel → stream released
```

## Error handling and edge cases

- Hotkey with no modifier (pure key): no pre-warm possible; Layer 1 applies.
- All three recording hotkeys share modifiers (the `⌘⇧` defaults): the monitor
  watches the union; any match warms once.
- Warm stream stale after device change / sleep: reuse the existing
  `acquireHealthyStream` track-health check; on failure fall back to cold
  acquire.
- Busy modifier chosen (e.g. `⌘⇧`): the dot will flicker during unrelated
  shortcuts. Accepted tradeoff; the max-warm timeout caps exposure. Could later
  warn in settings when a busy modifier is configured.
- Input Monitoring denied: Layer 2 no-ops, Layer 1 unaffected.
- `main` window must stay visible (existing gotcha 11) — the warm stream lives
  in its JS context; unaffected since the window is always visible.

## Testing

No Rust test suite exists. Manual QA:

- Record with immediate speech, Layer 1 only — confirm first words are present
  once the user waits for the red cue.
- Toggle `ready_chime` off — confirm the flash still appears, no chime.
- Enable `fast_recording_start` — confirm instant capture and that the dot
  appears on modifier-down, clears on release.
- Re-run the relevant `README.md` v0.1.0 acceptance smoke tests.

## Risks and open questions

1. **Input Monitoring permission — resolved.** A `CGEventTap` would have
   needed the Input Monitoring TCC permission. The implementation instead
   polls `CGEventSourceFlagsState` (current modifier state, not the keystroke
   stream), which needs no new permission. Verified on-device.
2. **Modifier-monitor false positives** on common modifiers — accepted and
   documented; mitigated by the max-warm timeout.
3. **Chime default.** `ready_chime` defaults to `true` to preserve current
   behaviour for existing users who disable it.

## Future work (not in this spec)

- Interactive recording pill: motion, richer states, and a "thinking" bubble
  during Prompt Mode while Sonnet rewrites (the pill currently goes quiet for
  5-10 s). Worth its own design.
