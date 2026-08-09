import { invoke } from "@tauri-apps/api/core";

export type RecordingMode = "press_and_hold" | "toggle";
export type MicSensitivity = "off" | "low" | "medium" | "high";

export type VocabEntry = {
  spoken: string;
  replace_with: string;
};

export type AppProfile = {
  /** Substring matched case-insensitively against the detected active-app name. First match wins. */
  app: string;
  /** Free-text tone guidance appended to the dictation cleanup prompt (e.g. "casual, no greetings"). */
  tone: string;
  /** Extra vocabulary words for this app; take precedence over global entries on conflict. */
  vocab: string[];
};

export type VocabImportSkip = {
  line: number;
  reason: string;
};

export type VocabImport = {
  to_add: VocabEntry[];
  skipped: VocabImportSkip[];
  already_existing: number;
};

/**
 * Parse a `spoken,replacement` CSV (header optional) into an import preview.
 * Passes `csv_text` explicitly in snake_case so the Rust↔JS contract is visible
 * at the call site and does not rely on Tauri's implicit camelCase→snake_case
 * argument conversion (aligns with `reportRecordingTimeout`).
 */
export async function importVocabularyCsv(
  csvText: string,
  existing: VocabEntry[],
): Promise<VocabImport> {
  return await invoke<VocabImport>("import_vocabulary_csv", { csv_text: csvText, existing });
}

export const SENSITIVITY_MULTIPLIER: Record<MicSensitivity, number> = {
  off: 0, // 0 == disable silence guard entirely (sentinel value)
  // The multiplier scales BOTH silence thresholds, so >1 tightens suppression
  // and <1 loosens it. Direction matches the Settings UI hint ("Higher =
  // looser"): high sensitivity halves the thresholds so quiet speakers aren't
  // discarded; low doubles them for noisy environments.
  low: 2.0,
  medium: 1.0,
  high: 0.5,
};

export const DEFAULT_SILENCE_PEAK = 6;
// Byte-rate floor for the silence guard when the mic hasn't been calibrated.
// Quiet-speech opus encodes at roughly 2–4 KB/s, so the old 2000 B/s default
// overlapped real speech and suppressed quiet speakers. 800 B/s sits safely
// below that band.
export const DEFAULT_MIN_BYTES_PER_SECOND = 800;

export type MicCalibration = {
  silence_peak: number;
  min_bytes_per_second: number;
  calibrated_at: number;
};

export type Settings = {
  version: number;
  general: {
    launch_on_login: boolean;
    show_overlay: boolean;
    recording_mode: RecordingMode;
    play_sounds: boolean;
    sound_volume: number;
    mic_sensitivity: MicSensitivity;
    notes_path: string;
    max_recording_seconds: number;
    ready_chime: boolean;
    dictation_complete_sound: boolean;
    fast_recording_start: boolean;
    quiet_notifications: boolean;
    verbose_logging: boolean;
    input_device_id: string;
    auto_update_check: boolean;
  };
  hotkeys: {
    dictation: string;
    action: string;
    prompt: string;
    command: string;
    cancel: string;
  };
  prompt_mode: {
    include_selected_text: boolean;
    show_preview: boolean;
    preview_timeout_seconds: number;
    manual_app_override: string | null;
    review_before_insert: boolean;
    user_profile: string;
    adaptive_refine: boolean;
  };
  dictation: {
    review_before_insert: boolean;
  };
  stt: { provider: string; model: string; language: string };
  cleanup_llm: { provider: string };
  prompt_llm: { provider: string };
  onboarding_completed: boolean;
  mic_calibration: MicCalibration | null;
  vocabulary: VocabEntry[];
  word_corrections: WordCorrections;
  profiles: AppProfile[];
};

export const HOTKEY_ACTIONS = ["dictation", "action", "prompt", "command", "cancel"] as const;
export type HotkeyAction = (typeof HOTKEY_ACTIONS)[number];

export const API_PROVIDERS = ["groq", "anthropic"] as const;
export type ApiProvider = (typeof API_PROVIDERS)[number];

export const API_KEY_NAMES: Record<ApiProvider, string> = {
  groq: "GROQ_API_KEY",
  anthropic: "ANTHROPIC_API_KEY",
};

export async function getSettings(): Promise<Settings> {
  return await invoke<Settings>("get_settings");
}

export async function saveSettings(settings: Settings): Promise<void> {
  await invoke("save_settings", { settings });
}

/** Toast that an app update is available (honours quiet notifications). */
export async function notifyUpdateAvailable(version: string): Promise<void> {
  await invoke("notify_update_available", { version });
}

export async function getApiKeyPresent(name: string): Promise<boolean> {
  return await invoke<boolean>("get_api_key_present", { key: name });
}

export async function saveApiKey(name: string, value: string): Promise<void> {
  await invoke("save_api_key", { key: name, value });
}

export async function testApiKey(
  provider: ApiProvider,
  value: string,
): Promise<string> {
  return await invoke<string>("test_api_key", { provider, value });
}

export async function updateHotkey(
  action: HotkeyAction,
  combo: string,
): Promise<void> {
  await invoke("update_hotkey", { action, combo });
}

export async function pauseHotkeys(): Promise<void> {
  await invoke("pause_hotkeys");
}

export async function resumeHotkeys(): Promise<void> {
  await invoke("resume_hotkeys");
}

export type PermissionStatus = "unknown" | "granted" | "denied";
export type PermissionPane =
  | "microphone"
  | "accessibility"
  | "screen_recording"
  | "automation";

export type PermissionsSnapshot = {
  microphone: PermissionStatus;
  accessibility: PermissionStatus;
  screen_recording: PermissionStatus;
  automation: PermissionStatus;
};

export async function getPermissions(): Promise<PermissionsSnapshot> {
  return await invoke<PermissionsSnapshot>("get_permissions");
}

export async function reportMicrophoneStatus(granted: boolean): Promise<void> {
  await invoke("report_microphone_status", { granted });
}

export async function openSystemSettings(pane: PermissionPane): Promise<void> {
  await invoke("open_system_settings", { pane });
}

export async function requestScreenRecordingAccess(): Promise<void> {
  await invoke("request_screen_recording_access");
}

export async function completeOnboarding(): Promise<void> {
  await invoke("complete_onboarding");
}

export type HistoryEntry = {
  id: number;
  timestamp: number;
  mode: string;
  active_app: string | null;
  raw_transcript: string;
  output: string | null;
  action_id: string | null;
  duration_ms: number | null;
  status: string;
  /** Anthropic token usage (summed when a mode made two LLM calls); null when none was captured. */
  input_tokens: number | null;
  output_tokens: number | null;
};

export async function getHistory(limit = 100): Promise<HistoryEntry[]> {
  return await invoke<HistoryEntry[]>("get_history", { limit });
}

export async function clearHistory(): Promise<void> {
  await invoke("clear_history");
}

export async function exportHistoryCsv(): Promise<string> {
  return await invoke<string>("export_history_csv");
}

export async function reinjectText(text: string): Promise<void> {
  await invoke("reinject_text", { text });
}

export type LoadedAction = {
  id: string;
  name: string;
  description: string;
  triggers: string[];
  type: string;
  command: string;
  destructive: boolean;
  enabled: boolean;
};

export async function listActions(): Promise<LoadedAction[]> {
  return await invoke<LoadedAction[]>("list_actions");
}

export async function reportSilentRecording(
  mode: string,
  durationMs: number,
  peakAmplitude: number,
  bytes: number,
  session: number,
): Promise<void> {
  await invoke("report_silent_recording", {
    mode,
    durationMs,
    peakAmplitude,
    bytes,
    // Session id lets the backend retire the recording session on this
    // terminal path (no process_audio call follows), keeping the Esc guard's
    // active-session check honest.
    session,
  });
}

export type WordCorrectionEntry = {
  replacement: string;
  count: number;
  auto_apply: boolean;
};

export type WordCorrections = {
  enabled: boolean;
  threshold: number;
  entries: Record<string, WordCorrectionEntry>;
  learn_from_edits: boolean;
};

export async function getWordCorrections(): Promise<WordCorrections> {
  return await invoke<WordCorrections>("get_word_corrections");
}

export async function submitWordCorrection(
  original: string,
  replacement: string,
): Promise<boolean> {
  return await invoke<boolean>("submit_word_correction", { original, replacement });
}

export async function saveWordCorrections(
  corrections: WordCorrections,
): Promise<void> {
  await invoke("save_word_corrections", { corrections });
}

export async function reportRecordingTimeout(
  maxSeconds: number,
  session: number,
): Promise<void> {
  // Send the field name explicitly in snake_case so this binding does not rely
  // on Tauri's implicit camelCase→snake_case argument conversion. Removes a
  // class of confusing "missing field max_seconds" runtime failures and makes
  // the Rust↔JS contract obvious in either direction. `session` lets the
  // backend retire the recording session on this terminal path.
  await invoke("report_recording_timeout", { max_seconds: maxSeconds, session });
}

/**
 * Resolve effective silence thresholds for the current recording, applying
 * the user's sensitivity multiplier on top of their calibration (or the
 * baked-in defaults).  Returns null when sensitivity is "off" — silence guard
 * is skipped entirely in that mode.
 */
export function resolveSilenceThresholds(
  settings: Settings | null,
): { peak: number; bytesPerSecond: number } | null {
  const sens = settings?.general.mic_sensitivity ?? "medium";
  if (sens === "off") return null;
  const multiplier = SENSITIVITY_MULTIPLIER[sens] ?? 1;
  const cal = settings?.mic_calibration ?? null;
  const peak = (cal?.silence_peak ?? DEFAULT_SILENCE_PEAK) * multiplier;
  const bytesPerSecond =
    (cal?.min_bytes_per_second ?? DEFAULT_MIN_BYTES_PER_SECOND) * multiplier;
  return { peak, bytesPerSecond };
}
