import { useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import {
  startRecording,
  stopRecording,
  cancelRecording,
  blobToBase64,
  MicTrackUnhealthyError,
} from "./lib/audio";
import { processAudio, type RecordingMode } from "./lib/tauri";
import {
  getSettings,
  reportSilentRecording,
  resolveSilenceThresholds,
  type Settings,
} from "./lib/settings";
import { useRecording } from "./store/recording";
import RecordingOverlay from "./components/RecordingOverlay";
import SettingsPage from "./pages/Settings";
import OnboardingPage from "./pages/Onboarding";

const START_EVENT = "wisspa://start-recording";
const STOP_EVENT = "wisspa://stop-recording";
const CANCEL_EVENT = "wisspa://cancel-recording";
const MODE_EVENT = "wisspa://recording-mode";
const STATUS_EVENT = "wisspa://recording-status";

type StatusFlash = {
  kind: "no-speech" | "error";
  message: string;
};

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
  // Transient warning state shown directly on the always-visible pill so the
  // user gets feedback even if they missed the macOS notification banner.
  const [statusFlash, setStatusFlash] = useState<StatusFlash | null>(null);
  const flashTimerRef = useRef<number | null>(null);
  // Mode the in-flight recording is for. Set from `wisspa://recording-mode`
  // emitted by Rust at hotkey-press time; read at hotkey-release time.
  const modeRef = (Runtime as unknown as { _modeRef?: { current: RecordingMode } })
    ._modeRef ??
    ((Runtime as unknown as { _modeRef: { current: RecordingMode } })._modeRef = {
      current: "dictation",
    });
  // Cached settings so the silence guard can read thresholds without an
  // invoke round-trip per recording. Refreshed on settings.json changes via
  // a 10 s poll (cheap; replaces a more complex event subscription).
  const settingsRef = useRef<Settings | null>(null);
  useEffect(() => {
    const fetchOnce = () =>
      getSettings()
        .then((s) => {
          settingsRef.current = s;
        })
        .catch(() => {});
    fetchOnce();
    const id = setInterval(fetchOnce, 10000);
    return () => clearInterval(id);
  }, []);

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
    // Cancellation-safe subscription. React 18 StrictMode mounts effects
    // twice in dev; if we just `unlistens.push(u)` inside the .then(), the
    // cleanup runs before the promise resolves, leaks the listener, and we
    // end up with parallel duplicate pipelines per hotkey press.
    let cancelled = false;
    const unlistens: Array<() => void> = [];
    const track = (u: () => void) => {
      if (cancelled) u();
      else unlistens.push(u);
    };

    listen<string>(MODE_EVENT, (e) => {
      const m = e.payload as RecordingMode;
      if (m === "dictation" || m === "action" || m === "prompt") {
        modeRef.current = m;
      }
    }).then(track);

    listen(START_EVENT, async () => {
      try {
        setError(null);
        await startRecording();
        setRecording(true);
      } catch (err) {
        const msg =
          err instanceof MicTrackUnhealthyError
            ? "Mic unavailable — check input device"
            : String(err);
        setError(msg);
        setRecording(false);
      }
    }).then(track);

    listen(STOP_EVENT, async () => {
      try {
        const result = await stopRecording();
        setRecording(false);
        if (!result || result.blob.size === 0) return;
        const { blob, peakAmplitude, durationMs } = result;

        // Layer 3: apply silence guard before calling Whisper.
        const thresholds = resolveSilenceThresholds(settingsRef.current);
        if (thresholds) {
          const bytesPerSecond = blob.size / Math.max(0.001, durationMs / 1000);
          const peakSilent = peakAmplitude < thresholds.peak;
          const dataSilent = bytesPerSecond < thresholds.bytesPerSecond;
          if (peakSilent || dataSilent) {
            console.warn(
              `silent recording suppressed: peak=${peakAmplitude.toFixed(2)} ` +
                `bytes=${blob.size} duration=${durationMs}ms ` +
                `(thresholds peak=${thresholds.peak.toFixed(2)} bps=${thresholds.bytesPerSecond.toFixed(0)})`,
            );
            await reportSilentRecording(
              modeRef.current,
              durationMs,
              peakAmplitude,
              blob.size,
            );
            return;
          }
        }

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
    }).then(track);

    listen(CANCEL_EVENT, () => {
      cancelRecording();
      setRecording(false);
    }).then(track);

    listen<StatusFlash>(STATUS_EVENT, (e) => {
      const payload = e.payload as StatusFlash;
      if (!payload || !payload.kind) return;
      setStatusFlash(payload);
      if (flashTimerRef.current !== null) {
        clearTimeout(flashTimerRef.current);
      }
      flashTimerRef.current = window.setTimeout(() => {
        setStatusFlash(null);
        flashTimerRef.current = null;
      }, 2800);
    }).then(track);

    return () => {
      cancelled = true;
      unlistens.forEach((u) => u());
      if (flashTimerRef.current !== null) {
        clearTimeout(flashTimerRef.current);
        flashTimerRef.current = null;
      }
    };
  }, [setError, setRecording, setTranscript]);

  // Pill state precedence: error flash > idle. Recording state is shown by
  // the overlay window which stacks on top, so the runtime pill only renders
  // idle + warning here.
  const flashing = statusFlash !== null;
  return (
    <div className="h-screen w-screen flex items-center justify-center">
      <div
        className={`flex items-center gap-2 rounded-full px-4 py-2 backdrop-blur-md shadow-lg transition-colors duration-200 ${
          flashing ? "bg-amber-500/85" : "bg-black/60"
        }`}
      >
        <span
          className={`inline-block h-2.5 w-2.5 rounded-full ${
            flashing ? "bg-white wisspa-flash" : "bg-white/40"
          }`}
        />
        <span
          className={`text-sm font-medium tracking-wide ${
            flashing ? "text-white" : "text-white/80"
          }`}
        >
          {flashing ? statusFlash!.message : "Wisspa"}
        </span>
        {!flashing && lastError && (
          <span className="text-red-400 text-[10px] truncate max-w-[80px]">
            {lastError}
          </span>
        )}
      </div>
    </div>
  );
}
