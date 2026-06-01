import { invoke } from "@tauri-apps/api/core";

export type RecordingMode = "dictation" | "action" | "prompt";

export async function processAudio(
  audioB64: string,
  mimeType: string,
  mode: RecordingMode,
  session: number,
): Promise<string> {
  return await invoke<string>("process_audio", { audioB64, mimeType, mode, session });
}
