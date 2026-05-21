# Recording Pre-warm + Ready-cue Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Stop the first one or two spoken words being lost when a recording hotkey is pressed.

**Architecture:** Two layers. Layer 1 (ready-cue) moves the "speak now" signal from hotkey-press to the moment capture is genuinely live, so the user keys off an honest cue — no words lost, no new permission. Layer 2 (pre-warm) opens the mic when the hotkey's modifier key goes down so capture starts instantly; it is opt-in because it likely needs the macOS Input Monitoring permission.

**Tech Stack:** Tauri 2, Rust 2021, React 18 + TypeScript, `MediaRecorder`, macOS `CGEventTap`.

**Verification note:** Wisspa has no Rust test suite (CLAUDE.md §10). Tasks verify with `cargo check`, `pnpm tsc`, `pnpm build`, and explicit manual checks.

---

## Layer 1 — Ready-cue

### Task 1: Settings schema — `ready_chime` and `fast_recording_start`

**Files:**
- Modify: `src-tauri/src/settings_store.rs`
- Modify: `src/lib/settings.ts`

- [ ] **Step 1: Add fields + defaults to Rust `General`**

In `settings_store.rs`, add to `struct General` after `max_recording_seconds`:

```rust
    /// Play the "ready" chime when capture goes live. Gated by play_sounds too.
    #[serde(default = "default_ready_chime")]
    pub ready_chime: bool,
    /// Opt-in: warm the mic on the hotkey's modifier key-down (needs Input
    /// Monitoring permission). Off by default.
    #[serde(default = "default_fast_recording_start")]
    pub fast_recording_start: bool,
```

Add the default fns near `default_max_recording_seconds`:

```rust
fn default_ready_chime() -> bool {
    true
}

fn default_fast_recording_start() -> bool {
    false
}
```

In `impl Default for Settings`, add to the `General { ... }` literal:

```rust
                ready_chime: default_ready_chime(),
                fast_recording_start: default_fast_recording_start(),
```

- [ ] **Step 2: Mirror in the TS `Settings` type**

In `src/lib/settings.ts`, add to `general` after `max_recording_seconds`:

```ts
    ready_chime: boolean;
    fast_recording_start: boolean;
```

- [ ] **Step 3: Verify**

Run: `cd src-tauri && cargo check` — Expected: PASS.
Run: `pnpm tsc --noEmit` — Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/settings_store.rs src/lib/settings.ts
git commit -m "feat: add ready_chime and fast_recording_start settings"
```

### Task 2: Emit `recording-armed` when capture goes live

**Files:**
- Modify: `src/lib/audio.ts`

- [ ] **Step 1: Import `emit`**

At the top of `audio.ts`:

```ts
import { emit } from "@tauri-apps/api/event";
```

- [ ] **Step 2: Emit on `MediaRecorder.onstart`**

In `startRecording()`, after `mediaRecorder.ondataavailable = ...` and before `mediaRecorder.start()`, add:

```ts
    mediaRecorder.onstart = () => {
      void emit("wisspa://recording-armed");
    };
```

- [ ] **Step 3: Verify**

Run: `pnpm tsc --noEmit` — Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add src/lib/audio.ts
git commit -m "feat: emit recording-armed event when capture goes live"
```

### Task 3: Move the start chime from hotkey-press to capture-armed

**Files:**
- Modify: `src-tauri/src/hotkeys.rs`
- Modify: `src-tauri/src/main.rs`

- [ ] **Step 1: Stop playing `Cue::Start` on press**

In `hotkeys.rs`, in each of the three `Pressed` arms (`dictation`, `action`, `prompt`), delete the line:

```rust
                    crate::sounds::play(app, crate::sounds::Cue::Start);
```

Leave `show_overlay(app)` and the emits intact.

- [ ] **Step 2: Play the chime when `recording-armed` fires**

In `main.rs`, inside the `.setup(|app| { ... })` closure, after hotkey registration, add:

```rust
            let armed_handle = app.handle().clone();
            app.handle().listen("wisspa://recording-armed", move |_| {
                crate::sounds::play(&armed_handle, crate::sounds::Cue::Start);
            });
```

Ensure `tauri::Listener` is in scope (add `use tauri::Listener;` if `cargo check` reports `listen` not found).

- [ ] **Step 3: Verify**

Run: `cd src-tauri && cargo check` — Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/hotkeys.rs src-tauri/src/main.rs
git commit -m "feat: play ready chime when capture is armed, not on key press"
```

### Task 4: Gate the start chime behind `ready_chime`

**Files:**
- Modify: `src-tauri/src/sounds.rs`

- [ ] **Step 1: Skip `Cue::Start` when `ready_chime` is off**

In `sounds.rs::play`, after the `if !settings.general.play_sounds { return; }` block, add:

```rust
    if matches!(cue, Cue::Start) && !settings.general.ready_chime {
        return;
    }
```

- [ ] **Step 2: Verify**

Run: `cd src-tauri && cargo check` — Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/sounds.rs
git commit -m "feat: gate ready chime behind ready_chime setting"
```

### Task 5: Overlay warming/armed states

**Files:**
- Modify: `src/components/RecordingOverlay.tsx`

- [ ] **Step 1: Render two states driven by events**

Replace the body of `RecordingOverlay.tsx` with:

```tsx
import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";

export default function RecordingOverlay() {
  const [armed, setArmed] = useState(false);

  useEffect(() => {
    const unlistens: Array<() => void> = [];
    listen("wisspa://start-recording", () => setArmed(false)).then((u) =>
      unlistens.push(u),
    );
    listen("wisspa://recording-armed", () => setArmed(true)).then((u) =>
      unlistens.push(u),
    );
    return () => unlistens.forEach((u) => u());
  }, []);

  return (
    <div className="h-screen w-screen flex items-center justify-center">
      <div className="flex items-center gap-2 rounded-full bg-black/75 px-4 py-2 backdrop-blur-md shadow-lg">
        <span
          className={`inline-block h-2.5 w-2.5 rounded-full ${
            armed ? "bg-red-500 wisspa-flash" : "bg-amber-400/70"
          }`}
        />
        <span className="text-white text-sm font-semibold tracking-wide">
          {armed ? "Listening" : "Wisspa"}
        </span>
      </div>
    </div>
  );
}
```

- [ ] **Step 2: Verify**

Run: `pnpm tsc --noEmit` — Expected: PASS.
Run: `pnpm build` — Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add src/components/RecordingOverlay.tsx
git commit -m "feat: overlay shows warming then armed state"
```

### Task 6: `ready_chime` toggle in General settings

**Files:**
- Modify: `src/components/settings/GeneralTab.tsx`

- [ ] **Step 1: Add the toggle under the sound volume slider**

In `GeneralTab.tsx`, inside the `{g.play_sounds && ( ... )}` block, after the sound-volume `Row`, add a second row. Change the block to render both rows (wrap in a fragment):

```tsx
      {g.play_sounds && (
        <>
          <Row label="Sound volume">
            <Slider
              value={g.sound_volume}
              min={0}
              max={1}
              step={0.05}
              onChange={(v) => patch({ sound_volume: v })}
              format={(v) => `${Math.round(v * 100)}%`}
            />
          </Row>
          <Row
            label="Ready chime"
            hint="Play a chime the moment the mic is live and ready for speech."
          >
            <Toggle
              checked={g.ready_chime}
              onChange={(v) => patch({ ready_chime: v })}
              label="Ready chime"
            />
          </Row>
        </>
      )}
```

- [ ] **Step 2: Verify**

Run: `pnpm tsc --noEmit` — Expected: PASS.
Run: `pnpm build` — Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add src/components/settings/GeneralTab.tsx
git commit -m "feat: ready chime toggle in General settings"
```

### Task 7: Manual verification of Layer 1

- [ ] Build and launch (`pnpm tauri build`, open the app).
- [ ] Press the dictation hotkey, speak immediately. Confirm: pill shows amber "Wisspa", then turns red "Listening" — speaking after red loses no words.
- [ ] Toggle "Ready chime" off in Settings. Confirm the flash still happens, no chime.
- [ ] Toggle it on, confirm the chime returns at the red transition (not at key-press).

---

## Layer 2 — Pre-warm (opt-in)

### Task 8: Verify the Input Monitoring permission requirement

- [ ] Confirm whether a listen-only `CGEventTap` for `flagsChanged` works under Wisspa's current grants, or triggers the Input Monitoring prompt. This decides whether the onboarding/permission step (Task 12) is needed. Update this plan with the finding before continuing.

### Task 9: `audio.ts` warm-stream slot

**Files:**
- Modify: `src/lib/audio.ts`

- [ ] **Step 1: Add a retained warm-stream slot and helpers**

Add a module-level `let warmStream: MediaStream | null = null;`. Add:

```ts
export async function warmMic(): Promise<void> {
  if (warmStream || (mediaRecorder && mediaRecorder.state === "recording")) {
    return;
  }
  try {
    warmStream = await acquireHealthyStream();
  } catch (err) {
    console.warn("warmMic failed:", err);
    warmStream = null;
  }
}

export function releaseWarmStream(): void {
  if (warmStream && warmStream !== activeStream) {
    warmStream.getTracks().forEach((t) => t.stop());
  }
  warmStream = null;
}
```

- [ ] **Step 2: Consume the warm stream in `startRecording()`**

In `startRecording()`, replace `const stream = await acquireHealthyStream();` with:

```ts
    let stream: MediaStream;
    if (warmStream && warmStream.getAudioTracks()[0]?.readyState === "live") {
      stream = warmStream;
      warmStream = null;
    } else {
      releaseWarmStream();
      stream = await acquireHealthyStream();
    }
```

- [ ] **Step 3: Release warm stream in `cancelRecording()`**

In `cancelRecording()`, add `releaseWarmStream();` before the final resets.

- [ ] **Step 4: Verify**

Run: `pnpm tsc --noEmit` — Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/lib/audio.ts
git commit -m "feat: warm-stream slot for instant capture start"
```

### Task 10: `prearm.rs` — macOS modifier monitor

**Files:**
- Create: `src-tauri/src/prearm.rs`
- Modify: `src-tauri/src/main.rs`, `src-tauri/src/lib.rs`, `src-tauri/Cargo.toml`

- [ ] **Step 1: Add the `core-graphics` and `core-foundation` deps**

In `Cargo.toml` `[dependencies]`, add `core-graphics = "0.24"` and `core-foundation = "0.10"` (align versions with whatever `cargo tree` already resolves to avoid duplicates).

- [ ] **Step 2: Implement the monitor**

Create `prearm.rs` with a `start(app: AppHandle, modifier_masks: Vec<CGEventFlags>)` that spawns a thread running a `CGEventTap` (listen-only, `kCGEventFlagsChanged`) on its own `CFRunLoop`. On a flags-changed event whose masked modifiers equal one of `modifier_masks`, `app.emit("wisspa://prewarm-mic", ())`. When the modifiers clear, start a ~1s timer; if no recording started, `app.emit("wisspa://prewarm-cancel", ())`. Enforce a ~4s max-warm timeout. Derive `modifier_masks` from the dictation/action/prompt hotkey strings in settings (modifier portion only); skip hotkeys with no modifier.

- [ ] **Step 3: Wire into setup**

In `main.rs` setup, if `settings.general.fast_recording_start`, call `prearm::start(...)`. Add `mod prearm;` to `lib.rs`/`main.rs` as the codebase pattern requires.

- [ ] **Step 4: Verify**

Run: `cd src-tauri && cargo check` — Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/prearm.rs src-tauri/src/main.rs src-tauri/src/lib.rs src-tauri/Cargo.toml
git commit -m "feat: macOS modifier monitor for mic pre-warm"
```

### Task 11: `App.tsx` pre-warm listeners

**Files:**
- Modify: `src/App.tsx`

- [ ] **Step 1: Listen for prewarm events**

In the listener `useEffect`, add:

```tsx
    listen("wisspa://prewarm-mic", () => {
      void import("./lib/audio").then((m) => m.warmMic());
    }).then(track);

    listen("wisspa://prewarm-cancel", () => {
      void import("./lib/audio").then((m) => m.releaseWarmStream());
    }).then(track);
```

- [ ] **Step 2: Verify**

Run: `pnpm tsc --noEmit` — Expected: PASS.
Run: `pnpm build` — Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add src/App.tsx
git commit -m "feat: warm mic on prewarm events"
```

### Task 12: `fast_recording_start` toggle + permission guidance

**Files:**
- Modify: `src/components/settings/GeneralTab.tsx`
- Modify: `src-tauri/Info.plist` (if Task 8 confirms Input Monitoring is required)

- [ ] **Step 1: Add the toggle**

Add a `Row` for `fast_recording_start` with a `Toggle` bound to `g.fast_recording_start`, hint: "Warm the mic when you press the hotkey's modifier so recording starts instantly. Needs macOS Input Monitoring permission; the mic indicator appears as you reach for the key." Note in the hint that a restart applies the change (the monitor is set up at launch).

- [ ] **Step 2: Verify**

Run: `pnpm tsc --noEmit && pnpm build` — Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add src/components/settings/GeneralTab.tsx src-tauri/Info.plist
git commit -m "feat: fast recording start toggle"
```

### Task 13: Manual verification of Layer 2

- [ ] Enable "Fast recording start", restart, grant Input Monitoring when prompted.
- [ ] Press and hold the hotkey modifier alone — the mic indicator appears; release — it clears within ~1s.
- [ ] Complete the hotkey — capture is instant, no startup delay before the red cue.
- [ ] Disable the setting / deny the permission — confirm Layer 1 still works and nothing errors.
