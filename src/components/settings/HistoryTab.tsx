import { useEffect, useState } from "react";
import {
  HistoryEntry,
  clearHistory,
  exportHistoryCsv,
  getHistory,
} from "../../lib/settings";
import { Button } from "./ui";

export default function HistoryTab() {
  const [entries, setEntries] = useState<HistoryEntry[] | null>(null);
  const [busy, setBusy] = useState(false);

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

  return (
    <div className="space-y-3">
      <div className="flex items-center justify-between">
        <div className="text-xs text-slate-500">
          Showing {entries.length} most recent · stored locally in{" "}
          <code className="font-mono">history.db</code>
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
            </tr>
          </thead>
          <tbody>
            {entries.map((e) => (
              <Row key={e.id} entry={e} />
            ))}
          </tbody>
        </table>
      </div>
    </div>
  );
}

function Row({ entry }: { entry: HistoryEntry }) {
  const when = new Date(entry.timestamp);
  const snippet = (entry.output ?? entry.raw_transcript ?? "").trim();
  const short = snippet.length > 80 ? snippet.slice(0, 80) + "…" : snippet;
  const modeStyles: Record<string, string> = {
    dictation: "bg-blue-100 text-blue-700",
    action: "bg-violet-100 text-violet-700",
    prompt: "bg-emerald-100 text-emerald-700",
  };
  const statusStyles: Record<string, string> = {
    success: "bg-green-100 text-green-700",
    failure: "bg-red-100 text-red-700",
    cancelled: "bg-slate-100 text-slate-600",
  };
  return (
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
    </tr>
  );
}
