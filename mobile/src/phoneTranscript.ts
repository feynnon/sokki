/** Without a Mac. Two ways to compare (Robin, 25.11.2026):
 *  "mac"   (B, default): meetings need Sokki on a Mac; without one the
 *          Meeting tab says so and doesn't record.
 *  "phone" (A): recordings get a transcript on the phone, with the same
 *          speech model as dictation (no speakers, no summary). A Mac
 *          connected later adds them.
 * The transcript uses the Dictate tab's engine (dictation_start/push/stop
 * and its "dictation-piece" / "dictation-done" events), fed from the
 * recorded file instead of the microphone. */
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

export type NoMac = "mac" | "phone";
const KEY = "sokki.noMac";

export function getNoMac(): NoMac {
  try {
    return localStorage.getItem(KEY) === "phone" ? "phone" : "mac";
  } catch {
    return "mac";
  }
}

export function setNoMac(m: NoMac) {
  try {
    localStorage.setItem(KEY, m);
  } catch {
    /* ignore */
  }
}

const RATE = 16_000;
const BLOCK = RATE * 5; // 5 s per push

function toBase64(samples: Float32Array): string {
  const bytes = new Uint8Array(samples.length * 2);
  const view = new DataView(bytes.buffer);
  for (let i = 0; i < samples.length; i++) {
    const s = Math.max(-1, Math.min(1, samples[i]));
    view.setInt16(i * 2, s < 0 ? s * 0x8000 : s * 0x7fff, true);
  }
  let bin = "";
  for (let i = 0; i < bytes.length; i += 0x8000)
    bin += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
  return btoa(bin);
}

/** Thrown when the speech model isn't on the phone yet. */
export const NO_MODEL = "no-model";

let busy: Promise<unknown> = Promise.resolve();

/** The recording's text, transcribed on the phone. One at a time. */
export function transcribeOnPhone(audio: Blob): Promise<string> {
  const run = busy.then(() => transcribe(audio));
  busy = run.catch(() => undefined);
  return run;
}

async function transcribe(audio: Blob): Promise<string> {
  const model = await invoke<{ present: boolean }>("dictation_model");
  if (!model.present) throw new Error(NO_MODEL);

  // The browser decodes the WebM/Opus file and resamples it to 16 kHz.
  const ctx = new AudioContext({ sampleRate: RATE });
  let samples: Float32Array;
  try {
    const buf = await ctx.decodeAudioData(await audio.arrayBuffer());
    samples = new Float32Array(buf.length);
    for (let c = 0; c < buf.numberOfChannels; c++) {
      const ch = buf.getChannelData(c);
      for (let i = 0; i < ch.length; i++)
        samples[i] += ch[i] / buf.numberOfChannels;
    }
  } finally {
    await ctx.close();
  }

  const texts: string[] = [];
  const offPiece = await listen<{ index: number; text: string }>(
    "dictation-piece",
    (e) => {
      texts[e.payload.index] = e.payload.text;
    },
  );
  let offDone: (() => void) | undefined;
  const done = new Promise<void>((resolve) => {
    void listen("dictation-done", () => resolve()).then(
      (off) => (offDone = off),
    );
  });
  try {
    await invoke("dictation_start", {});
    for (let i = 0; i < samples.length; i += BLOCK)
      await invoke("dictation_push", {
        pcm: toBase64(samples.subarray(i, i + BLOCK)),
      });
    await invoke("dictation_stop");
    await done;
  } finally {
    offPiece();
    offDone?.();
  }
  return texts
    .filter((t) => t && t.trim())
    .join(" ")
    .trim();
}
