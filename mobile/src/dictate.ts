/** Microphone → 16 kHz 16-bit PCM in ~200 ms blocks (base64), for on-device dictation. */
import { invoke } from "@tauri-apps/api/core";

const RATE = 16_000;
const BLOCK = RATE / 5; // 200 ms

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

const WORKLET = `
class Tap extends AudioWorkletProcessor {
  process(inputs) {
    const ch = inputs[0] && inputs[0][0];
    if (ch) this.port.postMessage(ch.slice(0));
    return true;
  }
}
registerProcessor("tap", Tap);
`;

/** Starts the microphone; returns a function that stops it. */
export async function startMic(): Promise<() => Promise<void>> {
  const stream = await navigator.mediaDevices.getUserMedia({
    audio: {
      echoCancellation: false,
      noiseSuppression: false,
      autoGainControl: true,
    },
  });
  const ctx = new AudioContext({ sampleRate: RATE });
  const url = URL.createObjectURL(
    new Blob([WORKLET], { type: "text/javascript" }),
  );
  await ctx.audioWorklet.addModule(url);
  const source = ctx.createMediaStreamSource(stream);
  const tap = new AudioWorkletNode(ctx, "tap");
  let pending: number[] = [];
  const sends: Promise<unknown>[] = [];
  tap.port.onmessage = (e: MessageEvent<Float32Array>) => {
    for (const v of e.data) pending.push(v);
    if (pending.length >= BLOCK) {
      const block = Float32Array.from(pending);
      pending = [];
      sends.push(invoke("dictation_push", { pcm: toBase64(block) }));
    }
  };
  source.connect(tap);
  return async () => {
    source.disconnect();
    tap.disconnect();
    stream.getTracks().forEach((t) => t.stop());
    if (pending.length)
      sends.push(
        invoke("dictation_push", { pcm: toBase64(Float32Array.from(pending)) }),
      );
    await Promise.all(sends);
    await ctx.close();
  };
}

/** Feeds a 16 kHz mono 16-bit WAV (base64) as if spoken live, for tests. */
export async function feedWav(b64: string): Promise<void> {
  const bytes = Uint8Array.from(atob(b64), (c) => c.charCodeAt(0));
  const pcm = new Int16Array(
    bytes.buffer,
    44,
    Math.floor((bytes.length - 44) / 2),
  );
  for (let i = 0; i < pcm.length; i += BLOCK) {
    const block = Float32Array.from(
      pcm.subarray(i, i + BLOCK),
      (v) => v / 32768,
    );
    await invoke("dictation_push", { pcm: toBase64(block) });
    await new Promise((r) => setTimeout(r, 200));
  }
}
