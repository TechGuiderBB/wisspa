import { useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import type { RecordingMode } from "../lib/tauri";
import { INPUT_LEVEL_EVENT } from "../lib/audio";
import {
  type PromptRoute,
  PROMPT_ROUTE_EVENT,
  routeLabel,
  routeBadge,
} from "../lib/promptRoute";

const ROUTE_TONE_CLASS: Record<"ai" | "content" | "neutral", string> = {
  ai: "text-violet-300",
  content: "text-emerald-300",
  neutral: "text-white/50",
};

export default function RecordingOverlay() {
  // `false` = warming (mic opening), `true` = armed (capture live, speak now).
  const [armed, setArmed] = useState(false);
  const [mode, setMode] = useState<RecordingMode>("dictation");
  const [route, setRoute] = useState<PromptRoute | null>(null);
  // Live input level 0–1, decayed so the bar relaxes between peaks instead of
  // snapping to zero. 0 = no data yet (analyser down → bar just stays empty).
  const [level, setLevel] = useState(0);
  // Session ref for the stale-event guard (mirrors the discipline in Runtime).
  const sessionRef = useRef(0);

  useEffect(() => {
    // Cancellation-safe: React 18 StrictMode runs effect cleanup before the
    // listen() promises resolve in dev, which would otherwise leak listeners.
    let cancelled = false;
    const unlistens: Array<() => void> = [];
    const track = (u: () => void) => {
      if (cancelled) u();
      else unlistens.push(u);
    };

    listen<{ mode: RecordingMode; session: number }>(
      "wisspa://start-recording",
      (e) => {
        setArmed(false);
        setRoute(null);
        setLevel(0);
        sessionRef.current = e.payload?.session ?? 0;
        const m = e.payload?.mode;
        if (m === "dictation" || m === "action" || m === "prompt" || m === "command") setMode(m);
      },
    ).then(track);

    listen("wisspa://recording-armed", () => setArmed(true)).then(track);

    // Level events stream from the runtime window's analyser at 10 Hz. Each
    // tick both takes the new peak and decays the previous value, giving a
    // smooth falloff without a separate timer.
    listen<number>(INPUT_LEVEL_EVENT, (e) => {
      const raw = typeof e.payload === "number" ? e.payload : 0;
      // Raw peak is 0–128; ~64 ≈ loud speech on a typical built-in mic.
      const next = Math.min(1, Math.max(0, raw / 64));
      setLevel((prev) => Math.max(next, prev * 0.75));
    }).then(track);

    listen<PromptRoute>(PROMPT_ROUTE_EVENT, (e) => {
      const payload = e.payload;
      if (!payload) return;
      // Session guard: ignore stale events from superseded recordings.
      // session === 0 is legacy passthrough for older backend builds.
      if (payload.session !== 0 && payload.session !== sessionRef.current) return;
      setRoute(payload);
    }).then(track);

    return () => {
      cancelled = true;
      unlistens.forEach((u) => u());
    };
  }, []);

  const routeChipText =
    route === null
      ? null
      : route.branch === "unknown"
        ? "detecting…"
        : routeLabel(route);
  const routeChipTone = route ? routeBadge(route).tone : "neutral";

  // Two rows inside the unchanged 220×56 window: status + esc hint on top,
  // level bar + route chip below. Fixed pill width keeps the bar meaningful
  // and guarantees the widened chip can never overflow the window.
  return (
    <div className="h-screen w-screen flex items-center justify-center">
      <div className="flex w-[204px] flex-col gap-1 rounded-full bg-black/75 px-4 py-1.5 backdrop-blur-md shadow-lg">
        <div className="flex items-center gap-2">
          <span
            className={`inline-block h-2.5 w-2.5 rounded-full ${
              armed ? "bg-red-500 wisspa-flash" : "bg-amber-400/70"
            }`}
          />
          <span className="text-white text-sm font-semibold tracking-wide">
            {armed ? "Listening" : "Wisspa"}
          </span>
          <span className="ml-auto text-[9px] text-white/40">esc to cancel</span>
        </div>
        <div className="flex h-[14px] items-center gap-2">
          <div className="h-[3px] min-w-[24px] flex-1 overflow-hidden rounded-full bg-white/15">
            <div
              className="h-full rounded-full bg-red-500 transition-[width] duration-100"
              style={{ width: `${Math.round(level * 100)}%` }}
            />
          </div>
          {mode === "prompt" && routeChipText !== null && (
            <span
              className={`text-[10px] truncate max-w-[140px] ${ROUTE_TONE_CLASS[routeChipTone]}`}
            >
              {routeChipText}
            </span>
          )}
        </div>
      </div>
    </div>
  );
}
