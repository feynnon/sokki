import React, { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Check, ChevronDown, Copy, Globe } from "lucide-react";
import { save } from "@tauri-apps/plugin-dialog";
import { commands, type MeetingNoteParts } from "@/bindings";
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
const SummaryView: React.FC<{ markdown: string }> = ({ markdown }) => {
  const blocks: React.ReactNode[] = [];
  let list: React.ReactNode[] = [];
  const flush = () => {
    if (list.length) {
      blocks.push(
        <ul key={`l${blocks.length}`} className="flex flex-col gap-1.5">
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
            className={`mt-[3px] size-3.5 shrink-0 rounded-[4px] border ${task[1] === "x" ? "bg-logo-primary border-logo-primary" : "border-line-strong"}`}
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
    <div className="flex flex-col gap-2 text-[13.5px] leading-[1.55] text-ink-2 select-text">
      {blocks}
    </div>
  );
};

/** Summary of a finished meeting, unfolded in the list, plus its transcript to copy or read. */
export const MeetingDetails: React.FC<{
  id: string;
  noteKey: string;
  /** Title and date as shown in the list; the web page uses them too. */
  title: string;
  date: string;
}> = ({ id, noteKey, title, date }) => {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const [parts, setParts] = useState<MeetingNoteParts | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [showTranscript, setShowTranscript] = useState(false);
  const [copied, setCopied] = useState<"transcript" | "summary" | null>(null);
  const [saved, setSaved] = useState(false);

  const load = async () => {
    const r = await commands.getMeetingNote(id);
    if (r.status === "ok") {
      setParts(r.data);
      setError(null);
    } else {
      setError(r.error);
    }
    return r.status === "ok" ? r.data : null;
  };

  // Reload when the note changes (renamed speaker, processed again).
  useEffect(() => {
    if (open) void load();
  }, [open, noteKey]);

  const copy = async (what: "transcript" | "summary") => {
    const data = parts ?? (await load());
    if (!data) return;
    if (await copyToClipboard(data[what])) {
      setCopied(what);
      setTimeout(() => setCopied(null), 1600);
    }
  };

  /** One HTML file anyone can open in a browser: title, date, summary. */
  const savePage = async () => {
    const name = title.replace(/[/\\:*?"<>|]/g, " ").trim() || "Meeting";
    const path = await save({
      defaultPath: `${name}.html`,
      filters: [{ name: "Web page", extensions: ["html"] }],
    });
    if (!path) return;
    const r = await commands.saveMeetingPage(id, path, date);
    if (r.status === "ok") {
      setSaved(true);
      setTimeout(() => setSaved(false), 1600);
    } else {
      setError(r.error);
    }
  };

  const chip =
    "inline-flex items-center gap-1.5 h-6 px-2 rounded-md text-[12.5px] text-ink-2 bg-wash border border-line hover:text-text cursor-pointer focus:outline-none focus-visible:ring-1 focus-visible:ring-logo-primary";

  return (
    <div className="flex flex-col gap-2">
      <div className="flex flex-wrap items-center gap-2">
        <button
          type="button"
          aria-expanded={open}
          onClick={() => setOpen((o) => !o)}
          className="inline-flex items-center gap-1 text-[13px] font-medium text-text hover:text-logo-primary cursor-pointer focus:outline-none focus-visible:ring-1 focus-visible:ring-logo-primary rounded"
        >
          <ChevronDown
            width={14}
            height={14}
            className={`transition-transform ${open ? "" : "-rotate-90"}`}
          />
          {t("meetings.details.summary")}
        </button>
        <button
          type="button"
          onClick={() => void copy("summary")}
          className={chip}
        >
          {copied === "summary" ? (
            <Check width={13} height={13} className="text-moss" />
          ) : (
            <Copy width={13} height={13} />
          )}
          {copied === "summary"
            ? t("meetings.details.copied")
            : t("meetings.details.copySummary")}
        </button>
        <button
          type="button"
          onClick={() => void copy("transcript")}
          className={chip}
        >
          {copied === "transcript" ? (
            <Check width={13} height={13} className="text-moss" />
          ) : (
            <Copy width={13} height={13} />
          )}
          {copied === "transcript"
            ? t("meetings.details.copied")
            : t("meetings.details.copyTranscript")}
        </button>
        <button
          type="button"
          onClick={() => void savePage()}
          className={chip}
          title={t("meetings.details.savePageHint")}
        >
          {saved ? (
            <Check width={13} height={13} className="text-moss" />
          ) : (
            <Globe width={13} height={13} />
          )}
          {saved ? t("meetings.details.saved") : t("meetings.details.savePage")}
        </button>
      </div>
      {error && <p className="text-xs text-red-500 select-text">{error}</p>}
      {open && parts && (
        <div className="flex flex-col gap-3 rounded-lg bg-wash border border-line p-3">
          {parts.summary ? (
            <SummaryView markdown={parts.summary} />
          ) : (
            <p className="text-[13px] text-ink-3">
              {t("meetings.details.noSummary")}
            </p>
          )}
          <button
            type="button"
            aria-expanded={showTranscript}
            onClick={() => setShowTranscript((s) => !s)}
            className="self-start inline-flex items-center gap-1 text-[12.5px] text-ink-2 hover:text-text cursor-pointer"
          >
            <ChevronDown
              width={13}
              height={13}
              className={`transition-transform ${showTranscript ? "" : "-rotate-90"}`}
            />
            {t("meetings.details.transcript")}
          </button>
          {showTranscript && (
            <pre className="max-h-80 overflow-auto whitespace-pre-wrap rounded-md bg-background border border-line p-3 text-[12.5px] leading-[1.6] text-ink-2 select-text font-sans">
              {parts.transcript}
            </pre>
          )}
        </div>
      )}
    </div>
  );
};
