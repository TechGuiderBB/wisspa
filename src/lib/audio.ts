let mediaRecorder: MediaRecorder | null = null;
let chunks: Blob[] = [];
let activeStream: MediaStream | null = null;
let stopPromise: Promise<Blob> | null = null;
let stopResolver: ((b: Blob) => void) | null = null;

const PREFERRED_MIME = "audio/webm;codecs=opus";

function pickMime(): string {
  if (typeof MediaRecorder === "undefined") return PREFERRED_MIME;
  if (MediaRecorder.isTypeSupported(PREFERRED_MIME)) return PREFERRED_MIME;
  if (MediaRecorder.isTypeSupported("audio/webm")) return "audio/webm";
  if (MediaRecorder.isTypeSupported("audio/mp4")) return "audio/mp4";
  return "";
}

export async function startRecording(): Promise<void> {
  if (mediaRecorder && mediaRecorder.state === "recording") return;
  const stream = await navigator.mediaDevices.getUserMedia({ audio: true });
  activeStream = stream;
  chunks = [];

  const mime = pickMime();
  mediaRecorder = mime
    ? new MediaRecorder(stream, { mimeType: mime })
    : new MediaRecorder(stream);

  mediaRecorder.ondataavailable = (e) => {
    if (e.data && e.data.size > 0) chunks.push(e.data);
  };

  stopPromise = new Promise<Blob>((resolve) => {
    stopResolver = resolve;
  });

  mediaRecorder.onstop = () => {
    const type = mediaRecorder?.mimeType || "audio/webm";
    const blob = new Blob(chunks, { type });
    stopResolver?.(blob);
    activeStream?.getTracks().forEach((t) => t.stop());
    activeStream = null;
    mediaRecorder = null;
  };

  mediaRecorder.start();
}

export async function stopRecording(): Promise<Blob | null> {
  if (!mediaRecorder) return null;
  if (mediaRecorder.state !== "recording") return null;
  const pending = stopPromise!;
  mediaRecorder.stop();
  const blob = await pending;
  stopPromise = null;
  stopResolver = null;
  return blob;
}

export function cancelRecording(): void {
  if (!mediaRecorder) return;
  try {
    mediaRecorder.ondataavailable = null;
    mediaRecorder.onstop = null;
    if (mediaRecorder.state === "recording") mediaRecorder.stop();
  } catch {
    // best effort
  }
  activeStream?.getTracks().forEach((t) => t.stop());
  activeStream = null;
  mediaRecorder = null;
  chunks = [];
  stopPromise = null;
  stopResolver = null;
}

export async function blobToBase64(blob: Blob): Promise<string> {
  const buf = await blob.arrayBuffer();
  const bytes = new Uint8Array(buf);
  let binary = "";
  const chunk = 0x8000;
  for (let i = 0; i < bytes.length; i += chunk) {
    binary += String.fromCharCode.apply(
      null,
      bytes.subarray(i, i + chunk) as unknown as number[],
    );
  }
  return btoa(binary);
}
