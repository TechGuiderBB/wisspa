# Auto-learn word corrections from post-paste edits

**Feature:** Dictation mode observes the focused field 8 seconds after a paste and feeds any single-word corrections the user made into the existing `WordCorrections` learning system.

---

## How it works (end to end)

1. Wisspa pastes `final_text` into the focused field via `dictation::run`.
2. If the user has opted in (`word_corrections.enabled && word_corrections.learn_from_edits == true`), `dictation::run` spawns a detached background task via `tauri::async_runtime::spawn`.
3. The task sleeps for `EDIT_SNAPSHOT_DELAY_SECS` (8 seconds), then re-checks the frontmost app. If focus has moved, it stops quietly.
4. It calls `ax_snapshot::focused_field_value()` to read the focused field's current value once via the macOS Accessibility API.
5. It calls `learning::diff_corrections(&final_text, &observed, &raw_transcript)` to extract genuine single-word corrections.
6. For each candidate, it calls `settings_store::record_correction`, which increments the count and flips `auto_apply` once the configured threshold is reached.
7. When a correction crosses the auto-apply threshold for the first time, one toast is shown: `Wisspa will now auto-correct "lisa" → "LeaseR"`.

The background task changes nothing about `run`'s return value or timing — it returns immediately as before.

---

## Design decisions

### Dictation mode only
Action mode injects action results and Prompt mode injects a rewritten prompt; neither is a transcription to correct. The snapshot task is spawned only from `dictation.rs`.

### Single snapshot at a fixed 8-second delay
Defined as `EDIT_SNAPSHOT_DELAY_SECS: u64 = 8`. Long enough for the user to read a short dictation and fix a wrong word; short enough that they usually haven't written paragraphs more. Declared as a named constant so it is easy to tune.

### Opt-in, default OFF
Reading another app's focused-field contents after every dictation is privacy-sensitive. `learn_from_edits: bool` defaults to `false`. The observed text is used only transiently for the diff and is never persisted or sent anywhere.

### AX snapshot: `ax_snapshot.rs`
Uses the macOS Accessibility API (`AXUIElementCreateSystemWide`, `AXUIElementCopyAttributeValue`) via raw `extern "C"` declarations, following the style of `injector.rs`. All returned CF objects are wrapped with `core-foundation`'s `TCFType::wrap_under_create_rule` so they release correctly on drop. Password fields (`AXSecureTextField`) are skipped. Web views and Electron apps expose no usable `AXValue` — that is an expected miss, logged at debug level. Non-macOS builds get a stub returning `None`.

### Diff logic: `learning.rs`
Pure, unit-testable functions with no Tauri dependency. Uses the `similar` crate for token-level diffing of word slices. Conservative filters applied in order:

1. **Overlap gate** — ≥ 60% of `inserted` tokens must survive unchanged; otherwise the field is not a lightly-edited copy (focus moved, user wrote more, etc.).
2. **Substitution count gate** — only 1–3 total 1-for-1 substitutions accepted; more means a rewrite, not a correction.
3. **Per-candidate filters** — both tokens alphabetic, non-empty; differ by more than case; Levenshtein distance ≤ `max(2, corrected.len() / 2)` (phonetically/visually close); `heard` appears as a whole word in `raw_transcript` (corrections key off the raw Whisper text because `apply_corrections` runs before Haiku).

### Toast threshold: first crossing only
`settings_store::record_correction` returns `(now_auto, just_crossed)`. The background task toasts only when `just_crossed == true` — i.e., the correction crossed the threshold on this specific call. No toast on every learn, and no repeat toast once already active.

### The `record_correction` helper
Factored into `settings_store.rs` so both `commands::submit_word_correction` (the Tauri command) and the auto-learn background task share the same implementation. The command returns `now_auto` (backward-compatible); the background task uses `just_crossed`.

### Fail-safe error paths
Any AX error, missing `AXValue`, focus change, or settings load error in the background task results in "learn nothing" — never a crash, never user-visible noise. All failures are logged at `debug` or `warn` level.

---

## New files

| File | Role |
|---|---|
| `src-tauri/src/ax_snapshot.rs` | macOS AX snapshot; returns `Option<FocusedField>` |
| `src-tauri/src/learning.rs` | Pure diff logic; `diff_corrections` + unit tests |

## Modified files

| File | Change |
|---|---|
| `src-tauri/Cargo.toml` | +`core-foundation = "0.10"`, +`similar = "2"` |
| `src-tauri/src/settings_store.rs` | +`learn_from_edits` field; +`record_correction` helper |
| `src-tauri/src/commands.rs` | `submit_word_correction` now delegates to `record_correction` |
| `src-tauri/src/modes/dictation.rs` | Spawns `snapshot_and_learn` task when opted in |
| `src-tauri/src/main.rs` | +`mod ax_snapshot; mod learning;` |
| `src/lib/settings.ts` | +`learn_from_edits: boolean` in `WordCorrections` type |
| `src/components/settings/CorrectionsTab.tsx` | +Toggle row for `learn_from_edits` |
