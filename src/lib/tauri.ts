import { invoke } from "@tauri-apps/api/core";

export type RecordingMode = "dictation" | "action" | "prompt" | "command";

export async function processAudio(
  audioB64: string,
  mimeType: string,
  mode: RecordingMode,
  session: number,
): Promise<string> {
  return await invoke<string>("process_audio", { audioB64, mimeType, mode, session });
}

/** Insert the (possibly edited) reviewed prompt for the given recording. */
export async function submitPromptReview(
  session: number,
  text: string,
): Promise<void> {
  await invoke("submit_prompt_review", { session, text });
}

/** Cancel the reviewed prompt — nothing is pasted. */
export async function cancelPromptReview(session: number): Promise<void> {
  await invoke("cancel_prompt_review", { session });
}
