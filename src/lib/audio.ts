// Audio capture pipeline.
//
// Plan A (silence-stream resilience):
//   Layer 1: pre-flight track health check (readyState/muted) with 1 retry
//   Layer 2: live AnalyserNode peak tracking, 100ms sample interval
//   Returns { blob, peakAmplitude, durationMs } so App.tsx can apply the
//   silence guard before invoking processAudio.

import { emit } from "@tauri-apps/api/event";

export class MicTrackUnhealthyError extends Error {
  constructor(reason: string) {
    super(`Mic track unhealthy: ${reason}`);
    this.name = "MicTrackUnhealthyError";
  }
}

export type RecordingResult = {
  blob: Blob;
  peakAmplitude: number; // 0–128 (deviation from uint8 mid-point 128)
  durationMs: number;
};

const PREFERRED_MIME = "audio/webm;codecs=opus";
const SAMPLE_INTERVAL_MS = 100;

/// Live per-interval input peak (0–128 deviation from the uint8 midpoint),
/// broadcast from this window at the analyser's 10 Hz sample rate so the
/// recording overlay can render a level meter without a Rust round-trip
/// (same window-to-window pattern as `wisspa://recording-armed`). Also fires
/// during onboarding calibration samples — harmless, no listener is shown.
export const INPUT_LEVEL_EVENT = "wisspa://input-level";

let mediaRecorder: MediaRecorder | null = null;
let chunks: Blob[] = [];
let activeStream: MediaStream | null = null;
// Pre-warmed stream held open between a hotkey-modifier press and the full
// combo, so `startRecording` can skip the cold getUserMedia. See prearm.rs.
let warmStream: MediaStream | null = null;
// The device the retained warm stream was opened with. A warm stream from a
// previously selected input must not be promoted after the setting changes.
let warmDeviceId: string | undefined;
let analyserCtx: AudioContext | null = null;
let analyser: AnalyserNode | null = null;
let analyserBuffer: Uint8Array | null = null;
let analyserInterval: number | null = null;
let peakAmplitude = 0;
let startedAt = 0;
let stopPromise: Promise<RecordingResult | null> | null = null;
let stopResolver: ((r: RecordingResult | null) => void) | null = null;
let starting = false; // re-entrance guard — duplicate START events bail
let stopping = false; // re-entrance guard — duplicate STOP events bail

function pickMime(): string {
  if (typeof MediaRecorder === "undefined") return PREFERRED_MIME;
  if (MediaRecorder.isTypeSupported(PREFERRED_MIME)) return PREFERRED_MIME;
  if (MediaRecorder.isTypeSupported("audio/webm")) return "audio/webm";
  if (MediaRecorder.isTypeSupported("audio/mp4")) return "audio/mp4";
  return "";
}

/// Open an input stream on the preferred device. A saved device that has
/// since vanished (USB mic unplugged) must never hard-fail a recording:
/// NotFoundError/OverconstrainedError fall back to the system default input.
async function openInputStream(deviceId?: string): Promise<MediaStream> {
  if (!deviceId) {
    return navigator.mediaDevices.getUserMedia({ audio: true });
  }
  try {
    return await navigator.mediaDevices.getUserMedia({
      audio: { deviceId: { exact: deviceId } },
    });
  } catch (err) {
    if (
      err instanceof DOMException &&
      (err.name === "NotFoundError" || err.name === "OverconstrainedError")
    ) {
      console.warn(
        `selected input device unavailable, falling back to system default: ${err.message}`,
      );
      return navigator.mediaDevices.getUserMedia({ audio: true });
    }
    throw err;
  }
}

async function acquireHealthyStream(deviceId?: string): Promise<MediaStream> {
  const tryOnce = async (): Promise<MediaStream> => {
    const stream = await openInputStream(deviceId);
    const tracks = stream.getAudioTracks();
    if (tracks.length === 0) {
      stream.getTracks().forEach((t) => t.stop());
      throw new MicTrackUnhealthyError("no audio tracks");
    }
    const track = tracks[0];
    if (track.readyState !== "live") {
      stream.getTracks().forEach((t) => t.stop());
      throw new MicTrackUnhealthyError(`track readyState=${track.readyState}`);
    }
    if (track.muted) {
      stream.getTracks().forEach((t) => t.stop());
      throw new MicTrackUnhealthyError("track muted");
    }
    return stream;
  };

  try {
    return await tryOnce();
  } catch (firstErr) {
    if (!(firstErr instanceof MicTrackUnhealthyError)) throw firstErr;
    // One retry — sometimes the OS hands back a stale track on the first
    // call right after wake-from-sleep.
    console.warn("first stream attempt unhealthy, retrying:", firstErr.message);
    await new Promise((r) => setTimeout(r, 150));
    return await tryOnce();
  }
}

function startAnalyser(stream: MediaStream) {
  try {
    analyserCtx = new AudioContext();
    const source = analyserCtx.createMediaStreamSource(stream);
    analyser = analyserCtx.createAnalyser();
    analyser.fftSize = 1024;
    source.connect(analyser);
    analyserBuffer = new Uint8Array(analyser.fftSize);
    peakAmplitude = 0;
    analyserInterval = window.setInterval(() => {
      if (!analyser || !analyserBuffer) return;
      analyser.getByteTimeDomainData(analyserBuffer);
      let localMax = 0;
      for (let i = 0; i < analyserBuffer.length; i++) {
        const dev = Math.abs(analyserBuffer[i] - 128);
        if (dev > localMax) localMax = dev;
      }
      if (localMax > peakAmplitude) peakAmplitude = localMax;
      void emit(INPUT_LEVEL_EVENT, localMax);
    }, SAMPLE_INTERVAL_MS);
  } catch (err) {
    console.warn("AnalyserNode setup failed; continuing without peak tracking:", err);
    teardownAnalyser();
  }
}

function teardownAnalyser() {
  if (analyserInterval !== null) {
    clearInterval(analyserInterval);
    analyserInterval = null;
  }
  if (analyserCtx) {
    void analyserCtx.close().catch(() => {});
    analyserCtx = null;
  }
  analyser = null;
  analyserBuffer = null;
}

/// Open the mic stream ahead of a full hotkey press and retain it. No-op if a
/// warm stream already exists or a recording is in progress. Warms the same
/// input device `startRecording` would use.
export async function warmMic(deviceId?: string): Promise<void> {
  if (warmStream || (mediaRecorder && mediaRecorder.state === "recording")) {
    return;
  }
  try {
    const stream = await acquireHealthyStream(deviceId);
    // A recording may have started (cold) while getUserMedia was in flight.
    // If so this warm stream is redundant — stop it now rather than orphan an
    // open mic with no consumer, which would leave the indicator stuck on.
    if (
      warmStream ||
      starting ||
      activeStream ||
      (mediaRecorder && mediaRecorder.state === "recording")
    ) {
      stream.getTracks().forEach((t) => t.stop());
      return;
    }
    warmStream = stream;
    warmDeviceId = deviceId;
  } catch (err) {
    console.warn("warmMic failed:", err);
    warmStream = null;
    warmDeviceId = undefined;
  }
}

/// Release a retained warm stream (closes the mic, clears the macOS indicator).
/// Leaves a stream alone if it has already been promoted to the active
/// recording stream.
export function releaseWarmStream(): void {
  if (warmStream && warmStream !== activeStream) {
    warmStream.getTracks().forEach((t) => t.stop());
  }
  warmStream = null;
  warmDeviceId = undefined;
}

export async function startRecording(deviceId?: string): Promise<void> {
  if (starting) return;
  if (mediaRecorder && mediaRecorder.state === "recording") return;
  starting = true;
  try {
    let stream: MediaStream;
    if (
      warmStream &&
      warmDeviceId === deviceId &&
      warmStream.getAudioTracks()[0]?.readyState === "live"
    ) {
      stream = warmStream;
      warmStream = null;
      warmDeviceId = undefined;
    } else {
      releaseWarmStream();
      stream = await acquireHealthyStream(deviceId);
    }
    activeStream = stream;
    chunks = [];

    const mime = pickMime();
    mediaRecorder = mime
      ? new MediaRecorder(stream, { mimeType: mime })
      : new MediaRecorder(stream);

    startAnalyser(stream);
    startedAt = performance.now();

    mediaRecorder.ondataavailable = (e) => {
      if (e.data && e.data.size > 0) chunks.push(e.data);
    };

    mediaRecorder.onstart = () => {
      void emit("wisspa://recording-armed");
    };

    stopPromise = new Promise<RecordingResult | null>((resolve) => {
      stopResolver = resolve;
    });

    mediaRecorder.onstop = () => {
      const type = mediaRecorder?.mimeType || "audio/webm";
      const blob = new Blob(chunks, { type });
      const durationMs = Math.max(0, Math.round(performance.now() - startedAt));
      const result: RecordingResult = {
        blob,
        peakAmplitude,
        durationMs,
      };
      stopResolver?.(result);
      teardownAnalyser();
      activeStream?.getTracks().forEach((t) => t.stop());
      activeStream = null;
      mediaRecorder = null;
    };

    mediaRecorder.start();
  } finally {
    starting = false;
  }
}

export async function stopRecording(): Promise<RecordingResult | null> {
  // A second concurrent call must NOT also resolve the same pending promise;
  // that's what causes the double-paste / clipboard-race symptom.
  if (stopping) return null;
  if (!mediaRecorder) return null;
  if (mediaRecorder.state !== "recording") return null;
  stopping = true;
  try {
    const pending = stopPromise!;
    mediaRecorder.stop();
    const result = await pending;
    stopPromise = null;
    stopResolver = null;
    if (!result) return null;
    return result;
  } finally {
    stopping = false;
  }
}

export function cancelRecording(): void {
  if (!mediaRecorder) return;
  // Unblock any stopRecording() that is awaiting the stop promise.
  // Without this, a CANCEL_EVENT that races a STOP_EVENT leaves stopRecording
  // hanging forever because onstop is cleared before MediaRecorder fires it.
  stopResolver?.(null);
  try {
    mediaRecorder.ondataavailable = null;
    mediaRecorder.onstop = null;
    if (mediaRecorder.state === "recording") mediaRecorder.stop();
  } catch {
    // best effort
  }
  teardownAnalyser();
  activeStream?.getTracks().forEach((t) => t.stop());
  activeStream = null;
  releaseWarmStream();
  mediaRecorder = null;
  chunks = [];
  stopPromise = null;
  stopResolver = null;
  peakAmplitude = 0;
  startedAt = 0;
  starting = false;
  stopping = false;
}

export async function blobToBase64(blob: Blob): Promise<string> {
  const buf = await blob.arrayBuffer();
  const bytes = new Uint8Array(buf);
  let binary = "";
  const chunk = 0x8000;
  for (let i = 0; i < bytes.length; i += chunk) {
    binary += String.fromCharCode.apply(
      null,
      bytes.subarray(i, i + chunk) as unknown as number[],
    );
  }
  return btoa(binary);
}

// === Calibration helpers (Layer 4) ===
//
// Capture-only flows for the onboarding wizard. They reuse the same
// AnalyserNode pipeline but skip MediaRecorder when only peak data is needed.
// Both take the selected input device so calibration measures the mic the
// user will actually record with (undefined = system default).

export type AmbientSample = { peakAmplitude: number; durationMs: number };
export type SpeechSample = AmbientSample & { bytesPerSecond: number };

export async function sampleAmbient(
  durationMs: number,
  deviceId?: string,
): Promise<AmbientSample> {
  const stream = await acquireHealthyStream(deviceId);
  try {
    startAnalyser(stream);
    await new Promise((r) => setTimeout(r, durationMs));
    const peak = peakAmplitude;
    return { peakAmplitude: peak, durationMs };
  } finally {
    teardownAnalyser();
    stream.getTracks().forEach((t) => t.stop());
    peakAmplitude = 0;
  }
}

export async function sampleSpeech(
  durationMs: number,
  deviceId?: string,
): Promise<SpeechSample> {
  const stream = await acquireHealthyStream(deviceId);
  const mime = pickMime();
  const recorder = mime
    ? new MediaRecorder(stream, { mimeType: mime })
    : new MediaRecorder(stream);
  const buf: Blob[] = [];
  recorder.ondataavailable = (e) => {
    if (e.data && e.data.size > 0) buf.push(e.data);
  };
  try {
    startAnalyser(stream);
    const startedAt = performance.now();
    recorder.start();
    await new Promise((r) => setTimeout(r, durationMs));
    const stopped = new Promise<Blob>((resolve) => {
      recorder.onstop = () => resolve(new Blob(buf, { type: recorder.mimeType || "audio/webm" }));
    });
    recorder.stop();
    const blob = await stopped;
    const realDuration = Math.max(1, performance.now() - startedAt);
    const bytesPerSecond = Math.round((blob.size / realDuration) * 1000);
    return {
      peakAmplitude,
      durationMs: Math.round(realDuration),
      bytesPerSecond,
    };
  } finally {
    teardownAnalyser();
    stream.getTracks().forEach((t) => t.stop());
    peakAmplitude = 0;
  }
}
