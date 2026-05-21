import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";

export default function RecordingOverlay() {
  // `false` = warming (mic opening), `true` = armed (capture live, speak now).
  const [armed, setArmed] = useState(false);

  useEffect(() => {
    const unlistens: Array<() => void> = [];
    listen("wisspa://start-recording", () => setArmed(false)).then((u) =>
      unlistens.push(u),
    );
    listen("wisspa://recording-armed", () => setArmed(true)).then((u) =>
      unlistens.push(u),
    );
    return () => unlistens.forEach((u) => u());
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
