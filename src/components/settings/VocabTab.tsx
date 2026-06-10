import { useRef, useState, type ChangeEvent } from "react";
import {
  importVocabularyCsv,
  type Settings,
  type VocabEntry,
  type VocabImport,
} from "../../lib/settings";
import { Button } from "./ui";

type Props = {
  settings: Settings;
  onUpdate: (vocabulary: VocabEntry[]) => void;
};

export default function VocabTab({ settings, onUpdate }: Props) {
  const vocab = settings.vocabulary ?? [];
  const [spoken, setSpoken] = useState("");
  const [replaceWith, setReplaceWith] = useState("");

  const fileRef = useRef<HTMLInputElement>(null);
  const [preview, setPreview] = useState<VocabImport | null>(null);
  const [importing, setImporting] = useState(false);
  const [importError, setImportError] = useState<string | null>(null);

  function addEntry() {
    const s = spoken.trim();
    const r = replaceWith.trim();
    if (!s || !r) return;
    onUpdate([...vocab, { spoken: s, replace_with: r }]);
    setSpoken("");
    setReplaceWith("");
  }

  function removeEntry(index: number) {
    onUpdate(vocab.filter((_, i) => i !== index));
  }

  async function onFile(e: ChangeEvent<HTMLInputElement>) {
    const file = e.target.files?.[0];
    if (!file) return;
    setImporting(true);
    setImportError(null);
    setPreview(null);
    try {
      if (file.size > 1_048_576) {
        throw new Error("File is too large (max 1 MB). Please trim the CSV and try again.");
      }
      const text = await file.text();
      const result = await importVocabularyCsv(text, vocab);
      setPreview(result);
    } catch (err) {
      setImportError(String(err));
    } finally {
      setImporting(false);
      // Reset so re-selecting the same file fires onChange again.
      e.target.value = "";
    }
  }

  function confirmImport() {
    if (!preview || preview.to_add.length === 0) return;
    onUpdate([...vocab, ...preview.to_add]);
    setPreview(null);
  }

  function cancelImport() {
    setPreview(null);
    setImportError(null);
  }

  const isEmptyResult =
    preview !== null &&
    preview.to_add.length === 0 &&
    preview.skipped.length === 0 &&
    preview.already_existing === 0;

  return (
    <div className="space-y-6">
      <p className="text-sm text-neutral-600">
        Teach Wisspa to fix words the transcriber gets wrong. When Wisspa hears
        the word in the <strong>Heard as</strong> column it substitutes the word
        in <strong>Replace with</strong> before pasting.
      </p>

      <table className="w-full text-sm border-collapse">
        <thead>
          <tr className="border-b border-neutral-200 text-left text-xs text-neutral-500 uppercase tracking-wide">
            <th className="py-2 pr-4 font-medium">Heard as</th>
            <th className="py-2 pr-4 font-medium">Replace with</th>
            <th className="py-2 w-8" />
          </tr>
        </thead>
        <tbody>
          {vocab.length === 0 && (
            <tr>
              <td colSpan={3} className="py-4 text-center text-neutral-400 text-xs">
                No entries yet — add one below.
              </td>
            </tr>
          )}
          {vocab.map((entry, i) => (
            <tr key={i} className="border-b border-neutral-100 group">
              <td className="py-2 pr-4 font-mono text-neutral-700">
                {entry.spoken}
              </td>
              <td className="py-2 pr-4 font-mono text-neutral-900 font-medium">
                {entry.replace_with}
              </td>
              <td className="py-2">
                <button
                  type="button"
                  onClick={() => removeEntry(i)}
                  className="opacity-0 group-hover:opacity-100 focus-visible:opacity-100 transition-opacity text-neutral-400 hover:text-red-500 text-xs px-1"
                  aria-label="Remove entry"
                >
                  ✕
                </button>
              </td>
            </tr>
          ))}
        </tbody>
      </table>

      <div className="flex items-center gap-3">
        <input
          type="text"
          placeholder="Heard as (e.g. Whisper)"
          value={spoken}
          onChange={(e) => setSpoken(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && addEntry()}
          className="flex-1 rounded-md border border-neutral-300 bg-white px-3 py-1.5 text-sm focus:outline-none focus:border-blue-400 focus:ring-2 focus:ring-blue-400/30"
        />
        <span className="text-neutral-400 text-sm">→</span>
        <input
          type="text"
          placeholder="Replace with (e.g. Wisspa)"
          value={replaceWith}
          onChange={(e) => setReplaceWith(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && addEntry()}
          className="flex-1 rounded-md border border-neutral-300 bg-white px-3 py-1.5 text-sm focus:outline-none focus:border-blue-400 focus:ring-2 focus:ring-blue-400/30"
        />
        <Button
          variant="primary"
          onClick={addEntry}
          disabled={!spoken.trim() || !replaceWith.trim()}
        >
          Add
        </Button>
      </div>
      <p className="text-xs text-neutral-400">
        Matching is case-insensitive and whole-word only. Changes take effect on
        the next recording.
      </p>

      <div className="border-t border-neutral-200 pt-5 space-y-3">
        <div className="flex items-center gap-3">
          <Button
            variant="secondary"
            onClick={() => fileRef.current?.click()}
            disabled={importing}
          >
            {importing ? "Importing…" : "Import CSV"}
          </Button>
          <span className="text-xs text-neutral-400">
            CSV format: two columns — spoken,replacement (a header row is
            optional).
          </span>
          <input
            ref={fileRef}
            type="file"
            accept=".csv,text/csv"
            className="hidden"
            onChange={onFile}
          />
        </div>

        {importError && (
          <p className="text-xs text-red-500">{importError}</p>
        )}

        {preview && (
          <div className="rounded-md border border-neutral-200 bg-neutral-50 p-4 space-y-3">
            {isEmptyResult ? (
              <p className="text-sm text-neutral-600">No rows found in that file.</p>
            ) : (
              <p className="text-sm text-neutral-700">
                <strong>{preview.to_add.length}</strong> new term
                {preview.to_add.length === 1 ? "" : "s"} ready ·{" "}
                {preview.already_existing} already in your list ·{" "}
                {preview.skipped.length} skipped
              </p>
            )}

            {preview.to_add.length > 0 && (
              <ul className="text-xs font-mono text-neutral-700 space-y-0.5">
                {preview.to_add.slice(0, 5).map((entry, i) => (
                  <li key={i}>
                    {entry.spoken} → {entry.replace_with}
                  </li>
                ))}
                {preview.to_add.length > 5 && (
                  <li className="text-neutral-400">
                    …and {preview.to_add.length - 5} more
                  </li>
                )}
              </ul>
            )}

            {preview.skipped.length > 0 && (
              <div className="text-xs text-neutral-500 space-y-0.5 max-h-32 overflow-y-auto">
                {preview.skipped.slice(0, 20).map((skip, i) => (
                  <div key={i}>
                    Line {skip.line}: {skip.reason}
                  </div>
                ))}
                {preview.skipped.length > 20 && (
                  <div className="text-neutral-400">
                    …and {preview.skipped.length - 20} more
                  </div>
                )}
              </div>
            )}

            <div className="flex items-center gap-3 pt-1">
              <Button
                variant="primary"
                onClick={confirmImport}
                disabled={preview.to_add.length === 0}
              >
                Add {preview.to_add.length} term
                {preview.to_add.length === 1 ? "" : "s"}
              </Button>
              <Button variant="secondary" onClick={cancelImport}>
                Cancel
              </Button>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
