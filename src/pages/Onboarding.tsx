import { useEffect, useState } from "react";
import {
  API_KEY_NAMES,
  API_PROVIDERS,
  ApiProvider,
  PermissionPane,
  PermissionsSnapshot,
  PermissionStatus,
  completeOnboarding,
  getPermissions,
  getSettings,
  openSystemSettings,
  reportMicrophoneStatus,
  requestScreenRecordingAccess,
  saveApiKey,
  saveSettings,
  testApiKey,
} from "../lib/settings";
import { sampleAmbient, sampleSpeech } from "../lib/audio";

type Step =
  | "welcome"
  | "microphone"
  | "accessibility"
  | "screen_recording"
  | "automation"
  | "api_keys"
  | "hotkeys"
  | "test";

const ORDER: Step[] = [
  "welcome",
  "microphone",
  "accessibility",
  "screen_recording",
  "automation",
  "api_keys",
  "hotkeys",
  "test",
];

export default function OnboardingPage() {
  const [step, setStep] = useState<Step>("welcome");
  const [perms, setPerms] = useState<PermissionsSnapshot | null>(null);

  async function refreshPerms() {
    try {
      setPerms(await getPermissions());
    } catch (err) {
      console.error("getPermissions failed:", err);
    }
  }

  useEffect(() => {
    refreshPerms();
    const id = setInterval(refreshPerms, 2000);
    return () => clearInterval(id);
  }, []);

  const idx = ORDER.indexOf(step);
  const total = ORDER.length;

  function next() {
    const i = ORDER.indexOf(step);
    if (i + 1 < ORDER.length) setStep(ORDER[i + 1]);
  }
  function back() {
    const i = ORDER.indexOf(step);
    if (i > 0) setStep(ORDER[i - 1]);
  }

  return (
    <div className="h-screen w-screen flex flex-col bg-slate-100 text-slate-900">
      <header className="px-8 py-4 border-b border-slate-200 bg-white flex items-center gap-3 shadow-sm">
        <div className="h-9 w-9 rounded-xl bg-wisspa-gradient flex items-center justify-center text-white font-bold shadow-md">
          W
        </div>
        <div className="flex-1">
          <div className="font-semibold text-sm text-slate-900">
            Wisspa onboarding
          </div>
          <div className="text-xs text-slate-500">
            Step {idx + 1} of {total}
          </div>
        </div>
        <div className="flex gap-1">
          {ORDER.map((s, i) => (
            <div
              key={s}
              className={`h-1.5 w-6 rounded-full transition-colors ${
                i <= idx
                  ? "bg-gradient-to-r from-blue-500 to-violet-500"
                  : "bg-slate-200"
              }`}
            />
          ))}
        </div>
      </header>

      <main className="flex-1 overflow-y-auto px-10 py-8 bg-slate-100">
        {step === "welcome" && <Welcome onNext={next} />}
        {step === "microphone" && (
          <MicrophoneStep
            status={perms?.microphone ?? "unknown"}
            onRefresh={refreshPerms}
          />
        )}
        {step === "accessibility" && (
          <AccessibilityStep
            status={perms?.accessibility ?? "unknown"}
            onRefresh={refreshPerms}
          />
        )}
        {step === "screen_recording" && (
          <ScreenRecordingStep
            status={perms?.screen_recording ?? "unknown"}
            onRefresh={refreshPerms}
          />
        )}
        {step === "automation" && (
          <AutomationStep
            status={perms?.automation ?? "unknown"}
            onRefresh={refreshPerms}
          />
        )}
        {step === "api_keys" && <ApiKeysStep />}
        {step === "hotkeys" && <HotkeysStep />}
        {step === "test" && <TestStep />}
      </main>

      <footer className="px-8 py-4 border-t border-slate-200 bg-white flex items-center justify-between shadow-[0_-1px_0_rgba(0,0,0,0.04)]">
        <button
          type="button"
          onClick={back}
          disabled={idx === 0}
          className="text-sm text-slate-600 hover:text-slate-900 disabled:opacity-30 disabled:cursor-not-allowed"
        >
          ← Back
        </button>
        {step === "test" ? (
          <button
            type="button"
            onClick={async () => {
              await completeOnboarding();
            }}
            className="rounded-lg bg-wisspa-gradient px-5 py-2.5 text-sm font-semibold text-white shadow-md hover:shadow-lg hover:brightness-110 transition"
          >
            Finish setup
          </button>
        ) : (
          <button
            type="button"
            onClick={next}
            className="rounded-lg bg-wisspa-gradient px-5 py-2.5 text-sm font-semibold text-white shadow-md hover:shadow-lg hover:brightness-110 transition"
          >
            Continue →
          </button>
        )}
      </footer>
    </div>
  );
}

function Card({ children, className = "" }: { children: React.ReactNode; className?: string }) {
  return (
    <div
      className={`bg-white rounded-2xl border border-slate-200 shadow-sm ${className}`}
    >
      {children}
    </div>
  );
}

function Kbd({ children }: { children: React.ReactNode }) {
  return (
    <kbd className="inline-block rounded-md border border-slate-300 bg-slate-100 px-1.5 py-0.5 text-[11px] font-mono text-slate-800 shadow-[inset_0_-1px_0_rgba(0,0,0,0.06)]">
      {children}
    </kbd>
  );
}

function Welcome({ onNext }: { onNext: () => void }) {
  return (
    <div className="max-w-2xl mx-auto text-center pt-8">
      <div className="mx-auto mb-6 h-24 w-24 rounded-3xl bg-wisspa-gradient flex items-center justify-center text-white text-4xl font-bold shadow-xl">
        W
      </div>
      <h1 className="text-3xl font-bold tracking-tight text-slate-900">
        Welcome to Wisspa
      </h1>
      <p className="mt-3 text-base text-slate-600 leading-relaxed max-w-lg mx-auto">
        A system-wide AI voice tool for macOS. Hold a hotkey, speak, and Wisspa
        types — cleaned, structured, and aware of the app you're in.
      </p>
      <div className="mt-10 grid grid-cols-3 gap-4 text-left">
        <FeatureCard
          color="from-blue-500 to-blue-600"
          emoji="🎙"
          title="Dictate"
          body="Speak into anything. Cleaned prose, no filler words."
        />
        <FeatureCard
          color="from-violet-500 to-fuchsia-500"
          emoji="🪄"
          title="Act"
          body="Voice commands: screenshot, search, open app, more."
        />
        <FeatureCard
          color="from-emerald-500 to-teal-500"
          emoji="✨"
          title="Prompt"
          body="Rewrite rough speech into a polished AI prompt."
        />
      </div>
      <button
        onClick={onNext}
        className="mt-10 rounded-xl bg-wisspa-gradient px-7 py-3 text-base font-semibold text-white shadow-lg hover:shadow-xl hover:brightness-110 transition"
      >
        Get started
      </button>
    </div>
  );
}

function FeatureCard({
  color,
  emoji,
  title,
  body,
}: {
  color: string;
  emoji: string;
  title: string;
  body: string;
}) {
  return (
    <div className="rounded-2xl border border-slate-200 bg-white p-4 shadow-sm relative overflow-hidden">
      <div
        className={`absolute top-0 left-0 right-0 h-1 bg-gradient-to-r ${color}`}
      />
      <div className="text-2xl">{emoji}</div>
      <div className="mt-2 font-semibold text-sm text-slate-900">{title}</div>
      <div className="text-xs text-slate-600 mt-1 leading-relaxed">{body}</div>
    </div>
  );
}

function PermissionPanel({
  icon,
  iconColor,
  title,
  body,
  status,
  primary,
  secondary,
}: {
  icon: string;
  iconColor: string;
  title: string;
  body: React.ReactNode;
  status: PermissionStatus;
  primary?: { label: string; onClick: () => void };
  secondary?: { label: string; onClick: () => void };
}) {
  return (
    <div className="max-w-2xl mx-auto">
      <Card className="p-6">
        <div className="flex items-start gap-4 mb-4">
          <div
            className={`h-12 w-12 rounded-2xl ${iconColor} flex items-center justify-center text-2xl shadow-sm`}
          >
            {icon}
          </div>
          <div className="flex-1 min-w-0">
            <h2 className="text-xl font-bold tracking-tight text-slate-900">
              {title}
            </h2>
            <div className="mt-1">
              <StatusBadge status={status} />
            </div>
          </div>
        </div>
        <div className="text-sm text-slate-700 leading-relaxed">{body}</div>
        <div className="mt-6 flex flex-wrap gap-3">
          {primary && (
            <button
              onClick={primary.onClick}
              className="rounded-lg bg-wisspa-gradient px-4 py-2 text-sm font-semibold text-white shadow-md hover:shadow-lg hover:brightness-110 transition"
            >
              {primary.label}
            </button>
          )}
          {secondary && (
            <button
              onClick={secondary.onClick}
              className="rounded-lg border border-slate-300 bg-white px-4 py-2 text-sm font-semibold text-slate-800 hover:bg-slate-50 shadow-sm"
            >
              {secondary.label}
            </button>
          )}
        </div>
      </Card>
    </div>
  );
}

function StatusBadge({ status }: { status: PermissionStatus }) {
  const styles: Record<PermissionStatus, string> = {
    granted: "bg-emerald-100 text-emerald-800 border border-emerald-200",
    denied: "bg-amber-100 text-amber-800 border border-amber-200",
    unknown: "bg-slate-100 text-slate-600 border border-slate-200",
  };
  const labels: Record<PermissionStatus, string> = {
    granted: "✓ Granted",
    denied: "⚠ Needs permission",
    unknown: "○ Not yet checked",
  };
  return (
    <span
      className={`inline-block text-[11px] font-semibold px-2 py-0.5 rounded-full ${styles[status]}`}
    >
      {labels[status]}
    </span>
  );
}

function MicrophoneStep({
  status,
  onRefresh,
}: {
  status: PermissionStatus;
  onRefresh: () => void;
}) {
  async function request() {
    try {
      const stream = await navigator.mediaDevices.getUserMedia({ audio: true });
      stream.getTracks().forEach((t) => t.stop());
      await reportMicrophoneStatus(true);
    } catch (err) {
      console.error("getUserMedia:", err);
      await reportMicrophoneStatus(false);
    }
    onRefresh();
  }
  return (
    <PermissionPanel
      icon="🎙"
      iconColor="bg-blue-50"
      title="Microphone access"
      status={status}
      body={
        <>
          Wisspa transcribes your speech with Groq Whisper. Audio is only
          captured while you hold a hotkey, and the buffer is discarded after
          the STT request completes.
        </>
      }
      primary={{ label: "Request access", onClick: request }}
      secondary={{
        label: "Open System Settings",
        onClick: () => openSystemSettings("microphone"),
      }}
    />
  );
}

function AccessibilityStep({
  status,
  onRefresh,
}: {
  status: PermissionStatus;
  onRefresh: () => void;
}) {
  return (
    <PermissionPanel
      icon="🛡"
      iconColor="bg-violet-50"
      title="Accessibility access"
      status={status}
      body={
        <>
          Wisspa simulates <Kbd>Cmd</Kbd>+<Kbd>V</Kbd> to paste transcribed
          text into whatever app you have focused. macOS requires Accessibility
          permission to send synthetic keystrokes.
          <div className="mt-3 text-xs text-slate-500 leading-relaxed">
            In the panel that opens, add <Kbd>Wisspa</Kbd> to the list and
            toggle it on.
          </div>
        </>
      }
      primary={{
        label: "Open System Settings",
        onClick: async () => {
          await openSystemSettings("accessibility");
          setTimeout(onRefresh, 800);
        },
      }}
      secondary={{ label: "I've granted it — re-check", onClick: onRefresh }}
    />
  );
}

function ScreenRecordingStep({
  status,
  onRefresh,
}: {
  status: PermissionStatus;
  onRefresh: () => void;
}) {
  return (
    <PermissionPanel
      icon="📸"
      iconColor="bg-amber-50"
      title="Screen Recording (optional)"
      status={status}
      body={
        <>
          Only needed for the <Kbd>screenshot</Kbd> and{" "}
          <Kbd>save screenshot</Kbd> voice actions. Safe to skip if you don't
          plan to use screen capture commands.
        </>
      }
      primary={{
        label: "Request access",
        onClick: async () => {
          await requestScreenRecordingAccess();
          await openSystemSettings("screen_recording");
          setTimeout(onRefresh, 800);
        },
      }}
      secondary={{ label: "Re-check", onClick: onRefresh }}
    />
  );
}

function AutomationStep({
  status,
  onRefresh,
}: {
  status: PermissionStatus;
  onRefresh: () => void;
}) {
  return (
    <PermissionPanel
      icon="🤖"
      iconColor="bg-emerald-50"
      title="Automation (System Events)"
      status={status}
      body={
        <>
          Wisspa uses Apple Events to detect the focused app, reliably
          simulate <Kbd>Cmd</Kbd>+<Kbd>V</Kbd>, and run AppleScript voice
          actions like "mute audio". macOS prompts on first use — you can also
          pre-approve here.
        </>
      }
      primary={{
        label: "Open System Settings",
        onClick: async () => {
          await openSystemSettings("automation");
          setTimeout(onRefresh, 800);
        },
      }}
      secondary={{ label: "Re-check", onClick: onRefresh }}
    />
  );
}

function ApiKeysStep() {
  return (
    <div className="max-w-2xl mx-auto">
      <Card className="p-6">
        <div className="flex items-start gap-4 mb-4">
          <div className="h-12 w-12 rounded-2xl bg-blue-50 flex items-center justify-center text-2xl shadow-sm">
            🔑
          </div>
          <div className="flex-1">
            <h2 className="text-xl font-bold tracking-tight text-slate-900">
              API keys
            </h2>
            <p className="text-xs text-slate-500 mt-0.5">
              Stored in macOS Keychain · never on disk
            </p>
          </div>
        </div>
        <p className="text-sm text-slate-700 leading-relaxed mb-5">
          Wisspa uses Groq for speech-to-text and Anthropic Claude for cleanup
          and prompt rewriting. Keys never leave your machine except to call
          the providers directly.
        </p>
        <div className="space-y-3">
          {API_PROVIDERS.map((p) => (
            <KeyInput key={p} provider={p} />
          ))}
        </div>
      </Card>
    </div>
  );
}

function KeyInput({ provider }: { provider: ApiProvider }) {
  const name = API_KEY_NAMES[provider];
  const link =
    provider === "groq"
      ? "https://console.groq.com/keys"
      : "https://console.anthropic.com/settings/keys";
  const providerName = provider === "groq" ? "Groq" : "Anthropic";
  const [value, setValue] = useState("");
  const [status, setStatus] = useState<{ kind: "ok" | "err" | "info"; text: string } | null>(
    null,
  );

  async function save() {
    setStatus({ kind: "info", text: "Saving…" });
    try {
      await saveApiKey(name, value);
      const msg = await testApiKey(provider, value);
      setStatus({ kind: "ok", text: `Saved · ${msg}` });
      setValue("");
    } catch (e) {
      setStatus({ kind: "err", text: String(e) });
    }
  }

  return (
    <div className="rounded-xl border border-slate-200 bg-slate-50 p-4">
      <div className="flex items-baseline justify-between">
        <div className="font-semibold text-sm text-slate-900">
          {providerName}
        </div>
        <a
          href={link}
          target="_blank"
          rel="noreferrer"
          className="text-xs text-blue-600 underline decoration-dotted"
        >
          Get a key →
        </a>
      </div>
      <div className="mt-3 flex items-center gap-2">
        <input
          type="password"
          value={value}
          onChange={(e) => setValue(e.target.value)}
          placeholder="Paste your API key"
          className="flex-1 rounded-md border border-slate-300 bg-white px-3 py-2 text-sm font-mono text-slate-900 focus:outline-none focus:border-blue-500 focus:ring-2 focus:ring-blue-500/20"
        />
        <button
          onClick={save}
          disabled={!value}
          className="rounded-md bg-wisspa-gradient px-3 py-2 text-sm font-semibold text-white disabled:opacity-40 disabled:cursor-not-allowed hover:brightness-110 transition"
        >
          Save & test
        </button>
      </div>
      {status && (
        <div
          className={`mt-2 text-xs font-medium ${
            status.kind === "err"
              ? "text-red-600"
              : status.kind === "ok"
              ? "text-emerald-700"
              : "text-slate-500"
          }`}
        >
          {status.text}
        </div>
      )}
    </div>
  );
}

function HotkeysStep() {
  return (
    <div className="max-w-2xl mx-auto">
      <Card className="p-6">
        <div className="flex items-start gap-4 mb-4">
          <div className="h-12 w-12 rounded-2xl bg-slate-100 flex items-center justify-center text-2xl shadow-sm">
            ⌨
          </div>
          <div className="flex-1">
            <h2 className="text-xl font-bold tracking-tight text-slate-900">
              Default hotkeys
            </h2>
            <p className="text-xs text-slate-500 mt-0.5">
              Change any of these in Settings → Hotkeys
            </p>
          </div>
        </div>
        <p className="text-sm text-slate-700 leading-relaxed mb-5">
          Wisspa ships with safe push-and-hold combos. Single keys like{" "}
          <Kbd>F18</Kbd> or <Kbd>F19</Kbd> work great if you'd prefer one-key
          triggers.
        </p>
        <div className="space-y-2">
          <Combo label="Dictation" combo="Cmd+Shift+Space" tint="blue" />
          <Combo label="Action mode" combo="Cmd+Shift+A" tint="violet" />
          <Combo label="Prompt mode" combo="Cmd+Shift+P" tint="emerald" />
          <Combo label="Cancel recording" combo="Esc" tint="slate" />
        </div>
      </Card>
    </div>
  );
}

function Spinner() {
  return (
    <span
      className="inline-block h-3 w-3 rounded-full border-2 border-blue-500 border-t-transparent animate-spin"
      aria-hidden
    />
  );
}

function Stat({ label, value }: { label: string; value: string }) {
  return (
    <div className="rounded-md bg-white border border-slate-200 px-2 py-1.5">
      <div className="text-[10px] uppercase tracking-wide text-slate-500">
        {label}
      </div>
      <div className="font-mono text-sm text-slate-900">{value}</div>
    </div>
  );
}

function Combo({
  label,
  combo,
  tint,
}: {
  label: string;
  combo: string;
  tint: "blue" | "violet" | "emerald" | "slate";
}) {
  const dot = {
    blue: "bg-blue-500",
    violet: "bg-violet-500",
    emerald: "bg-emerald-500",
    slate: "bg-slate-400",
  }[tint];
  return (
    <div className="flex items-center gap-3 px-4 py-3 rounded-lg border border-slate-200 bg-slate-50">
      <span className={`h-2 w-2 rounded-full ${dot}`} />
      <div className="flex-1 text-sm text-slate-800">{label}</div>
      <code className="text-xs font-mono text-slate-700 bg-white border border-slate-200 px-2 py-1 rounded shadow-sm">
        {combo}
      </code>
    </div>
  );
}

type CalibrationStage = "idle" | "ambient" | "speech" | "done" | "error";

function TestStep() {
  const [stage, setStage] = useState<CalibrationStage>("idle");
  const [ambientPeak, setAmbientPeak] = useState<number | null>(null);
  const [speechPeak, setSpeechPeak] = useState<number | null>(null);
  const [bytesPerSecond, setBytesPerSecond] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [text, setText] = useState("");

  async function runCalibration() {
    setError(null);
    setAmbientPeak(null);
    setSpeechPeak(null);
    setBytesPerSecond(null);
    try {
      setStage("ambient");
      const amb = await sampleAmbient(2000);
      setAmbientPeak(amb.peakAmplitude);

      // Short pause so the user can transition into "say hello".
      await new Promise((r) => setTimeout(r, 500));

      setStage("speech");
      const speech = await sampleSpeech(2500);
      setSpeechPeak(speech.peakAmplitude);
      setBytesPerSecond(speech.bytesPerSecond);

      // Compute and persist thresholds.
      const silencePeak = Math.max(6, amb.peakAmplitude * 2);
      const minBytesPerSecond = Math.max(
        1500,
        Math.round(speech.bytesPerSecond * 0.3),
      );

      const settings = await getSettings();
      const next = {
        ...settings,
        mic_calibration: {
          silence_peak: silencePeak,
          min_bytes_per_second: minBytesPerSecond,
          calibrated_at: Date.now(),
        },
      };
      await saveSettings(next);
      setStage("done");
    } catch (err) {
      console.error("calibration failed:", err);
      setError(String(err));
      setStage("error");
    }
  }

  return (
    <div className="max-w-2xl mx-auto">
      <Card className="p-6">
        <div className="flex items-start gap-4 mb-4">
          <div className="h-12 w-12 rounded-2xl bg-emerald-50 flex items-center justify-center text-2xl shadow-sm">
            🎚
          </div>
          <div className="flex-1">
            <h2 className="text-xl font-bold tracking-tight text-slate-900">
              Mic calibration & test
            </h2>
            <p className="text-xs text-slate-500 mt-0.5">
              Two quick samples so Wisspa knows your mic's normal levels
            </p>
          </div>
        </div>

        <p className="text-sm text-slate-700 leading-relaxed mb-4">
          We'll record 2 seconds of silence, then 2.5 seconds of you saying{" "}
          <strong>"hello, hello, hello"</strong>. Wisspa uses the difference to
          decide when a recording was really silent (so we never feed dead air
          to the transcriber).
        </p>

        <div className="rounded-xl border border-slate-200 bg-slate-50 p-4 mb-4">
          {stage === "idle" && (
            <button
              onClick={runCalibration}
              className="rounded-lg bg-wisspa-gradient px-4 py-2 text-sm font-semibold text-white shadow-sm hover:brightness-110"
            >
              Start calibration
            </button>
          )}
          {stage === "ambient" && (
            <div className="flex items-center gap-3 text-sm">
              <Spinner /> Recording 2s of silence — please stay quiet…
            </div>
          )}
          {stage === "speech" && (
            <div className="flex items-center gap-3 text-sm">
              <Spinner /> Now say <strong>"hello, hello, hello"</strong>…
            </div>
          )}
          {stage === "done" && (
            <div className="space-y-2 text-sm">
              <div className="font-semibold text-emerald-700">
                ✓ Calibration saved
              </div>
              <div className="grid grid-cols-3 gap-3 text-xs">
                <Stat label="Ambient peak" value={ambientPeak?.toFixed(1) ?? "—"} />
                <Stat label="Speech peak" value={speechPeak?.toFixed(1) ?? "—"} />
                <Stat label="Speech bytes/s" value={bytesPerSecond?.toLocaleString() ?? "—"} />
              </div>
              <button
                onClick={runCalibration}
                className="mt-1 text-xs underline decoration-dotted text-slate-600 hover:text-slate-900"
              >
                Re-run calibration
              </button>
            </div>
          )}
          {stage === "error" && (
            <div className="space-y-2 text-sm">
              <div className="text-red-600 font-medium">
                Calibration failed: {error}
              </div>
              <button
                onClick={runCalibration}
                className="rounded-lg bg-wisspa-gradient px-3 py-1.5 text-xs font-semibold text-white"
              >
                Try again
              </button>
            </div>
          )}
        </div>

        <div>
          <p className="text-xs text-slate-500 leading-relaxed mb-2">
            Optional final check: click into the box below, hold{" "}
            <Kbd>Cmd</Kbd>+<Kbd>Shift</Kbd>+<Kbd>Space</Kbd>, dictate something
            normally, release. Cleaned text should land here.
          </p>
          <textarea
            value={text}
            onChange={(e) => setText(e.target.value)}
            placeholder="Click here, then dictate…"
            rows={4}
            className="w-full rounded-lg border border-slate-300 bg-white px-3 py-2.5 text-sm font-mono text-slate-900 placeholder:text-slate-400 focus:outline-none focus:border-blue-500 focus:ring-2 focus:ring-blue-500/20"
          />
        </div>

        <p className="text-xs text-slate-500 mt-4 leading-relaxed">
          Click <strong className="text-slate-700">Finish setup</strong> below
          when you're ready. You can re-run calibration any time from
          Settings → General.
        </p>
      </Card>
    </div>
  );
}
