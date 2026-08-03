import { useEffect, useRef, useState } from "react";
import {
  HistoryEntry,
  clearHistory,
  exportHistoryCsv,
  getHistory,
  reinjectText,
} from "../../lib/settings";
import { Button, Select } from "./ui";

type ModeFilter = "all" | "dictation" | "prompt" | "action" | "command";

export default function HistoryTab() {
  const [entries, setEntries] = useState<HistoryEntry[] | null>(null);
  const [busy, setBusy] = useState(false);
  const [query, setQuery] = useState("");
  const [modeFilter, setModeFilter] = useState<ModeFilter>("all");

  async function refresh() {
    try {
      setEntries(await getHistory(100));
    } catch (err) {
      console.error("getHistory failed:", err);
    }
  }

  useEffect(() => {
    refresh();
    const id = setInterval(refresh, 4000);
    return () => clearInterval(id);
  }, []);

  async function onClear() {
    if (!confirm("Clear all history? This cannot be undone.")) return;
    setBusy(true);
    try {
      await clearHistory();
      await refresh();
    } finally {
      setBusy(false);
    }
  }

  async function onExport() {
    setBusy(true);
    try {
      const csv = await exportHistoryCsv();
      const blob = new Blob([csv], { type: "text/csv" });
      const url = URL.createObjectURL(blob);
      const a = document.createElement("a");
      a.href = url;
      a.download = `wisspa-history-${new Date().toISOString().slice(0, 10)}.csv`;
      a.click();
      URL.revokeObjectURL(url);
    } finally {
      setBusy(false);
    }
  }

  if (!entries) {
    return (
      <div className="text-sm text-slate-500">Loading history…</div>
    );
  }

  if (entries.length === 0) {
    return (
      <div className="rounded-md bg-slate-50 border border-slate-200 px-4 py-8 text-center text-sm text-slate-500">
        No history yet. Dictate, run an action, or rewrite a prompt — entries
        will appear here.
      </div>
    );
  }

  // Client-side filter over the already-loaded entries (display cap is 100).
  // The search text is the same output-first text the row displays and copies.
  const q = query.trim().toLowerCase();
  const filtering = q !== "" || modeFilter !== "all";
  const filtered = entries.filter((e) => {
    if (modeFilter !== "all" && e.mode !== modeFilter) return false;
    if (q) {
      const text = (e.output ?? e.raw_transcript ?? "").toLowerCase();
      if (!text.includes(q)) return false;
    }
    return true;
  });

  // Anthropic token metering over the loaded window. The tab loads the most
  // recent 100 rows, so the summary is honestly labelled "last 100 entries";
  // rows without captured usage (cancelled/failed runs, pre-metering rows)
  // contribute nothing. Rendered only when at least one row has token data,
  // so a pre-migration history doesn't show a misleading 0.
  const anyTokens = entries.some(
    (e) => e.input_tokens != null || e.output_tokens != null,
  );
  const totalTokens = entries.reduce(
    (sum, e) => sum + (e.input_tokens ?? 0) + (e.output_tokens ?? 0),
    0,
  );

  return (
    <div className="space-y-3">
      <div className="flex items-center justify-between">
        <div className="text-xs text-slate-500">
          {filtering
            ? `Showing ${filtered.length} of ${entries.length} loaded entries`
            : `Showing ${entries.length} most recent`}{" "}
          · stored locally in <code className="font-mono">history.db</code>
          {anyTokens && (
            <> · last 100 entries: {totalTokens.toLocaleString()} tokens</>
          )}
        </div>
        <div className="flex gap-2">
          <Button variant="secondary" onClick={onExport} disabled={busy}>
            Export CSV
          </Button>
          <Button variant="secondary" onClick={onClear} disabled={busy}>
            Clear
          </Button>
        </div>
      </div>

      <div className="flex items-center gap-2">
        <input
          type="search"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          placeholder="Search history…"
          aria-label="Search history"
          className="flex-1 rounded-md border border-slate-300 bg-white px-2 py-1.5 text-xs focus:outline-none focus:border-accent focus:ring-2 focus:ring-accent/30"
        />
        <Select<ModeFilter>
          value={modeFilter}
          onChange={setModeFilter}
          options={[
            { value: "all", label: "All modes" },
            { value: "dictation", label: "Dictation" },
            { value: "prompt", label: "Prompt" },
            { value: "action", label: "Action" },
            { value: "command", label: "Command" },
          ]}
        />
      </div>

      {filtered.length === 0 ? (
        <div className="rounded-md bg-slate-50 border border-slate-200 px-4 py-8 text-center text-sm text-slate-500">
          No entries match your search.
        </div>
      ) : (
        <div className="rounded-lg border border-slate-200 bg-white overflow-hidden">
          <table className="w-full text-xs">
            <thead className="bg-slate-50 text-slate-600">
              <tr>
                <th className="text-left px-3 py-2 font-medium">When</th>
                <th className="text-left px-3 py-2 font-medium">Mode</th>
                <th className="text-left px-3 py-2 font-medium">App</th>
                <th className="text-left px-3 py-2 font-medium">Snippet</th>
                <th className="text-right px-3 py-2 font-medium">ms</th>
                <th className="text-left px-3 py-2 font-medium">Status</th>
                <th className="text-right px-3 py-2 font-medium">Actions</th>
              </tr>
            </thead>
            <tbody>
              {filtered.map((e) => (
                <Row key={e.id} entry={e} />
              ))}
            </tbody>
          </table>
        </div>
      )}
    </div>
  );
}

function Row({ entry }: { entry: HistoryEntry }) {
  const when = new Date(entry.timestamp);
  // Same precedence as the visible snippet column: output first, then raw_transcript.
  const text = (entry.output ?? entry.raw_transcript ?? "").trim();
  const short = text.length > 80 ? text.slice(0, 80) + "…" : text;
  const hasText = text.length > 0;

  const [copied, setCopied] = useState(false);
  const [copyFailed, setCopyFailed] = useState(false);
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState("");
  const [editCopied, setEditCopied] = useState(false);
  const [editCopyFailed, setEditCopyFailed] = useState(false);
  const [injecting, setInjecting] = useState(false);
  const [injectFailed, setInjectFailed] = useState(false);

  // Store timer IDs so we can clear them on unmount (prevents setState on unmounted row).
  const copyTimerRef = useRef<number | null>(null);
  const editCopyTimerRef = useRef<number | null>(null);
  const injectTimerRef = useRef<number | null>(null);

  useEffect(() => {
    return () => {
      if (copyTimerRef.current !== null) window.clearTimeout(copyTimerRef.current);
      if (editCopyTimerRef.current !== null) window.clearTimeout(editCopyTimerRef.current);
      if (injectTimerRef.current !== null) window.clearTimeout(injectTimerRef.current);
    };
  }, []);

  const modeStyles: Record<string, string> = {
    dictation: "bg-blue-100 text-blue-700",
    action: "bg-violet-100 text-violet-700",
    prompt: "bg-emerald-100 text-emerald-700",
  };
  const statusStyles: Record<string, string> = {
    success: "bg-green-100 text-green-700",
    failure: "bg-red-100 text-red-700",
    // Prompt mode pasted the raw transcript because the rewrite failed.
    fallback: "bg-amber-100 text-amber-700",
    cancelled: "bg-slate-100 text-slate-600",
  };

  async function handleCopyText() {
    try {
      await navigator.clipboard.writeText(text);
      setCopied(true);
      setCopyFailed(false);
      if (copyTimerRef.current !== null) window.clearTimeout(copyTimerRef.current);
      copyTimerRef.current = window.setTimeout(() => setCopied(false), 1500);
    } catch (err) {
      console.error("Clipboard write failed:", err);
      setCopied(false);
      setCopyFailed(true);
      if (copyTimerRef.current !== null) window.clearTimeout(copyTimerRef.current);
      copyTimerRef.current = window.setTimeout(() => setCopyFailed(false), 1500);
    }
  }

  async function handleCopyDraft() {
    try {
      await navigator.clipboard.writeText(draft);
      setEditCopied(true);
      setEditCopyFailed(false);
      if (editCopyTimerRef.current !== null) window.clearTimeout(editCopyTimerRef.current);
      editCopyTimerRef.current = window.setTimeout(() => setEditCopied(false), 1500);
    } catch (err) {
      console.error("Clipboard write failed:", err);
      setEditCopied(false);
      setEditCopyFailed(true);
      if (editCopyTimerRef.current !== null) window.clearTimeout(editCopyTimerRef.current);
      editCopyTimerRef.current = window.setTimeout(() => setEditCopyFailed(false), 1500);
    }
  }

  function handleReuseClick() {
    if (!editing) {
      // Always re-seed from the entry on open — drafts are ephemeral.
      setDraft(text);
    }
    setEditing(!editing);
  }

  // Paste the entry's text again into whatever app is frontmost right now.
  // Re-inject targets the *live* frontmost app by design — the destination is
  // whichever field the user focuses before clicking, not the original app.
  async function handleReinject() {
    setInjecting(true);
    try {
      await reinjectText(text);
      setInjectFailed(false);
    } catch (err) {
      console.error("Re-inject failed:", err);
      setInjectFailed(true);
      if (injectTimerRef.current !== null) window.clearTimeout(injectTimerRef.current);
      injectTimerRef.current = window.setTimeout(() => setInjectFailed(false), 2000);
    } finally {
      setInjecting(false);
    }
  }

  const copyLabel = copyFailed ? "Failed" : copied ? "Copied" : "Copy";
  const editCopyLabel = editCopyFailed ? "Failed" : editCopied ? "Copied" : "Copy edited";
  const reinjectLabel = injecting ? "Pasting…" : injectFailed ? "Failed" : "Paste again";

  // Anthropic token total for the row (input + output, summed across the
  // mode's LLM calls). Null on both sides = no usage captured (cancelled or
  // failed runs, rows from before metering) — show nothing rather than "0".
  const hasTokens = entry.input_tokens != null || entry.output_tokens != null;
  const totalTokens = (entry.input_tokens ?? 0) + (entry.output_tokens ?? 0);

  // NOTE: colSpan={7} must match the 7-column thead above (When, Mode, App, Snippet, ms, Status, Actions).
  return (
    <>
      <tr className="border-t border-slate-100 hover:bg-slate-50/60">
        <td className="px-3 py-2 text-slate-500 tabular-nums whitespace-nowrap">
          {when.toLocaleString([], {
            month: "short",
            day: "numeric",
            hour: "2-digit",
            minute: "2-digit",
          })}
        </td>
        <td className="px-3 py-2">
          <span
            className={`text-[10px] font-semibold uppercase px-1.5 py-0.5 rounded ${
              modeStyles[entry.mode] ?? "bg-slate-100 text-slate-600"
            }`}
          >
            {entry.mode}
          </span>
        </td>
        <td className="px-3 py-2 text-slate-600 whitespace-nowrap max-w-[120px] truncate">
          {entry.active_app ?? "—"}
        </td>
        <td className="px-3 py-2 text-slate-800">{short || "—"}</td>
        <td className="px-3 py-2 text-right text-slate-500 tabular-nums">
          {entry.duration_ms ?? "—"}
          {hasTokens && (
            <div
              className="text-[10px] text-slate-400"
              title={`${(entry.input_tokens ?? 0).toLocaleString()} in / ${(entry.output_tokens ?? 0).toLocaleString()} out (Anthropic)`}
            >
              {totalTokens.toLocaleString()} tok
            </div>
          )}
        </td>
        <td className="px-3 py-2">
          <span
            className={`text-[10px] font-medium px-1.5 py-0.5 rounded ${
              statusStyles[entry.status] ?? "bg-slate-100 text-slate-600"
            }`}
          >
            {entry.status}
          </span>
        </td>
        <td className="px-3 py-2">
          <div className="flex justify-end gap-1">
            <Button
              variant="ghost"
              className="px-2 py-1 text-xs"
              disabled={!hasText}
              onClick={handleCopyText}
            >
              {copyLabel}
            </Button>
            <Button
              variant="ghost"
              className="px-2 py-1 text-xs"
              disabled={!hasText}
              onClick={handleReuseClick}
              aria-expanded={editing}
            >
              Re-use
            </Button>
            <Button
              variant="ghost"
              className="px-2 py-1 text-xs"
              disabled={!hasText || injecting}
              title="Paste this text into the currently focused app"
              onClick={handleReinject}
            >
              {reinjectLabel}
            </Button>
          </div>
        </td>
      </tr>
      {editing && (
        <tr className="border-t border-slate-100 bg-slate-50/40">
          <td colSpan={7} className="px-3 py-3">
            <label
              htmlFor={`history-edit-${entry.id}`}
              className="block text-xs text-slate-600 mb-1"
            >
              Edit before copying
            </label>
            <textarea
              id={`history-edit-${entry.id}`}
              value={draft}
              onChange={(e) => setDraft(e.target.value)}
              rows={4}
              className="w-full rounded-md border border-slate-300 bg-white px-2 py-1.5 text-xs font-mono focus:outline-none focus:border-accent focus:ring-2 focus:ring-accent/30 resize-none"
            />
            <div className="flex gap-2 justify-end mt-2">
              <Button
                variant="ghost"
                className="px-2 py-1 text-xs"
                onClick={() => setEditing(false)}
              >
                Done
              </Button>
              <Button
                variant="secondary"
                className="px-2 py-1 text-xs"
                disabled={draft.trim().length === 0}
                onClick={handleCopyDraft}
              >
                {editCopyLabel}
              </Button>
            </div>
          </td>
        </tr>
      )}
    </>
  );
}
