import type { Settings, RecordingMode, Theme } from "../../lib/settings";
import { Row, Toggle, Radio, Select, Slider } from "./ui";

type Props = {
  settings: Settings;
  patch: (p: Partial<Settings["general"]>) => void;
};

export default function GeneralTab({ settings, patch }: Props) {
  const g = settings.general;
  return (
    <div className="space-y-3">
      <Row label="Launch on login">
        <Toggle
          checked={g.launch_on_login}
          onChange={(v) => patch({ launch_on_login: v })}
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

      <Row label="Sound on start / stop">
        <Toggle
          checked={g.play_sounds}
          onChange={(v) => patch({ play_sounds: v })}
          label="Sound on start / stop"
        />
      </Row>

      {g.play_sounds && (
        <Row label="Volume">
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
    </div>
  );
}
