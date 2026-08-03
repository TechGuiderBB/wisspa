import { useState } from "react";
import { type AppProfile, type Settings } from "../../lib/settings";
import { Button } from "./ui";

type Props = {
  settings: Settings;
  onUpdate: (profiles: AppProfile[]) => void;
};

export default function ProfilesTab({ settings, onUpdate }: Props) {
  const profiles = settings.profiles ?? [];
  const [app, setApp] = useState("");
  const [tone, setTone] = useState("");
  const [vocab, setVocab] = useState("");

  function addProfile() {
    const a = app.trim();
    const t = tone.trim();
    if (!a || !t) return;
    const words = vocab
      .split(",")
      .map((w) => w.trim())
      .filter((w) => w.length > 0);
    onUpdate([...profiles, { app: a, tone: t, vocab: words }]);
    setApp("");
    setTone("");
    setVocab("");
  }

  function removeProfile(index: number) {
    onUpdate(profiles.filter((_, i) => i !== index));
  }

  return (
    <div className="space-y-6">
      <p className="text-sm text-neutral-600">
        Per-app profiles tailor dictation to the app you're speaking into. When
        the active app's name contains the <strong>App match</strong> text, the
        profile's <strong>Tone</strong> note is added to the cleanup prompt and
        its <strong>Vocabulary</strong> words are added to the transcription
        hint (and shielded from global word replacements). Profiles apply to
        dictation only.
      </p>

      <table className="w-full text-sm border-collapse">
        <thead>
          <tr className="border-b border-neutral-200 text-left text-xs text-neutral-500 uppercase tracking-wide">
            <th className="py-2 pr-4 font-medium">App match</th>
            <th className="py-2 pr-4 font-medium">Tone</th>
            <th className="py-2 pr-4 font-medium">Vocabulary</th>
            <th className="py-2 w-8" />
          </tr>
        </thead>
        <tbody>
          {profiles.length === 0 && (
            <tr>
              <td colSpan={4} className="py-4 text-center text-neutral-400 text-xs">
                No profiles yet — add one below.
              </td>
            </tr>
          )}
          {profiles.map((p, i) => (
            <tr key={i} className="border-b border-neutral-100 group">
              <td className="py-2 pr-4 font-mono text-neutral-900 font-medium">
                {p.app}
              </td>
              <td className="py-2 pr-4 text-neutral-700">{p.tone}</td>
              <td className="py-2 pr-4 font-mono text-neutral-700">
                {p.vocab.join(", ")}
              </td>
              <td className="py-2">
                <button
                  type="button"
                  onClick={() => removeProfile(i)}
                  className="opacity-0 group-hover:opacity-100 focus-visible:opacity-100 transition-opacity text-neutral-400 hover:text-red-500 text-xs px-1"
                  aria-label="Remove profile"
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
          placeholder="App match (e.g. Slack)"
          value={app}
          onChange={(e) => setApp(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && addProfile()}
          className="w-40 rounded-md border border-neutral-300 bg-white px-3 py-1.5 text-sm focus:outline-none focus:border-blue-400 focus:ring-2 focus:ring-blue-400/30"
        />
        <input
          type="text"
          placeholder="Tone (e.g. casual, no greetings)"
          value={tone}
          onChange={(e) => setTone(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && addProfile()}
          className="flex-1 rounded-md border border-neutral-300 bg-white px-3 py-1.5 text-sm focus:outline-none focus:border-blue-400 focus:ring-2 focus:ring-blue-400/30"
        />
        <input
          type="text"
          placeholder="Vocabulary (comma-separated)"
          value={vocab}
          onChange={(e) => setVocab(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && addProfile()}
          className="flex-1 rounded-md border border-neutral-300 bg-white px-3 py-1.5 text-sm focus:outline-none focus:border-blue-400 focus:ring-2 focus:ring-blue-400/30"
        />
        <Button
          variant="primary"
          onClick={addProfile}
          disabled={!app.trim() || !tone.trim()}
        >
          Add
        </Button>
      </div>
      <p className="text-xs text-neutral-400">
        Matching is a case-insensitive substring of the active app's name; the
        first matching profile wins. Changes take effect on the next recording.
      </p>
    </div>
  );
}
