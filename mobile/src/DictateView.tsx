import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { feedWav, startMic } from "./dictate";

interface ModelInfo {
  present: boolean;
  /** The previous speech model is there (before the streaming one). */
  old_present: boolean;
  path: string;
  loaded: boolean;
}

type Phase =
  | "checking"
  | "missing"
  | "downloading"
  | "loading"
  | "ready"
  | "listening"
  | "finishing";

/** Dictation on the phone itself: text appears while you speak. */
export function DictateView() {
  const [phase, setPhase] = useState<Phase>("checking");
  const [pieces, setPieces] = useState<string[]>([]);
  const [download, setDownload] = useState(0);
  const [info, setInfo] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);
  const stopMic = useRef<(() => Promise<void>) | null>(null);
  const done = useRef<((ms: number) => void) | null>(null);
  // Per piece: seconds of audio, ms to transcribe, ms since start (tests).
  const stats = useRef<{ audio_s: number; ms: number; at: number }[]>([]);
  const started = useRef(0);

  const fetchModel = async () => {
    setPhase("downloading");
    try {
      await invoke("dictation_download");
      await prepare();
    } catch (e) {
      setError(String(e));
      setPhase("missing");
    }
  };

  const prepare = async (): Promise<void> => {
    setError(null);
    const m = await invoke<ModelInfo>("dictation_model");
    // Had the previous model: the person chose on-device dictation, so the
    // new one comes by itself (it replaces the old one).
    if (!m.present && m.old_present) return fetchModel();
    if (!m.present) return setPhase("missing");
    if (!m.loaded) {
      setPhase("loading");
      try {
        // Tests read the load time here; people don't need it (it read
        // "Model loaded in 0 s." with the streaming model).
        const ms = await invoke<number>("dictation_load");
        console.info(`Speech model loaded in ${ms} ms`);
      } catch (e) {
        setError(String(e));
        return setPhase("missing");
      }
    }
    setPhase("ready");
  };

  useEffect(() => {
    void prepare();
    const subs = [
      listen<{ index: number; text: string; audio_s: number; ms: number }>(
        "dictation-piece",
        (e) => {
          stats.current.push({
            audio_s: Math.round(e.payload.audio_s * 10) / 10,
            ms: e.payload.ms,
            at: Date.now() - started.current,
          });
          setPieces((p) => {
            const next = [...p];
            next[e.payload.index] = e.payload.text;
            return next;
          });
        },
      ),
      listen<{ after_stop_ms: number }>("dictation-done", (e) => {
        setInfo(
          `Text ready ${(e.payload.after_stop_ms / 1000).toFixed(1)} s after you stopped.`,
        );
        setPhase("ready");
        // Counted for the "get Sokki on your computer" card.
        try {
          const n = Number(localStorage.getItem("sokki.dictations") ?? "0") + 1;
          localStorage.setItem("sokki.dictations", String(n));
        } catch {
          /* ignore */
        }
        done.current?.(e.payload.after_stop_ms);
        done.current = null;
      }),
      listen<{ done: number; total: number }>("dictation-download", (e) =>
        setDownload(e.payload.total ? e.payload.done / e.payload.total : 0),
      ),
    ];
    return () => subs.forEach((s) => s.then((f) => f()));
  }, []);

  const begin = async (lengths?: { minS?: number; maxS?: number }) => {
    setError(null);
    setInfo(null);
    setPieces([]);
    stats.current = [];
    started.current = Date.now();
    await invoke("dictation_start", lengths ?? {});
    setPhase("listening");
  };

  const start = async () => {
    try {
      await begin();
      stopMic.current = await startMic();
    } catch (e) {
      setError(`Microphone not available: ${(e as Error).message ?? e}`);
      setPhase("ready");
    }
  };

  const stop = async () => {
    setPhase("finishing");
    await stopMic.current?.();
    stopMic.current = null;
    await invoke("dictation_stop");
  };

  // For automated tests: dictate a WAV as if spoken; resolves with the text
  // and the seconds from stop to final text.
  useEffect(() => {
    (window as unknown as { __dictate: unknown }).__dictate = async (
      b64: string,
      lengths?: { minS?: number; maxS?: number },
    ) => {
      await begin(lengths);
      await feedWav(b64);
      const after = new Promise<number>((r) => (done.current = r));
      setPhase("finishing");
      await invoke("dictation_stop");
      const ms = await after;
      return { afterStopMs: ms, pieces: stats.current };
    };
    (window as unknown as { __dictateText: unknown }).__dictateText = () =>
      document.getElementById("dictation-text")?.textContent ?? "";
  });

  const text = pieces.filter(Boolean).join(" ");
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(text);
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    } catch {
      /* the text stays selectable */
    }
  };

  if (phase === "missing" || phase === "downloading") {
    return (
      <section className="dictate">
        <p className="muted">
          Dictation on the phone needs the speech model once (740 MB). It then
          works without the Mac and without internet.
        </p>
        <button
          className="primary"
          disabled={phase === "downloading"}
          onClick={() => void fetchModel()}
        >
          {phase === "downloading"
            ? `Downloading… ${Math.round(download * 100)} %`
            : "Download speech model"}
        </button>
        {error && <p className="error">{error}</p>}
      </section>
    );
  }

  const listening = phase === "listening";
  return (
    <section className="dictate">
      <div className="recorder">
        <button
          className={`rec ${listening ? "on" : ""}`}
          disabled={
            phase === "checking" || phase === "loading" || phase === "finishing"
          }
          onClick={listening ? stop : start}
          aria-label={listening ? "Stop dictating" : "Start dictating"}
        >
          <span className="rec-inner" />
        </button>
        <p className="rec-label">
          {phase === "loading"
            ? "Preparing the speech model…"
            : phase === "finishing"
              ? "Finishing…"
              : listening
                ? "Listening — speak, then tap to stop"
                : "Tap to dictate"}
        </p>
      </div>
      <div id="dictation-text" className="dictation-text">
        {text || (
          <span className="muted">Your words appear here while you speak.</span>
        )}
      </div>
      {text && phase === "ready" && (
        <button className="primary" onClick={() => void copy()}>
          {copied ? "Copied" : "Copy text"}
        </button>
      )}
      {info && <p className="muted small">{info}</p>}
      {error && <p className="error">{error}</p>}
    </section>
  );
}
