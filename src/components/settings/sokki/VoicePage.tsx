import React, { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { FolderOpen, Trash2, Users } from "lucide-react";
import {
  commands,
  events,
  type CommunityTotal,
  type VoiceDataStats,
} from "@/bindings";
import { Button } from "@/components/ui/Button";
import { Page, PageHeader } from "./parts";
import { DataChoices } from "./DataChoices";

/** "3 h 12 min", "12 min", "45 s". */
function duration(seconds: number): string {
  if (seconds < 60) return `${Math.round(seconds)} s`;
  const m = Math.round(seconds / 60);
  if (m < 60) return `${m} min`;
  const h = Math.floor(m / 60);
  return `${h} h ${String(m % 60).padStart(2, "0")} min`;
}

function size(bytes: number): string {
  if (bytes < 1e9) return `${Math.max(1, Math.round(bytes / 1e6))} MB`;
  return `${(bytes / 1e9).toFixed(1)} GB`;
}

function languageName(code: string, locale: string, other: string): string {
  if (code === "other") return other;
  try {
    return (
      new Intl.DisplayNames([locale], { type: "language" }).of(code) ?? code
    );
  } catch {
    return code;
  }
}

/** Bars per key, longest first. */
const Breakdown: React.FC<{
  values: Partial<Record<string, number>>;
  label: (key: string) => string;
}> = ({ values, label }) => {
  const rows = Object.entries(values)
    .map(([k, v]) => [k, v ?? 0] as const)
    .sort((a, b) => b[1] - a[1]);
  const max = Math.max(1, ...rows.map(([, v]) => v));
  return (
    <ul className="flex flex-col gap-2">
      {rows.map(([k, v]) => (
        <li
          key={k}
          className="grid grid-cols-[96px_1fr_88px] items-center gap-3 text-[13px]"
        >
          <span className="truncate text-ink-2">{label(k)}</span>
          <span className="h-2 rounded-full bg-wash overflow-hidden">
            <span
              className="block h-full rounded-full bg-logo-primary"
              style={{ width: `${Math.max(2, (100 * v) / max)}%` }}
            />
          </span>
          <span className="text-end tabular-nums text-ink-2">
            {duration(v)}
          </span>
        </li>
      ))}
    </ul>
  );
};

type Asking = "recordings" | "shared" | null;

/** "Your data": the user's own recordings on this Mac, the two choices,
 * buyer messages and the delete buttons. */
export const SokkiVoicePage: React.FC = () => {
  const { t, i18n } = useTranslation();
  const [stats, setStats] = useState<VoiceDataStats | null>(null);
  const [community, setCommunity] = useState<CommunityTotal | null>(null);
  const [asking, setAsking] = useState<Asking>(null);
  const [status, setStatus] = useState<string | null>(null);

  const load = useCallback(async () => {
    const r = await commands.voiceDataStats();
    if (r.status === "ok") setStats(r.data);
    const c = await commands.voiceDataCommunity();
    if (c.status === "ok") setCommunity(c.data);
  }, []);

  useEffect(() => {
    void load();
    // A processed meeting may have added recordings.
    const unlisten = events.meetingChanged.listen(() => void load());
    return () => {
      unlisten.then((fn) => fn());
    };
  }, [load]);

  if (!stats) return null;
  const lang = (k: string) =>
    languageName(k, i18n.language, t("sokki.voice.otherLanguage"));
  const source = (k: string) =>
    k === "meeting"
      ? t("sokki.voice.fromMeetings")
      : t("sokki.voice.fromDictation");
  const buyer = stats.buyer;
  // A plain-text message from Sokki (a buyer message is JSON and shows as
  // the buyer card instead).
  const plainNotice =
    community?.notice && !community.notice.trim().startsWith("{")
      ? community.notice
      : null;

  const answerBuyer = async (answer: "interested" | "no") => {
    const r = await commands.voiceDataAnswerBuyer(answer);
    setStatus(r.status === "ok" ? null : t("sokki.voice.failed"));
    void load();
  };

  const forget = async () => {
    const r = await commands.voiceDataForget();
    setAsking(null);
    setStatus(
      r.status === "ok"
        ? t("sokki.voice.deleteSharedDone")
        : t("sokki.voice.failed"),
    );
    void load();
  };

  return (
    <Page>
      <PageHeader
        title={t("sokki.voice.title")}
        description={t("sokki.voice.intro")}
      />

      {buyer && (
        <section className="vb-card p-4 flex flex-col gap-2 border-logo-primary">
          <h2 className="text-[14px] font-semibold">
            {t("sokki.voice.buyer.title", {
              language: languageName(buyer.language, i18n.language, ""),
            })}
          </h2>
          <p className="text-[13.5px]">
            {t("sokki.voice.buyer.text", { count: buyer.hours })}
          </p>
          {buyer.text && (
            <p className="text-[13px] text-ink-2 select-text whitespace-pre-line">
              {buyer.text}
            </p>
          )}
          {buyer.answer === "interested" ? (
            <p className="text-[13px] text-ink-2">
              {t("sokki.voice.buyer.thanks")}
            </p>
          ) : buyer.answer === "no" ? null : (
            <div className="flex gap-2 pt-1">
              <Button
                variant="primary"
                size="sm"
                onClick={() => void answerBuyer("interested")}
              >
                {t("sokki.voice.buyer.interested")}
              </Button>
              <Button
                variant="secondary"
                size="sm"
                onClick={() => void answerBuyer("no")}
              >
                {t("sokki.voice.buyer.no")}
              </Button>
            </div>
          )}
        </section>
      )}

      <section className="vb-card p-5 flex flex-col gap-5">
        <p className="text-[14px]">
          {stats.clips > 0
            ? t("sokki.voice.onThisMac", {
                duration: duration(stats.total_seconds),
                size: size(stats.bytes),
              })
            : t("sokki.voice.empty")}
        </p>
        {stats.total_seconds > 0 && (
          <div className="grid gap-6 sm:grid-cols-2">
            <div className="flex flex-col gap-2">
              <h2 className="text-[12px] font-semibold uppercase tracking-wide text-ink-3">
                {t("sokki.voice.byLanguage")}
              </h2>
              <Breakdown values={stats.seconds_by_language} label={lang} />
            </div>
            <div className="flex flex-col gap-2">
              <h2 className="text-[12px] font-semibold uppercase tracking-wide text-ink-3">
                {t("sokki.voice.bySource")}
              </h2>
              <Breakdown values={stats.seconds_by_source} label={source} />
            </div>
          </div>
        )}
      </section>

      <section className="vb-card p-4">
        <DataChoices
          value={{ improve: stats.improve, support: stats.support }}
          onChange={(v) =>
            void commands
              .voiceDataSetChoices(v.improve, v.support)
              .then(() => load())
          }
          variant="switch"
        />
      </section>

      {plainNotice && (
        <section className="vb-card p-4 flex flex-col gap-1 border-logo-primary">
          <h2 className="text-[12px] font-semibold uppercase tracking-wide text-logo-primary">
            {t("sokki.voice.notice")}
          </h2>
          <p className="text-[13.5px] select-text whitespace-pre-line">
            {plainNotice}
          </p>
        </section>
      )}

      {community && community.installs > 0 && (
        <section className="vb-card p-4 flex items-center gap-3">
          <Users
            width={18}
            height={18}
            className="text-logo-primary shrink-0"
          />
          <p className="text-[13.5px]">
            {t(
              community.donors > 0
                ? "sokki.voice.communityDonors"
                : "sokki.voice.community",
              { count: community.installs, donors: community.donors },
            )}
          </p>
        </section>
      )}

      <div className="flex flex-wrap items-center gap-2 px-1">
        {asking ? (
          <>
            <span className="text-[12.5px] text-ink-2">
              {asking === "recordings"
                ? t("sokki.voice.deleteRecordingsAsk", {
                    duration: duration(stats.total_seconds),
                  })
                : t("sokki.voice.deleteSharedAsk")}
            </span>
            <Button
              size="sm"
              variant="secondary"
              onClick={() => setAsking(null)}
            >
              {t("sokki.voice.cancel")}
            </Button>
            <Button
              size="sm"
              variant="danger"
              onClick={() =>
                asking === "recordings"
                  ? void commands.voiceDataDelete().then(() => {
                      setAsking(null);
                      void load();
                    })
                  : void forget()
              }
            >
              {t("sokki.voice.delete")}
            </Button>
          </>
        ) : (
          <>
            <Button
              variant="secondary"
              size="sm"
              className="inline-flex items-center gap-1.5"
              onClick={() => void commands.voiceDataOpenFolder()}
            >
              <FolderOpen width={14} height={14} />
              {t("sokki.voice.showFolder")}
            </Button>
            {stats.clips > 0 && (
              <Button
                variant="danger-ghost"
                size="sm"
                className="inline-flex items-center gap-1.5"
                onClick={() => {
                  setStatus(null);
                  setAsking("recordings");
                }}
              >
                <Trash2 width={14} height={14} />
                {t("sokki.voice.deleteRecordings")}
              </Button>
            )}
            {stats.shared && (
              <Button
                variant="danger-ghost"
                size="sm"
                className="inline-flex items-center gap-1.5"
                onClick={() => {
                  setStatus(null);
                  setAsking("shared");
                }}
              >
                <Trash2 width={14} height={14} />
                {t("sokki.voice.deleteShared")}
              </Button>
            )}
          </>
        )}
        {status && (
          <span className="text-[12.5px] text-ink-2" role="status">
            {status}
          </span>
        )}
      </div>
    </Page>
  );
};
