import React, { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { platform } from "@tauri-apps/plugin-os";
import { FolderOpen, Trash2 } from "lucide-react";
import { commands, type JournalStats } from "@/bindings";
import { useSettings } from "@/hooks/useSettings";
import { SettingsGroup } from "@/components/ui/SettingsGroup";
import { ToggleSwitch } from "@/components/ui/ToggleSwitch";
import { Button } from "@/components/ui/Button";

/** "312 KB", "1.4 MB". */
function size(bytes: number): string {
  if (bytes < 1000) return `${bytes} B`;
  if (bytes < 1_000_000) return `${Math.round(bytes / 1000)} KB`;
  return `${(bytes / 1_000_000).toFixed(1)} MB`;
}

/** "24 Nov" in the app's language. */
function day(iso: string, locale: string): string {
  const d = new Date(`${iso}T12:00:00`);
  return Number.isNaN(d.getTime())
    ? iso
    : d.toLocaleDateString(locale, { day: "numeric", month: "short" });
}

/** Settings › Developer mode: "Keep what I write" (sokki-journal), with
 * what it keeps and never keeps, its size, and its files. Mac only. */
export const DeveloperGroup: React.FC = () => {
  const { t, i18n } = useTranslation();
  const { getSetting, updateSetting, isUpdating } = useSettings();
  const on = getSetting("keep_what_i_write") ?? true;
  const [stats, setStats] = useState<JournalStats | null>(null);
  const [asking, setAsking] = useState(false);

  const load = useCallback(async () => {
    const r = await commands.journalStats();
    if (r.status === "ok") setStats(r.data);
  }, []);

  useEffect(() => {
    if (!on) return;
    void load();
    // New entries arrive as you write elsewhere.
    const timer = window.setInterval(() => void load(), 5000);
    return () => window.clearInterval(timer);
  }, [on, load]);

  if (platform() !== "macos") return null;

  const entries = stats?.entries ?? 0;
  return (
    <SettingsGroup title={t("sokki.developer.title")}>
      <ToggleSwitch
        checked={on}
        onChange={(enabled) => updateSetting("keep_what_i_write", enabled)}
        isUpdating={isUpdating("keep_what_i_write")}
        label={t("sokki.developer.keep")}
        description={t("sokki.developer.keepDescription")}
        descriptionMode="inline"
        grouped
      />
      {on && (
        <>
          <dl className="px-4 pb-3 grid grid-cols-[110px_1fr] gap-x-3 gap-y-1.5 text-[13px] leading-[19px]">
            <dt className="text-ink-3">{t("sokki.developer.kept")}</dt>
            <dd className="text-ink-2">{t("sokki.developer.keptText")}</dd>
            <dt className="text-ink-3">{t("sokki.developer.never")}</dt>
            <dd className="text-ink-2">{t("sokki.developer.neverText")}</dd>
            <dt className="text-ink-3">{t("sokki.developer.where")}</dt>
            <dd className="text-ink-2">{t("sokki.developer.whereText")}</dd>
          </dl>
          <ToggleSwitch
            checked={getSetting("learn_words") ?? true}
            onChange={(enabled) => updateSetting("learn_words", enabled)}
            isUpdating={isUpdating("learn_words")}
            label={t("sokki.developer.learn")}
            description={t("sokki.developer.learnDescription")}
            descriptionMode="inline"
            grouped
          />
          <div className="flex flex-wrap items-center gap-2 px-4 py-3 border-t border-line">
            <div className="flex-1 min-w-0">
              <p className="text-[14px] font-medium leading-5">
                {t("sokki.developer.entries", { count: entries })}
              </p>
              {stats?.since && (
                <p className="text-[12.5px] leading-[18px] text-ink-3 mt-0.5">
                  {t("sokki.developer.since", {
                    date: day(stats.since, i18n.language),
                    size: size(stats.bytes),
                  })}
                </p>
              )}
            </div>
            {asking ? (
              <>
                <span className="text-small text-ink-2">
                  {t("sokki.developer.deleteAsk", { count: entries })}
                </span>
                <Button
                  size="sm"
                  variant="secondary"
                  onClick={() => setAsking(false)}
                >
                  {t("sokki.developer.cancel")}
                </Button>
                <Button
                  size="sm"
                  variant="danger"
                  onClick={() =>
                    void commands.journalDeleteAll().then(() => {
                      setAsking(false);
                      void load();
                    })
                  }
                >
                  {t("sokki.developer.delete")}
                </Button>
              </>
            ) : (
              <>
                <Button
                  variant="secondary"
                  size="sm"
                  className="inline-flex items-center gap-1.5"
                  onClick={() => void commands.journalOpenFolder()}
                >
                  <FolderOpen width={14} height={14} />
                  {t("sokki.developer.showFiles")}
                </Button>
                {entries > 0 && (
                  <Button
                    variant="danger-ghost"
                    size="sm"
                    className="inline-flex items-center gap-1.5"
                    onClick={() => setAsking(true)}
                  >
                    <Trash2 width={14} height={14} />
                    {t("sokki.developer.deleteAll")}
                  </Button>
                )}
              </>
            )}
          </div>
        </>
      )}
    </SettingsGroup>
  );
};
