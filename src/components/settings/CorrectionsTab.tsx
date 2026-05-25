import { useEffect, useState } from "react";
import {
  WordCorrections,
  getWordCorrections,
  saveWordCorrections,
} from "../../lib/settings";
import { Button, Row, Toggle } from "./ui";

export default function CorrectionsTab() {
  const [corrections, setCorrections] = useState<WordCorrections | null>(null);
  const [newOriginal, setNewOriginal] = useState("");
  const [newReplacement, setNewReplacement] = useState("");
  const [busy, setBusy] = useState(false);
  const [addError, setAddError] = useState("");
  const [listError, setListError] = useState("");

  async function refresh() {
    try {
      setCorrections(await getWordCorrections());
    } catch (err) {
      console.error("getWordCorrections failed:", err);
    }
  }

  useEffect(() => {
    refresh();
  }, []);

  async function onToggleEnabled() {
    if (!corrections) return;
    const next = { ...corrections, enabled: !corrections.enabled };
    setCorrections(next);
    await saveWordCorrections(next).catch(console.error);
  }

  async function onAdd() {
    if (busy || !corrections) return;
    const orig = newOriginal.trim().toLowerCase();
    const repl = newReplacement.trim();
    if (!orig || !repl) {
      setAddError("Both fields are required.");
      return;
    }
    setAddError("");
    setBusy(true);
    try {
      // An explicitly-added correction is active immediately. Write the entry
      // directly with count pinned at the threshold rather than looping submit
      // IPC calls — that would over-increment the learning count.
      const existing = corrections.entries[orig];
      const count = Math.max(corrections.threshold, existing?.count ?? 0);
      const next: WordCorrections = {
        ...corrections,
        entries: {
          ...corrections.entries,
          [orig]: { replacement: repl, count, auto_apply: true },
        },
      };
      await saveWordCorrections(next);
      setCorrections(next);
      setNewOriginal("");
      setNewReplacement("");
    } catch (err) {
      setAddError(String(err));
    } finally {
      setBusy(false);
    }
  }

  async function onDelete(original: string) {
    if (busy || !corrections) return;
    setListError("");
    setBusy(true);
    try {
      const next: WordCorrections = {
        ...corrections,
        entries: Object.fromEntries(
          Object.entries(corrections.entries).filter(([k]) => k !== original),
        ),
      };
      await saveWordCorrections(next);
      setCorrections(next);
    } catch (err) {
      console.error("saveWordCorrections (delete) failed:", err);
      setListError(`Could not remove correction: ${String(err)}`);
      // Resync from disk so the row doesn't appear deleted when it isn't.
      await refresh();
    } finally {
      setBusy(false);
    }
  }

  if (!corrections) {
    return <div className="text-sm text-slate-500">Loading corrections…</div>;
  }

  const entries = Object.entries(corrections.entries).sort(([a], [b]) =>
    a.localeCompare(b),
  );

  return (
    <div className="space-y-6">
      <div className="divide-y divide-neutral-100">
        <Row
          label="Enable word corrections"
          hint="Apply learned corrections automatically during dictation"
        >
          <Toggle checked={corrections.enabled} onChange={onToggleEnabled} />
        </Row>
        <Row
          label="Auto-apply threshold"
          hint="How many times a correction must be submitted before it is applied automatically"
        >
          <input
            type="number"
            min={1}
            max={10}
            value={corrections.threshold}
            onChange={(e) => {
              const next = {
                ...corrections,
                threshold: Math.min(10, Math.max(1, Number(e.target.value))),
              };
              setCorrections(next);
              saveWordCorrections(next).catch(console.error);
            }}
            className="w-16 rounded-md border border-neutral-300 bg-white px-2 py-1 text-sm focus:outline-none focus:border-accent focus:ring-2 focus:ring-accent/30"
          />
          <span className="text-xs text-neutral-500">submissions</span>
        </Row>
      </div>

      <div>
        <h3 className="text-sm font-semibold text-neutral-700 mb-3">
          Add correction
        </h3>
        <div className="flex items-center gap-2 flex-wrap">
          <input
            type="text"
            placeholder="Whisper says…"
            value={newOriginal}
            onChange={(e) => setNewOriginal(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && onAdd()}
            className="rounded-md border border-neutral-300 bg-white px-2 py-1.5 text-sm w-36 focus:outline-none focus:border-accent focus:ring-2 focus:ring-accent/30"
          />
          <span className="text-neutral-400 text-sm">→</span>
          <input
            type="text"
            placeholder="Should be…"
            value={newReplacement}
            onChange={(e) => setNewReplacement(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && onAdd()}
            className="rounded-md border border-neutral-300 bg-white px-2 py-1.5 text-sm w-36 focus:outline-none focus:border-accent focus:ring-2 focus:ring-accent/30"
          />
          <Button onClick={onAdd} disabled={busy}>
            Add
          </Button>
        </div>
        {addError && (
          <p className="text-xs text-red-500 mt-1">{addError}</p>
        )}
      </div>

      <div>
        <h3 className="text-sm font-semibold text-neutral-700 mb-3">
          Corrections ({entries.length})
        </h3>
        {listError && (
          <p className="text-xs text-red-500 mb-2">{listError}</p>
        )}
        {entries.length === 0 ? (
          <p className="text-sm text-neutral-400">
            No corrections yet. Add one above.
          </p>
        ) : (
          <div className="rounded-lg border border-neutral-200 overflow-hidden">
            <table className="w-full text-sm">
              <thead>
                <tr className="bg-neutral-50 text-left text-xs text-neutral-500 uppercase tracking-wide">
                  <th className="px-4 py-2 font-medium">Hears</th>
                  <th className="px-4 py-2 font-medium">Inserts</th>
                  <th className="px-4 py-2 font-medium text-center">
                    Times
                  </th>
                  <th className="px-4 py-2 font-medium text-center">
                    Active
                  </th>
                  <th className="px-4 py-2" />
                </tr>
              </thead>
              <tbody className="divide-y divide-neutral-100">
                {entries.map(([original, entry]) => (
                  <tr key={original} className="hover:bg-neutral-50">
                    <td className="px-4 py-2 font-mono text-neutral-700">
                      {original}
                    </td>
                    <td className="px-4 py-2 font-mono text-neutral-900 font-medium">
                      {entry.replacement}
                    </td>
                    <td className="px-4 py-2 text-center text-neutral-500">
                      {entry.count}
                    </td>
                    <td className="px-4 py-2 text-center">
                      {entry.auto_apply ? (
                        <span className="inline-block h-2 w-2 rounded-full bg-green-500" />
                      ) : (
                        <span
                          className="inline-block h-2 w-2 rounded-full bg-neutral-300"
                          title={`${corrections.threshold - entry.count} more submission(s) needed`}
                        />
                      )}
                    </td>
                    <td className="px-4 py-2 text-right">
                      <Button
                        variant="ghost"
                        onClick={() => onDelete(original)}
                        disabled={busy}
                        className="text-xs text-red-500 hover:text-red-700"
                      >
                        Remove
                      </Button>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </div>

      <p className="text-xs text-neutral-400">
        Corrections apply during dictation mode. Whisper often mishears proper
        nouns, acronyms, and technical terms — add them here. A green dot means
        the correction is active.
      </p>
    </div>
  );
}
