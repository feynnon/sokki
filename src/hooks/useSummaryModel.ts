import { useCallback, useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { commands, type LlmDownload, type SummarySetup } from "@/bindings";
import { useSettings } from "./useSettings";

/** What the built-in summary model needs while it writes a summary:
 * Gemma 4 E4B Q4_K_M with its MTP draft model through llama-server at 16k
 * context, 5.6 GB resident at its peak (5.1 GB of files plus working
 * memory; measured 2026-11-25 on the sample meeting, counted as the 9B's
 * 6.4 GB was). The summary test's 4.7 GB was measured without the draft
 * model and may not count the whole mapped file. */
export const SUMMARY_NEEDS_BYTES = 5.6e9;
/** The model and its draft model (`BUILTIN_MODEL.download_size()`,
 * crates/sokki-engine/src/llm.rs); dictation cleanup uses the same files. */
export const SUMMARY_DOWNLOAD_BYTES = 4_977_171_584 + 98_653_248;
/** Below this the Mac counts as too small. Set for the 9B (6.4 GB): an 8 GB
 * Mac would have kept 1.6 GB next to it for macOS, Sokki itself (0.5–1.5
 * GB, memguard.rs) and every other app; Apple sells no Mac between 8 and
 * 16 GB. With Gemma 4 E4B (5.6 GB) it would keep 2.4 GB; whether 8 GB Macs
 * get the built-in model is Robin's call, so this is unchanged. */
export const SMALL_MAC_BYTES = 12 * 2 ** 30;

export type SummaryState =
  | "loading"
  | "ready"
  | "missing"
  | "downloading"
  | "failed"
  | "unavailable";

/**
 * The built-in summary model for setup and the Meetings page: is it on
 * disk, does it fit this Mac, and its download, which runs in the app's
 * background (`meeting::pull`) and survives leaving the screen.
 * `preview`: nothing is asked or started (setup's debug preview).
 */
export function useSummaryModel(preview = false) {
  const { settings, refreshSettings } = useSettings();
  const [setup, setSetup] = useState<SummarySetup | null>(null);
  const [download, setDownload] = useState<LlmDownload | null>(null);
  const [failed, setFailed] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    const r = await commands.summaryModelSetup();
    if (r.status !== "ok") return;
    setSetup(r.data);
    setDownload(r.data.download);
  }, []);

  useEffect(() => {
    if (preview) return;
    void refresh();
    const unlisten = listen<LlmDownload>("meeting-llm-pull", (e) =>
      setDownload(e.payload),
    );
    return () => {
      unlisten.then((fn) => fn());
    };
  }, [preview, refresh]);

  // While it downloads, check now and then for its end (the last event
  // comes before the file is in place).
  const running = !!download;
  useEffect(() => {
    if (preview || !running) return;
    const timer = setInterval(() => void refresh(), 2000);
    return () => clearInterval(timer);
  }, [preview, running, refresh]);

  /** Makes the built-in model the summary model and downloads it. */
  const start = useCallback(async () => {
    if (preview) return;
    setFailed(null);
    setDownload({ status: "starting", completed: 0, total: 0 });
    const m = settings?.meetings;
    if (m && (m.llm_provider !== "builtin" || !m.summary_enabled)) {
      await commands.setMeetingSettings({
        ...m,
        llm_provider: "builtin",
        summary_enabled: true,
      });
      await refreshSettings();
    }
    void commands.pullMeetingLlm().then((r) => {
      if (r.status === "error") setFailed(r.error);
      void refresh();
    });
  }, [preview, settings?.meetings, refreshSettings, refresh]);

  const memory = setup?.memory_bytes ?? 0;
  const state: SummaryState = preview
    ? "missing"
    : !setup
      ? "loading"
      : setup.builtin === "ready"
        ? "ready"
        : download
          ? "downloading"
          : failed
            ? "failed"
            : setup.builtin === "unavailable"
              ? "unavailable"
              : "missing";
  return {
    state,
    download,
    failed,
    /** The Mac's memory in bytes; 0 when unknown. */
    memory,
    /** Unknown memory counts as enough. */
    fits: memory === 0 || memory >= SMALL_MAC_BYTES,
    start,
    refresh,
  };
}

/** Bytes as "6.4 GB" in the UI language (decimal, as macOS shows sizes). */
export function gigabytes(bytes: number, lang: string, digits = 1): string {
  return `${new Intl.NumberFormat(lang, {
    maximumFractionDigits: digits,
  }).format(bytes / 1e9)} GB`;
}

/** The Mac's memory as macOS names it: "16 GB" (binary, rounded). */
export function macMemory(bytes: number, lang: string): string {
  return `${new Intl.NumberFormat(lang).format(Math.round(bytes / 2 ** 30))} GB`;
}
