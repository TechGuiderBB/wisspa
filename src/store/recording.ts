import { create } from "zustand";
import type { PromptRoute } from "../lib/promptRoute";

type RecordingState = {
  isRecording: boolean;
  lastTranscript: string | null;
  lastError: string | null;
  currentRoute: PromptRoute | null;
  setRecording: (v: boolean) => void;
  setTranscript: (t: string | null) => void;
  setError: (e: string | null) => void;
  setRoute: (r: PromptRoute | null) => void;
};

export const useRecording = create<RecordingState>((set) => ({
  isRecording: false,
  lastTranscript: null,
  lastError: null,
  currentRoute: null,
  setRecording: (v) => set({ isRecording: v }),
  setTranscript: (t) => set({ lastTranscript: t }),
  setError: (e) => set({ lastError: e }),
  setRoute: (r) => set({ currentRoute: r }),
}));
