import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { commands } from "@/bindings";
import { useSettings } from "@/hooks/useSettings";

/**
 * Gmail for setup: whether this build can send through it, the connected
 * address, and connecting (Google's consent page in the browser).
 *
 * The commands come from the notes-to-all work (`meeting/gmail_account.rs`:
 * `gmail_status`, `gmail_connect`; pre-meeting-mail's
 * `gmail_connect_for_meetings`, `premeeting_mail_status`). Called by name, not through bindings.ts,
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
  // This build has the mail before a meeting (pre-meeting-mail): its
  // sign-in also asks to see who replied, and the notice says so.
  const [withReplies, setWithReplies] = useState(false);

  useEffect(() => {
    if (preview) return;
    const status = invoke<{ available: boolean; connected: string | null }>(
      "gmail_status",
    ).then((s) => {
      setAvailable(!!s?.available);
      setConnected(s?.connected ?? null);
    });
    // Only asks whether the command exists; it changes nothing.
    const replies = invoke("premeeting_mail_status").then(
      (r) => setWithReplies(r != null),
      () => setWithReplies(false),
    );
    Promise.allSettled([status, replies]).then((r) => {
      if (r[0].status === "rejected") setAvailable(false);
      setLoaded(true);
    });
  }, [preview]);

  /** Opens Google's consent page and waits for the answer (up to 5 min). Until Google has
   * verified the app, Google warns "unverified app" there; cancelling or
   * failing leaves the row as it was, with the reason. */
  const connect = useCallback(async () => {
    setBusy(true);
    setError(null);
    try {
      // The notice beside the button names exactly what this asks for.
      const address = await invoke<string>(
        withReplies ? "gmail_connect_for_meetings" : "gmail_connect",
      );
      setConnected(address || "Gmail");
      // Connecting writes meetings.gmail_address; a later save of a stale
      // copy of the settings would wipe it.
      await refreshSettings();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }, [refreshSettings, withReplies]);

  return {
    available,
    connected,
    busy,
    error,
    connect,
    loaded,
    withReplies,
  };
}

/* Which sign-in: with the mail before a meeting, one sign-in asks to send
 * and to see the sender and subject of mails (`gmail.metadata`: a guest's
 * reply to that mail stops the recording), `gmail_connect_for_meetings`.
 * Without it, send only, `gmail_connect`. Google wants the notice beside
 * the button to describe exactly the page it opens (privacy-google,
 * notes-to-all, 2026-11-24). */

/** The Mac's region ("DE" from "en-DE"), as System Settings sets it; null
 * when unknown. */
export function useMacRegion(preview = false): string | null {
  const [region, setRegion] = useState<string | null>(null);
  useEffect(() => {
    if (preview) return;
    void commands
      .dateFormatLocale()
      .then((tag) => {
        const r = tag
          ?.split("-u-")[0]
          .split("-")
          .slice(1)
          .find((p) => /^[A-Za-z]{2}$/.test(p));
        setRegion(r ? r.toUpperCase() : null);
      })
      .catch(() => setRegion(null));
  }, [preview]);
  return region;
}
