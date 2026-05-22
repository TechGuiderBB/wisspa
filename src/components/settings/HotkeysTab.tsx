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
  // Only one row can be capturing at a time. `activeAction` tracks which.
  const [activeAction, setActiveAction] = useState<HotkeyAction | null>(null);
  const [errors, setErrors] = useState<Partial<Record<HotkeyAction, string>>>({});
  // Live preview of modifiers currently held during capture.
  const [previewParts, setPreviewParts] = useState<string[]>([]);

  // Pause global shortcuts whenever any row is capturing so the keys reach
  // the webview rather than firing dictation / action / prompt.
  useEffect(() => {
    if (activeAction === null) return;
    let cancelled = false;
    (async () => {
      try {
        await pauseHotkeys();
      } catch (err) {
        console.error("pauseHotkeys failed:", err);
      }
      if (cancelled) {
        try {
          await resumeHotkeys();
        } catch (e) {
          console.error("resumeHotkeys (cancelled) failed:", e);
        }
      }
    })();
    return () => {
      cancelled = true;
      void resumeHotkeys().catch((e) => console.error("resumeHotkeys failed:", e));
    };
  }, [activeAction]);

  // Document-level keydown listener decoupled from button focus.  This is
  // what fixes the "Cmd press steals focus" problem — macOS may move the
  // menu-bar highlight on modifier press, so we can't rely on the button
  // staying focused.
  useEffect(() => {
    if (activeAction === null) return;

    const handler = (e: KeyboardEvent) => {
      e.preventDefault();
      e.stopPropagation();

      // Esc with no modifiers cancels capture without committing.
      if (
        e.code === "Escape" &&
        !e.metaKey &&
        !e.ctrlKey &&
        !e.altKey &&
        !e.shiftKey
      ) {
        setActiveAction(null);
        setPreviewParts([]);
        return;
      }

      const parts: string[] = [];
      if (e.metaKey) parts.push("CmdOrCtrl");
      if (e.ctrlKey && !e.metaKey) parts.push("Control");
      if (e.altKey) parts.push("Alt");
      if (e.shiftKey) parts.push("Shift");

      const key = normalizeKey(e.code, e.key);
      if (!key) return;
      const isModifierOnly = [
        "Meta",
        "Control",
        "Alt",
        "Shift",
        "CmdOrCtrl",
      ].includes(key);

      if (isModifierOnly) {
        // Update preview so the user sees Cmd / Shift / etc. registering.
        setPreviewParts(parts);
        return;
      }
      parts.push(key);

      const combo = parts.join("+");
      const action = activeAction;
      commit(action, combo);
    };

    document.addEventListener("keydown", handler, { capture: true });
    return () =>
      document.removeEventListener("keydown", handler, { capture: true });
  }, [activeAction]);

  async function commit(action: HotkeyAction, combo: string) {
    patch({ [action]: combo } as Partial<Settings["hotkeys"]>);
    setActiveAction(null);
    setPreviewParts([]);
    try {
      await updateHotkey(action, combo);
      setErrors((prev) => ({ ...prev, [action]: undefined }));
    } catch (err) {
      setErrors((prev) => ({ ...prev, [action]: String(err) }));
    }
  }

  async function resetDefaults() {
    // Single patch call so only one saveSettings fires — avoids stale-closure
    // race where 4 separate patch()+persist() calls each spread from the same
    // frozen `settings` prop and only the last write survives to disk.
    patch(DEFAULTS);
    // Clear stale per-row errors up front; a successful reset must not leave
    // an old failure message rendering. Failures below repopulate as needed.
    setErrors({});
    for (const a of HOTKEY_ACTIONS) {
      try {
        await updateHotkey(a, DEFAULTS[a]);
      } catch (err) {
        console.error(`reset ${a} failed:`, err);
        setErrors((prev) => ({ ...prev, [a]: String(err) }));
      }
    }
  }

  return (
    <div className="space-y-3">
      {HOTKEY_ACTIONS.map((action) => (
        <HotkeyRow
          key={action}
          label={LABELS[action]}
          combo={settings.hotkeys[action]}
          capturing={activeAction === action}
          previewParts={activeAction === action ? previewParts : []}
          error={errors[action]}
          onStart={() => setActiveAction(action)}
          onCancel={() => {
            setActiveAction(null);
            setPreviewParts([]);
          }}
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
        Click <em>Change</em>, then press your combo. Single keys (e.g.{" "}
        <code>F18</code>, <code>F19</code>) work great for push-and-hold — no
        modifier needed. Press <kbd className="rounded border px-1 bg-white">Esc</kbd>{" "}
        while capturing to cancel.
      </p>
    </div>
  );
}

function HotkeyRow({
  label,
  combo,
  capturing,
  previewParts,
  error,
  onStart,
  onCancel,
}: {
  label: string;
  combo: string;
  capturing: boolean;
  previewParts: string[];
  error: string | undefined;
  onStart: () => void;
  onCancel: () => void;
}) {
  const buttonRef = useRef<HTMLButtonElement | null>(null);
  // Re-focus the capture indicator when capturing starts.  The document-level
  // listener handles input either way; this is just for visual focus styling.
  useEffect(() => {
    if (capturing) {
      buttonRef.current?.focus();
    }
  }, [capturing]);

  return (
    <div className="grid grid-cols-[200px_1fr] items-center gap-4">
      <div className="text-sm text-neutral-700">{label}</div>
      <div className="flex items-center gap-2">
        <div
          ref={buttonRef as unknown as React.RefObject<HTMLDivElement>}
          tabIndex={-1}
          className={`min-w-[240px] rounded-md border px-3 py-1.5 text-sm font-mono select-none ${
            capturing
              ? "border-accent bg-blue-50 text-accent ring-2 ring-accent/30"
              : "border-neutral-300 bg-white text-slate-800"
          }`}
        >
          {capturing
            ? previewParts.length > 0
              ? `${previewParts.join("+")}+…`
              : "Press a combo…"
            : combo}
        </div>
        {capturing ? (
          <button
            type="button"
            onClick={onCancel}
            className="rounded-md border border-neutral-300 bg-white px-3 py-1.5 text-sm font-medium text-slate-700 hover:bg-slate-50"
          >
            Cancel
          </button>
        ) : (
          <button
            type="button"
            onClick={onStart}
            className="rounded-md bg-wisspa-gradient px-3 py-1.5 text-sm font-semibold text-white shadow-sm hover:brightness-110"
          >
            Change
          </button>
        )}
      </div>
      {error && <div className="col-start-2 text-xs text-red-600 mt-1">{error}</div>}
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
