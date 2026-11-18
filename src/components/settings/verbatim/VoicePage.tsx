import React, { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { FolderOpen, Trash2, Users } from "lucide-react";
import {
  commands,
  events,
  type CommunityTotal,
  type VoiceDataStats,
} from "@/bindings";
import { ToggleSwitch } from "@/components/ui/ToggleSwitch";
import { Button } from "@/components/ui/Button";
import { Page, PageHeader } from "./parts";

/** "3 h 12 min", "12 min", "45 s". */
function duration(seconds: number): string {
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

/** Bars per language, longest first. */
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

/** "Your voice": how much of your own voice is kept, and the opt-in. */
export const VerbatimVoicePage: React.FC = () => {
  const { t, i18n } = useTranslation();
  const [stats, setStats] = useState<VoiceDataStats | null>(null);
  const [community, setCommunity] = useState<CommunityTotal | null>(null);
  const [asking, setAsking] = useState(false);

  const load = useCallback(async () => {
    const r = await commands.voiceDataStats();
    if (r.status === "ok") setStats(r.data);
    const c = await commands.voiceDataCommunity();
    if (c.status === "ok") setCommunity(c.data);
  }, []);

  useEffect(() => {
    void load();
    // A processed meeting may have added clips.
    const unlisten = events.meetingChanged.listen(() => void load());
    return () => {
      unlisten.then((fn) => fn());
    };
  }, [load]);

  if (!stats) return null;
  const lang = (k: string) =>
    languageName(k, i18n.language, t("verbatim.voice.otherLanguage"));
  const source = (k: string) =>
    k === "meeting"
      ? t("verbatim.voice.fromMeetings")
      : t("verbatim.voice.fromDictation");

  return (
    <Page>
      <PageHeader
        title={t("verbatim.voice.title")}
        description={t("verbatim.voice.description")}
      />

      <section className="vb-card p-5 flex flex-col gap-5">
        <div className="flex flex-wrap items-end gap-x-10 gap-y-3">
          <div>
            <div className="text-[34px] font-semibold tracking-[-0.02em] tabular-nums leading-none">
              {duration(stats.total_seconds)}
            </div>
            <div className="text-[12.5px] text-ink-3 mt-1.5">
              {t("verbatim.voice.kept", { count: stats.clips })}
            </div>
          </div>
          {stats.bytes > 0 && (
            <div className="text-[12.5px] text-ink-3">
              {t("verbatim.voice.onDisk", {
                size: `${(stats.bytes / 1e9).toFixed(stats.bytes > 1e9 ? 1 : 2)} GB`,
              })}
            </div>
          )}
        </div>
        {stats.total_seconds > 0 ? (
          <div className="grid gap-6 sm:grid-cols-2">
            <div className="flex flex-col gap-2">
              <h2 className="text-[12px] font-semibold uppercase tracking-wide text-ink-3">
                {t("verbatim.voice.byLanguage")}
              </h2>
              <Breakdown values={stats.seconds_by_language} label={lang} />
            </div>
            <div className="flex flex-col gap-2">
              <h2 className="text-[12px] font-semibold uppercase tracking-wide text-ink-3">
                {t("verbatim.voice.bySource")}
              </h2>
              <Breakdown values={stats.seconds_by_source} label={source} />
            </div>
          </div>
        ) : (
          <p className="text-[13px] text-ink-3">
            {t(
              stats.opt_in
                ? "verbatim.voice.emptyOn"
                : "verbatim.voice.emptyOff",
            )}
          </p>
        )}
      </section>

      <section className="vb-card overflow-hidden">
        <ToggleSwitch
          checked={stats.opt_in}
          onChange={(on) =>
            void commands.voiceDataSetOptIn(on).then(() => load())
          }
          label={t("verbatim.voice.optIn")}
          description={t("verbatim.voice.optInDescription")}
          grouped
        />
        <ul className="border-t border-line px-4 py-3 flex flex-col gap-1.5 text-[12.5px] leading-[18px] text-ink-2 list-disc ps-8">
          <li>{t("verbatim.voice.point1")}</li>
          <li>{t("verbatim.voice.point2")}</li>
          <li>{t("verbatim.voice.point3")}</li>
          <li>{t("verbatim.voice.point4")}</li>
        </ul>
      </section>

      {community && community.installs > 0 && (
        <section className="vb-card p-4 flex items-center gap-3">
          <Users
            width={18}
            height={18}
            className="text-logo-primary shrink-0"
          />
          <p className="text-[13.5px]">
            {t("verbatim.voice.community", {
              hours: Math.round(community.total_seconds / 3600).toLocaleString(
                i18n.language,
              ),
              count: community.installs,
            })}
          </p>
        </section>
      )}

      <div className="flex flex-wrap items-center gap-2 px-1">
        <Button
          variant="secondary"
          size="sm"
          className="inline-flex items-center gap-1.5"
          onClick={() => void commands.voiceDataOpenFolder()}
        >
          <FolderOpen width={14} height={14} />
          {t("verbatim.voice.showFolder")}
        </Button>
        {stats.clips > 0 &&
          (asking ? (
            <>
              <span className="text-[12.5px] text-ink-2">
                {t("verbatim.voice.deleteAsk", { count: stats.clips })}
              </span>
              <Button
                size="sm"
                variant="secondary"
                onClick={() => setAsking(false)}
              >
                {t("verbatim.voice.cancel")}
              </Button>
              <Button
                size="sm"
                variant="danger"
                onClick={() =>
                  void commands.voiceDataDelete().then(() => {
                    setAsking(false);
                    void load();
                  })
                }
              >
                {t("verbatim.voice.delete")}
              </Button>
            </>
          ) : (
            <Button
              variant="danger-ghost"
              size="sm"
              className="inline-flex items-center gap-1.5"
              onClick={() => setAsking(true)}
            >
              <Trash2 width={14} height={14} />
              {t("verbatim.voice.deleteAll")}
            </Button>
          ))}
      </div>
    </Page>
  );
};
