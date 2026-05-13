import { useEffect } from "react";
import { listen } from "@tauri-apps/api/event";
import {
  startRecording,
  stopRecording,
  cancelRecording,
  blobToBase64,
} from "./lib/audio";
import { processAudio, type RecordingMode } from "./lib/tauri";
import { useRecording } from "./store/recording";
import RecordingOverlay from "./components/RecordingOverlay";
import SettingsPage from "./pages/Settings";
import OnboardingPage from "./pages/Onboarding";

const START_EVENT = "wisspa://start-recording";
const STOP_EVENT = "wisspa://stop-recording";
const CANCEL_EVENT = "wisspa://cancel-recording";
const MODE_EVENT = "wisspa://recording-mode";

const HASH = typeof window !== "undefined" ? window.location.hash : "";
const isOverlayWindow = HASH === "#overlay";
const isSettingsWindow = HASH === "#settings";
const isOnboardingWindow = HASH === "#onboarding";

export default function App() {
  if (isOverlayWindow) {
    return <RecordingOverlay />;
  }
  if (isSettingsWindow) {
    return <SettingsPage />;
  }
  if (isOnboardingWindow) {
    return <OnboardingPage />;
  }
  return <Runtime />;
}

function Runtime() {
  const { lastError, setRecording, setTranscript, setError } = useRecording();
  // Mode the in-flight recording is for. Set from `wisspa://recording-mode`
  // emitted by Rust at hotkey-press time; read at hotkey-release time.
  const modeRef = (Runtime as unknown as { _modeRef?: { current: RecordingMode } })
    ._modeRef ??
    ((Runtime as unknown as { _modeRef: { current: RecordingMode } })._modeRef = {
      current: "dictation",
    });

  useEffect(() => {
    // Eagerly request mic permission so macOS shows its prompt at launch
    // instead of waiting for the first hotkey press. Also report the result
    // to Rust so the permissions panel reflects current state.
    navigator.mediaDevices
      .getUserMedia({ audio: true })
      .then((stream) => {
        stream.getTracks().forEach((t) => t.stop());
        console.log("mic permission granted");
        void import("./lib/settings").then((m) => m.reportMicrophoneStatus(true));
      })
      .catch((err) => {
        console.error("mic permission failed:", err);
        setError(`mic permission: ${String(err)}`);
        void import("./lib/settings").then((m) => m.reportMicrophoneStatus(false));
      });
  }, [setError]);

  useEffect(() => {
    const unlistens: Array<() => void> = [];

    listen<string>(MODE_EVENT, (e) => {
      const m = e.payload as RecordingMode;
      if (m === "dictation" || m === "action" || m === "prompt") {
        modeRef.current = m;
      }
    }).then((u) => unlistens.push(u));

    listen(START_EVENT, async () => {
      try {
        setError(null);
        await startRecording();
        setRecording(true);
      } catch (err) {
        setError(String(err));
        setRecording(false);
      }
    }).then((u) => unlistens.push(u));

    listen(STOP_EVENT, async () => {
      try {
        const blob = await stopRecording();
        setRecording(false);
        if (!blob || blob.size === 0) return;
        const b64 = await blobToBase64(blob);
        const transcript = await processAudio(
          b64,
          blob.type || "audio/webm",
          modeRef.current,
        );
        setTranscript(transcript);
      } catch (err) {
        setError(String(err));
        setRecording(false);
      }
    }).then((u) => unlistens.push(u));

    listen(CANCEL_EVENT, () => {
      cancelRecording();
      setRecording(false);
    }).then((u) => unlistens.push(u));

    return () => {
      unlistens.forEach((u) => u());
    };
  }, [setError, setRecording, setTranscript]);

  return (
    <div className="h-screen w-screen flex items-center justify-center">
      <div className="flex items-center gap-2 rounded-full bg-black/60 px-4 py-2 backdrop-blur-md shadow-lg">
        <span className="inline-block h-2.5 w-2.5 rounded-full bg-white/40" />
        <span className="text-white/80 text-sm font-medium tracking-wide">
          Wisspa
        </span>
        {lastError && (
          <span className="text-red-400 text-[10px] truncate max-w-[80px]">
            {lastError}
          </span>
        )}
      </div>
    </div>
  );
}
