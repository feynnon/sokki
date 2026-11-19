import React, { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { commands, type CommunityTotal, type VoiceDataStats } from "@/bindings";
import { SettingsGroup } from "@/components/ui/SettingsGroup";
import { DataChoices } from "./DataChoices";
import { duration } from "./VoicePage";

/** Settings' first group: the sharing choices, how much this Mac has
 * shared, and how many people share. */
export const SharingGroup: React.FC = () => {
  const { t, i18n } = useTranslation();
  const [stats, setStats] = useState<VoiceDataStats | null>(null);
  const [community, setCommunity] = useState<CommunityTotal | null>(null);

  const load = useCallback(async () => {
    const r = await commands.voiceDataStats();
    if (r.status === "ok") setStats(r.data);
  }, []);

  useEffect(() => {
    void load();
    void commands.voiceDataCommunity().then((c) => {
      if (c.status === "ok") setCommunity(c.data);
    });
  }, [load]);

  // Uploads run in the background; while sharing is on, "Shared so far"
  // follows them.
  const sharing = stats?.support ?? false;
  useEffect(() => {
    if (!sharing) return;
    const timer = window.setInterval(() => void load(), 5000);
    return () => window.clearInterval(timer);
  }, [sharing, load]);

  if (!stats) return null;
  // Shown while sharing, once anything was shared.
  const shared =
    stats.support && stats.donated_clips > 0
      ? t("sokki.voice.donated", {
          duration: duration(stats.donated_seconds),
        })
      : null;
  // The counter reports only 1000, 10000, …; 0 below 1000.
  const people =
    community && community.donors >= 1000
      ? t("sokki.settings.sharers", {
          people: community.donors.toLocaleString(i18n.language),
        })
      : null;

  return (
    <SettingsGroup title={t("sokki.settings.sharing")}>
      <DataChoices
        value={{
          improve: stats.improve,
          buyers: stats.buyers,
          support: stats.support,
        }}
        onChange={(v) =>
          void commands
            .voiceDataSetChoices(v.improve, v.support, v.buyers)
            .then(() => load())
        }
        variant="switch"
      />
      {(shared || people) && (
        <p className="px-4 py-3 text-[12.5px] leading-[18px] text-ink-3">
          {[people, shared].filter(Boolean).join(" ")}
        </p>
      )}
    </SettingsGroup>
  );
};
