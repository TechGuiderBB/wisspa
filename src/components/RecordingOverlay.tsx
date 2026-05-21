import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";

export default function RecordingOverlay() {
  // `false` = warming (mic opening), `true` = armed (capture live, speak now).
  const [armed, setArmed] = useState(false);

  useEffect(() => {
    // Cancellation-safe: React 18 StrictMode runs effect cleanup before the
    // listen() promises resolve in dev, which would otherwise leak listeners.
    let cancelled = false;
    const unlistens: Array<() => void> = [];
    const track = (u: () => void) => {
      if (cancelled) u();
      else unlistens.push(u);
    };
    listen("wisspa://start-recording", () => setArmed(false)).then(track);
    listen("wisspa://recording-armed", () => setArmed(true)).then(track);
    return () => {
      cancelled = true;
      unlistens.forEach((u) => u());
    };
  }, []);

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
      </div>
    </div>
  );
}
