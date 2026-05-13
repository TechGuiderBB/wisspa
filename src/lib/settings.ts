import { invoke } from "@tauri-apps/api/core";

export type RecordingMode = "press_and_hold" | "toggle";
export type Theme = "system" | "light" | "dark";

export type Settings = {
  version: number;
  general: {
    launch_on_login: boolean;
    show_overlay: boolean;
    recording_mode: RecordingMode;
    play_sounds: boolean;
    sound_volume: number;
    theme: Theme;
  };
  hotkeys: {
    dictation: string;
    action: string;
    prompt: string;
    cancel: string;
  };
  prompt_mode: {
    include_selected_text: boolean;
    show_preview: boolean;
    preview_timeout_seconds: number;
    manual_app_override: string | null;
  };
  stt: { provider: string; model: string; language: string };
  cleanup_llm: { provider: string; model: string };
  prompt_llm: { provider: string; model: string };
  onboarding_completed: boolean;
};

export const HOTKEY_ACTIONS = ["dictation", "action", "prompt", "cancel"] as const;
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
