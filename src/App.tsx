import { useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { check } from "@tauri-apps/plugin-updater";
import {
  startRecording,
  stopRecording,
  cancelRecording,
  blobToBase64,
  warmMic,
  releaseWarmStream,
  MicTrackUnhealthyError,
} from "./lib/audio";
import { processAudio, type RecordingMode } from "./lib/tauri";
import {
  getSettings,
  notifyUpdateAvailable,
  reportRecordingTimeout,
  reportSilentRecording,
  resolveSilenceThresholds,
  type Settings,
} from "./lib/settings";
import {
  type PromptRoute,
  PROMPT_ROUTE_EVENT,
  routeLabel,
  routeBadge,
} from "./lib/promptRoute";
import { useRecording } from "./store/recording";
import RecordingOverlay from "./components/RecordingOverlay";
import PromptReview from "./components/PromptReview";
import SettingsPage from "./pages/Settings";
import OnboardingPage from "./pages/Onboarding";

const START_EVENT = "wisspa://start-recording";
const STOP_EVENT = "wisspa://stop-recording";
const CANCEL_EVENT = "wisspa://cancel-recording";
const MODE_EVENT = "wisspa://recording-mode";
const STATUS_EVENT = "wisspa://recording-status";
const PREWARM_EVENT = "wisspa://prewarm-mic";
const PREWARM_CANCEL_EVENT = "wisspa://prewarm-cancel";

type StatusFlash =
  | { kind: "no-speech" | "error"; message: string }
  | { kind: "route"; message: string; tone: "ai" | "content" | "neutral" };

const HASH = typeof window !== "undefined" ? window.location.hash : "";
const isOverlayWindow = HASH === "#overlay";
const isSettingsWindow = HASH === "#settings";
const isOnboardingWindow = HASH === "#onboarding";
const isReviewWindow = HASH === "#review";

export default function App() {
  if (isOverlayWindow) {
    return <RecordingOverlay />;
  }
  if (isReviewWindow) {
    return <PromptReview />;
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
  // Processing state: true while awaiting STT/LLM after recording stops.
  const [isProcessing, setIsProcessing] = useState(false);
  const [processingMode, setProcessingMode] = useState<RecordingMode>("dictation");
  // Elapsed seconds of the in-flight processing pipeline, shown next to the
  // "Transcribing..." / "Writing prompt..." label so a slow STT/LLM call
  // reads as progress rather than a hang.
  const [processingElapsedSec, setProcessingElapsedSec] = useState(0);
  const processingStartRef = useRef<number | null>(null);
  const flashTimerRef = useRef<number | null>(null);
  // Auto-stop timer for in-progress recordings — guardrail against
  // accidentally long captures (hotkey held while typing, etc).
  const recordingTimerRef = useRef<number | null>(null);
  // Monotonic id for processAudio pipelines. A newer STOP bumps it; a stale
  // completion checks it before applying transcript / clearing isProcessing.
  const processIdRef = useRef(0);
  // Backend session id for the in-flight recording, minted by Rust on hotkey
  // press and delivered in the START payload. Passed to process_audio so the
  // backend can cancel/supersede the pipeline (issue #31). 0 = no recording.
  const sessionRef = useRef(0);
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

  // Processing elapsed timer: starts when a processAudio pipeline begins
  // (setIsProcessing(true) right before the invoke) and stops on any path
  // that clears it — completion, error, or Esc cancel. 250 ms tick keeps the
  // displayed second within a quarter second of wall time.
  useEffect(() => {
    if (!isProcessing) {
      processingStartRef.current = null;
      return;
    }
    processingStartRef.current = performance.now();
    setProcessingElapsedSec(0);
    const id = window.setInterval(() => {
      if (processingStartRef.current !== null) {
        setProcessingElapsedSec(
          Math.floor((performance.now() - processingStartRef.current) / 1000),
        );
      }
    }, 250);
    return () => window.clearInterval(id);
  }, [isProcessing]);

  useEffect(() => {
    // Eagerly request mic permission so macOS shows its prompt at launch
    // instead of waiting for the first hotkey press. Also report the result
    // to Rust so the permissions panel reflects current state.
    navigator.mediaDevices
      .getUserMedia({ audio: true })
      .then((stream) => {
        stream.getTracks().forEach((t) => t.stop());
        void import("./lib/settings").then((m) => m.reportMicrophoneStatus(true));
      })
      .catch((err) => {
        console.error("mic permission failed:", err);
        setError(`mic permission: ${String(err)}`);
        void import("./lib/settings").then((m) => m.reportMicrophoneStatus(false));
      });
  }, [setError]);

  // Silent update check shortly after launch. Delayed so it doesn't contend
  // with startup work (mic permission prompt, hotkey registration, tray). An
  // available update only raises a native toast pointing at Settings → About
  // — it is never auto-downloaded. Any failure (offline, endpoint down,
  // unsigned dev build) is a debug log only.
  useEffect(() => {
    const id = window.setTimeout(() => {
      void (async () => {
        try {
          const s = await getSettings();
          if (!s.general.auto_update_check) return;
          const update = await check();
          if (update) {
            console.debug(`update available: v${update.version}`);
            await notifyUpdateAvailable(update.version);
          }
        } catch (err) {
          console.debug("auto update check failed:", err);
        }
      })();
    }, 15_000);
    return () => window.clearTimeout(id);
  }, []);

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

    listen<{ mode: RecordingMode; session: number }>(START_EVENT, async (e) => {
      // Mode + session arrive together in one event so this recording is bound
      // to the right mode and a unique backend session (issue #31).
      const payload = e.payload;
      if (
        payload?.mode === "dictation" ||
        payload?.mode === "action" ||
        payload?.mode === "prompt"
      ) {
        modeRef.current = payload.mode;
      }
      sessionRef.current = payload?.session ?? 0;
      try {
        setError(null);
        await startRecording(settingsRef.current?.general.input_device_id || undefined);
        setRecording(true);
        // Arm the max-recording-duration timer so a stuck-down hotkey
        // (or modifier+key combo held during typing) can't capture
        // unbounded audio.
        const maxSec = settingsRef.current?.general.max_recording_seconds ?? 30;
        if (recordingTimerRef.current !== null) {
          clearTimeout(recordingTimerRef.current);
        }
        recordingTimerRef.current = window.setTimeout(async () => {
          recordingTimerRef.current = null;
          console.warn(`recording exceeded ${maxSec}s cap — auto-stopping`);
          cancelRecording();
          setRecording(false);
          try {
            await reportRecordingTimeout(maxSec, sessionRef.current);
          } catch (err) {
            console.error("reportRecordingTimeout failed:", err);
          }
        }, maxSec * 1000);
      } catch (err) {
        const msg =
          err instanceof MicTrackUnhealthyError
            ? "Mic unavailable — check input device"
            : String(err);
        setError(msg);
        setRecording(false);
      }
    }).then(track);

    listen<{ mode: RecordingMode; session: number }>(STOP_EVENT, async (e) => {
      if (recordingTimerRef.current !== null) {
        clearTimeout(recordingTimerRef.current);
        recordingTimerRef.current = null;
      }
      // Bind to the mode + session carried by THIS stop event (the one its own
      // press began), not the refs — a second recording hotkey pressed before
      // this one was released would have overwritten the refs (issue #31).
      // Fall back to the refs if an older backend omitted the payload.
      const payload = e.payload;
      const mode: RecordingMode =
        payload?.mode === "dictation" || payload?.mode === "action" || payload?.mode === "prompt"
          ? payload.mode
          : modeRef.current;
      const session = payload?.session ?? sessionRef.current;
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
          // AND, not OR: discard only when BOTH signals look silent. With OR,
          // a quiet voice (low peak) or efficient low-bitrate encoding of soft
          // speech (low B/s) alone could nuke a real dictation. The backend
          // confidence gate now handles noise-induced hallucinations, so this
          // guard only needs to catch recordings silent on every signal.
          if (peakSilent && dataSilent) {
            console.warn(
              `silent recording suppressed: peak=${peakAmplitude.toFixed(2)} ` +
                `bytes=${blob.size} duration=${durationMs}ms ` +
                `(thresholds peak=${thresholds.peak.toFixed(2)} bps=${thresholds.bytesPerSecond.toFixed(0)})`,
            );
            await reportSilentRecording(
              mode,
              durationMs,
              peakAmplitude,
              blob.size,
              session,
            );
            return;
          }
        }

        const b64 = await blobToBase64(blob);
        // Bump the pipeline id so an earlier in-flight processAudio (user
        // started a second recording before this one finished) cannot apply
        // its stale transcript or clear isProcessing out of order.
        const processId = ++processIdRef.current;
        setIsProcessing(true);
        setProcessingMode(mode);
        try {
          const transcript = await processAudio(
            b64,
            blob.type || "audio/webm",
            mode,
            session,
          );
          if (processId === processIdRef.current) {
            setTranscript(transcript);
          }
        } finally {
          if (processId === processIdRef.current) {
            setIsProcessing(false);
          }
        }
      } catch (err) {
        setError(String(err));
        setRecording(false);
      }
    }).then(track);

    listen(CANCEL_EVENT, () => {
      if (recordingTimerRef.current !== null) {
        clearTimeout(recordingTimerRef.current);
        recordingTimerRef.current = null;
      }
      // Bump the pipeline id so any in-flight processAudio that started before
      // Esc was pressed discards its result instead of injecting stale text.
      processIdRef.current++;
      cancelRecording();
      setRecording(false);
      setIsProcessing(false);
    }).then(track);

    // Opt-in pre-warm (fast_recording_start): the Rust modifier monitor warms
    // the mic when the hotkey's modifier is held, and releases it if the combo
    // is never completed.
    listen(PREWARM_EVENT, () => {
      void warmMic(settingsRef.current?.general.input_device_id || undefined);
    }).then(track);

    listen(PREWARM_CANCEL_EVENT, () => {
      releaseWarmStream();
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

    listen<PromptRoute>(PROMPT_ROUTE_EVENT, (e) => {
      const payload = e.payload;
      if (!payload) return;
      // Session guard: ignore stale events from superseded recordings.
      // session === 0 is legacy passthrough for older backend builds.
      if (payload.session !== 0 && payload.session !== sessionRef.current) return;
      // Post-result flash: only when the branch is resolved (Sonnet has returned).
      if (payload.branch !== "unknown") {
        const badge = routeBadge(payload);
        const label = routeLabel(payload);
        const message = `${badge.text} → ${label}`;
        setStatusFlash({ kind: "route", message, tone: badge.tone });
        if (flashTimerRef.current !== null) clearTimeout(flashTimerRef.current);
        flashTimerRef.current = window.setTimeout(() => {
          setStatusFlash(null);
          flashTimerRef.current = null;
        }, 2800);
      }
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

  // Pill state precedence: error flash > route flash > thinking > idle.
  // Recording state is shown by the overlay window which stacks on top.
  const flashing = statusFlash !== null;
  const thinking = isProcessing && !flashing;
  const thinkingLabel =
    processingMode === "prompt"
      ? "Writing prompt..."
      : processingMode === "action"
        ? "Running..."
        : "Transcribing...";

  // Background colour for the route flash adapts to the detected destination type.
  const flashBg =
    statusFlash?.kind === "route"
      ? statusFlash.tone === "content"
        ? "bg-emerald-600/80"
        : statusFlash.tone === "neutral"
          ? "bg-black/60"
          : "bg-violet-600/80"
      : "bg-amber-500/85";

  return (
    <div className="h-screen w-screen flex items-center justify-center">
      <div
        className={`flex items-center gap-2 rounded-full px-4 py-2 backdrop-blur-md shadow-lg transition-colors duration-200 ${
          flashing
            ? flashBg
            : thinking
              ? "bg-violet-600/80"
              : "bg-black/60"
        }`}
      >
        <span
          className={`inline-block h-2.5 w-2.5 rounded-full ${
            flashing
              ? "bg-white wisspa-flash"
              : thinking
                ? "bg-white wisspa-thinking"
                : "bg-white/40"
          }`}
        />
        <span
          className={`text-sm font-medium tracking-wide ${
            flashing || thinking ? "text-white" : "text-white/80"
          }`}
        >
          {flashing ? statusFlash!.message : thinking ? thinkingLabel : "Wisspa"}
        </span>
        {thinking && (
          <span className="text-white/70 text-xs tabular-nums">
            {processingElapsedSec}s
          </span>
        )}
        {!flashing && !thinking && lastError && (
          <span className="text-red-400 text-[10px] truncate max-w-[80px]">
            {lastError}
          </span>
        )}
      </div>
    </div>
  );
}
