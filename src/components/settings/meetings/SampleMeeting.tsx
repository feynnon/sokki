import React, { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Play, Square } from "lucide-react";
import { commands, type LiveLine, type MeetingRecord } from "@/bindings";
import { Button } from "../../ui/Button";
import { ProgressLine } from "../../shared/ProgressLine";
import {
  SUMMARY_DOWNLOAD_BYTES,
  gigabytes,
  useSummaryModel,
} from "@/hooks/useSummaryModel";
import { useMeetingStore } from "@/stores/meetingStore";

/** The sample call's length (resources/sample-meeting/call.flac). */
export const SAMPLE_SECONDS = 120;

/** "1:05" */
const clock = (s: number) =>
  `${Math.floor(s / 60)}:${String(Math.floor(s % 60)).padStart(2, "0")}`;

/** The small "Sample" mark beside a sample meeting's title. */
export const SampleTag: React.FC = () => {
  const { t } = useTranslation();
  return (
    <span className="shrink-0 self-start rounded-md bg-logo-primary/20 px-[7px] py-[3px] font-sans text-[11px] font-semibold uppercase tracking-[0.04em] text-logo-primary">
      {t("meetings.sample.tag")}
    </span>
  );
};

/**
 * For someone with no meetings yet: a two-minute call of invented people
 * that Sokki plays out loud and takes notes of, as it would in a real
 * call. It waits for a summary model: the built-in one's download shows
 * here, else the way to set one up.
 */
export const SampleCard: React.FC<{
  /** The selected language model is ready to summarise. */
  llmReady: boolean;
  /** Opens Models → Language model. */
  onSetUp: () => void;
}> = ({ llmReady, onSetUp }) => {
  const { t, i18n } = useTranslation();
  const summary = useSummaryModel();
  const reload = useMeetingStore((s) => s.reload);
  const select = useMeetingStore((s) => s.select);
  const [error, setError] = useState<string | null>(null);
  const [starting, setStarting] = useState(false);
  // Ready once the model is in; the status above is re-read on settings,
  // so the built-in download's end counts as ready too.
  const ready = llmReady || summary.state === "ready";

  const play = async () => {
    setError(null);
    setStarting(true);
    const r = await commands.startSampleMeeting();
    setStarting(false);
    if (r.status === "error") return setError(r.error);
    select(r.data);
    await reload();
  };

  let wait: React.ReactNode = null;
  if (!ready && summary.state === "downloading" && summary.download) {
    const d = summary.download;
    const value = d.total > 0 ? d.completed / d.total : 0;
    wait = (
      <div className="flex flex-col gap-[6px]">
        <ProgressLine value={value} busy={!value} />
        <span role="status" className="text-[12px] tabular-nums text-ink-3">
          {t("meetings.sample.waiting", {
            percent: Math.min(99, Math.floor(value * 100)),
            total: gigabytes(SUMMARY_DOWNLOAD_BYTES, i18n.language),
          })}
        </span>
      </div>
    );
  } else if (!ready) {
    const builtin = summary.fits && summary.state === "missing";
    wait = (
      <div className="flex flex-wrap items-center gap-x-3 gap-y-2">
        <span className="text-[13px] text-ink-2">
          {t("meetings.sample.needsModel")}
        </span>
        {builtin ? (
          <Button
            size="sm"
            variant="secondary"
            onClick={() => void summary.start()}
          >
            {t("onboarding.model.download", {
              size: gigabytes(SUMMARY_DOWNLOAD_BYTES, i18n.language),
            })}
          </Button>
        ) : (
          <Button size="sm" variant="secondary" onClick={onSetUp}>
            {t("meetings.sample.setUp")}
          </Button>
        )}
      </div>
    );
  }

  return (
    <section
      aria-labelledby="sample-title"
      className="max-w-[560px] flex flex-col gap-4 rounded-panel border border-line bg-background px-6 py-5"
    >
      <div className="flex flex-col gap-1.5">
        <SampleTag />
        <h2
          id="sample-title"
          className="m-0 mt-2 font-display text-[22px] font-[650] leading-[1.2] tracking-[-0.01em]"
        >
          {t("meetings.sample.cardTitle")}
        </h2>
        <p className="m-0 text-[13px] leading-[1.55] text-ink-2">
          {t("meetings.sample.body")}
        </p>
      </div>
      {wait}
      <div className="flex flex-wrap items-center gap-3">
        <Button
          variant="primary"
          disabled={!ready || starting}
          onClick={() => void play()}
          className="inline-flex items-center gap-2"
        >
          <Play size={14} aria-hidden="true" />
          {t("meetings.sample.play")}
        </Button>
        <span className="text-[12px] text-ink-3">
          {t("meetings.sample.length")}
        </span>
      </div>
      {error && <p className="m-0 text-xs text-red-500 select-text">{error}</p>}
      <p className="m-0 border-t border-line pt-3 text-[12px] text-ink-3">
        {t("meetings.notesWhen")}
      </p>
    </section>
  );
};

/** While the sample plays: how far it is, what Sokki has heard so far,
 * and a way to stop it (which deletes it). */
export const LiveTranscript: React.FC<{ record: MeetingRecord }> = ({
  record,
}) => {
  const { t } = useTranslation();
  const [lines, setLines] = useState<LiveLine[]>([]);
  const [now, setNow] = useState(Date.now());
  const reload = useMeetingStore((s) => s.reload);

  useEffect(() => {
    let live = true;
    const read = () =>
      void commands.meetingLiveTranscript().then((l) => {
        if (live) setLines(l);
      });
    read();
    const timer = setInterval(() => {
      read();
      setNow(Date.now());
    }, 1000);
    return () => {
      live = false;
      clearInterval(timer);
    };
  }, [record.id]);

  const elapsed = Math.min(
    SAMPLE_SECONDS,
    Math.max(0, (now - Date.parse(record.started_at)) / 1000),
  );
  const stop = async () => {
    await commands.stopSampleMeeting();
    await reload();
  };

  return (
    <div className="flex flex-col gap-3">
      <div className="flex items-center gap-3">
        <span
          className="inline-block size-2 shrink-0 rounded-full animate-pulse"
          style={{ background: "var(--vb-rec)" }}
          aria-hidden="true"
        />
        <span
          role="status"
          className="text-[13px] font-medium tabular-nums"
          style={{ color: "var(--vb-rec)" }}
        >
          {t("meetings.sample.playing", {
            at: clock(elapsed),
            length: clock(SAMPLE_SECONDS),
          })}
        </span>
        <Button
          size="sm"
          variant="ghost"
          onClick={() => void stop()}
          className="ms-auto inline-flex items-center gap-1.5"
        >
          <Square size={11} aria-hidden="true" />
          {t("meetings.sample.stop")}
        </Button>
      </div>
      <ProgressLine value={elapsed / SAMPLE_SECONDS} className="h-[3px]" />
      <p className="m-0 text-[13px] text-ink-3">{t("meetings.sample.live")}</p>
      <div className="flex flex-col gap-1.5 font-mono text-[13px] leading-[1.7] text-ink-2 select-text">
        {lines.length === 0 ? (
          <span className="animate-pulse text-ink-3">
            {t("meetings.sample.listening")}
          </span>
        ) : (
          lines.map((l, i) => (
            <div key={i} className="sokki-live-line">
              <span className="text-ink-3">[{clock(l.start_s)}]</span> {l.text}
            </div>
          ))
        )}
      </div>
    </div>
  );
};
