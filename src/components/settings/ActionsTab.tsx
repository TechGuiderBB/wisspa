export default function ActionsTab() {
  return (
    <div className="space-y-4">
      <div className="rounded-md bg-amber-50 border border-amber-200 px-3 py-2 text-xs text-amber-900">
        Action mode is implemented in <strong>Phase 4</strong>. The Settings UI
        for managing custom actions and the runtime matcher both come online
        together.
      </div>

      <div className="rounded-lg border border-neutral-200 p-4">
        <h3 className="text-sm font-semibold">What Action Mode does</h3>
        <p className="text-xs text-neutral-600 mt-1 leading-relaxed">
          You hold the action hotkey and speak a command. Wisspa matches your
          phrase against an editable registry of voice actions and runs the
          matching one. Shipped defaults will include:
        </p>
        <ul className="text-xs text-neutral-700 mt-2 grid grid-cols-2 gap-x-4 gap-y-1 list-disc pl-4">
          <li>"screenshot"</li>
          <li>"open Cursor"</li>
          <li>"search Google for…"</li>
          <li>"search GitHub for…"</li>
          <li>"new note…"</li>
          <li>"start screen recording"</li>
          <li>"copy that" / "paste"</li>
          <li>"lock screen"</li>
          <li>"mute audio"</li>
          <li>…and a few more.</li>
        </ul>
        <p className="text-xs text-neutral-600 mt-2 leading-relaxed">
          Actions live as YAML files in{" "}
          <code className="font-mono">
            ~/Library/Application Support/Wisspa/actions/
          </code>
          . When Phase 4 ships, you'll be able to add, edit, import, and export
          them from this tab. Wisspa hot-reloads any change to that folder, so
          you can also drop YAML in directly.
        </p>
      </div>
    </div>
  );
}
