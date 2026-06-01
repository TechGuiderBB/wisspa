import { useEffect, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { invoke } from "@tauri-apps/api/core";
import { check } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import {
  getPermissions,
  getSettings,
  openSystemSettings,
  type MicCalibration,
  type PermissionPane,
  type PermissionStatus,
  type PermissionsSnapshot,
} from "../../lib/settings";

type UpdateState =
  | { kind: "idle" }
  | { kind: "checking" }
  | { kind: "uptodate" }
  | { kind: "available"; version: string; notes: string | null }
  | { kind: "downloading"; progress: number }
  | { kind: "ready" }
  | { kind: "error"; message: string };

export default function AboutTab() {
  const [perms, setPerms] = useState<PermissionsSnapshot | null>(null);
  const [cal, setCal] = useState<MicCalibration | null>(null);
  const [update, setUpdate] = useState<UpdateState>({ kind: "idle" });
  const [appVersion, setAppVersion] = useState<string>("…");
  const [diag, setDiag] = useState<
    { kind: "idle" } | { kind: "busy" } | { kind: "done"; path: string } | { kind: "error"; message: string }
  >({ kind: "idle" });

  async function exportDiagnostics() {
    setDiag({ kind: "busy" });
    try {
      const path = await invoke<string>("export_diagnostics");
      setDiag({ kind: "done", path });
    } catch (err) {
      setDiag({ kind: "error", message: String(err) });
    }
  }

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
    getVersion().then(setAppVersion).catch(() => {});
    refresh();
    const id = setInterval(refresh, 4000);
    return () => clearInterval(id);
  }, []);

  async function checkForUpdates() {
    setUpdate({ kind: "checking" });
    try {
      const result = await check();
      if (!result) {
        setUpdate({ kind: "uptodate" });
        return;
      }
      setUpdate({
        kind: "available",
        version: result.version,
        notes: result.body ?? null,
      });
    } catch (err) {
      setUpdate({ kind: "error", message: String(err) });
    }
  }

  async function downloadAndInstall() {
    try {
      const result = await check();
      if (!result) {
        setUpdate({ kind: "error", message: "Update no longer available — try checking again." });
        return;
      }
      let downloaded = 0;
      let total = 0;
      setUpdate({ kind: "downloading", progress: 0 });
      await result.downloadAndInstall((event) => {
        if (event.event === "Started") {
          total = event.data.contentLength ?? 0;
        } else if (event.event === "Progress") {
          downloaded += event.data.chunkLength;
          const pct = total > 0 ? Math.round((downloaded / total) * 100) : 0;
          setUpdate({ kind: "downloading", progress: pct });
        }
      });
      setUpdate({ kind: "ready" });
      await relaunch();
    } catch (err) {
      setUpdate({ kind: "error", message: String(err) });
    }
  }

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
              System-wide AI voice tool for macOS · v{appVersion}
            </div>
          </div>
        </div>
      </div>

      <Section title="Updates">
        <div className="rounded-lg border border-neutral-200 bg-white px-4 py-3 flex items-center gap-3">
          <div className="flex-1 text-xs text-neutral-700">
            <UpdateLabel state={update} />
          </div>
          {update.kind === "available" ? (
            <button
              onClick={downloadAndInstall}
              className="rounded-md bg-wisspa-gradient px-3 py-1.5 text-xs font-semibold text-white shadow-sm hover:brightness-110"
            >
              Install v{update.version}
            </button>
          ) : (
            <button
              onClick={checkForUpdates}
              disabled={update.kind === "checking" || update.kind === "downloading"}
              className="rounded-md border border-neutral-300 bg-white px-3 py-1.5 text-xs font-medium text-slate-700 hover:bg-slate-50 disabled:opacity-50"
            >
              {update.kind === "checking" ? "Checking…" : "Check for updates"}
            </button>
          )}
        </div>
      </Section>

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

      <Section title="Diagnostics">
        <div className="rounded-lg border border-neutral-200 bg-white px-4 py-3 flex items-center gap-3">
          <div className="flex-1 text-xs text-neutral-700">
            Export a <code className="font-mono">.zip</code> of redacted logs, app
            version, permission state and hotkeys for a bug report. Transcripts and
            clipboard text are not included unless Verbose logging is on.
            {diag.kind === "done" && (
              <div className="mt-1 text-emerald-700 break-all">Saved to {diag.path}</div>
            )}
            {diag.kind === "error" && (
              <div className="mt-1 text-red-600 break-all">{diag.message}</div>
            )}
          </div>
          <button
            type="button"
            onClick={() => void exportDiagnostics()}
            disabled={diag.kind === "busy"}
            className="rounded-md border border-neutral-300 bg-white px-3 py-1.5 text-xs font-medium text-slate-700 hover:bg-slate-50 disabled:opacity-50"
          >
            {diag.kind === "busy" ? "Exporting…" : "Export Diagnostics"}
          </button>
        </div>
      </Section>

      <Section title="Platform">
        <ul className="text-xs space-y-1 text-neutral-700">
          <li>macOS 13 or later · Apple Silicon</li>
        </ul>
      </Section>
    </div>
  );
}

function UpdateLabel({ state }: { state: UpdateState }) {
  switch (state.kind) {
    case "idle":
      return <span>Check for newer releases.</span>;
    case "checking":
      return <span>Checking for updates…</span>;
    case "uptodate":
      return <span className="text-emerald-700">You're on the latest version.</span>;
    case "available":
      return (
        <span>
          <span className="font-semibold text-slate-900">v{state.version} available.</span>{" "}
          {state.notes && <span className="text-neutral-500">{state.notes.slice(0, 80)}…</span>}
        </span>
      );
    case "downloading":
      return <span>Downloading update… {state.progress}%</span>;
    case "ready":
      return <span className="text-emerald-700">Installed. Relaunching…</span>;
    case "error":
      return <span className="text-red-600">{state.message}</span>;
  }
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
