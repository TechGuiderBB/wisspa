import { create } from "zustand";

type RecordingState = {
  isRecording: boolean;
  lastTranscript: string | null;
  lastError: string | null;
  setRecording: (v: boolean) => void;
  setTranscript: (t: string | null) => void;
  setError: (e: string | null) => void;
};

export const useRecording = create<RecordingState>((set) => ({
  isRecording: false,
  lastTranscript: null,
  lastError: null,
  setRecording: (v) => set({ isRecording: v }),
  setTranscript: (t) => set({ lastTranscript: t }),
  setError: (e) => set({ lastError: e }),
}));
