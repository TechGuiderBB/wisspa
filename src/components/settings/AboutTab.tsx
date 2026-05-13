import { useEffect, useState } from "react";
import {
  getPermissions,
  openSystemSettings,
  type PermissionPane,
  type PermissionStatus,
  type PermissionsSnapshot,
} from "../../lib/settings";

export default function AboutTab() {
  const [perms, setPerms] = useState<PermissionsSnapshot | null>(null);

  async function refresh() {
    try {
      setPerms(await getPermissions());
    } catch (err) {
      console.error("getPermissions failed:", err);
    }
  }

  useEffect(() => {
    refresh();
    const id = setInterval(refresh, 4000);
    return () => clearInterval(id);
  }, []);

  return (
    <div className="space-y-5">
      <div className="rounded-xl bg-wisspa-gradient text-white p-5 shadow-lg">
        <div className="flex items-center gap-3">
          <div className="h-10 w-10 rounded-xl bg-white/20 backdrop-blur flex items-center justify-center font-bold text-lg">
            W
          </div>
          <div>
            <div className="text-lg font-bold">Wisspa</div>
            <div className="text-xs opacity-90">
              System-wide AI voice tool for macOS · v0.1.0
            </div>
          </div>
        </div>
      </div>

      <Section title="Permissions">
        <div className="rounded-lg border border-neutral-200 bg-white divide-y">
          <PermRow
            pane="microphone"
            label="Microphone"
            hint="Capture speech while a hotkey is held."
            status={perms?.microphone ?? "unknown"}
          />
          <PermRow
            pane="accessibility"
            label="Accessibility"
            hint="Simulate Cmd+V to paste into the focused app."
            status={perms?.accessibility ?? "unknown"}
          />
          <PermRow
            pane="screen_recording"
            label="Screen Recording"
            hint="Optional — for screenshot voice actions."
            status={perms?.screen_recording ?? "unknown"}
          />
          <PermRow
            pane="automation"
            label="Automation (System Events)"
            hint="Detect active app · run AppleScript actions."
            status={perms?.automation ?? "unknown"}
          />
        </div>
        <button
          onClick={refresh}
          className="mt-2 text-xs underline decoration-dotted text-neutral-600 hover:text-neutral-900"
        >
          Refresh
        </button>
      </Section>

      <Section title="Stack">
        <ul className="text-xs space-y-1 text-neutral-700">
          <li>
            <span className="font-semibold">STT:</span> Groq Whisper (
            <code className="font-mono text-[11px]">whisper-large-v3-turbo</code>)
          </li>
          <li>
            <span className="font-semibold">Cleanup:</span> Claude Haiku 4.5
          </li>
          <li>
            <span className="font-semibold">Prompt rewrites:</span> Claude Sonnet 4.6
          </li>
          <li>
            <span className="font-semibold">Target:</span> macOS 13+ on Apple Silicon
          </li>
        </ul>
      </Section>
    </div>
  );
}

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <div>
      <div className="text-xs font-semibold uppercase tracking-wide text-neutral-500 mb-1.5">
        {title}
      </div>
      <div>{children}</div>
    </div>
  );
}

function PermRow({
  pane,
  label,
  hint,
  status,
}: {
  pane: PermissionPane;
  label: string;
  hint: string;
  status: PermissionStatus;
}) {
  return (
    <div className="flex items-center gap-3 px-4 py-3">
      <div className="flex-1">
        <div className="text-sm font-medium">{label}</div>
        <div className="text-xs text-neutral-500">{hint}</div>
      </div>
      <StatusPill status={status} />
      <button
        type="button"
        onClick={() => void openSystemSettings(pane)}
        className="text-xs underline decoration-dotted text-accent"
      >
        Open settings
      </button>
    </div>
  );
}

function StatusPill({ status }: { status: PermissionStatus }) {
  const styles: Record<PermissionStatus, string> = {
    granted: "bg-green-100 text-green-700",
    denied: "bg-amber-100 text-amber-700",
    unknown: "bg-neutral-100 text-neutral-500",
  };
  const labels: Record<PermissionStatus, string> = {
    granted: "Granted",
    denied: "Needs perm",
    unknown: "Unknown",
  };
  return (
    <span
      className={`text-[11px] font-medium px-2 py-0.5 rounded-full ${styles[status]}`}
    >
      {labels[status]}
    </span>
  );
}
