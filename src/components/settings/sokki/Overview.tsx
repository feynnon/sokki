import React, { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  Check,
  ChevronRight,
  Copy,
  ListChecks,
  MessageSquareText,
  Mic,
  ShieldCheck,
} from "lucide-react";
import { commands, events, type Totals, type VoiceDataStats } from "@/bindings";
import { useSettings } from "@/hooks/useSettings";
import { useOsType } from "@/hooks/useOsType";
import { useShortcutCapture } from "@/hooks/useShortcutCapture";
import { keycapLabels } from "@/lib/utils/keyboard";
import { openMeeting, useMeetingStore } from "@/stores/meetingStore";
import { minutes, readerWhen } from "../meetings/meetingDates";
import { useCopyNote } from "../meetings/MeetingDetails";

/** Set once the "Your dictations are now under Dictations" tip is done. */
const TIP_KEY = "sokki.tip.dictations";

function readFlag(key: string): boolean {
  try {
    return localStorage.getItem(key) !== null;
  } catch {
    return true;
  }
}
function setFlag(key: string) {
  try {
    localStorage.setItem(key, "1");
  } catch {
    // Not remembered: the tip may show once more.
  }
}

/** "2,4 GB", "380 MB" in the page's language. */
export function sizeText(bytes: number, locale: string): string {
  if (bytes < 1e9)
    return `${Math.max(1, Math.round(bytes / 1e6)).toLocaleString(locale)} MB`;
  return `${(bytes / 1e9).toLocaleString(locale, { maximumFractionDigits: 1 })} GB`;
}

/** "6,3" below ten hours, "31" above. */
export function hoursText(seconds: number, locale: string): string {
  const h = seconds / 3600;
  return h < 10
    ? (Math.round(h * 10) / 10).toLocaleString(locale)
    : Math.round(h).toLocaleString(locale);
}

const Label: React.FC<{ children: React.ReactNode; id?: string }> = ({
  children,
  id,
}) => (
  <h2 id={id} className="mt-7 mb-2 text-[13px] font-medium text-ink-3">
    {children}
  </h2>
);

const LinkButton: React.FC<{
  onClick: () => void;
  children: React.ReactNode;
}> = ({ onClick, children }) => (
  <button
    type="button"
    onClick={onClick}
    className="h-7 px-2.5 shrink-0 inline-flex items-center rounded-[10px] text-[13px] font-medium text-logo-primary hover:bg-wash cursor-pointer focus:outline-none focus-visible:ring-1 focus-visible:ring-logo-primary"
  >
    {children}
  </button>
);

/** One line from the three sharing choices, and "Change". */
const SharingLine: React.FC<{
  stats: VoiceDataStats;
  onChange: () => void;
}> = ({ stats, onChange }) => {
  const { t } = useTranslation();
  const text = stats.support
    ? t("sokki.home.shareAll")
    : stats.improve
      ? t("sokki.home.shareImprove")
      : t("sokki.home.shareNothing");
  return (
    <>
      <Label>{t("sokki.home.sharing")}</Label>
      <div className="vb-card px-4 py-3.5 flex items-center gap-3">
        <span
          className="size-8 shrink-0 rounded-full flex items-center justify-center text-logo-primary"
          style={{ background: "var(--vb-accent-wash)" }}
        >
          <ShieldCheck width={16} height={16} />
        </span>
        <p className="flex-1 min-w-0 text-[13px] text-ink-2">{text}</p>
        <LinkButton onClick={onChange}>{t("sokki.home.change")}</LinkButton>
      </div>
    </>
  );
};

/** A shortcut's keycaps and "Change", which records new keys in place. */
const ShortcutKeys: React.FC<{ bindingId: string }> = ({ bindingId }) => {
  const { t } = useTranslation();
  const { getSetting } = useSettings();
  const osType = useOsType();
  const { capturing, partial, start, cancel } = useShortcutCapture(
    bindingId,
    osType,
    { escapeCancels: true },
  );
  const binding = getSetting("bindings")?.[bindingId]?.current_binding ?? "";
  const caps = (combo: string) =>
    keycapLabels(combo, osType).map((k) =>
      k.toLowerCase() === "space" ? t("sokki.home.space") : k,
    );
  const key =
    "min-w-[30px] h-[30px] px-2 inline-flex items-center justify-center rounded-md bg-wash border border-line-strong text-[13px] font-medium text-text";

  if (capturing) {
    return (
      <div className="flex items-center gap-2 shrink-0">
        <span className="h-[30px] px-3 inline-flex items-center gap-1.5 rounded-md border border-dashed border-logo-primary text-[13px] text-ink-2">
          {partial
            ? caps(partial).join(" ")
            : t("settings.general.shortcut.pressKeys")}
        </span>
        <LinkButton onClick={cancel}>{t("sokki.home.cancel")}</LinkButton>
      </div>
    );
  }
  return (
    <div className="flex items-center gap-1.5 shrink-0">
      {caps(binding).map((k, i) => (
        <span key={i} className={key}>
          {k}
        </span>
      ))}
      <LinkButton onClick={() => void start()}>
        {t("sokki.home.change")}
      </LinkButton>
    </div>
  );
};

/** E1: nothing recorded yet. The two ways in, with their real shortcuts. */
const Empty: React.FC = () => {
  const { t } = useTranslation();
  const rows = [
    {
      icon: MessageSquareText,
      title: t("sokki.home.dictateTitle"),
      line: t("sokki.home.dictateLine"),
      binding: "transcribe",
    },
    {
      icon: Mic,
      title: t("sokki.home.meetingTitle"),
      line: t("sokki.home.meetingLine"),
      binding: "meeting",
    },
  ];
  return (
    <>
      <h1 className="font-display text-title font-[650] leading-[1.15] tracking-[-0.015em]">
        {t("sokki.home.emptyTitle")}
      </h1>
      <p className="mt-1 text-body text-ink-2">{t("sokki.home.emptyLine")}</p>
      <Label>{t("sokki.home.twoWays")}</Label>
      <div className="vb-card">
        {rows.map((r, i) => {
          const Icon = r.icon;
          return (
            <div
              key={r.binding}
              className={`px-5 py-[18px] flex items-center gap-4 ${i > 0 ? "border-t border-line" : ""}`}
            >
              <span
                className="size-9 shrink-0 rounded-[10px] flex items-center justify-center text-logo-primary"
                style={{ background: "var(--vb-accent-wash)" }}
              >
                <Icon width={18} height={18} />
              </span>
              <div className="flex-1 min-w-0">
                <div className="font-display text-[17px] leading-[1.3] font-[650]">
                  {r.title}
                </div>
                <div className="mt-0.5 text-body text-ink-2">{r.line}</div>
              </div>
              <ShortcutKeys bindingId={r.binding} />
            </div>
          );
        })}
      </div>
    </>
  );
};

/** An icon button that copies, with its name as tooltip; a check once done. */
const CopyIcon: React.FC<{
  label: string;
  done: boolean;
  onClick: () => void;
  children: React.ReactNode;
}> = ({ label, done, onClick, children }) => (
  <button
    type="button"
    aria-label={label}
    title={label}
    onClick={(e) => {
      e.stopPropagation();
      onClick();
    }}
    className="size-8 shrink-0 flex items-center justify-center rounded-lg text-ink-2 hover:bg-wash hover:text-text cursor-pointer focus:outline-none focus-visible:ring-1 focus-visible:ring-logo-primary"
  >
    {done ? <Check width={16} height={16} className="text-moss" /> : children}
  </button>
);

/** F6: hours, words and meetings all time; the latest meeting, opened by
 * a click anywhere on its row. */
const Filled: React.FC<{ stats: VoiceDataStats; totals: Totals }> = ({
  stats,
  totals,
}) => {
  const { t, i18n } = useTranslation();
  const lang = i18n.language;
  const meetings = useMeetingStore((s) => s.meetings);
  const latest = meetings.find((m) => m.status.kind !== "recording") ?? null;
  const { copied, copy } = useCopyNote(latest?.id ?? null);
  const tile = "vb-card px-4 py-3.5 flex flex-col gap-0.5";
  const num = "font-display text-title leading-[1.15] font-[650] tabular-nums";
  const tiles = [
    [hoursText(totals.seconds, lang), t("sokki.home.hoursTile")],
    [totals.words.toLocaleString(lang), t("sokki.home.words")],
    [totals.meetings.toLocaleString(lang), t("sokki.home.meetings")],
  ];
  const ready = latest?.status.kind === "done" && !!latest.note_path;

  return (
    <>
      <h1 className="font-display text-title font-[650] leading-[1.15] tracking-[-0.015em]">
        {t("sokki.voice.title")}
      </h1>
      <p className="mt-1 text-body text-ink-2">
        {t("sokki.home.onThisMac", { size: sizeText(stats.bytes, lang) })}
      </p>

      <div className="mt-7 grid grid-cols-3 gap-3">
        {tiles.map(([value, label]) => (
          <div key={label} className={tile}>
            <span className={num}>{value}</span>
            <span className="text-[13px] text-ink-2">{label}</span>
          </div>
        ))}
      </div>

      {latest && (
        <>
          <Label>{t("sokki.home.latest")}</Label>
          {/* The row opens the meeting; the icons copy without opening. */}
          <div
            role="link"
            tabIndex={0}
            onClick={() => openMeeting(latest.id)}
            onKeyDown={(e) => {
              if (e.key === "Enter") openMeeting(latest.id);
            }}
            className="vb-card ps-4 pe-3 py-3.5 flex items-center gap-1 cursor-pointer hover:bg-wash focus:outline-none focus-visible:ring-1 focus-visible:ring-logo-primary"
          >
            <div className="flex-1 min-w-0 me-2">
              <div className="font-display text-[17px] leading-[1.3] font-[650] truncate">
                {latest.title ?? t("meetings.untitled")}
              </div>
              <div className="text-[13px] text-ink-3 truncate">
                {[
                  readerWhen(
                    new Date(Date.parse(latest.started_at)),
                    new Date(),
                    lang,
                    (g) => t(`meetings.group.${g}`),
                  ),
                  minutes(latest.duration_s, lang),
                  latest.source_app,
                ]
                  .filter(Boolean)
                  .join(" · ")}
              </div>
            </div>
            {ready && (
              <>
                <CopyIcon
                  label={t("meetings.details.copyTranscript")}
                  done={copied === "transcript"}
                  onClick={() => void copy("transcript")}
                >
                  <Copy width={16} height={16} />
                </CopyIcon>
                <CopyIcon
                  label={t("meetings.details.copySummary")}
                  done={copied === "summary"}
                  onClick={() => void copy("summary")}
                >
                  <ListChecks width={16} height={16} />
                </CopyIcon>
              </>
            )}
            <ChevronRight
              width={16}
              height={16}
              className="shrink-0 ms-1 text-ink-3"
              aria-hidden="true"
            />
          </div>
        </>
      )}
    </>
  );
};

/** "Your data", the window's first view (design F6; E1 while nothing is
 * on this Mac). `onChangeSharing` opens the choices in Settings. */
export const Overview: React.FC<{
  stats: VoiceDataStats;
  onChangeSharing: () => void;
}> = ({ stats, onChangeSharing }) => {
  const { t } = useTranslation();
  const [totals, setTotals] = useState<Totals | null>(null);
  const [tip, setTip] = useState(false);
  const meetingCount = useMeetingStore((s) => s.meetings.length);

  useEffect(() => {
    const load = () =>
      void commands.usageTotals().then((r) => {
        if (r.status !== "ok") return;
        setTotals(r.data);
        // Once, after the update that moved dictations off the start:
        // only for someone who has dictations to look for.
        if (!readFlag(TIP_KEY)) {
          if (r.data.words > 0) setTip(true);
          else setFlag(TIP_KEY);
        }
      });
    load();
    const offMeeting = events.meetingChanged.listen(load);
    const offHistory = events.historyUpdatePayload.listen(load);
    return () => {
      offMeeting.then((fn) => fn());
      offHistory.then((fn) => fn());
    };
  }, [meetingCount]);

  if (!totals) return null;
  const empty =
    totals.seconds === 0 &&
    totals.words === 0 &&
    totals.meetings === 0 &&
    meetingCount === 0;

  return (
    <div className="relative">
      {tip && (
        <div
          role="status"
          className="mb-7 flex items-center gap-3 rounded-xl ps-4 pe-3 py-2.5"
          style={{ background: "var(--vb-accent-wash)" }}
        >
          <MessageSquareText
            width={16}
            height={16}
            className="shrink-0 text-logo-primary"
          />
          <span className="flex-1 text-[13px]">
            {t("sokki.dictations.tip")}
          </span>
          <LinkButton
            onClick={() => {
              setFlag(TIP_KEY);
              setTip(false);
            }}
          >
            {t("sokki.dictations.ok")}
          </LinkButton>
        </div>
      )}
      {empty ? <Empty /> : <Filled stats={stats} totals={totals} />}
      <SharingLine stats={stats} onChange={onChangeSharing} />
    </div>
  );
};
