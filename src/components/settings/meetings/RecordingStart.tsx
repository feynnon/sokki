import React, { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  commands,
  type AutoStart,
  type MeetingSettings as MeetingSettingsType,
} from "@/bindings";
import { SettingsGroup } from "../../ui/SettingsGroup";
import { ToggleSwitch } from "../../ui/ToggleSwitch";
import { useFormatLocale } from "./useFormatLocale";

/**
 * Settings › Meetings, first group: the two ways a recording starts
 * without a click (canvas version C, Robin's pick 2026-11-26): asking
 * when a call starts, and conversations that start by themselves
 * (`meeting/auto.rs`), with the last three of those ("but at most 3").
 */
export const RecordingStart: React.FC<{
  meetingSettings: MeetingSettingsType;
  save: (patch: Partial<MeetingSettingsType>) => Promise<void>;
}> = ({ meetingSettings, save }) => {
  const { t } = useTranslation();
  const [recent, setRecent] = useState<AutoStart[]>([]);
  useEffect(() => {
    commands
      .autoMeetingRecent()
      .then((r) => setRecent(r ?? []))
      .catch(() => setRecent([]));
  }, []);

  return (
    <SettingsGroup title={t("meetings.settings.startTitle")}>
      <ToggleSwitch
        checked={meetingSettings.detection_enabled}
        onChange={(v) => void save({ detection_enabled: v })}
        label={t("meetings.settings.detection")}
        description={t("meetings.settings.detectionDescription")}
        grouped
      />
      <ToggleSwitch
        checked={meetingSettings.auto_start_enabled}
        onChange={(v) =>
          void save({ auto_start_enabled: v, auto_start_set: true })
        }
        label={t("meetings.settings.autoStart")}
        description={t("meetings.settings.autoStartDescription")}
        grouped
      />
      {recent.length > 0 && <Recent recent={recent} />}
    </SettingsGroup>
  );
};

const Recent: React.FC<{ recent: AutoStart[] }> = ({ recent }) => {
  const { t } = useTranslation();
  const locale = useFormatLocale();
  const when = (iso: string) => {
    const d = new Date(iso);
    const today = new Date();
    const yesterday = new Date();
    yesterday.setDate(today.getDate() - 1);
    const time = d.toLocaleTimeString(locale, {
      hour: "2-digit",
      minute: "2-digit",
    });
    if (d.toDateString() === today.toDateString())
      return t("meetings.settings.autoToday", { time });
    if (d.toDateString() === yesterday.toDateString())
      return t("meetings.settings.autoYesterday", { time });
    return d.toLocaleString(locale, {
      weekday: "short",
      day: "numeric",
      month: "numeric",
      hour: "2-digit",
      minute: "2-digit",
    });
  };
  const what = (e: AutoStart) => {
    const minutes = Math.max(1, Math.round((e.talk_s ?? 0) / 60));
    switch (e.outcome) {
      case "recording":
        return t("meetings.settings.autoRecording");
      case "deleted":
        return t("meetings.settings.autoDeleted");
      case "stopped":
        return t("meetings.settings.autoStopped");
      default:
        return t("meetings.settings.autoMinutes", { count: minutes });
    }
  };
  return (
    <div className="px-4 pt-1 pb-3">
      <h4 className="text-[12px] leading-5 text-ink-3">
        {t("meetings.settings.autoRecent")}
      </h4>
      <ul aria-label={t("meetings.settings.autoRecent")}>
        {recent.slice(0, 3).map((e) => (
          <li
            key={e.meeting_id}
            className="flex items-center gap-3 border-t border-line py-1.5 text-[12.5px] leading-[18px]"
          >
            <span className="w-32 shrink-0 text-ink-3 tabular-nums">
              {when(e.started_at)}
            </span>
            <span className="min-w-0 flex-1 truncate text-ink-2">
              {e.title ?? t("meetings.settings.autoUntitled")}
            </span>
            <span className="shrink-0 text-ink-3">{what(e)}</span>
          </li>
        ))}
      </ul>
    </div>
  );
};
