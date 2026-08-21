# Wisspa

A local-first macOS voice app that turns held-key speech into text, prompts, edits, or actions in whatever app you are using. Its signature move is voice-to-good-prompt; it also dictates, rewrites selections, and runs voice-triggered actions.

## Capture Modes

**Prompt Mode**:
The mode that turns a rambled spoken intent into a well-formed prompt (or, for a non-AI destination, finished content) shaped for the app you are in. The reason the product exists.
_Avoid_: prompt rewrite, prompt engineering mode

**Dictation Mode**:
The mode that turns free speech into cleaned-up text inserted at the cursor, with filler removed, punctuation added, and tone matched to the destination app.
_Avoid_: speech-to-text, transcription mode, STT mode

**Command Mode**:
The mode that rewrites the user's current text selection in place from a spoken instruction such as "make this formal" or "translate to French". Meaningless without a selection.
_Avoid_: edit mode, rewrite mode, transform mode, selection mode

**Action Mode**:
The mode that maps a spoken phrase to a named, editable command that does something on the machine (take a screenshot, lock the screen, search GitHub) rather than producing text.
_Avoid_: command mode (that is a different mode), voice command mode, shortcut mode

**Cancel**:
The abort gesture (default `Esc`) that stops an in-progress recording before any transcription or model call, so nothing is charged or inserted.
_Avoid_: escape, stop, abort recording

## Voice & Transcription

**Whisper**:
The role of turning captured audio into a raw transcript. Runs on Groq's speech-to-text model.
_Avoid_: STT engine, speech recognizer, transcriber

**Haiku**:
The role of cleaning up dictation and applying Command Mode instructions. The fast, cheap text model in the pipeline.
_Avoid_: cleanup model, the small model

**Sonnet**:
The role of the Prompt Mode rewrite, including an optional second critique-and-revise pass for complex asks. The higher-capability text model in the pipeline.
_Avoid_: the big model, prompt model

**Transcript**:
The raw words Whisper produces from a recording, treated downstream as content rather than as instructions to the model.
_Avoid_: raw text, speech text, dictation output

**Divergence**:
The guardrail condition where Haiku's cleanup strays too far from the transcript (e.g. it answered a question instead of tidying words), causing the raw text to be used instead.
_Avoid_: hallucination, drift, model deviation

## Targeting & Context

**Press-Time Snapshot**:
The record of which app (and browser tab) was focused at the instant the hotkey was pressed, taken as the authoritative destination even if focus moves during recording.
_Avoid_: capture context, focus snapshot, active-window snapshot

**Target App**:
The app resolved as the destination for a capture, sourced from the press-time snapshot and falling back to the frontmost app if the snapshot is unavailable.
_Avoid_: active app, destination window

**Frontmost App**:
The app currently in front on screen, used only as the live fallback when no press-time snapshot exists.
_Avoid_: foreground app, current app

**Branch**:
The Prompt Mode split between an AI-tool destination, which receives a prompt, and a non-AI destination, which receives finished content. Determined by the target app or browser host.
_Avoid_: route, prompt/content split, path

**Pre-Warm**:
The opt-in behaviour that starts the microphone the moment a hotkey's modifier keys are held, so completing the combo begins capture instantly.
_Avoid_: pre-arm, mic warmup, fast start

## Hotkeys & Recording

**Hotkey**:
The reassignable keyboard combination that starts a given capture mode. Distinct from an Action's spoken trigger.
_Avoid_: shortcut, keybinding, key combo

**Recording Mode**:
Whether a hotkey is push-to-talk (hold to record, release to process) or toggle (press to start, press again to stop).
_Avoid_: capture style, input mode

**Pill**:
The small always-on Wisspa indicator at the top of the screen that flashes while recording. The whole interface until settings are needed.
_Avoid_: overlay badge, status dot, HUD

**Review Gate**:
The opt-in window that lets the user edit generated text before it is inserted, shared by Prompt Mode and dictation edit-before-insert.
_Avoid_: preview window, confirm dialog, edit-before-insert popup

## Actions

**Action**:
A single named voice command defined as one editable, hot-reloaded YAML file. The unit that Action Mode matches against.
_Avoid_: command, macro, shortcut

**Trigger**:
The leading spoken phrase that selects an Action (e.g. "take a note"). Distinct from a hotkey.
_Avoid_: hotword, wake phrase, keyword

**Query**:
The spoken remainder after an Action's trigger, used as that action's payload (e.g. the note body, the search term).
_Avoid_: argument, body, payload

**Action Registry**:
The live collection of all loaded Actions, file-watched so edits and additions apply without a restart.
_Avoid_: action list, command catalog

## Vocabulary & Learning

**Vocabulary**:
User-supplied words that bias transcription toward the right spelling and are protected from substitution rewrites.
_Avoid_: dictionary, custom words, terms list

**Correction**:
An auto-learned replacement that begins applying only after the user has made the same fix a set number of times.
_Avoid_: substitution, fix, word replacement

**Profile**:
A per-app bundle of tone and vocabulary for dictation, taking precedence over global vocabulary for its matched app.
_Avoid_: app preset, per-app setting, persona

## Licensing & Distribution

**BYO-API-Key**:
The model of the product being free, with each user supplying their own Groq and Anthropic keys stored in the macOS Keychain. As of 0.4.1 (#78) this is the only path — there is no paid tier.
_Avoid_: Supporter, supporter license, entitlement, license key, LemonSqueezy, free tier (all removed in 0.4.1)

**MIT License**:
The open-source terms Wisspa ships under; the whole ask is attribution.
_Avoid_: open source license, licence

**Local-First**:
The principle that there is no backend, account, telemetry, or analytics; the only network calls are Groq, Anthropic, and an optional update check, all made with the user's own keys.
_Avoid_: offline, privacy-first, no-cloud
