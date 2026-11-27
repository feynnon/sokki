/** Talking to Sokki on the Mac: direct and end-to-end encrypted, from any
 * network (Rust side: src-tauri/src/link.rs, protocol: crates/sokki-link).
 * Every call is one attempt; when the Mac is off it fails with a message. */
import { invoke } from "@tauri-apps/api/core";

export interface Pairing {
  mac: string; // the Mac's key
  name: string; // "Sokki"
}

export interface RemoteStatus {
  id: string;
  status: "recording" | "processing" | "done" | "failed";
  stage: string | null;
  progress: number | null;
  error: string | null;
  title: string | null;
  summary: string | null;
  transcript: string | null;
}

export const pairing = () => invoke<Pairing | null>("link_pairing");

/** "Pixel 7" from the WebView's user agent, else "Phone". */
export function deviceName(): string {
  const m = navigator.userAgent.match(/Android [\d.]+; ([^;)]+?)(?: Build|\))/);
  return m?.[1]?.trim() || "Phone";
}

export const pair = (ticket: string) =>
  invoke<Pairing>("link_pair", { ticket: ticket.trim(), device: deviceName() });

export const unpair = () => invoke<void>("link_unpair");

export const ping = () => invoke<{ name: string; version: string }>("link_ping");

/** One day of the keyboard's journal (JSONL) to the Mac, which keeps it
 * next to its own (Mac: typing_log/phone/<file>). */
export const sendJournal = (file: string, text: string) =>
  invoke<{ saved: number }>("link_send_journal", { file, text });

export async function upload(audio: Blob, startedAt: Date): Promise<string> {
  // Base64: Android's IPC carries no raw bytes.
  const b64 = await new Promise<string>((resolve, reject) => {
    const r = new FileReader();
    r.onload = () => resolve(String(r.result).split(",")[1] ?? "");
    r.onerror = () => reject(r.error);
    r.readAsDataURL(audio);
  });
  return invoke<string>("link_upload", {
    audio: b64,
    startedAt: startedAt.toISOString(),
    mime: audio.type || "audio/webm",
  });
}

async function base64(audio: Blob): Promise<string> {
  return new Promise<string>((resolve, reject) => {
    const r = new FileReader();
    r.onload = () => resolve(String(r.result).split(",")[1] ?? "");
    r.onerror = () => reject(r.error);
    r.readAsDataURL(audio);
  });
}

/** A meeting sent as it records, so the Mac transcribes it live and the
 * note is ready soon after the stop (src-tauri/src/link.rs, Mac:
 * meeting/phone_live.rs). Returns the Mac's meeting id; an older Mac
 * answers an error, and the recording goes whole after the stop. */
export const liveStart = (startedAt: Date, mime: string) =>
  invoke<string>("link_live_start", {
    startedAt: startedAt.toISOString(),
    mime: mime || "audio/webm",
  });

/** The recording's bytes from `offset`; returns the bytes the Mac has. */
export const livePart = async (id: string, offset: number, audio: Blob) =>
  invoke<number>("link_live_part", { id, offset, audio: await base64(audio) });

/** The recording is complete at `total` bytes; the Mac processes it once
 * it has them all (`have === total`). */
export const liveEnd = (id: string, total: number) =>
  invoke<{ have: number; id?: string }>("link_live_end", { id, total });

/** The link failed on the way (no answer in time, the Mac not reachable),
 * as opposed to the Mac refusing: worth trying again. */
export const linkFailed = (e: unknown) =>
  /no answer in time|Can't reach the Mac|connection|timed out/i.test(String(e));

export const status = (id: string) => invoke<RemoteStatus>("link_status", { id });
