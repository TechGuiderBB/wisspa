import { useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import type { RecordingMode } from "../lib/tauri";
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
        sessionRef.current = e.payload?.session ?? 0;
        const m = e.payload?.mode;
        if (m === "dictation" || m === "action" || m === "prompt") setMode(m);
      },
    ).then(track);

    listen("wisspa://recording-armed", () => setArmed(true)).then(track);

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

  return (
    <div className="h-screen w-screen flex items-center justify-center">
      <div className="flex items-center gap-2 rounded-full bg-black/75 px-4 py-2 backdrop-blur-md shadow-lg">
        <span
          className={`inline-block h-2.5 w-2.5 rounded-full ${
            armed ? "bg-red-500 wisspa-flash" : "bg-amber-400/70"
          }`}
        />
        <span className="text-white text-sm font-semibold tracking-wide">
          {armed ? "Listening" : "Wisspa"}
        </span>
        {mode === "prompt" && routeChipText !== null && (
          <span
            className={`text-[10px] truncate max-w-[70px] ${ROUTE_TONE_CLASS[routeChipTone]}`}
          >
            {routeChipText}
          </span>
        )}
      </div>
    </div>
  );
}
