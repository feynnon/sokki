import React, { useEffect, useLayoutEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  AlignLeft,
  Check,
  Copy,
  FileText,
  FolderOpen,
  Globe,
  Loader2,
  Monitor,
  MoreHorizontal,
  RotateCcw,
  Send,
  Trash2,
} from "lucide-react";
import { save } from "@tauri-apps/plugin-dialog";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import {
  commands,
  events,
  type MailPreview,
  type MeetingNoteParts,
  type MeetingRecord,
  type VideoMeta,
} from "@/bindings";
import { usePressAway } from "@/hooks/usePressAway";
import { useMeetingStore } from "@/stores/meetingStore";
import { copyToClipboard } from "../history/clipboard";
import { ErrorBoundary } from "../../ErrorBoundary";

/** "**bold**" inside a line becomes <strong>. */
function inline(text: string): React.ReactNode[] {
  return text
    .split(/(\*\*[^*]+\*\*)/g)
    .filter(Boolean)
    .map((part, i) =>
      part.startsWith("**") && part.endsWith("**") ? (
        <strong key={i} className="font-semibold text-text">
          {part.slice(2, -2)}
        </strong>
      ) : (
        <React.Fragment key={i}>{part}</React.Fragment>
      ),
    );
}

/** The note's summary part: headings, bullets, tasks, plain lines. */
export const SummaryView: React.FC<{ markdown: string }> = ({ markdown }) => {
  const blocks: React.ReactNode[] = [];
  let list: React.ReactNode[] = [];
  const flush = () => {
    if (list.length) {
      blocks.push(
        <ul key={`l${blocks.length}`} className="flex flex-col gap-1">
          {list}
        </ul>,
      );
      list = [];
    }
  };
  markdown.split("\n").forEach((raw, i) => {
    const line = raw.trimEnd();
    if (!line.trim()) return flush();
    if (line.startsWith("## ")) {
      flush();
      blocks.push(
        <h4
          key={i}
          className="text-[12px] font-semibold uppercase tracking-wide text-ink-3 pt-2"
        >
          {line.slice(3)}
        </h4>,
      );
      return;
    }
    const task = line.match(/^- \[( |x)\] (.*)$/);
    if (task) {
      list.push(
        <li key={i} className="flex gap-2">
          <span
            aria-hidden="true"
            className={`mt-[5px] size-3.5 shrink-0 rounded-[4px] border ${task[1] === "x" ? "bg-logo-primary border-logo-primary" : "border-line-strong"}`}
          />
          <span>{inline(task[2])}</span>
        </li>,
      );
      return;
    }
    if (line.startsWith("- ")) {
      list.push(
        <li key={i} className="flex gap-2">
          <span aria-hidden="true" className="text-ink-3">
            •
          </span>
          <span>{inline(line.slice(2))}</span>
        </li>,
      );
      return;
    }
    flush();
    blocks.push(
      <p key={i} className="text-ink-2">
        {inline(line.replace(/^_(.*)_$/, "$1"))}
      </p>,
    );
  });
  flush();
  return (
    <div className="flex flex-col gap-2 text-[15px] leading-[24px] text-text select-text">
      {blocks}
    </div>
  );
};

/** "[00:03] Speaker 2: Hi." lines, time and name set apart. */
export const TranscriptView: React.FC<{ text: string }> = ({ text }) => (
  <div className="flex flex-col gap-1.5 font-mono text-[13px] leading-[1.7] text-ink-2 select-text">
    {text
      .split("\n")
      .filter((l) => l.trim())
      .map((line, i) => {
        const m = line.match(/^(\[[\d:]+\])\s+([^:]{1,60}):\s?(.*)$/);
        if (!m) return <div key={i}>{line}</div>;
        return (
          <div key={i}>
            <span className="text-ink-3">{m[1]}</span>{" "}
            <b className="font-semibold text-text">{m[2]}:</b> {m[3]}
          </div>
        );
      })}
  </div>
);

/** A finished meeting's note, loaded when shown or changed. */
export function useMeetingNote(record: MeetingRecord | null) {
  const [parts, setParts] = useState<MeetingNoteParts | null>(null);
  const [error, setError] = useState<string | null>(null);
  const ready = !!record && record.status.kind === "done" && !!record.note_path;
  const key = record
    ? `${record.id}|${record.title}|${record.speakers.join(",")}|${record.note_path}|${record.status.kind}`
    : "";
  useEffect(() => {
    setParts(null);
    setError(null);
    if (!ready || !record) return;
    let live = true;
    void commands.getMeetingNote(record.id).then((r) => {
      if (!live) return;
      if (r.status === "ok") setParts(r.data);
      else setError(r.error);
    });
    return () => {
      live = false;
    };
  }, [key, ready]);
  return { parts, error, ready };
}

/** Copies part of a meeting's note; shows "Copied" for a moment. */
export function useCopyNote(id: string | null) {
  const [copied, setCopied] = useState<"transcript" | "summary" | null>(null);
  const copy = async (what: "transcript" | "summary") => {
    if (!id) return;
    const r = await commands.getMeetingNote(id);
    if (r.status !== "ok") return;
    if (await copyToClipboard(r.data[what])) {
      setCopied(what);
      setTimeout(() => setCopied(null), 1600);
    }
  };
  return { copied, copy };
}

/** The screen video recorded with this meeting ("Meeting with screen"), once ready. */
const useScreenVideo = (meetingId: string): VideoMeta | null => {
  const [video, setVideo] = useState<VideoMeta | null>(null);
  useEffect(() => {
    let live = true;
    const load = () =>
      void commands.videoList().then((r) => {
        if (!live || r.status !== "ok") return;
        const v = (r.data ?? []).find((x) => x.meeting_id === meetingId);
        setVideo(v?.status === "ready" ? v : null);
      });
    load();
    const unlisten = events.videoChanged.listen(load);
    return () => {
      live = false;
      void unlisten.then((fn) => fn());
    };
  }, [meetingId]);
  return video;
};

/** What "Send to everyone" would do for this meeting; reloaded when it changes. */
const useMailPreview = (record: MeetingRecord, done: boolean) => {
  const [preview, setPreview] = useState<MailPreview | null>(null);
  const key = `${record.id}|${record.status.kind}|${record.note_path}|${record.mailed?.at ?? ""}`;
  useEffect(() => {
    if (!done) return setPreview(null);
    let live = true;
    void commands
      .meetingMailPreview(record.id)
      .then((r) => {
        if (live && r?.status === "ok") setPreview(r.data ?? null);
      })
      .catch(() => {
        if (live) setPreview(null);
      });
    return () => {
      live = false;
    };
  }, [key, done]);
  return preview;
};

/**
 * "Send to everyone": the note's summary, decisions and action items, from
 * the person's own Mail, to everyone in the meeting's calendar event. One
 * click sends; the tooltip names who gets it.
 */
const SendToAll: React.FC<{
  record: MeetingRecord;
  done: boolean;
  /** Icon only (narrow window); the label moves to the tooltip. */
  compact: boolean;
  onError: (e: string) => void;
}> = ({ record, done, compact, onError }) => {
  const { t } = useTranslation();
  const preview = useMailPreview(record, done);
  const [sending, setSending] = useState(false);
  if (!done || record.sample || !preview || preview.problem === "no_summary")
    return null;
  const mailed = record.mailed ?? preview.mailed;
  const busy = sending || preview.sending;
  const names = (preview.to ?? []).map((r) => r.name || r.email).join(", ");
  const blocked =
    preview.problem === "no_event" ||
    preview.problem === "no_recipients" ||
    preview.problem === "no_people" ||
    preview.problem === "calendar_denied";
  const hint = mailed
    ? t("meetings.mail.sentTo", { names: (mailed.to ?? []).join(", ") })
    : preview.problem
      ? t(`meetings.mail.problem.${preview.problem}`)
      : t("meetings.mail.sendHint", { names, count: preview.to.length });
  const send = async () => {
    setSending(true);
    try {
      const r = await commands.meetingMailSend(record.id);
      if (r?.status === "error") onError(r.error);
    } catch (e) {
      onError(String(e));
    }
    setSending(false);
  };
  const label = busy
    ? t("meetings.mail.sending")
    : mailed
      ? t("meetings.mail.sent", { count: (mailed.to ?? []).length })
      : t("meetings.mail.send");
  return (
    <button
      type="button"
      title={compact ? `${label}: ${hint}` : hint}
      aria-label={compact ? label : undefined}
      disabled={busy || blocked || !!mailed}
      onClick={() => void send()}
      className={`h-7 shrink-0 whitespace-nowrap inline-flex items-center justify-center gap-1.5 rounded-[10px] text-[13px] font-medium cursor-pointer focus:outline-none focus-visible:ring-2 focus-visible:ring-logo-primary ${mailed || blocked ? "bg-[color-mix(in_srgb,var(--color-text)_9%,transparent)] text-ink-2 cursor-default" : "bg-background-ui text-white hover:bg-background-ui/90 disabled:opacity-60 disabled:cursor-default"} ${compact ? "w-7" : "px-3"}`}
    >
      {busy ? (
        <Loader2 width={14} height={14} className="animate-spin" />
      ) : mailed ? (
        <Check width={16} height={16} className="text-moss" />
      ) : (
        <Send width={14} height={14} />
      )}
      {!compact && label}
    </button>
  );
};

/**
 * How much of the toolbar shows its labels, by the space it has: 0 all, 1
 * the secondary buttons as icons, 2 every button as an icon. `row` holds
 * the buttons at their natural width; `room` is the space for them.
 * Remembers the width each tier needed, to step back up when it fits.
 */
function useToolbarTier(deps: unknown[]) {
  const room = useRef<HTMLDivElement>(null);
  const row = useRef<HTMLDivElement>(null);
  const [tier, setTier] = useState(0);
  const need = useRef<number[]>([]);
  const fit = () => {
    const r = room.current;
    const w = row.current;
    if (!r || !w) return;
    const free = r.clientWidth;
    setTier((t) => {
      need.current[t] = w.offsetWidth;
      if (w.offsetWidth > free + 0.5 && t < 2) return t + 1;
      if (t > 0 && (need.current[t - 1] ?? Infinity) <= free) return t - 1;
      return t;
    });
  };
  useLayoutEffect(fit, [tier, ...deps]);
  useEffect(() => {
    const o = new ResizeObserver(fit);
    if (room.current) o.observe(room.current);
    if (row.current) o.observe(row.current);
    return () => o.disconnect();
  }, []);
  return { room, row, tier };
}

/** The reader's toolbar: copy transcript, copy summary (grey, filled),
 * send to everyone (blue, like "Meeting starten"), and "…" (design C,
 * Robin 2026-11-26). */
export const ReaderToolbar: React.FC<{
  record: MeetingRecord;
  title: string;
  date: string;
  onDeleted: () => void;
  onError: (e: string) => void;
}> = ({ record, title, date, onDeleted, onError }) => {
  const { t, i18n } = useTranslation();
  const { copied, copy } = useCopyNote(record.id);
  const screenVideo = useScreenVideo(record.id);
  const [menu, setMenu] = useState(false);
  const [saved, setSaved] = useState(false);
  const menuRef = useRef<HTMLDivElement>(null);
  usePressAway(menu, menuRef, () => setMenu(false));
  const done = record.status.kind === "done" && !!record.note_path;
  const canReprocess =
    !record.audio_deleted &&
    (record.status.kind === "done" || record.status.kind === "failed");

  /** One HTML file anyone can open in a browser: title, date, summary. */
  const savePage = async () => {
    const name = title.replace(/[/\\:*?"<>|]/g, " ").trim() || "Meeting";
    const path = await save({
      defaultPath: `${name}.html`,
      filters: [{ name: "Web page", extensions: ["html"] }],
    });
    if (!path) return;
    const r = await commands.saveMeetingPage(record.id, path, date);
    if (r.status === "ok") {
      setSaved(true);
      setTimeout(() => setSaved(false), 1600);
    } else onError(r.error);
  };

  const item =
    "flex w-full items-center gap-2 rounded-lg px-2 py-1.5 text-[13px] text-start hover:bg-wash cursor-pointer focus:outline-none focus-visible:bg-wash";
  const check = <Check width={16} height={16} className="text-moss" />;

  const transcriptLabel =
    copied === "transcript"
      ? t("meetings.details.copied")
      : t("meetings.details.copyTranscript");
  const summaryLabel =
    copied === "summary"
      ? t("meetings.details.copied")
      : t("meetings.details.copySummary");
  // Buttons never wrap: when the labels don't fit, the secondary buttons
  // become icons first, then the primary one too (Robin, 2026-11-25).
  const { room, row, tier } = useToolbarTier([done, copied, i18n.language]);
  const iconSecondary = tier >= 1;
  const iconPrimary = tier >= 2;

  return (
    <div
      data-reader-toolbar
      className="h-[52px] shrink-0 flex items-center justify-end gap-2 px-4"
    >
      <div ref={room} className="flex-1 min-w-0 flex justify-end">
        <div ref={row} className="w-max shrink-0 flex items-center gap-2">
          {done && (
            <>
              <button
                type="button"
                onClick={() => void copy("transcript")}
                title={iconPrimary ? transcriptLabel : undefined}
                aria-label={iconPrimary ? transcriptLabel : undefined}
                className={`h-7 shrink-0 whitespace-nowrap inline-flex items-center justify-center gap-1.5 rounded-[10px] text-[13px] font-medium text-text bg-[color-mix(in_srgb,var(--color-text)_9%,transparent)] hover:bg-[color-mix(in_srgb,var(--color-text)_14%,transparent)] cursor-pointer focus:outline-none focus-visible:ring-1 focus-visible:ring-logo-primary ${iconPrimary ? "w-7" : "px-3"}`}
              >
                {copied === "transcript" ? (
                  check
                ) : (
                  <Copy width={16} height={16} />
                )}
                {!iconPrimary && transcriptLabel}
              </button>
              <button
                type="button"
                onClick={() => void copy("summary")}
                title={iconSecondary ? summaryLabel : undefined}
                aria-label={iconSecondary ? summaryLabel : undefined}
                className={`h-7 shrink-0 whitespace-nowrap inline-flex items-center justify-center gap-1.5 rounded-[10px] text-[13px] font-medium text-text bg-[color-mix(in_srgb,var(--color-text)_9%,transparent)] hover:bg-[color-mix(in_srgb,var(--color-text)_14%,transparent)] cursor-pointer focus:outline-none focus-visible:ring-1 focus-visible:ring-logo-primary ${iconSecondary ? "w-7" : "px-3"}`}
              >
                {copied === "summary" ? (
                  check
                ) : (
                  <AlignLeft width={16} height={16} />
                )}
                {!iconSecondary && summaryLabel}
              </button>
              <ErrorBoundary context="Send to everyone">
                <SendToAll
                  record={record}
                  done={done}
                  compact={iconSecondary}
                  onError={onError}
                />
              </ErrorBoundary>
            </>
          )}
        </div>
      </div>
      <div ref={menuRef} className="relative shrink-0">
        <button
          type="button"
          aria-label={t("meetings.group.more")}
          aria-haspopup="menu"
          aria-expanded={menu}
          onClick={() => setMenu((m) => !m)}
          className="size-7 flex items-center justify-center rounded-lg text-ink-2 hover:bg-wash hover:text-text cursor-pointer focus:outline-none focus-visible:ring-1 focus-visible:ring-logo-primary"
        >
          {saved ? check : <MoreHorizontal width={16} height={16} />}
        </button>
        {menu && (
          <div
            role="menu"
            className="absolute end-0 top-full z-30 mt-1 w-60 rounded-xl border border-line bg-background p-1 shadow-lg"
          >
            {record.note_path && (
              <button
                type="button"
                role="menuitem"
                className={item}
                onClick={() => {
                  setMenu(false);
                  void commands.openMeetingNote(record.id);
                }}
              >
                <FileText width={16} height={16} className="text-ink-2" />
                {t("meetings.openNote")}
              </button>
            )}
            {record.note_path && (
              <button
                type="button"
                role="menuitem"
                className={item}
                onClick={() => {
                  setMenu(false);
                  void revealItemInDir(record.note_path!).catch((e) =>
                    onError(String(e)),
                  );
                }}
              >
                <FolderOpen width={16} height={16} className="text-ink-2" />
                {t("meetings.group.show")}
              </button>
            )}
            {screenVideo && (
              <button
                type="button"
                role="menuitem"
                className={item}
                onClick={() => {
                  setMenu(false);
                  const v = screenVideo;
                  void commands.videoReveal(
                    v.rendered ? `${v.folder}/${v.rendered}` : v.folder,
                  );
                }}
              >
                <Monitor width={16} height={16} className="text-ink-2" />
                {t("meetings.screenVideo")}
              </button>
            )}
            {done && (
              <button
                type="button"
                role="menuitem"
                className={item}
                title={t("meetings.details.savePageHint")}
                onClick={() => {
                  setMenu(false);
                  void savePage();
                }}
              >
                <Globe width={16} height={16} className="text-ink-2" />
                {t("meetings.details.savePage")}
              </button>
            )}
            {canReprocess && (
              <button
                type="button"
                role="menuitem"
                className={item}
                onClick={() => {
                  setMenu(false);
                  void commands.reprocessMeeting(record.id);
                }}
              >
                <RotateCcw width={16} height={16} className="text-ink-2" />
                {t("meetings.reprocess")}
              </button>
            )}
            {record.status.kind !== "recording" && (
              <button
                type="button"
                role="menuitem"
                className={`${item} text-error`}
                onClick={() => {
                  setMenu(false);
                  // Gone from the list at once; the files follow.
                  useMeetingStore.setState((s) => ({
                    meetings: s.meetings.filter((m) => m.id !== record.id),
                  }));
                  void commands.deleteMeeting(record.id).then(onDeleted);
                }}
              >
                <Trash2 width={16} height={16} />
                {t("meetings.delete")}
              </button>
            )}
          </div>
        )}
      </div>
    </div>
  );
};
