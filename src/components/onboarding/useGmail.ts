import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useSettings } from "@/hooks/useSettings";

/**
 * Gmail for setup: whether this build can send through it, the connected
 * address, and connecting (Google's consent page in the browser).
 *
 * The commands come from the notes-to-all work (`meeting/gmail_account.rs`:
 * `gmail_status`, `gmail_connect`, which asks Google for sending only).
 * Called by name, not through bindings.ts,
 * so setup works with or without them: a build without them has no Gmail
 * row. Switch to `commands.gmailStatus` / `gmailConnect` once they are in
 * bindings.ts on meetings.
 */
export function useGmail(preview = false) {
  const { refreshSettings } = useSettings();
  const [available, setAvailable] = useState(false);
  const [loaded, setLoaded] = useState(preview);
  const [connected, setConnected] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (preview) return;
    invoke<{ available: boolean; connected: string | null }>("gmail_status")
      .then((s) => {
        setAvailable(!!s?.available);
        setConnected(s?.connected ?? null);
      })
      .catch(() => setAvailable(false))
      .finally(() => setLoaded(true));
  }, [preview]);

  /** Opens Google's consent page and waits for the answer (up to 5 min). Until Google has
   * verified the app, Google warns "unverified app" there; cancelling or
   * failing leaves the row as it was, with the reason. */
  const connect = useCallback(async () => {
    setBusy(true);
    setError(null);
    try {
      // The notice beside the button names exactly what this asks for.
      const address = await invoke<string>("gmail_connect");
      setConnected(address || "Gmail");
      // Connecting writes meetings.gmail_address; a later save of a stale
      // copy of the settings would wipe it.
      await refreshSettings();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }, [refreshSettings]);

  return {
    available,
    connected,
    busy,
    error,
    connect,
    loaded,
  };
}
