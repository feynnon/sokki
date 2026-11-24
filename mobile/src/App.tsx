import { useCallback, useEffect, useRef, useState } from "react";
import "./App.css";
import * as link from "./link";
import { DictateView } from "./DictateView";
import { KeyboardSetup } from "./KeyboardSetup";
import { GetComputerCard } from "./GetComputerCard";

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
  (window as unknown as { SokkiScreen?: { keepOn(on: boolean): void } })
    .SokkiScreen;
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
  | { name: "detail"; id: string };

function clock(s: number) {
  const m = Math.floor(s / 60);
  return `${m}:${String(Math.floor(s % 60)).padStart(2, "0")}`;
}

function when(iso: string) {
  return new Date(iso).toLocaleString(undefined, {
    weekday: "short",
    hour: "2-digit",
    minute: "2-digit",
    day: "numeric",
    month: "short",
  });
}

const STAGE: Record<string, string> = {
  waiting: "Waiting on the Mac…",
  diarizing: "Finding speakers…",
  transcribing: "Transcribing…",
  summarizing: "Writing summary…",
  writing: "Saving note…",
};

function stateLine(r: Recording) {
  switch (r.state) {
    case "waiting":
      return r.error ? "Not sent: Mac not reachable" : "Not sent yet";
    case "uploading":
      return "Sending to the Mac…";
    case "processing":
      return STAGE[r.stage ?? ""] ?? "Processing on the Mac…";
    case "done":
      return "Ready";
    case "failed":
      return r.error ?? "Failed";
  }
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
  const [checking, setChecking] = useState(false);
  const [checkNote, setCheckNote] = useState<string | null>(null);
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
      if (
        !opened &&
        !openedToRecord.current &&
        !p &&
        localStorage.getItem("sokki.setupSkipped") !== "1"
      )
        setView({ name: "setup" });
    });
  }, []);
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

  const send = useCallback(
    async (r: Recording): Promise<boolean> => {
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
        return true;
      } catch (e) {
        update(r.localId, { state: "waiting", error: String(e) });
        return false;
      }
    },
    [update],
  );

  /** A finished recording: keep it on the phone, then send it. */
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
      void send(r);
      return r.localId;
    },
    [send],
  );

  /** Only when the user asks (and once right after recording): send what
   * is unsent, fetch what the Mac has finished. No retries in between. */
  const checkAgain = useCallback(async () => {
    setChecking(true);
    setCheckNote(null);
    let unreachable = 0;
    let open = 0;
    for (const r of listRef.current) {
      if (r.state === "waiting" || r.state === "uploading") {
        if (!(await send(r))) unreachable++;
      } else if (r.state === "processing" && r.remoteId) {
        try {
          const s = await link.status(r.remoteId);
          if (s.status === "done")
            update(r.localId, {
              state: "done",
              title: s.title,
              summary: s.summary,
              transcript: s.transcript,
              stage: null,
            });
          else if (s.status === "failed")
            update(r.localId, { state: "failed", error: s.error });
          else {
            update(r.localId, { stage: s.stage });
            open++;
          }
        } catch {
          unreachable++;
        }
      }
    }
    setChecking(false);
    setCheckNote(
      unreachable
        ? "Can't reach the Mac right now. Is it on and is Sokki running?"
        : open
          ? "The Mac is still working on it. Check again in a minute."
          : null,
    );
  }, [send, update]);

  // For automated tests: add a recording without the microphone.
  useEffect(() => {
    (window as unknown as { __verbatim: unknown }).__verbatim = {
      add: (b64: string, type: string, durationS: number) => {
        const bytes = Uint8Array.from(atob(b64), (c) => c.charCodeAt(0));
        return add(new Blob([bytes], { type }), new Date(), durationS);
      },
      list: () => listRef.current,
      checkAgain,
      pair: (ticket: string) => link.pair(ticket).then((p) => (setPairing(p), p)),
    };
  }, [add, checkAgain]);

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

  if (view.name === "detail") {
    const r = list.find((x) => x.localId === view.id);
    if (!r) return null;
    return (
      <Detail
        r={r}
        checking={checking}
        onCheck={() => void checkAgain()}
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
        finishedRecording={list.some((r) => r.state === "done")}
      />

      {mode === "dictate" ? (
        <>
          <KeyboardSetup />
          <DictateView />
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
          </section>

          {list.some((r) => r.state === "waiting" || r.state === "processing") && (
            <div className="check">
              <button
                className="secondary"
                disabled={checking}
                onClick={() => void checkAgain()}
              >
                {checking ? "Checking…" : "Check again"}
              </button>
              {checkNote && <p className="muted small">{checkNote}</p>}
            </div>
          )}

          <section className="list">
            {list.length === 0 && (
              <p className="empty">
                Your recordings appear here. The Mac turns each one into a note
                with a summary.
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
                  {clock(r.durationS)} · {stateLine(r)}
                </span>
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
  checking,
  onCheck,
  onBack,
  onDelete,
}: {
  r: Recording;
  checking: boolean;
  onCheck: () => void;
  onBack: () => void;
  onDelete: () => void;
}) {
  const [copied, setCopied] = useState(false);
  const [showTranscript, setShowTranscript] = useState(false);
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(r.transcript ?? "");
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    } catch {
      setShowTranscript(true);
    }
  };
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
      <h1>{r.title ?? when(r.startedAt)}</h1>
      <p className="muted">
        {when(r.startedAt)} · {clock(r.durationS)} · {stateLine(r)}
      </p>
      {r.state === "done" ? (
        <>
          <button className="primary" onClick={() => void copy()}>
            {copied ? "Copied" : "Copy transcript"}
          </button>
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
            {r.state === "waiting"
              ? "This recording is on your phone. It is sent when you tap Check again and the Mac is on."
              : "The Mac turns it into a note. Tap Check again to fetch it."}
          </p>
          {r.error && r.state === "waiting" && (
            <p className="muted small">{r.error}</p>
          )}
          <button className="secondary" disabled={checking} onClick={onCheck}>
            {checking ? "Checking…" : "Check again"}
          </button>
        </>
      )}
    </main>
  );
}
