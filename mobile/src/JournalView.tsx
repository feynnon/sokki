/** Developer page: "Keep what I write". The Sokki keyboard's journal
 * (mobile/keyboard/.../dev/verbatim/sokki/Journal.kt): what was typed and
 * dictated, per app and time, on this phone only. Here it is switched on,
 * read, sent to the paired Mac (over the encrypted link) and deleted.
 * Files are read through the app's bridge (JournalBridge.kt). */
import { useEffect, useState } from "react";
import * as link from "./link";

interface Day {
  file: string; // journal-2026-11-24.jsonl
  entries: number;
  bytes: number;
}

interface Entry {
  written: number;
  app: string;
  text: string;
  prefill?: string;
  dictated?: { at: number; raw: string }[];
}

const bridge = () =>
  (
    window as unknown as {
      SokkiJournal?: {
        available(): boolean;
        isOn(): boolean;
        setOn(on: boolean): void;
        days(): string;
        read(file: string): string;
        deleteAll(): number;
      };
    }
  ).SokkiJournal;

function parse(jsonl: string): Entry[] {
  return jsonl
    .split("\n")
    .filter(Boolean)
    .flatMap((l) => {
      try {
        const e = JSON.parse(l);
        return e.t === "entry" ? [e as Entry] : [];
      } catch {
        return [];
      }
    })
    .reverse();
}

/** "com.whatsapp" → "whatsapp". */
const appName = (pkg: string) =>
  pkg
    .split(".")
    .filter((p) => p !== "com" && p !== "android" && p !== "apps")
    .pop() ?? pkg;

const dayLabel = (file: string) => {
  const d = new Date(file.slice(8, 18) + "T12:00:00");
  return isNaN(d.getTime())
    ? file
    : d.toLocaleDateString(undefined, {
        weekday: "short",
        day: "numeric",
        month: "short",
      });
};

const time = (ms: number) =>
  new Date(ms).toLocaleTimeString(undefined, {
    hour: "2-digit",
    minute: "2-digit",
  });

/** The page's link shows only for developers (JournalBridge.available). */
export const journalAvailable = () => {
  try {
    return bridge()?.available() ?? false;
  } catch {
    return false;
  }
};

export function JournalView({
  pairing,
  onBack,
}: {
  pairing: link.Pairing | null;
  onBack: () => void;
}) {
  const [on, setOn] = useState(() => bridge()?.isOn() ?? false);
  const [days, setDays] = useState<Day[]>([]);
  const [open, setOpen] = useState<string | null>(null);
  const [entries, setEntries] = useState<Entry[]>([]);
  const [sending, setSending] = useState(false);
  const [note, setNote] = useState<string | null>(null);
  const [confirmDelete, setConfirmDelete] = useState(false);

  const reload = () => {
    try {
      setDays(JSON.parse(bridge()?.days() ?? "[]"));
    } catch {
      setDays([]);
    }
  };
  useEffect(() => {
    reload();
    // Back from another app: show what the keyboard wrote meanwhile.
    const onShow = () => document.visibilityState === "visible" && reload();
    document.addEventListener("visibilitychange", onShow);
    return () => document.removeEventListener("visibilitychange", onShow);
  }, []);

  const toggle = (v: boolean) => {
    bridge()?.setOn(v);
    setOn(bridge()?.isOn() ?? v);
  };

  const show = (file: string) => {
    if (open === file) return setOpen(null);
    setOpen(file);
    setEntries(parse(bridge()?.read(file) ?? ""));
  };

  const send = async () => {
    setSending(true);
    setNote(null);
    try {
      for (const d of days)
        await link.sendJournal(d.file, bridge()?.read(d.file) ?? "");
      setNote(
        `Sent ${days.length === 1 ? "1 day" : `${days.length} days`} to ${pairing?.name ?? "the Mac"}.`,
      );
    } catch (e) {
      setNote(String(e));
    } finally {
      setSending(false);
    }
  };

  const deleteAll = () => {
    if (!confirmDelete) return setConfirmDelete(true);
    bridge()?.deleteAll();
    setConfirmDelete(false);
    setOpen(null);
    setNote("Deleted.");
    reload();
  };

  const total = days.reduce((n, d) => n + d.entries, 0);
  const kb = Math.max(
    1,
    Math.round(days.reduce((n, d) => n + d.bytes, 0) / 1024),
  );

  return (
    <main className="page journal">
      <header className="top">
        <button className="link" onClick={onBack}>
          ‹ Back
        </button>
      </header>
      <h1>Keep what I write</h1>
      <p className="muted">
        Saves what you type and dictate, with the app and time, on this phone
        only. Never passwords or number fields. For improving Sokki on your own
        data.
      </p>
      <p className="muted small">
        For a dictation it also keeps what Sokki heard, next to what you sent.
        Off in incognito mode.
      </p>

      <label className="switch-row">
        <span>Keep what I write</span>
        <input
          type="checkbox"
          checked={on}
          onChange={(e) => toggle(e.target.checked)}
        />
      </label>

      {days.length > 0 ? (
        <>
          <p className="muted small">
            {total} {total === 1 ? "entry" : "entries"} · {kb} KB · since{" "}
            {dayLabel(days[days.length - 1].file)}
          </p>
          <div className="journal-actions">
            {pairing && (
              <button
                className="secondary"
                disabled={sending}
                onClick={() => void send()}
              >
                {sending ? "Sending…" : `Send to ${pairing.name}`}
              </button>
            )}
            <button className="secondary danger" onClick={deleteAll}>
              {confirmDelete
                ? `Delete ${total} ${total === 1 ? "entry" : "entries"}?`
                : "Delete all"}
            </button>
          </div>
          {note && <p className="muted small">{note}</p>}
          <section className="list">
            {days.map((d) => (
              <div key={d.file}>
                <button className="item" onClick={() => show(d.file)}>
                  <span className="item-title">{dayLabel(d.file)}</span>
                  <span className="item-state">
                    {d.entries} {d.entries === 1 ? "entry" : "entries"}
                  </span>
                </button>
                {open === d.file &&
                  entries.map((e, i) => (
                    <div key={i} className="journal-entry">
                      <span className="muted small">
                        {time(e.written)} · {appName(e.app)}
                      </span>
                      <p>{e.text}</p>
                      {e.dictated?.map((x, j) => (
                        <p key={j} className="muted small">
                          Heard: {x.raw}
                        </p>
                      ))}
                    </div>
                  ))}
              </div>
            ))}
          </section>
        </>
      ) : (
        <p className="empty">
          {on
            ? "Nothing kept yet. Write something with the Sokki keyboard."
            : "Nothing kept."}
        </p>
      )}
      {note && days.length === 0 && <p className="muted small">{note}</p>}
    </main>
  );
}
