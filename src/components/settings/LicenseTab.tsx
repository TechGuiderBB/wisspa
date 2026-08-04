import { useEffect, useState } from "react";
import {
  deleteLicenseKey,
  getLicenseKeyPresent,
  getLicenseStatus,
  saveLicenseKey,
  validateLicense,
  type LicenseState,
  type LicenseStatus,
} from "../../lib/settings";
import { Button } from "./ui";

type Status =
  | { kind: "idle" }
  | { kind: "working"; message: string }
  | { kind: "ok"; message: string }
  | { kind: "err"; message: string };

export default function LicenseTab() {
  const [key, setKey] = useState("");
  const [present, setPresent] = useState<boolean | null>(null);
  const [license, setLicense] = useState<LicenseStatus | null>(null);
  const [status, setStatus] = useState<Status>({ kind: "idle" });

  useEffect(() => {
    let cancelled = false;
    async function load() {
      const [keyPresent, cached] = await Promise.all([
        getLicenseKeyPresent().catch(() => false),
        getLicenseStatus().catch(() => null),
      ]);
      if (cancelled) return;
      setPresent(keyPresent);
      setLicense(cached);
      // A stored key with no verdict yet (fresh save, or a cleared cache):
      // validate once on open so the panel shows the truth, not a stale no_key.
      if (keyPresent && (!cached || cached.state === "no_key")) {
        setStatus({ kind: "working", message: "Checking license…" });
        try {
          const fresh = await validateLicense();
          if (!cancelled) setLicense(fresh);
        } catch (e) {
          if (!cancelled) setStatus({ kind: "err", message: String(e) });
        } finally {
          if (!cancelled) setStatus({ kind: "idle" });
        }
      }
    }
    load();
    return () => {
      cancelled = true;
    };
  }, []);

  async function onSaveValidate() {
    if (!key.trim()) {
      setStatus({ kind: "err", message: "Paste your license key first." });
      return;
    }
    setStatus({ kind: "working", message: "Saving & validating…" });
    try {
      await saveLicenseKey(key.trim());
      setPresent(true);
      setKey("");
      const fresh = await validateLicense();
      setLicense(fresh);
      setStatus(
        fresh.state === "active"
          ? { kind: "ok", message: "Saved · license validated." }
          : { kind: "err", message: `Saved · ${stateMessage(fresh)}` },
      );
    } catch (e) {
      setStatus({ kind: "err", message: String(e) });
    }
  }

  async function onRecheck() {
    setStatus({ kind: "working", message: "Checking license…" });
    try {
      const fresh = await validateLicense();
      setLicense(fresh);
      setStatus({ kind: "idle" });
    } catch (e) {
      setStatus({ kind: "err", message: String(e) });
    }
  }

  async function onRemove() {
    if (
      !confirm(
        "Remove the license key from this Mac? Wisspa keeps working without it.",
      )
    )
      return;
    setStatus({ kind: "working", message: "Removing…" });
    try {
      await deleteLicenseKey();
      setPresent(false);
      setKey("");
      setLicense(await getLicenseStatus());
      setStatus({ kind: "ok", message: "License removed." });
    } catch (e) {
      setStatus({ kind: "err", message: String(e) });
    }
  }

  const badge = license ? stateBadge(license.state) : null;

  return (
    <div className="space-y-4">
      <div className="rounded-lg border border-neutral-200 bg-white p-4">
        <div className="flex items-baseline justify-between gap-4">
          <div>
            <h3 className="font-semibold text-sm">Supporter License</h3>
            <p className="text-xs text-neutral-500 mt-0.5">
              A$24.99 once. Signed, auto-updating builds + you fund
              open-source development. Wisspa works without it.{" "}
              <a
                href="https://www.wisspa.app/download"
                target="_blank"
                rel="noreferrer"
                className="underline decoration-dotted text-accent"
              >
                Buy a license →
              </a>
            </p>
          </div>
          <span
            className={`text-xs px-2 py-0.5 rounded-full font-medium ${
              badge?.classes ?? "bg-neutral-100 text-neutral-500"
            }`}
          >
            {badge?.label ?? "…"}
          </span>
        </div>

        <div className="mt-3 flex items-center gap-2">
          <input
            type="password"
            value={key}
            onChange={(e) => setKey(e.target.value)}
            placeholder={
              present ? "Enter a new key to replace" : "Paste your license key"
            }
            className="flex-1 rounded-md border border-neutral-300 px-3 py-1.5 text-sm font-mono bg-white focus:outline-none focus:border-accent focus:ring-2 focus:ring-accent/30"
          />
          <button
            type="button"
            onClick={onSaveValidate}
            disabled={!key.trim() || status.kind === "working"}
            className="rounded-md bg-wisspa-gradient px-3 py-1.5 text-sm font-semibold text-white disabled:opacity-40 disabled:cursor-not-allowed hover:brightness-110 transition"
          >
            Save &amp; validate
          </button>
          {present && (
            <Button
              variant="secondary"
              onClick={onRecheck}
              disabled={status.kind === "working"}
            >
              Re-check
            </Button>
          )}
        </div>

        {status.kind !== "idle" && (
          <div
            className={`mt-2 text-xs ${
              status.kind === "err"
                ? "text-red-600"
                : status.kind === "ok"
                ? "text-green-700"
                : "text-neutral-500"
            }`}
          >
            {statusText(status)}
          </div>
        )}

        {license && (
          <div className="mt-3 flex items-start gap-2.5 rounded-md border border-neutral-100 bg-neutral-50 px-3 py-2.5">
            <span
              aria-hidden
              className={`mt-px text-sm font-bold ${stateIcon(license.state).classes}`}
            >
              {stateIcon(license.state).glyph}
            </span>
            <div className="min-w-0">
              <div className="text-sm text-neutral-800">
                {stateMessage(license)}
              </div>
              {license.last_checked_at > 0 && (
                <div className="text-xs text-neutral-500 mt-0.5">
                  Checked{" "}
                  {new Date(license.last_checked_at * 1000).toLocaleString()}
                </div>
              )}
            </div>
          </div>
        )}

        {present && (
          <div className="mt-3">
            <Button
              variant="ghost"
              onClick={onRemove}
              disabled={status.kind === "working"}
              className="text-red-600 hover:text-red-700"
            >
              Remove license
            </Button>
          </div>
        )}
      </div>
      <p className="text-xs text-neutral-500">
        The key is stored in the macOS Keychain under service{" "}
        <code className="font-mono">Wisspa</code>. Validation sends the key, a
        per-install id and this Mac's name to the license server — nothing else.
      </p>
    </div>
  );
}

function statusText(s: Status): string {
  return s.kind === "idle" ? "" : s.message;
}

function stateBadge(state: LicenseState): { label: string; classes: string } {
  switch (state) {
    case "active":
      return { label: "Active", classes: "bg-green-100 text-green-700" };
    case "no_key":
      return { label: "No license", classes: "bg-neutral-100 text-neutral-500" };
    case "unreachable":
      return { label: "Unreachable", classes: "bg-amber-100 text-amber-700" };
    case "server_error":
      return { label: "Server error", classes: "bg-amber-100 text-amber-700" };
    // not_found | refunded | disabled | activation_limit
    default:
      return { label: "Problem", classes: "bg-red-100 text-red-700" };
  }
}

function stateIcon(state: LicenseState): { glyph: string; classes: string } {
  switch (state) {
    case "active":
      return { glyph: "✓", classes: "text-green-600" };
    case "no_key":
      return { glyph: "○", classes: "text-neutral-400" };
    case "unreachable":
    case "server_error":
      return { glyph: "⚠", classes: "text-amber-600" };
    default:
      return { glyph: "✕", classes: "text-red-600" };
  }
}

function stateMessage(s: LicenseStatus): string {
  switch (s.state) {
    case "active": {
      const used = s.activations_used;
      const limit = s.activations_limit;
      if (used != null && limit != null) {
        return `Supporter License active · ${used} of ${limit} machines`;
      }
      if (used != null) {
        return `Supporter License active · active on ${used} ${
          used === 1 ? "machine" : "machines"
        }`;
      }
      return "Supporter License active";
    }
    case "no_key":
      return "No license entered";
    case "not_found":
      return "Not recognised — check for typos";
    case "refunded":
      return "This key was refunded";
    case "disabled":
      return "This key has been disabled";
    case "activation_limit":
      return "Already active on the maximum number of machines";
    case "unreachable":
      return "Couldn't reach the license server — your license is unaffected";
    case "server_error":
      return "The license server had a problem — try again shortly";
  }
}
