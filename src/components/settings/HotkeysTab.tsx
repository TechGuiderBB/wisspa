import { useEffect, useRef, useState } from "react";
import {
  HOTKEY_ACTIONS,
  HotkeyAction,
  Settings,
  pauseHotkeys,
  resumeHotkeys,
  updateHotkey,
} from "../../lib/settings";

type Props = {
  settings: Settings;
  patch: (p: Partial<Settings["hotkeys"]>) => void;
};

const LABELS: Record<HotkeyAction, string> = {
  dictation: "Dictation",
  action: "Action mode",
  prompt: "Prompt mode",
  cancel: "Cancel recording",
};

const DEFAULTS: Settings["hotkeys"] = {
  dictation: "CmdOrCtrl+Shift+Space",
  action: "CmdOrCtrl+Shift+A",
  prompt: "CmdOrCtrl+Shift+P",
  cancel: "Escape",
};

export default function HotkeysTab({ settings, patch }: Props) {
  const captureCount = useRef(0);

  async function beginCapture() {
    captureCount.current += 1;
    if (captureCount.current === 1) {
      try {
        await pauseHotkeys();
      } catch (err) {
        console.error("pauseHotkeys failed:", err);
      }
    }
  }

  async function endCapture() {
    captureCount.current = Math.max(0, captureCount.current - 1);
    if (captureCount.current === 0) {
      try {
        await resumeHotkeys();
      } catch (err) {
        console.error("resumeHotkeys failed:", err);
      }
    }
  }

  useEffect(() => {
    return () => {
      if (captureCount.current > 0) {
        captureCount.current = 0;
        void resumeHotkeys();
      }
    };
  }, []);

  async function resetDefaults() {
    patch(DEFAULTS);
    for (const a of HOTKEY_ACTIONS) {
      try {
        await updateHotkey(a, DEFAULTS[a]);
      } catch (err) {
        console.error(`reset ${a} failed:`, err);
      }
    }
  }

  return (
    <div className="space-y-3">
      {HOTKEY_ACTIONS.map((action) => (
        <HotkeyRow
          key={action}
          action={action}
          combo={settings.hotkeys[action]}
          onChange={(combo) => patch({ [action]: combo } as Partial<Settings["hotkeys"]>)}
          beginCapture={beginCapture}
          endCapture={endCapture}
        />
      ))}

      <div className="pt-2">
        <button
          type="button"
          onClick={resetDefaults}
          className="text-xs underline decoration-dotted text-neutral-600 hover:text-neutral-900"
        >
          Reset to defaults
        </button>
      </div>

      <p className="text-xs text-neutral-500 pt-2 leading-relaxed">
        Click a row, then press your combo. Single keys (e.g. <code>F18</code>,{" "}
        <code>F19</code>) work great for push-and-hold — just one key, no
        modifiers. Press <kbd className="rounded border px-1">Esc</kbd> while
        capturing to cancel.
      </p>
    </div>
  );
}

function HotkeyRow({
  action,
  combo,
  onChange,
  beginCapture,
  endCapture,
}: {
  action: HotkeyAction;
  combo: string;
  onChange: (combo: string) => void;
  beginCapture: () => Promise<void>;
  endCapture: () => Promise<void>;
}) {
  const [capturing, setCapturing] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function startCapture() {
    setError(null);
    await beginCapture();
    setCapturing(true);
  }

  async function stopCapture() {
    setCapturing(false);
    await endCapture();
  }

  async function onKeyDown(e: React.KeyboardEvent<HTMLButtonElement>) {
    if (!capturing) return;
    e.preventDefault();
    e.stopPropagation();

    // Allow Esc to cancel cleanly without committing.
    if (e.code === "Escape" && !e.metaKey && !e.ctrlKey && !e.altKey && !e.shiftKey) {
      await stopCapture();
      return;
    }

    const parts: string[] = [];
    if (e.metaKey) parts.push("CmdOrCtrl");
    if (e.ctrlKey && !e.metaKey) parts.push("Control");
    if (e.altKey) parts.push("Alt");
    if (e.shiftKey) parts.push("Shift");

    const key = normalizeKey(e.code, e.key);
    if (!key) return;
    if (["Meta", "Control", "Alt", "Shift", "CmdOrCtrl"].includes(key)) {
      // Modifier-only press; wait for a real key.
      return;
    }
    parts.push(key);

    const newCombo = parts.join("+");
    onChange(newCombo);
    try {
      await updateHotkey(action, newCombo);
      setError(null);
    } catch (err) {
      setError(String(err));
    }
    await stopCapture();
  }

  async function onBlur() {
    if (capturing) {
      await stopCapture();
    }
  }

  return (
    <div className="grid grid-cols-[200px_1fr] items-center gap-4">
      <div className="text-sm text-neutral-700">{LABELS[action]}</div>
      <div>
        <button
          type="button"
          onClick={() => void startCapture()}
          onBlur={() => void onBlur()}
          onKeyDown={onKeyDown}
          className={`min-w-[240px] rounded-md border px-3 py-1.5 text-sm font-mono text-left transition-colors ${
            capturing
              ? "border-accent bg-blue-50 text-accent ring-2 ring-accent/30"
              : "border-neutral-300 bg-white hover:border-neutral-400"
          }`}
        >
          {capturing ? "Press a combo…" : combo}
        </button>
        {error && <div className="mt-1 text-xs text-red-600">{error}</div>}
      </div>
    </div>
  );
}

function normalizeKey(code: string, key: string): string | null {
  if (code.startsWith("Key")) return code.slice(3);
  if (code.startsWith("Digit")) return code.slice(5);
  if (/^F\d{1,2}$/.test(code)) return code;
  const map: Record<string, string> = {
    Space: "Space",
    Escape: "Escape",
    Enter: "Enter",
    Tab: "Tab",
    Backspace: "Backspace",
    ArrowUp: "Up",
    ArrowDown: "Down",
    ArrowLeft: "Left",
    ArrowRight: "Right",
    Minus: "Minus",
    Equal: "Equal",
    Comma: "Comma",
    Period: "Period",
    Slash: "Slash",
    Semicolon: "Semicolon",
    Quote: "Quote",
    BracketLeft: "BracketLeft",
    BracketRight: "BracketRight",
    Backslash: "Backslash",
    Backquote: "Backquote",
  };
  if (map[code]) return map[code];
  if (key.length === 1) return key.toUpperCase();
  return null;
}
