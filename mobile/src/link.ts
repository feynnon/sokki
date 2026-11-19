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

export const status = (id: string) => invoke<RemoteStatus>("link_status", { id });
