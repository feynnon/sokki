import React from "react";
import { useTranslation } from "react-i18next";
import {
  commands,
  type MeetingSettings,
  type RecordingRetentionPeriod,
} from "@/bindings";
import { useSettings } from "@/hooks/useSettings";
import { Dropdown } from "@/components/ui/Dropdown";
import { SettingContainer } from "@/components/ui/SettingContainer";

/** One choice for how long dictation and meeting recordings are kept. */
const CHOICES = [
  { value: "days3", days: 3, key: "days3" },
  { value: "weeks2", days: 14, key: "weeks2" },
  { value: "months3", days: 90, key: "months3" },
  { value: "never", days: 36500, key: "forever" },
] as const;

export const KeepRecordings: React.FC = () => {
  const { t } = useTranslation();
  const { settings, getSetting, updateSetting, refreshSettings } =
    useSettings();
  const current = getSetting("recording_retention_period") ?? "preserve_limit";
  const options = CHOICES.map((c) => ({
    value: c.value as string,
    label: t(`verbatim.keep.${c.key}`),
  }));
  if (!CHOICES.some((c) => c.value === current)) {
    options.unshift({
      value: current,
      label: t("verbatim.keep.latest", {
        count: getSetting("history_limit") ?? 5,
      }),
    });
  }
  const choose = async (value: string) => {
    const c = CHOICES.find((x) => x.value === value);
    if (!c) return;
    // Stored as "days3" etc. (serde), though the generated type says "days_3".
    await updateSetting(
      "recording_retention_period",
      c.value as unknown as RecordingRetentionPeriod,
    );
    const meetings = settings?.meetings as MeetingSettings | undefined;
    if (meetings) {
      await commands.setMeetingSettings({
        ...meetings,
        keep_audio_days: c.days,
      });
      await refreshSettings();
    }
  };
  return (
    <SettingContainer
      title={t("verbatim.keep.title")}
      description={t("verbatim.keep.description")}
      grouped
    >
      <Dropdown
        className="min-w-44"
        selectedValue={current}
        options={options}
        onSelect={(v) => void choose(v)}
      />
    </SettingContainer>
  );
};
