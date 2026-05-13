import { useEffect, useState } from "react";
import {
  API_KEY_NAMES,
  API_PROVIDERS,
  ApiProvider,
  getApiKeyPresent,
  saveApiKey,
  testApiKey,
} from "../../lib/settings";
import { Button } from "./ui";

type Status =
  | { kind: "idle" }
  | { kind: "saving" }
  | { kind: "saved" }
  | { kind: "testing" }
  | { kind: "ok"; message: string }
  | { kind: "err"; message: string };

export default function ApiKeysTab() {
  return (
    <div className="space-y-4">
      {API_PROVIDERS.map((p) => (
        <KeyCard key={p} provider={p} />
      ))}
      <p className="text-xs text-neutral-500">
        Keys are stored in the macOS Keychain under service{" "}
        <code className="font-mono">Wisspa</code>. Saving an empty field removes
        the stored key.
      </p>
    </div>
  );
}

function KeyCard({ provider }: { provider: ApiProvider }) {
  const name = API_KEY_NAMES[provider];
  const [value, setValue] = useState("");
  const [present, setPresent] = useState<boolean | null>(null);
  const [status, setStatus] = useState<Status>({ kind: "idle" });

  useEffect(() => {
    getApiKeyPresent(name).then(setPresent).catch(() => setPresent(false));
  }, [name]);

  async function onSave() {
    setStatus({ kind: "saving" });
    try {
      await saveApiKey(name, value);
      setPresent(value.length > 0);
      setValue("");
      setStatus({ kind: "saved" });
    } catch (e) {
      setStatus({ kind: "err", message: String(e) });
    }
  }

  async function onTest() {
    if (!value) {
      setStatus({ kind: "err", message: "Enter a key above to test it." });
      return;
    }
    setStatus({ kind: "testing" });
    try {
      const msg = await testApiKey(provider, value);
      setStatus({ kind: "ok", message: msg });
    } catch (e) {
      setStatus({ kind: "err", message: String(e) });
    }
  }

  const link =
    provider === "groq"
      ? "https://console.groq.com/keys"
      : "https://console.anthropic.com/settings/keys";
  const providerName = provider === "groq" ? "Groq" : "Anthropic";

  return (
    <div className="rounded-lg border border-neutral-200 bg-white p-4">
      <div className="flex items-baseline justify-between gap-4">
        <div>
          <h3 className="font-semibold text-sm">{providerName}</h3>
          <p className="text-xs text-neutral-500 mt-0.5">
            <code className="font-mono">{name}</code> ·{" "}
            <a
              href={link}
              target="_blank"
              rel="noreferrer"
              className="underline decoration-dotted text-accent"
            >
              Get a key
            </a>
          </p>
        </div>
        <span
          className={`text-xs px-2 py-0.5 rounded-full font-medium ${
            present === null
              ? "bg-neutral-100 text-neutral-500"
              : present
              ? "bg-green-100 text-green-700"
              : "bg-amber-100 text-amber-700"
          }`}
        >
          {present === null ? "…" : present ? "Saved" : "Not set"}
        </span>
      </div>

      <div className="mt-3 flex items-center gap-2">
        <input
          type="password"
          value={value}
          onChange={(e) => setValue(e.target.value)}
          placeholder={present ? "Enter a new key to replace" : "Paste your API key"}
          className="flex-1 rounded-md border border-neutral-300 px-3 py-1.5 text-sm font-mono bg-white focus:outline-none focus:border-accent focus:ring-2 focus:ring-accent/30"
        />
        <Button variant="primary" onClick={onSave} disabled={status.kind === "saving"}>
          Save
        </Button>
        <Button variant="secondary" onClick={onTest} disabled={status.kind === "testing"}>
          Test
        </Button>
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
          {statusMessage(status)}
        </div>
      )}
    </div>
  );
}

function statusMessage(s: Status): string {
  switch (s.kind) {
    case "saving":
      return "Saving…";
    case "saved":
      return "Saved to Keychain.";
    case "testing":
      return "Testing…";
    case "ok":
      return s.message;
    case "err":
      return s.message;
    default:
      return "";
  }
}
