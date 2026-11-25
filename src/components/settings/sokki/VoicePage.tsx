import React, { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { FolderOpen, Trash2 } from "lucide-react";
import { commands, events, type VoiceDataStats } from "@/bindings";
import { Button } from "@/components/ui/Button";
import { Overview } from "./Overview";
import { SHARING_ANCHOR } from "./SharingGroup";
import { MilestoneCard, SPONSORS_LIVE } from "./MilestoneCard";

/** "3 h 12 min", "12 min", "45 s". */
export function duration(seconds: number): string {
  if (seconds < 60) return `${Math.round(seconds)} s`;
  const m = Math.round(seconds / 60);
  if (m < 60) return `${m} min`;
  const h = Math.floor(m / 60);
  return `${h} h ${String(m % 60).padStart(2, "0")} min`;
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

type Asking = "recordings" | "shared" | null;

/** "Your data", the window's first view: hours on this Mac, this week and
 * the latest meeting on top (design C4, E1 while empty); below, a matching
 * buyer message, the user's own recordings, the delete buttons and the
 * sharing choices. Paid tasks are paused. */
export const SokkiVoicePage: React.FC = () => {
  const { t, i18n } = useTranslation();
  const [stats, setStats] = useState<VoiceDataStats | null>(null);
  const [asking, setAsking] = useState<Asking>(null);
  const [status, setStatus] = useState<string | null>(null);

  const load = useCallback(async () => {
    const r = await commands.voiceDataStats();
    if (r.status === "ok") setStats(r.data);
  }, []);

  useEffect(() => {
    void load();
    // A processed meeting may have added recordings.
    const unlisten = events.meetingChanged.listen(() => void load());
    return () => {
      unlisten.then((fn) => fn());
    };
  }, [load]);

  // Uploads run in the background; while sharing is on, the delete button
  // for shared data appears once something was shared.
  const sharing = stats?.support ?? false;
  useEffect(() => {
    if (!sharing) return;
    const timer = window.setInterval(() => void load(), 5000);
    return () => window.clearInterval(timer);
  }, [sharing, load]);

  if (!stats) return null;
  const buyer = stats.buyer;

  const answerBuyer = async (answer: "interested" | "no") => {
    const r = await commands.voiceDataAnswerBuyer(answer);
    setStatus(r.status === "ok" ? null : t("sokki.voice.failed"));
    void load();
  };

  const forget = async () => {
    const r = await commands.voiceDataForget();
    setAsking(null);
    // The counter deletes within its daily limit; a large share finishes
    // over the next days on its own.
    setStatus(
      r.status !== "ok"
        ? t("sokki.voice.failed")
        : r.data
          ? t("sokki.voice.deleteSharedPending")
          : t("sokki.voice.deleteSharedDone"),
    );
    void load();
  };

  return (
    <div className="w-full max-w-[760px] mx-auto flex flex-col gap-7 pt-10 pb-10">
      {/* The design's glow, once, top right behind the title. */}
      <div
        aria-hidden="true"
        className="pointer-events-none absolute -top-[220px] -end-[200px] w-[640px] h-[520px] rounded-full"
        style={{
          background:
            "radial-gradient(closest-side, var(--vb-glow), transparent)",
        }}
      />
      <Overview
        stats={stats}
        onChangeSharing={() => {
          // Settings › Sharing: open the page, then bring the group up.
          window.dispatchEvent(
            new CustomEvent("sokki-open-section", { detail: "settings" }),
          );
          window.setTimeout(
            () =>
              document
                .getElementById(SHARING_ANCHOR)
                ?.scrollIntoView({ block: "start" }),
            60,
          );
        }}
      />
      {SPONSORS_LIVE && <MilestoneCard />}

      {buyer && (
        <section className="relative vb-card p-4 flex flex-col gap-2 border-logo-primary">
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

      <div className="flex flex-wrap items-center gap-2 px-1">
        {asking ? (
          <>
            <span className="text-small text-ink-2">
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
          <span className="text-small text-ink-2" role="status">
            {status}
          </span>
        )}
      </div>
    </div>
  );
};
