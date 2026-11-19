import React from "react";
import { useTranslation } from "react-i18next";
import { useSettings } from "@/hooks/useSettings";
import { SettingsGroup } from "@/components/ui/SettingsGroup";
import { SettingContainer } from "@/components/ui/SettingContainer";
import { MicrophoneSelector } from "../MicrophoneSelector";
import { MuteWhileRecording } from "../MuteWhileRecording";
import { VolumeSlider } from "../VolumeSlider";
import { Page, PageHeader, Segmented } from "./parts";

type Effect = "marimba" | "pop" | "off";

/** Sound effects (Marimba / Pop / Off), volume while they are on, the
 *  microphone and other audio: one group on the Settings page. */
export const SoundGroup: React.FC = () => {
  const { t } = useTranslation();
  const { getSetting, updateSetting } = useSettings();
  const on = getSetting("audio_feedback") ?? false;
  const theme = getSetting("sound_theme") ?? "marimba";
  const effect: Effect = !on ? "off" : theme === "pop" ? "pop" : "marimba";
  const choose = async (v: Effect) => {
    if (v === "off") {
      await updateSetting("audio_feedback", false);
      return;
    }
    await updateSetting("sound_theme", v);
    if (!on) await updateSetting("audio_feedback", true);
  };
  return (
    <SettingsGroup title={t("sokki.sound.title")}>
      <SettingContainer
        title={t("sokki.sound.effects")}
        description={t("sokki.sound.effectsDescription")}
        grouped
      >
        <Segmented<Effect>
          label={t("sokki.sound.effects")}
          value={effect}
          onChange={(v) => void choose(v)}
          options={[
            { value: "marimba", label: "Marimba" },
            { value: "pop", label: "Pop" },
            { value: "off", label: t("sokki.sound.off") },
          ]}
        />
      </SettingContainer>
      {on && <VolumeSlider />}
      <MicrophoneSelector descriptionMode="inline" grouped />
      <MuteWhileRecording descriptionMode="inline" grouped />
    </SettingsGroup>
  );
};

/** The old Sound section, kept reachable: the same group on its own page. */
export const SokkiSoundPage: React.FC = () => {
  const { t } = useTranslation();
  return (
    <Page>
      <PageHeader title={t("sokki.sound.title")} />
      <SoundGroup />
    </Page>
  );
};
