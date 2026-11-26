import { useCallback, useEffect, useRef, useState } from "react";
import "./App.css";
import * as link from "./link";
import { DictateView } from "./DictateView";
import { KeyboardSetup } from "./KeyboardSetup";
import { GetComputerCard } from "./GetComputerCard";
import { JournalView, journalAvailable } from "./JournalView";

/** "https://sokki.org/p#sokki1:…" or "sokki1:…" → the ticket. */
function ticketOf(text: string) {
  const t = text.trim();
  return t.includes("#") ? t.slice(t.indexOf("#") + 1) : t;
}

interface Links {
  takeTicket(): string | null;
}
const links = () => (window as unknown as { SokkiLinks?: Links }).SokkiLinks;
/** Keeps the screen on while recording (MainActivity.kt): with the screen
 * off the app is in the background and its microphone gives silence. */
const screen = () =>
  (
    window as unknown as {
      SokkiScreen?: { keepOn(on: boolean): void; uses24Hour?(): boolean };
    }
  ).SokkiScreen;
/** The Quick Settings tile (RecordTile.kt) opens Sokki to record and stops
 * a recording in place; the page tells it when it records. `silenced()`:
 * Android gives the recording silence right now (during a call).
 * `offerTile` shows Android's "Add tile" dialog, once per install. */
const tile = () =>
  (
    window as unknown as {
      SokkiRecorder?: {
        takeStart(): boolean;
        recording(on: boolean): void;
        silenced?(): boolean;
        offerTile?(): void;
      };
    }
  ).SokkiRecorder;
/** "Note ready" while Sokki is closed (NoteCheck.kt): the page reports each
 * recording the Mac has taken (`watch`) and each note it fetched itself
 * (`seen`); Android asks the Mac about the rest every 15 minutes. */
const notes = () =>
  (
    window as unknown as {
      SokkiNotes?: {
        watch(id: string): void;
        seen(id: string): void;
        askPermission(): void;
        share(title: string, text: string): void;
        copy?(text: string): void;
      };
    }
  ).SokkiNotes;
/** The note is on screen, so "Note ready" isn't needed (and is taken
 * back). Not while the app is out of sight: Android also runs the page's
 * checks when it wakes the app for NoteJob, and the notification must stay. */
function seenIfShown(remoteId: string) {
  if (document.visibilityState === "visible") notes()?.seen(remoteId);
}
/** While the app is open: the Mac's progress every few seconds, and an
 * unsent recording tried again every minute. */
const POLL_MS = 4000;
const RESEND_MS = 60_000;
import { Format, scan } from "@tauri-apps/plugin-barcode-scanner";
import { openUrl } from "@tauri-apps/plugin-opener";
import { inviteMailto } from "../../shared/invite";
import {
  type Recording,
  deleteAudio,
  getAudio,
  loadList,
  putAudio,
  saveList,
} from "./storage";

type View =
  | { name: "list" }
  | { name: "setup"; ticket?: string }
  | { name: "detail"; id: string }
  | { name: "journal" };

function clock(s: number) {
  const m = Math.floor(s / 60);
  return `${m}:${String(Math.floor(s % 60)).padStart(2, "0")}`;
}

/** "24.11. · 15:20" on a German phone, "11/24 · 2:45 AM" on a US one: the
 * phone's language, and its own 12/24-hour setting (like the Mac's list). */
function when(iso: string) {
  const d = new Date(iso);
  let h24: boolean | undefined;
  try {
    h24 = screen()?.uses24Hour?.();
  } catch {
    h24 = undefined;
  }
  const date = d.toLocaleDateString(undefined, {
    day: "numeric",
    month: "numeric",
  });
  const time = d.toLocaleTimeString(undefined, {
    hour: "numeric",
    minute: "2-digit",
    ...(h24 === undefined ? {} : { hour12: !h24 }),
  });
  return `${date} · ${time}`;
}

const STAGE: Record<string, string> = {
  waiting: "Waiting on the Mac…",
  diarizing: "Finding speakers…",
  transcribing: "Transcribing…",
  summarizing: "Writing summary…",
  writing: "Saving note…",
};

/** Without a Mac a recording waits on the phone: meetings are never
 * transcribed on the phone, it costs too much battery (Robin, 26.11.2026;
 * 30 minutes took 12.6 minutes and ~7 % battery on a Pixel 7,
 * docs/research/android-phone-transcript-2026-11.md). */
function stateLine(r: Recording, paired: boolean) {
  switch (r.state) {
    case "waiting":
      if (!paired) return "Connect your Mac for a transcript";
      return r.error ? "Not sent: Mac not reachable" : "Not sent yet";
    case "uploading":
      return "Sending to the Mac…";
    case "processing": {
      const stage = STAGE[r.stage ?? ""] ?? "Processing on the Mac…";
      return r.progress != null
        ? `${stage} ${Math.round(r.progress * 100)} %`
        : stage;
    }
    case "done":
      return "Ready";
    case "failed":
      return r.error ?? "Failed";
  }
}

/** The Mac's summary (Markdown) as plain text for pasting and sharing,
 * like NoteCheck.plain: "## Tasks" → "Tasks", "- [ ] x" → "☐ x",
 * "- x" → "• x", no "**". */
function plain(md: string) {
  return md
    .split("\n")
    .map((raw) => {
      const l = raw.trimEnd().split("**").join("");
      if (l.startsWith("## ")) return l.slice(3);
      if (l.startsWith("- [ ] ")) return `☐ ${l.slice(6)}`;
      if (l.startsWith("- [x] ")) return `☑ ${l.slice(6)}`;
      if (l.startsWith("- ")) return `• ${l.slice(2)}`;
      return l;
    })
    .join("\n")
    .trim();
}

/** "**bold**" → <strong>. */
function inline(text: string) {
  return text
    .split(/(\*\*[^*]+\*\*)/g)
    .filter(Boolean)
    .map((p, i) =>
      p.startsWith("**") && p.endsWith("**") ? (
        <strong key={i}>{p.slice(2, -2)}</strong>
      ) : (
        <span key={i}>{p}</span>
      ),
    );
}

function Summary({ md }: { md: string }) {
  return (
    <div className="summary">
      {md.split("\n").map((raw, i) => {
        const line = raw.trimEnd();
        if (!line.trim()) return null;
        if (line.startsWith("## ")) return <h3 key={i}>{line.slice(3)}</h3>;
        const task = line.match(/^- \[( |x)\] (.*)$/);
        if (task)
          return (
            <p key={i} className="task">
              <span className={`box ${task[1] === "x" ? "on" : ""}`} />
              <span>{inline(task[2])}</span>
            </p>
          );
        if (line.startsWith("- "))
          return (
            <p key={i} className="bullet">
              <span className="dot">•</span>
              <span>{inline(line.slice(2))}</span>
            </p>
          );
        return <p key={i}>{inline(line)}</p>;
      })}
    </div>
  );
}

export default function App() {
  const [view, setView] = useState<View>({ name: "list" });
  const [mode, setMode] = useState<"meeting" | "dictate">(() =>
    localStorage.getItem("verbatim.mode") === "dictate" ? "dictate" : "meeting",
  );
  const switchMode = (m: "meeting" | "dictate") => {
    setMode(m);
    try {
      localStorage.setItem("verbatim.mode", m);
    } catch {
      /* ignore */
    }
  };
  const [pairing, setPairing] = useState<link.Pairing | null>(null);
  const [pairingKnown, setPairingKnown] = useState(false);
  useEffect(() => {
    // A pairing link (sokki.org/p#… or sokki://pair#…) opens pairing with it.
    const fromLink = () => {
      const t = links()?.takeTicket();
      if (t) setView({ name: "setup", ticket: t });
      return !!t;
    };
    (window as unknown as { __sokkiPairLink: unknown }).__sokkiPairLink = fromLink;
    const opened = fromLink();
    void link.pairing().then((p) => {
      setPairing(p);
      setPairingKnown(true);
      // Without a Mac the app starts on Dictate (the keyboard works on its
      // own); pairing is under "Mac" at the top.
      let chosen: string | null = null;
      try {
        chosen = localStorage.getItem("verbatim.mode");
      } catch {
        /* ignore */
      }
      if (!opened && !openedToRecord.current && !p && chosen === null)
        setMode("dictate");
    });
  }, []);
  const pairingRef = useRef<link.Pairing | null>(null);
  useEffect(() => {
    pairingRef.current = pairing;
  });
  const [list, setList] = useState<Recording[]>(loadList);
  const [recording, setRecording] = useState(false);
  const [elapsed, setElapsed] = useState(0);
  const [silenced, setSilenced] = useState(false);
  const [micError, setMicError] = useState<string | null>(null);
  const recorder = useRef<MediaRecorder | null>(null);
  const starting = useRef(false);
  const openedToRecord = useRef(false);
  const started = useRef<Date | null>(null);
  const listRef = useRef(list);

  const update = useCallback((localId: string, patch: Partial<Recording>) => {
    const next = listRef.current.map((r) =>
      r.localId === localId ? { ...r, ...patch } : r,
    );
    listRef.current = next;
    setList(next);
    saveList(next);
  }, []);

  const sending = useRef(new Set<string>());
  const send = useCallback(
    async (r: Recording): Promise<boolean> => {
      if (sending.current.has(r.localId)) return false;
      sending.current.add(r.localId);
      try {
        const audio = await getAudio(r.localId);
        if (!audio) {
          update(r.localId, { state: "failed", error: "The audio is gone." });
          return false;
        }
        update(r.localId, { state: "uploading", error: null });
        try {
          const id = await link.upload(audio, new Date(r.startedAt));
          update(r.localId, {
            state: "processing",
            remoteId: id,
            stage: "waiting",
          });
          await deleteAudio(r.localId);
          // "Note ready" also while Sokki is closed; Android asks once.
          notes()?.watch(id);
          notes()?.askPermission();
          return true;
        } catch (e) {
          update(r.localId, { state: "waiting", error: String(e) });
          return false;
        }
      } finally {
        sending.current.delete(r.localId);
      }
    },
    [update],
  );

  /** A finished recording: keep it on the phone, then send it to the Mac
   * (without a Mac it waits until one is connected). */
  const add = useCallback(
    async (audio: Blob, startedAt: Date, durationS: number) => {
      const r: Recording = {
        localId: `${Date.now()}`,
        startedAt: startedAt.toISOString(),
        durationS,
        remoteId: null,
        state: "waiting",
        stage: null,
        error: null,
        title: null,
        summary: null,
        transcript: null,
      };
      await putAudio(r.localId, audio);
      const next = [r, ...listRef.current];
      listRef.current = next;
      setList(next);
      saveList(next);
      if (pairingRef.current) void send(r);
      return r.localId;
    },
    [send],
  );

  /** Fetches the Mac's progress and finished notes, and with `resend`
   * sends what is unsent. Runs by itself while the app is open (no "Check
   * again"): every few seconds while the Mac works, every minute while a
   * recording waits, and whenever the app comes to the front. */
  const busy = useRef(false);
  const refresh = useCallback(
    async (resend: boolean) => {
      if (busy.current || !pairingRef.current) return;
      busy.current = true;
      try {
        for (const r of listRef.current) {
          if (r.state === "waiting" || r.state === "uploading") {
            if (resend) await send(r);
          } else if (r.state === "processing" && r.remoteId) {
            try {
              const s = await link.status(r.remoteId);
              if (s.status === "done") {
                update(r.localId, {
                  state: "done",
                  title: s.title,
                  summary: s.summary,
                  transcript: s.transcript,
                  stage: null,
                  progress: null,
                });
                seenIfShown(r.remoteId);
              } else if (s.status === "failed") {
                update(r.localId, { state: "failed", error: s.error });
                seenIfShown(r.remoteId);
              } else {
                update(r.localId, { stage: s.stage, progress: s.progress });
              }
            } catch {
              /* The Mac isn't reachable right now: the next round asks again. */
            }
          }
        }
      } finally {
        busy.current = false;
      }
    },
    [send, update],
  );
  const working = list.some((r) => r.state === "processing");
  const unsent = list.some((r) => r.state === "waiting");
  useEffect(() => {
    if (!pairing) return;
    void refresh(true);
    const onShow = () => {
      if (document.visibilityState !== "visible") return;
      // Notes fetched while the app was out of sight: now they're seen.
      for (const r of listRef.current)
        if (r.remoteId && (r.state === "done" || r.state === "failed"))
          notes()?.seen(r.remoteId);
      void refresh(true);
    };
    document.addEventListener("visibilitychange", onShow);
    return () => document.removeEventListener("visibilitychange", onShow);
  }, [pairing, refresh]);
  useEffect(() => {
    if (!pairing || !working) return;
    const t = setInterval(() => void refresh(false), POLL_MS);
    return () => clearInterval(t);
  }, [pairing, working, refresh]);
  useEffect(() => {
    if (!pairing || !unsent) return;
    const t = setInterval(() => void refresh(true), RESEND_MS);
    return () => clearInterval(t);
  }, [pairing, unsent, refresh]);

  // For automated tests: add a recording without the microphone.
  useEffect(() => {
    (window as unknown as { __verbatim: unknown }).__verbatim = {
      add: (b64: string, type: string, durationS: number) => {
        const bytes = Uint8Array.from(atob(b64), (c) => c.charCodeAt(0));
        return add(new Blob([bytes], { type }), new Date(), durationS);
      },
      list: () => listRef.current,
      checkAgain: () => refresh(true),
      pair: (ticket: string) => link.pair(ticket).then((p) => (setPairing(p), p)),
    };
  }, [add, refresh]);

  useEffect(() => {
    if (!recording) return;
    screen()?.keepOn(true);
    return () => screen()?.keepOn(false);
  }, [recording]);

  useEffect(() => {
    if (!recording) return;
    const t = setInterval(() => {
      if (started.current)
        setElapsed((Date.now() - started.current.getTime()) / 1000);
      setSilenced(tile()?.silenced?.() ?? false);
    }, 250);
    return () => {
      clearInterval(t);
      setSilenced(false);
    };
  }, [recording]);

  const start = async () => {
    if (recorder.current || starting.current) return;
    starting.current = true;
    setMicError(null);
    try {
      const stream = await navigator.mediaDevices.getUserMedia({ audio: true });
      const type = MediaRecorder.isTypeSupported("audio/webm;codecs=opus")
        ? "audio/webm;codecs=opus"
        : "";
      const rec = new MediaRecorder(stream, type ? { mimeType: type } : {});
      const chunks: Blob[] = [];
      rec.ondataavailable = (e) => e.data.size && chunks.push(e.data);
      rec.onstop = () => {
        stream.getTracks().forEach((tr) => tr.stop());
        const at = started.current ?? new Date();
        const dur = (Date.now() - at.getTime()) / 1000;
        void add(new Blob(chunks, { type: rec.mimeType }), at, dur);
      };
      started.current = new Date();
      setElapsed(0);
      rec.start(1000);
      recorder.current = rec;
      setRecording(true);
    } catch (e) {
      setMicError(`Microphone not available: ${(e as Error).message}`);
    } finally {
      starting.current = false;
    }
  };

  const stop = () => {
    recorder.current?.stop();
    recorder.current = null;
    setRecording(false);
  };

  // The tile: window.__sokkiRecord() starts a recording the tile asked for
  // (when Sokki opens, or from MainActivity if it's already open);
  // window.__sokkiRecord("stop") stops one.
  const controls = useRef({ start, stop });
  useEffect(() => {
    controls.current = { start, stop };
  });
  useEffect(() => {
    const record = (cmd?: "stop") => {
      if (cmd === "stop") return controls.current.stop();
      if (!tile()?.takeStart()) return;
      openedToRecord.current = true;
      setView({ name: "list" });
      setMode("meeting");
      void controls.current.start();
    };
    (window as unknown as { __sokkiRecord: unknown }).__sokkiRecord = record;
    record();
  }, []);
  useEffect(() => {
    tile()?.recording(recording);
  }, [recording]);
  // The first time the Meeting tab shows (after the Mac setup, if that comes
  // first), offer the tile: Android lets no app add it by itself.
  const onMeetingTab = pairingKnown && view.name === "list" && mode === "meeting";
  useEffect(() => {
    if (onMeetingTab) tile()?.offerTile?.();
  }, [onMeetingTab]);

  if (view.name === "setup") {
    return (
      <Setup
        key={view.ticket ?? "setup"}
        initial={pairing}
        fromLink={view.ticket}
        onDone={(p) => {
          setPairing(p);
          // Recordings made before the Mac was connected go to it now.
          pairingRef.current = p;
          setView({ name: "list" });
        }}
        onUnpair={async () => {
          await link.unpair();
          setPairing(null);
        }}
        onCancel={() => {
          try {
            localStorage.setItem("sokki.setupSkipped", "1");
          } catch {
            /* ignore */
          }
          setView({ name: "list" });
        }}
      />
    );
  }

  if (view.name === "journal") {
    return <JournalView pairing={pairing} onBack={() => setView({ name: "list" })} />;
  }

  if (view.name === "detail") {
    const r = list.find((x) => x.localId === view.id);
    if (!r) return null;
    return (
      <Detail
        r={r}
        paired={!!pairing}
        onConnect={() => setView({ name: "setup" })}
        onBack={() => setView({ name: "list" })}
        onDelete={async () => {
          await deleteAudio(r.localId);
          const next = listRef.current.filter((x) => x.localId !== r.localId);
          listRef.current = next;
          setList(next);
          saveList(next);
          setView({ name: "list" });
        }}
      />
    );
  }

  return (
    <main className="page">
      <header className="top">
        <Logo />
        <button className="link" onClick={() => setView({ name: "setup" })}>
          Mac
        </button>
      </header>

      <div className="tabs" role="tablist">
        {(["meeting", "dictate"] as const).map((m) => (
          <button
            key={m}
            role="tab"
            aria-selected={mode === m}
            className={mode === m ? "on" : ""}
            onClick={() => switchMode(m)}
          >
            {m === "meeting" ? "Meeting" : "Dictate"}
          </button>
        ))}
      </div>

      <GetComputerCard
        paired={!!pairing}
        recorded={list.length > 0}
      />

      {mode === "dictate" ? (
        <>
          <KeyboardSetup />
          <DictateView />
          {journalAvailable() && (
            <button className="link dev-link" onClick={() => setView({ name: "journal" })}>
              Developer: Keep what I write
            </button>
          )}
        </>
      ) : (
        <>
          <section className="recorder">
            <button
              className={`rec ${recording ? "on" : ""}`}
              onClick={recording ? stop : start}
              aria-label={recording ? "Stop recording" : "Start recording"}
            >
              <span className="rec-inner" />
            </button>
            <p className="rec-label">
              {recording ? clock(elapsed) : "Tap to record a meeting"}
            </p>
            {recording && silenced && (
              <p className="error">
                Android mutes the microphone during calls: this part stays
                silent
              </p>
            )}
            {micError && <p className="error">{micError}</p>}
            <p className="hint">Tell everyone you are recording.</p>
            {pairingKnown && !pairing && (
              <p className="hint">
                The transcript comes from Sokki on your Mac.{" "}
                <button
                  className="link inline"
                  onClick={() => setView({ name: "setup" })}
                >
                  Connect your Mac
                </button>
              </p>
            )}
          </section>

          <section className="list">
            {list.length === 0 && (
              <p className="empty">
                {pairing
                  ? "Your recordings appear here. The Mac turns each one into a note with a summary."
                  : "Your recordings appear here. Connect Sokki on your Mac for a transcript, with speakers and a summary."}
              </p>
            )}
            {list.map((r) => (
              <button
                key={r.localId}
                className="item"
                onClick={() => setView({ name: "detail", id: r.localId })}
              >
                <span className="item-title">
                  {r.title ?? when(r.startedAt)}
                </span>
                <span className={`item-state s-${r.state}`}>
                  {clock(r.durationS)} · {stateLine(r, !!pairing)}
                </span>
                {r.state === "processing" && r.progress != null && (
                  <span className="bar" aria-hidden="true">
                    <i style={{ width: `${Math.round(r.progress * 100)}%` }} />
                  </span>
                )}
                {r.state === "processing" && (
                  <span className="item-state">
                    We'll let you know when it's ready.
                  </span>
                )}
              </button>
            ))}
          </section>
        </>
      )}
    </main>
  );
}

function Logo() {
  const bars: [number, number, number][] = [
    [5, 10, 22],
    [13, 16, 22],
    [21, 22, 22],
    [29.5, 28, 26],
    [38, 22, 22],
    [46, 16, 22],
    [54, 10, 22],
  ];
  return (
    <span className="logo">
      <svg width="22" height="22" viewBox="0 0 64 64" aria-hidden="true">
        {bars.map(([x, y, h], i) => (
          <rect
            key={x}
            x={x}
            y={y}
            width={5}
            height={h}
            rx={2.5}
            fill={i === 3 ? "var(--accent)" : "currentColor"}
          />
        ))}
      </svg>
      Sokki
    </span>
  );
}

function Setup({
  initial,
  fromLink,
  onDone,
  onUnpair,
  onCancel,
}: {
  initial: link.Pairing | null;
  fromLink?: string;
  onDone: (p: link.Pairing) => void;
  onUnpair: () => Promise<void>;
  onCancel?: () => void;
}) {
  const [ticket, setTicket] = useState(fromLink ?? "");
  const [state, setState] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const connect = async (raw: string) => {
    const t = ticketOf(raw);
    if (!t.startsWith("sokki1:")) {
      setState("Scan the code shown on the Mac, or paste it here.");
      return;
    }
    setBusy(true);
    setState("Connecting to your Mac…");
    try {
      const p = await link.pair(t);
      setState(`Connected to ${p.name}.`);
      onDone(p);
    } catch (e) {
      setState(String(e));
    } finally {
      setBusy(false);
    }
  };
  // Opened from a pairing link: connect right away.
  useEffect(() => {
    if (fromLink) void connect(fromLink);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const scanCode = async () => {
    setState(null);
    try {
      const r = await scan({ formats: [Format.QRCode] });
      setTicket(r.content);
      await connect(r.content);
    } catch (e) {
      setState(`Scanning didn't work: ${String(e)}. Paste the code instead.`);
    }
  };
  return (
    <main className="page">
      <header className="top">
        <Logo />
        {onCancel && (
          <button className="link" onClick={onCancel}>
            {initial ? "Done" : "Skip"}
          </button>
        )}
      </header>
      <h1>Connect to your Mac</h1>
      {initial && (
        <p className="muted">
          Paired with {initial.name}.{" "}
          <button className="link danger" onClick={() => void onUnpair()}>
            Unpair
          </button>
        </p>
      )}
      <p className="muted">
        On the Mac: Sokki → Meetings → settings → Phone. Switch it on, then scan
        the code shown there. Phone and Mac can be on different networks;
        the Mac just has to be on when you send.
      </p>
      <button className="primary" onClick={() => void scanCode()} disabled={busy}>
        Scan the code
      </button>
      <label className="field">
        <span>Or paste the code</span>
        <input
          id="ticket"
          autoCapitalize="off"
          autoCorrect="off"
          value={ticket}
          onChange={(e) => setTicket(e.target.value)}
          placeholder="sokki.org/p#… or sokki1:…"
        />
      </label>
      <button
        className="secondary"
        onClick={() => void connect(ticket)}
        disabled={busy || !ticket.trim()}
      >
        {busy ? "Connecting…" : "Connect"}
      </button>
      {state && <p className="muted">{state}</p>}
      <p className="muted small">
        No Sokki on your Mac yet?{" "}
        <button
          className="link"
          onClick={() => void openUrl(inviteMailto("android"))}
        >
          Email me the setup
        </button>
      </p>
    </main>
  );
}

function Detail({
  r,
  paired,
  onConnect,
  onBack,
  onDelete,
}: {
  r: Recording;
  paired: boolean;
  onConnect: () => void;
  onBack: () => void;
  onDelete: () => void;
}) {
  const [copied, setCopied] = useState<"summary" | "transcript" | null>(null);
  const [showTranscript, setShowTranscript] = useState(false);
  const title = r.title ?? when(r.startedAt);
  const summary = plain(r.summary ?? "");
  const copy = async (what: "summary" | "transcript") => {
    const text =
      what === "summary" ? `${title}\n\n${summary}` : (r.transcript ?? "");
    try {
      // The WebView's own clipboard is refused (no secure context), so
      // Android's clipboard through the bridge.
      const bridge = notes();
      if (bridge?.copy) bridge.copy(text);
      else await navigator.clipboard.writeText(text);
      setCopied(what);
      setTimeout(() => setCopied(null), 1500);
    } catch {
      setShowTranscript(true);
    }
  };
  // The whole note: title, summary, then the transcript.
  const share = () =>
    notes()?.share(
      title,
      [title, summary, r.transcript ? `Transcript\n${r.transcript}` : ""]
        .filter(Boolean)
        .join("\n\n"),
    );
  return (
    <main className="page">
      <header className="top">
        <button className="link" onClick={onBack}>
          ‹ Back
        </button>
        <button className="link danger" onClick={onDelete}>
          Remove
        </button>
      </header>
      <h1>{title}</h1>
      <p className="muted">
        {when(r.startedAt)} · {clock(r.durationS)} · {stateLine(r, paired)}
      </p>
      {r.state === "done" ? (
        <>
          {notes() && (
            <button className="primary" onClick={share}>
              Share
            </button>
          )}
          <div className="row">
            <button className="secondary" onClick={() => void copy("summary")}>
              {copied === "summary" ? "Copied" : "Copy summary"}
            </button>
            <button
              className="secondary"
              onClick={() => void copy("transcript")}
            >
              {copied === "transcript" ? "Copied" : "Copy transcript"}
            </button>
          </div>
          {r.summary ? (
            <Summary md={r.summary} />
          ) : (
            <p className="muted">No summary.</p>
          )}
          <button className="link" onClick={() => setShowTranscript((s) => !s)}>
            {showTranscript ? "Hide transcript" : "Show transcript"}
          </button>
          {showTranscript && <pre className="transcript">{r.transcript}</pre>}
        </>
      ) : (
        <>
          <p className="muted">
            {!paired
              ? "This recording stays on your phone. Connect Sokki on your Mac and it writes the transcript, with speakers and a summary."
              : r.state === "waiting"
                ? "This recording is on your phone. Sokki sends it as soon as the Mac is reachable."
                : "The Mac turns it into a note with a summary. We'll let you know when it's ready."}
          </p>
          {paired && r.error && r.state === "waiting" && (
            <p className="muted small">{r.error}</p>
          )}
          {!paired && (
            <button className="primary" onClick={onConnect}>
              Connect your Mac
            </button>
          )}
        </>
      )}
    </main>
  );
}
