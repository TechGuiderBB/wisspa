import { useEffect, useState } from "react";
import {
  getSettings,
  listActions,
  saveSettings,
  type LoadedAction,
  type Settings,
} from "../../lib/settings";
import { Button } from "./ui";

export default function ActionsTab() {
  const [settings, setSettings] = useState<Settings | null>(null);
  const [notesPath, setNotesPath] = useState<string>("");
  const [saving, setSaving] = useState(false);
  const [savedAt, setSavedAt] = useState<number | null>(null);
  const [actions, setActions] = useState<LoadedAction[] | null>(null);

  useEffect(() => {
    getSettings()
      .then((s) => {
        setSettings(s);
        setNotesPath(s.general.notes_path ?? "~/Documents/voice-notes.md");
      })
      .catch(console.error);
    refreshActions();
    const id = setInterval(refreshActions, 5000);
    return () => clearInterval(id);
  }, []);

  async function refreshActions() {
    try {
      setActions(await listActions());
    } catch (err) {
      console.error("listActions failed:", err);
    }
  }

  async function saveNotesPath() {
    if (!settings) return;
    setSaving(true);
    try {
      const next: Settings = {
        ...settings,
        general: { ...settings.general, notes_path: notesPath },
      };
      await saveSettings(next);
      setSettings(next);
      setSavedAt(Date.now());
    } catch (e) {
      console.error("save notes_path failed:", e);
    } finally {
      setSaving(false);
    }
  }

  const dirty = settings ? settings.general.notes_path !== notesPath : false;
  const justSaved = savedAt !== null && Date.now() - savedAt < 2500;

  return (
    <div className="space-y-5">
      <div className="rounded-lg border border-neutral-200 bg-white p-4">
        <h3 className="text-sm font-semibold text-slate-900">Note save location</h3>
        <p className="text-xs text-neutral-600 mt-1 leading-relaxed">
          The "new note" / "make a note" voice command appends a bullet to
          this destination. Point at a <strong>file</strong> (notes get
          appended to it) or a <strong>folder</strong> (notes get appended
          to an <code className="font-mono">Inbox.md</code> inside it).
          Common targets: your Obsidian vault, an iCloud Drive folder, a
          plain text file anywhere on your Mac.
        </p>
        <div className="mt-3 flex items-center gap-2">
          <input
            type="text"
            value={notesPath}
            onChange={(e) => setNotesPath(e.target.value)}
            placeholder="~/Documents/voice-notes.md"
            spellCheck={false}
            className="flex-1 rounded-md border border-neutral-300 px-3 py-1.5 text-sm font-mono bg-white focus:outline-none focus:border-accent focus:ring-2 focus:ring-accent/30"
          />
          <Button
            variant="primary"
            onClick={saveNotesPath}
            disabled={!dirty || saving}
          >
            {saving ? "Saving…" : justSaved ? "Saved" : "Save"}
          </Button>
        </div>
        <div className="mt-2 text-[11px] text-neutral-500">
          <strong>Obsidian tip:</strong> right-click the vault in Obsidian →
          <em> Reveal vault in Finder</em> to find the path. Paste a folder
          like <code className="font-mono">…/MyVault</code> (notes go to{" "}
          <code className="font-mono">Inbox.md</code> inside) or a specific
          file like <code className="font-mono">…/MyVault/Notes/Inbox.md</code>.
          Missing parent folders are created automatically on the first note.
        </div>
      </div>

      <div>
        <div className="flex items-baseline justify-between mb-2">
          <h3 className="text-sm font-semibold text-slate-900">
            Loaded voice actions
          </h3>
          <span className="text-xs text-neutral-500">
            {actions ? `${actions.length} action${actions.length === 1 ? "" : "s"}` : "Loading…"}
          </span>
        </div>
        <p className="text-xs text-neutral-600 mb-3 leading-relaxed">
          Hold the action hotkey and start your sentence with one of the
          trigger phrases below. Anything after the trigger becomes the
          command's argument.
        </p>
        <div className="rounded-lg border border-neutral-200 bg-white overflow-hidden">
          <table className="w-full text-xs">
            <thead className="bg-slate-50 text-slate-600">
              <tr>
                <th className="text-left px-3 py-2 font-medium">Action</th>
                <th className="text-left px-3 py-2 font-medium">Triggers</th>
                <th className="text-left px-3 py-2 font-medium">Type</th>
              </tr>
            </thead>
            <tbody>
              {(actions ?? []).map((a) => (
                <Row key={a.id} action={a} />
              ))}
            </tbody>
          </table>
        </div>
      </div>

      <div className="rounded-md bg-slate-50 border border-slate-200 px-3 py-2 text-xs text-slate-700">
        Full GUI editor for adding, editing, and importing actions is on the
        Phase 4+ roadmap. For now, edit YAML files directly at{" "}
        <code className="font-mono">
          ~/Library/Application Support/com.techguider.wisspa/actions/
        </code>{" "}
        — Wisspa hot-reloads on save.
      </div>
    </div>
  );
}

function Row({ action }: { action: LoadedAction }) {
  const typeStyles: Record<string, string> = {
    shell: "bg-slate-100 text-slate-700",
    applescript: "bg-violet-100 text-violet-700",
    open_url: "bg-blue-100 text-blue-700",
    open_app: "bg-emerald-100 text-emerald-700",
    keystroke: "bg-amber-100 text-amber-700",
  };
  return (
    <tr className="border-t border-slate-100 align-top">
      <td className="px-3 py-2">
        <div className="font-medium text-slate-900">{action.name}</div>
        {action.destructive && (
          <span className="text-[10px] font-semibold text-red-600 uppercase tracking-wide">
            destructive
          </span>
        )}
      </td>
      <td className="px-3 py-2">
        <div className="flex flex-wrap gap-1">
          {action.triggers.map((t) => (
            <code
              key={t}
              className="font-mono text-[11px] bg-slate-50 border border-slate-200 px-1.5 py-0.5 rounded"
            >
              {t}
            </code>
          ))}
        </div>
      </td>
      <td className="px-3 py-2">
        <span
          className={`text-[10px] font-semibold uppercase px-1.5 py-0.5 rounded ${
            typeStyles[action.type] ?? "bg-slate-100 text-slate-600"
          }`}
        >
          {action.type}
        </span>
      </td>
    </tr>
  );
}
