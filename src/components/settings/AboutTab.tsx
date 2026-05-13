import { useEffect, useState } from "react";
import {
  getPermissions,
  getSettings,
  openSystemSettings,
  type MicCalibration,
  type PermissionPane,
  type PermissionStatus,
  type PermissionsSnapshot,
} from "../../lib/settings";

export default function AboutTab() {
  const [perms, setPerms] = useState<PermissionsSnapshot | null>(null);
  const [cal, setCal] = useState<MicCalibration | null>(null);

  async function refresh() {
    try {
      setPerms(await getPermissions());
      const s = await getSettings();
      setCal(s.mic_calibration);
    } catch (err) {
      console.error("about refresh failed:", err);
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

      <Section title="Mic calibration">
        {cal ? (
          <div className="rounded-lg border border-neutral-200 bg-white px-4 py-3 text-xs text-neutral-700">
            <div>
              Last calibrated{" "}
              <strong>{new Date(cal.calibrated_at).toLocaleString()}</strong>
            </div>
            <div className="mt-1 text-neutral-500">
              Silence peak threshold:{" "}
              <code className="font-mono">{cal.silence_peak.toFixed(1)}</code> ·
              Min bytes/sec:{" "}
              <code className="font-mono">
                {cal.min_bytes_per_second.toLocaleString()}
              </code>
            </div>
            <div className="mt-1 text-neutral-500">
              Re-calibrate any time from Settings → General.
            </div>
          </div>
        ) : (
          <div className="rounded-lg border border-neutral-200 bg-white px-4 py-3 text-xs text-neutral-500">
            Not yet calibrated. Defaults will be used until you run the
            calibration step (Settings → General → Re-calibrate mic).
          </div>
        )}
      </Section>

      <Section title="Platform">
        <ul className="text-xs space-y-1 text-neutral-700">
          <li>macOS 13 or later · Apple Silicon</li>
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
