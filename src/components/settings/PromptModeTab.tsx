import type { Settings } from "../../lib/settings";
import { Row, Toggle, Select, Slider } from "./ui";

type Props = {
  settings: Settings;
  patch: (p: Partial<Settings["prompt_mode"]>) => void;
};

const APP_OVERRIDES = [
  { value: "", label: "Auto-detect" },
  { value: "Claude", label: "Claude" },
  { value: "ChatGPT", label: "ChatGPT" },
  { value: "Cursor", label: "Cursor" },
  { value: "Gemini", label: "Gemini" },
  { value: "Generic", label: "Generic Markdown" },
];

export default function PromptModeTab({ settings, patch }: Props) {
  const p = settings.prompt_mode;
  return (
    <div className="space-y-3">
      <div className="rounded-md bg-emerald-50 border border-emerald-200 px-3 py-2 text-xs text-emerald-900">
        Prompt mode is live. Hold the prompt hotkey, describe what you want an
        AI to do, and Wisspa rewrites it via Claude Sonnet into a structured
        prompt formatted for the AI tool you're focused on.
      </div>

      <Row label="Include selected text as context">
        <Toggle
          checked={p.include_selected_text}
          onChange={(v) => patch({ include_selected_text: v })}
        />
      </Row>

      <Row
        label="Review and edit before insert"
        hint="Opens an editable window with the generated prompt; nothing is pasted until you approve."
      >
        <Toggle
          checked={p.review_before_insert}
          onChange={(v) => patch({ review_before_insert: v })}
        />
      </Row>

      <Row
        label="Show preview before insert"
        hint={
          p.review_before_insert
            ? "Superseded while review is on — the editable window replaces the timed preview."
            : undefined
        }
      >
        <Toggle
          checked={p.show_preview}
          disabled={p.review_before_insert}
          onChange={(v) => patch({ show_preview: v })}
        />
      </Row>

      {p.show_preview && !p.review_before_insert && (
        <Row label="Preview timeout">
          <Slider
            value={p.preview_timeout_seconds}
            min={3}
            max={10}
            onChange={(v) => patch({ preview_timeout_seconds: v })}
            format={(v) => `${v}s`}
          />
        </Row>
      )}

      <Row
        label="Override target format"
        hint="Used when active-app detection misses the AI tool."
      >
        <Select
          value={p.manual_app_override ?? ""}
          onChange={(v) => patch({ manual_app_override: v || null })}
          options={APP_OVERRIDES}
        />
      </Row>
    </div>
  );
}
