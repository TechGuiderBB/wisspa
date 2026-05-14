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
  type Theme,
} from "../../lib/settings";
import { sampleAmbient, sampleSpeech } from "../../lib/audio";
import { Row, Toggle, Radio, Select, Slider } from "./ui";

type Props = {
  settings: Settings;
  patch: (p: Partial<Settings["general"]>) => void;
};

export default function GeneralTab({ settings, patch }: Props) {
  const g = settings.general;
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
      )}

      <Row label="Theme">
        <Select<Theme>
          value={g.theme}
          onChange={(v) => patch({ theme: v })}
          options={[
            { value: "system", label: "System" },
            { value: "light", label: "Light" },
            { value: "dark", label: "Dark" },
          ]}
        />
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
