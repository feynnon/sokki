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

/** Sound effects (Marimba / Pop / Off) and volume, the microphone, other audio. */
export const VerbatimSoundPage: React.FC = () => {
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
    <Page>
      <PageHeader title={t("verbatim.sound.title")} />
      <SettingsGroup>
        <SettingContainer
          title={t("verbatim.sound.effects")}
          description={t("verbatim.sound.effectsDescription")}
          grouped
        >
          <Segmented<Effect>
            label={t("verbatim.sound.effects")}
            value={effect}
            onChange={(v) => void choose(v)}
            options={[
              { value: "marimba", label: "Marimba" },
              { value: "pop", label: "Pop" },
              { value: "off", label: t("verbatim.sound.off") },
            ]}
          />
        </SettingContainer>
        <VolumeSlider disabled={!on} />
      </SettingsGroup>
      <SettingsGroup title={t("verbatim.sound.recording")}>
        <MicrophoneSelector descriptionMode="inline" grouped />
        <MuteWhileRecording descriptionMode="inline" grouped />
      </SettingsGroup>
    </Page>
  );
};
