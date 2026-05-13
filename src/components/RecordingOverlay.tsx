export default function RecordingOverlay() {
  return (
    <div className="h-screen w-screen flex items-center justify-center">
      <div className="flex items-center gap-2 rounded-full bg-black/75 px-4 py-2 backdrop-blur-md shadow-lg">
        <span className="inline-block h-2.5 w-2.5 rounded-full bg-red-500 wisspa-flash" />
        <span className="text-white text-sm font-semibold tracking-wide">
          Wisspa
        </span>
      </div>
    </div>
  );
}
