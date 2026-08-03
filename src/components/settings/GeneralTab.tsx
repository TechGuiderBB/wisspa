import { useEffect, useState } from "react";
import {
  disable as disableAutostart,
  enable as enableAutostart,
  isEnabled as isAutostartEnabled,
} from "@tauri-apps/plugin-autostart";
import {
  getSettings,
  saveSettings,
  type MicSensitivity,
  type RecordingMode,
  type Settings,
} from "../../lib/settings";
import { sampleAmbient, sampleSpeech } from "../../lib/audio";
import { Row, Toggle, Radio, Select, Slider } from "./ui";

const STT_MODEL_OPTIONS = [
  { value: "whisper-large-v3-turbo", label: "Whisper Large v3 Turbo (fastest)" },
  { value: "whisper-large-v3", label: "Whisper Large v3 (most accurate)" },
  {
    value: "distil-whisper-large-v3-en",
    label: "Distil Whisper Large v3 (fastest, English only)",
  },
];

const STT_LANGUAGE_OPTIONS = [
  { value: "", label: "Auto-detect" },
  { value: "en", label: "English" },
  { value: "es", label: "Spanish" },
  { value: "fr", label: "French" },
  { value: "de", label: "German" },
  { value: "it", label: "Italian" },
  { value: "pt", label: "Portuguese" },
  { value: "ja", label: "Japanese" },
  { value: "zh", label: "Chinese" },
  { value: "ko", label: "Korean" },
  { value: "hi", label: "Hindi" },
  { value: "nl", label: "Dutch" },
];

const DISTIL_MODEL = "distil-whisper-large-v3-en";

type Props = {
  settings: Settings;
  patch: (p: Partial<Settings["general"]>) => void;
  patchStt: (p: Partial<Settings["stt"]>) => void;
};

export default function GeneralTab({ settings, patch, patchStt }: Props) {
  const g = settings.general;
  const stt = settings.stt;
  // distil-whisper is English-only: warn when it's paired with a pinned
  // non-English language or auto-detect (which could route non-English audio
  // to a model that can't transcribe it).
  const distilLanguageMismatch = stt.model === DISTIL_MODEL && stt.language !== "en";
  const cal = settings.mic_calibration;
  const [calStage, setCalStage] = useState<
    "idle" | "ambient" | "speech" | "done" | "error"
  >("idle");
  const [calError, setCalError] = useState<string | null>(null);

  // Sync the toggle against the OS-level Login Items state on mount in
  // case they drifted apart (e.g. user removed Wisspa via System Settings).
  useEffect(() => {
    (async () => {
      try {
        const actual = await isAutostartEnabled();
        if (actual !== g.launch_on_login) {
          patch({ launch_on_login: actual });
        }
      } catch (err) {
        console.warn("autostart isEnabled failed:", err);
      }
    })();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  async function setLaunchOnLogin(value: boolean) {
    patch({ launch_on_login: value });
    try {
      if (value) await enableAutostart();
      else await disableAutostart();
    } catch (err) {
      console.error("autostart toggle failed:", err);
      patch({ launch_on_login: !value });
    }
  }

  async function recalibrate() {
    setCalError(null);
    try {
      setCalStage("ambient");
      const amb = await sampleAmbient(2000);
      await new Promise((r) => setTimeout(r, 400));
      setCalStage("speech");
      const speech = await sampleSpeech(2500);
      const silencePeak = Math.max(6, amb.peakAmplitude * 2);
      const minBytesPerSecond = Math.max(
        1500,
        Math.round(speech.bytesPerSecond * 0.3),
      );
      const current = await getSettings();
      await saveSettings({
        ...current,
        mic_calibration: {
          silence_peak: silencePeak,
          min_bytes_per_second: minBytesPerSecond,
          calibrated_at: Date.now(),
        },
      });
      setCalStage("done");
      setTimeout(() => setCalStage("idle"), 2500);
    } catch (err) {
      setCalError(String(err));
      setCalStage("error");
    }
  }

  return (
    <div className="space-y-3">
      <Row label="Launch on login">
        <Toggle
          checked={g.launch_on_login}
          onChange={(v) => void setLaunchOnLogin(v)}
          label="Launch on login"
        />
      </Row>

      <Row label="Show recording overlay">
        <Toggle
          checked={g.show_overlay}
          onChange={(v) => patch({ show_overlay: v })}
          label="Show recording overlay"
        />
      </Row>

      <Row label="Recording mode">
        <Radio
          value={g.recording_mode}
          onChange={(v) => patch({ recording_mode: v as RecordingMode })}
          options={[
            { value: "press_and_hold", label: "Press & hold" },
            { value: "toggle", label: "Toggle" },
          ]}
        />
      </Row>

      <Row
        label="Max recording length"
        hint="Recording stops automatically at this limit, even in toggle mode or while the key is held."
      >
        <Slider
          value={g.max_recording_seconds}
          min={10}
          max={120}
          step={5}
          onChange={(v) => patch({ max_recording_seconds: v })}
          format={(v) => `${v}s`}
        />
      </Row>

      <Row
        label="Fast recording start"
        hint="Warm the mic when you press the hotkey's modifier so recording starts instantly. The macOS mic indicator appears as you reach for the key."
      >
        <Toggle
          checked={g.fast_recording_start}
          onChange={(v) => patch({ fast_recording_start: v })}
          label="Fast recording start"
        />
      </Row>

      <Row
        label="Quiet notifications"
        hint="Suppress the macOS banners for inserts, raw paste warnings, and learned-correction confirmations. Errors still appear so you'll see real failures."
      >
        <Toggle
          checked={g.quiet_notifications}
          onChange={(v) => patch({ quiet_notifications: v })}
          label="Quiet notifications"
        />
      </Row>

      <Row
        label="Verbose logging"
        hint="Off by default, only a redacted summary is logged. Turn on to include your transcripts, AI output and clipboard text in the log file when capturing a bug, then turn it back off. The log lives at ~/Library/Logs/Wisspa/."
      >
        <Toggle
          checked={g.verbose_logging}
          onChange={(v) => patch({ verbose_logging: v })}
          label="Verbose logging"
        />
      </Row>

      <Row
        label="Recording sounds"
        hint="Soft chimes when a recording starts, stops, is cancelled, or times out."
      >
        <Toggle
          checked={g.play_sounds}
          onChange={(v) => patch({ play_sounds: v })}
          label="Recording sounds"
        />
      </Row>

      {g.play_sounds && (
        <>
          <Row label="Sound volume">
            <Slider
              value={g.sound_volume}
              min={0}
              max={1}
              step={0.05}
              onChange={(v) => patch({ sound_volume: v })}
              format={(v) => `${Math.round(v * 100)}%`}
            />
          </Row>
          <Row
            label="Ready chime"
            hint="Play a chime the moment the mic is live and ready for speech."
          >
            <Toggle
              checked={g.ready_chime}
              onChange={(v) => patch({ ready_chime: v })}
              label="Ready chime"
            />
          </Row>
          <Row
            label="Dictation finished sound"
            hint="Play a subtle chime when dictation finishes and the text is inserted."
          >
            <Toggle
              checked={g.dictation_complete_sound}
              onChange={(v) => patch({ dictation_complete_sound: v })}
              label="Dictation finished sound"
            />
          </Row>
        </>
      )}

      <Row
        label="Transcription model"
        hint="The Groq Whisper model used for speech-to-text."
      >
        <Select<string>
          value={stt.model}
          onChange={(v) => patchStt({ model: v })}
          options={STT_MODEL_OPTIONS}
        />
      </Row>

      <Row
        label="Transcription language"
        hint="Pin the spoken language, or let Groq auto-detect it per recording."
      >
        <Select<string>
          value={stt.language}
          onChange={(v) => patchStt({ language: v })}
          options={STT_LANGUAGE_OPTIONS}
        />
        {distilLanguageMismatch && (
          <span className="text-xs text-amber-600">
            Distil Whisper is English-only — pick English or a multilingual
            model.
          </span>
        )}
      </Row>

      <Row
        label="Mic sensitivity"
        hint="How quickly Wisspa decides a recording is silent. Higher = looser."
      >
        <Radio
          value={g.mic_sensitivity}
          onChange={(v) => patch({ mic_sensitivity: v as MicSensitivity })}
          options={[
            { value: "off", label: "Off" },
            { value: "low", label: "Low" },
            { value: "medium", label: "Medium" },
            { value: "high", label: "High" },
          ]}
        />
      </Row>

      <Row
        label="Re-calibrate mic"
        hint={
          cal
            ? `Calibrated ${new Date(cal.calibrated_at).toLocaleDateString()} · silence peak ${cal.silence_peak.toFixed(1)}, min ${cal.min_bytes_per_second.toLocaleString()} B/s`
            : "Not yet calibrated — using defaults"
        }
      >
        <div className="flex items-center gap-2">
          <button
            type="button"
            onClick={recalibrate}
            disabled={calStage === "ambient" || calStage === "speech"}
            className="rounded-md border border-slate-300 bg-white px-3 py-1.5 text-sm font-medium text-slate-800 hover:bg-slate-50 disabled:opacity-50"
          >
            {calStage === "idle" && "Re-calibrate"}
            {calStage === "ambient" && "Stay quiet…"}
            {calStage === "speech" && "Say \"hello hello hello\"…"}
            {calStage === "done" && "✓ Saved"}
            {calStage === "error" && "Failed — retry"}
          </button>
          {calError && (
            <span className="text-xs text-red-600">{calError}</span>
          )}
        </div>
      </Row>
    </div>
  );
}
