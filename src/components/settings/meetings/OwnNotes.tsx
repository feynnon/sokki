import React, { useEffect, useLayoutEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { commands, type LiveLine, type MeetingRecord } from "@/bindings";

/** Waits this long after the last key before saving (ms). */
const SAVE_DELAY = 400;

/** The person's own notes: a plain text field, saved as they type
 * (`meetings/<id>/notes.md`). */
export const NotesEditor: React.FC<{
  record: MeetingRecord;
  autoFocus?: boolean;
  /** Fewer rows when shown after the meeting. */
  minRows?: number;
}> = ({ record, autoFocus, minRows = 12 }) => {
  const { t } = useTranslation();
  const [text, setText] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const area = useRef<HTMLTextAreaElement>(null);
  const pending = useRef<string | null>(null);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const id = record.id;

  const flush = () => {
    if (timer.current) clearTimeout(timer.current);
    timer.current = null;
    const value = pending.current;
    pending.current = null;
    if (value === null) return;
    void commands.setMeetingNotes(id, value).then((r) => {
      setError(r.status === "error" ? r.error : null);
    });
  };

  useEffect(() => {
    let live = true;
    setText(null);
    void commands.getMeetingNotes(id).then((v) => {
      if (live) setText(v);
    });
    return () => {
      live = false;
      flush();
    };
  }, [id]);

  // Grows with its text; the page scrolls, not the field.
  useLayoutEffect(() => {
    const el = area.current;
    if (!el) return;
    el.style.height = "auto";
    el.style.height = `${el.scrollHeight}px`;
  }, [text]);

  useEffect(() => {
    if (autoFocus && text !== null) area.current?.focus();
  }, [autoFocus, text !== null]);

  if (text === null) return null;
  return (
    <div className="flex flex-col gap-1">
      <textarea
        ref={area}
        data-own-notes
        value={text}
        rows={minRows}
        spellCheck
        aria-label={t("meetings.notes.label")}
        placeholder={t("meetings.notes.placeholder")}
        onChange={(e) => {
          setText(e.target.value);
          pending.current = e.target.value;
          if (timer.current) clearTimeout(timer.current);
          timer.current = setTimeout(flush, SAVE_DELAY);
        }}
        onBlur={flush}
        className="w-full resize-none overflow-hidden bg-transparent text-[15px] leading-[26px] text-text placeholder:text-ink-3 focus:outline-none select-text"
      />
      {error && <p className="text-xs text-red-500 select-text">{error}</p>}
    </div>
  );
};

function clock(s: number): string {
  const t = Math.max(0, Math.floor(s));
  const h = Math.floor(t / 3600);
  const m = Math.floor((t % 3600) / 60);
  const sec = String(t % 60).padStart(2, "0");
  return h
    ? `${h}:${String(m).padStart(2, "0")}:${sec}`
    : `${String(m).padStart(2, "0")}:${sec}`;
}

/** What the live transcript has heard so far, newest at the bottom. */
export const LiveColumn: React.FC<{ record: MeetingRecord }> = ({ record }) => {
  const { t } = useTranslation();
  const [lines, setLines] = useState<LiveLine[]>([]);
  const end = useRef<HTMLDivElement>(null);
  const box = useRef<HTMLDivElement>(null);

  useEffect(() => {
    let live = true;
    const read = () =>
      void commands.meetingLiveTranscript().then((l) => {
        if (live) setLines(l);
      });
    read();
    const timer = setInterval(read, 1000);
    return () => {
      live = false;
      clearInterval(timer);
    };
  }, [record.id]);

  // Follows the newest line unless the person scrolled up to read.
  useEffect(() => {
    const b = box.current;
    if (!b) return;
    if (b.scrollHeight - b.scrollTop - b.clientHeight < 80) {
      end.current?.scrollIntoView({ block: "end" });
    }
  }, [lines.length]);

  return (
    <div className="flex min-h-0 flex-col gap-3" data-live-column>
      <div className="flex items-center gap-2 text-[12px] font-semibold uppercase tracking-wide text-ink-3">
        <span
          className="inline-block size-1.5 rounded-full animate-pulse"
          style={{ background: "var(--vb-rec)" }}
          aria-hidden="true"
        />
        {t("meetings.notes.live")}
      </div>
      <div
        ref={box}
        className="min-h-0 flex-1 overflow-y-auto pe-1 flex flex-col gap-3 text-[14px] leading-[21px] text-ink-2 select-text"
        aria-live="polite"
      >
        {lines.length === 0 ? (
          <p className="text-ink-3">{t("meetings.notes.listening")}</p>
        ) : (
          lines.map((l, i) => (
            <div key={i} className="flex gap-3">
              <span className="w-[42px] shrink-0 pt-px font-mono text-[12px] text-ink-3">
                {clock(l.start_s)}
              </span>
              <span>{l.text}</span>
            </div>
          ))
        )}
        <div ref={end} />
      </div>
    </div>
  );
};

/** The "Meine Notizen" section: the person's lines bright and word for
 * word, Sokki's additions grey under them. */
export const CombinedView: React.FC<{ markdown: string }> = ({ markdown }) => {
  const { t } = useTranslation();
  const blocks: { line: string; added: string[] }[] = [];
  for (const raw of markdown.split("\n")) {
    const nested = raw.match(/^\s{2,}- (.*)$/);
    if (nested && blocks.length) {
      blocks[blocks.length - 1].added.push(nested[1]);
      continue;
    }
    const top = raw.match(/^- (.*)$/);
    if (top) blocks.push({ line: top[1], added: [] });
  }
  return (
    <div className="flex flex-col gap-3 select-text" data-combined>
      <p className="text-[12.5px] text-ink-3">
        {t("meetings.notes.combinedHint")}
      </p>
      {blocks.map((b, i) => (
        <div key={i} className="flex flex-col gap-0.5">
          <div className="text-[15px] leading-[24px] text-text">{b.line}</div>
          {b.added.length > 0 && (
            <ul className="flex flex-col gap-0.5 ps-4">
              {b.added.map((a, j) => (
                <li
                  key={j}
                  className="flex gap-2 text-[15px] leading-[24px] text-ink-3"
                >
                  <span aria-hidden="true">•</span>
                  <span>{a}</span>
                </li>
              ))}
            </ul>
          )}
        </div>
      ))}
    </div>
  );
};

export type NoteView = "notes" | "combined" | "summary" | "transcript";

const VIEW_KEY = "sokki.meetings.view";

/** The view the person chose last (all meetings share it). */
export function rememberedView(): NoteView | null {
  try {
    const v = localStorage.getItem(VIEW_KEY);
    return v === "notes" ||
      v === "combined" ||
      v === "summary" ||
      v === "transcript"
      ? v
      : null;
  } catch {
    return null;
  }
}

export function rememberView(v: NoteView) {
  try {
    localStorage.setItem(VIEW_KEY, v);
  } catch {
    // Private window or blocked storage: it just isn't remembered.
  }
}

/** Notizen | Kombiniert | AI-Zusammenfassung | Transkript. */
export const ViewSwitch: React.FC<{
  value: NoteView;
  views: NoteView[];
  onChange: (v: NoteView) => void;
}> = ({ value, views, onChange }) => {
  const { t } = useTranslation();
  return (
    <div
      role="tablist"
      aria-label={t("meetings.notes.views")}
      className="inline-flex self-start h-[var(--vb-control-h)] rounded-[8px] border border-line bg-wash p-[2px] text-[13px] font-medium"
    >
      {views.map((v) => (
        <button
          key={v}
          type="button"
          role="tab"
          aria-selected={value === v}
          data-view={v}
          onClick={() => onChange(v)}
          className={`h-full cursor-pointer whitespace-nowrap rounded-[6px] px-3 transition-colors duration-150 focus:outline-none focus-visible:ring-1 focus-visible:ring-logo-primary ${
            value === v
              ? "bg-[var(--vb-segment)] text-text shadow-[0_1px_2px_rgba(0,0,0,0.25)]"
              : "text-ink-2 hover:text-text"
          }`}
        >
          {t(`meetings.notes.view.${v}`)}
        </button>
      ))}
    </div>
  );
};
