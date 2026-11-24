import React, { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  Check,
  Copy,
  FileText,
  FolderOpen,
  Globe,
  Monitor,
  MoreHorizontal,
  RotateCcw,
  Trash2,
} from "lucide-react";
import { save } from "@tauri-apps/plugin-dialog";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import {
  commands,
  events,
  type MeetingNoteParts,
  type MeetingRecord,
  type VideoMeta,
} from "@/bindings";
import { usePressAway } from "@/hooks/usePressAway";
import { useMeetingStore } from "@/stores/meetingStore";
import { copyToClipboard } from "../history/clipboard";

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
        const v = r.data.find((x) => x.meeting_id === meetingId);
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

/** The reader's toolbar: copy transcript (accent), copy summary, and "…". */
export const ReaderToolbar: React.FC<{
  record: MeetingRecord;
  title: string;
  date: string;
  onDeleted: () => void;
  onError: (e: string) => void;
}> = ({ record, title, date, onDeleted, onError }) => {
  const { t } = useTranslation();
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

  return (
    <div className="h-[52px] shrink-0 flex items-center justify-end gap-2 px-4">
      {done && (
        <>
          <button
            type="button"
            onClick={() => void copy("transcript")}
            className="h-7 px-3 inline-flex items-center gap-1.5 rounded-[10px] text-[13px] font-medium text-logo-primary cursor-pointer focus:outline-none focus-visible:ring-1 focus-visible:ring-logo-primary"
            style={{ background: "var(--vb-accent-wash)" }}
          >
            {copied === "transcript" ? check : <Copy width={16} height={16} />}
            {copied === "transcript"
              ? t("meetings.details.copied")
              : t("meetings.details.copyTranscript")}
          </button>
          <button
            type="button"
            onClick={() => void copy("summary")}
            className="h-7 px-2.5 inline-flex items-center gap-1.5 rounded-[10px] border border-line-strong text-[13px] text-text cursor-pointer hover:bg-wash focus:outline-none focus-visible:ring-1 focus-visible:ring-logo-primary"
          >
            {copied === "summary" && check}
            {copied === "summary"
              ? t("meetings.details.copied")
              : t("meetings.details.copySummary")}
          </button>
        </>
      )}
      <div ref={menuRef} className="relative">
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
