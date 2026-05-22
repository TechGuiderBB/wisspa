import { useState } from "react";
import { type Settings, type VocabEntry } from "../../lib/settings";
import { Button } from "./ui";

type Props = {
  settings: Settings;
  onUpdate: (vocabulary: VocabEntry[]) => void;
};

export default function VocabTab({ settings, onUpdate }: Props) {
  const vocab = settings.vocabulary ?? [];
  const [spoken, setSpoken] = useState("");
  const [replaceWith, setReplaceWith] = useState("");

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
                  className="opacity-0 group-hover:opacity-100 transition-opacity text-neutral-400 hover:text-red-500 text-xs px-1"
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
    </div>
  );
}
