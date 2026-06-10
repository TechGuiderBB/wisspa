import { useEffect, useRef, useState, type KeyboardEvent } from "react";
import { listen } from "@tauri-apps/api/event";
import { submitPromptReview, cancelPromptReview } from "../lib/tauri";

const REVIEW_OPEN_EVENT = "wisspa://prompt-review-open";
const START_EVENT = "wisspa://start-recording";
const CANCEL_EVENT = "wisspa://cancel-recording";

type ReviewOpen = {
  session: number;
  text: string;
  app: string;
  branch: string;
};

/**
 * Edit-before-insert review window (#review). The backend pauses Prompt Mode
 * after Sonnet's rewrite and shows this window; nothing is pasted until the user
 * approves. Insert sends the edited text back to resume the paste; Cancel (and
 * Esc) abort it. Keyed by recording session so a superseded recording's reopen
 * never shows stale text and a late submit for a stale session is a backend
 * no-op.
 */
export default function PromptReview() {
  const [session, setSession] = useState(0);
  const [text, setText] = useState("");
  const [appLabel, setAppLabel] = useState("");
  const [branch, setBranch] = useState("");
  // Guards against a double-click (or Enter + click) resolving twice. The
  // backend is idempotent, but this keeps the UI honest.
  const [submitting, setSubmitting] = useState(false);
  const textareaRef = useRef<HTMLTextAreaElement | null>(null);

  useEffect(() => {
    // Cancellation-safe subscription (mirrors RecordingOverlay): React 18
    // StrictMode runs effect cleanup before the listen() promises resolve in
    // dev, which would otherwise leak listeners.
    let cancelled = false;
    const unlistens: Array<() => void> = [];
    const track = (u: () => void) => {
      if (cancelled) u();
      else unlistens.push(u);
    };

    listen<ReviewOpen>(REVIEW_OPEN_EVENT, (e) => {
      const p = e.payload;
      if (!p) return;
      setSession(p.session);
      setText(p.text ?? "");
      setAppLabel(p.app ?? "");
      setBranch(p.branch ?? "");
      setSubmitting(false);
      // Focus the textarea so the user can edit immediately. Defer to after the
      // state-driven render so the element exists and is enabled.
      requestAnimationFrame(() => {
        const el = textareaRef.current;
        if (el) {
          el.focus();
          el.setSelectionRange(el.value.length, el.value.length);
        }
      });
    }).then(track);

    // The backend hides this window on abort, but clear stale text too so a
    // flashed reopen never shows the previous recording's prompt.
    const reset = () => {
      setSession(0);
      setText("");
      setSubmitting(false);
    };
    listen(START_EVENT, reset).then(track);
    listen(CANCEL_EVENT, reset).then(track);

    return () => {
      cancelled = true;
      unlistens.forEach((u) => u());
    };
  }, []);

  const trimmedEmpty = text.trim().length === 0;

  async function handleInsert() {
    if (submitting || trimmedEmpty) return;
    setSubmitting(true);
    try {
      await submitPromptReview(session, text);
    } catch (err) {
      console.error("submitPromptReview failed:", err);
      setSubmitting(false);
    }
  }

  async function handleCancel() {
    if (submitting) return;
    setSubmitting(true);
    try {
      await cancelPromptReview(session);
    } catch (err) {
      console.error("cancelPromptReview failed:", err);
      setSubmitting(false);
    }
  }

  function onKeyDown(e: KeyboardEvent) {
    // Cmd/Ctrl+Enter inserts; Esc cancels. Esc is handled in-window as well as
    // by the global cancel hotkey — belt-and-braces, since a focused window may
    // swallow the global shortcut.
    if ((e.metaKey || e.ctrlKey) && e.key === "Enter") {
      e.preventDefault();
      void handleInsert();
    } else if (e.key === "Escape") {
      e.preventDefault();
      void handleCancel();
    }
  }

  const branchLabel = branch === "content" ? "content" : "prompt";

  return (
    <div
      className="h-screen w-screen flex flex-col bg-white text-neutral-900"
      onKeyDown={onKeyDown}
    >
      <header className="flex items-baseline gap-2 px-4 pt-3 pb-2 border-b border-neutral-200">
        <h1 className="text-sm font-semibold tracking-tight">Review prompt</h1>
        {appLabel && (
          <span className="text-xs text-neutral-500 truncate">
            → {appLabel}
            <span className="text-neutral-400"> · {branchLabel}</span>
          </span>
        )}
      </header>

      <label htmlFor="prompt-review-text" className="sr-only">
        Generated prompt — edit before inserting
      </label>
      <textarea
        id="prompt-review-text"
        ref={textareaRef}
        value={text}
        onChange={(e) => setText(e.target.value)}
        spellCheck={false}
        autoFocus
        className="flex-1 w-full resize-none px-4 py-3 text-sm font-mono leading-relaxed text-neutral-800 outline-none overflow-auto"
        placeholder="The generated prompt will appear here."
      />

      <footer className="flex items-center justify-between gap-3 px-4 py-3 border-t border-neutral-200">
        <span className="text-[11px] text-neutral-400">
          ⌘↵ insert · Esc cancel
        </span>
        <div className="flex items-center gap-2">
          <button
            type="button"
            onClick={handleCancel}
            disabled={submitting}
            className="rounded-md border border-neutral-300 bg-white px-3 py-1.5 text-sm font-medium text-neutral-800 transition-colors hover:border-neutral-400 hover:bg-neutral-50 disabled:opacity-50 disabled:cursor-not-allowed"
          >
            Cancel
          </button>
          <button
            type="button"
            onClick={handleInsert}
            disabled={submitting || trimmedEmpty}
            className="rounded-md bg-accent px-3 py-1.5 text-sm font-medium text-white shadow-sm transition-colors hover:bg-blue-600 active:bg-blue-700 disabled:opacity-50 disabled:cursor-not-allowed"
          >
            Insert
          </button>
        </div>
      </footer>
    </div>
  );
}
