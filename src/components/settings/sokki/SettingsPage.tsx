import React, { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { getVersion } from "@tauri-apps/api/app";
import { ChevronRight, ArrowLeft } from "lucide-react";
import { useSettings } from "@/hooks/useSettings";
import { commands, type OverlayStyle, type ShortcutActivation } from "@/bindings";
import { ToggleSwitch } from "@/components/ui/ToggleSwitch";
import { SettingsGroup } from "@/components/ui/SettingsGroup";
import { SettingContainer } from "@/components/ui/SettingContainer";
import { ShortcutInput } from "../ShortcutInput";
import { AutostartToggle } from "../AutostartToggle";
import { ThemeSelector } from "../ThemeSelector";
import { AppLanguageSelector } from "../AppLanguageSelector";
import { PasteMethodSetting } from "../PasteMethod";
import { ClipboardHandlingSetting } from "../ClipboardHandling";
import { AutoSubmit } from "../AutoSubmit";
import { AppendTrailingSpace } from "../AppendTrailingSpace";
import { FillerWordRemoval } from "../FillerWordRemoval";
import { VoiceActivityDetection } from "../VoiceActivityDetection";
import { ModelUnloadTimeoutSetting } from "../ModelUnloadTimeout";
import { TranslateToEnglish } from "../TranslateToEnglish";
import { AppDataDirectory } from "../AppDataDirectory";
import { LogDirectory } from "../debug/LogDirectory";
import { KeepRecordings } from "./KeepRecordings";
import { SoundGroup } from "./SoundPage";
import { SharingGroup } from "./SharingGroup";
import { Page, PageHeader, Segmented } from "./parts";

/** Videos on or off: off hides screen and camera recording everywhere. */
const VideosToggle: React.FC = () => {
  const { t } = useTranslation();
  const { settings, refreshSettings } = useSettings();
  const videos = settings?.videos;
  if (!videos) return null;
  return (
    <ToggleSwitch
      checked={videos.enabled}
      onChange={async (enabled) => {
        await commands.setVideoSettings({ ...videos, enabled });
        await refreshSettings();
      }}
      label={t("video.settings.title")}
      description={t("video.settings.description")}
      descriptionMode="inline"
      grouped
    />
  );
};

/** Classic / Mini / None, as Superwhisper calls it. */
const RecordingWindow: React.FC = () => {
  const { t } = useTranslation();
  const { getSetting, updateSetting } = useSettings();
  const style = (getSetting("overlay_style") ?? "live") as OverlayStyle;
  return (
    <SettingContainer
      title={t("sokki.settings.window")}
      description={t("sokki.settings.windowDescription")}
      grouped
    >
      <Segmented<OverlayStyle>
        label={t("sokki.settings.window")}
        value={style}
        onChange={(v) => void updateSetting("overlay_style", v)}
        options={[
          { value: "live", label: t("sokki.settings.windowLive") },
          { value: "minimal", label: t("sokki.settings.windowMini") },
          { value: "none", label: t("sokki.settings.windowNone") },
        ]}
      />
    </SettingContainer>
  );
};

/** How the dictation key works; the line under it explains the chosen one. */
const DictationMode: React.FC = () => {
  const { t } = useTranslation();
  const { getSetting, updateSetting } = useSettings();
  const mode = (getSetting("shortcut_activation") ??
    "hold_or_toggle") as ShortcutActivation;
  const explain: Record<ShortcutActivation, string> = {
    hold_or_toggle: t("sokki.settings.modeAutoDescription"),
    push_to_talk: t("sokki.settings.modeHoldDescription"),
    toggle: t("sokki.settings.modeTapDescription"),
  };
  return (
    <SettingContainer
      title={t("sokki.settings.mode")}
      description={explain[mode]}
      grouped
    >
      <Segmented<ShortcutActivation>
        label={t("sokki.settings.mode")}
        value={mode}
        onChange={(v) => void updateSetting("shortcut_activation", v)}
        options={[
          { value: "hold_or_toggle", label: t("sokki.settings.modeAuto") },
          { value: "push_to_talk", label: t("sokki.settings.modeHold") },
          { value: "toggle", label: t("sokki.settings.modeTap") },
        ]}
      />
    </SettingContainer>
  );
};

const OverlayPosition: React.FC = () => {
  const { t } = useTranslation();
  const { getSetting, updateSetting } = useSettings();
  const pos = getSetting("overlay_position") === "top" ? "top" : "bottom";
  return (
    <SettingContainer
      title={t("settings.advanced.overlay.position.title")}
      description={t("settings.advanced.overlay.position.description")}
      grouped
    >
      <Segmented<"top" | "bottom">
        label={t("settings.advanced.overlay.position.title")}
        value={pos}
        onChange={(v) => void updateSetting("overlay_position", v)}
        options={[
          {
            value: "top",
            label: t("settings.advanced.overlay.position.options.top"),
          },
          {
            value: "bottom",
            label: t("settings.advanced.overlay.position.options.bottom"),
          },
        ]}
      />
    </SettingContainer>
  );
};

/** Sokki's settings: look, shortcuts, app; the rest behind Advanced. */
export const SokkiSettingsPage: React.FC = () => {
  const { t } = useTranslation();
  const { getSetting } = useSettings();
  // Discard is only offered where a dictation keeps running after the key
  // is let go (Auto after a tap, and Tap); in Hold, letting go ends it.
  const activation = (getSetting("shortcut_activation") ??
    "hold_or_toggle") as ShortcutActivation;
  const [advanced, setAdvanced] = useState(false);
  const [version, setVersion] = useState("");
  useEffect(() => {
    void getVersion()
      .then(setVersion)
      .catch(() => {});
  }, []);

  if (advanced) {
    return (
      <Page>
        <button
          type="button"
          onClick={() => setAdvanced(false)}
          className="self-start inline-flex items-center gap-1.5 text-[13px] text-ink-2 hover:text-text px-1 cursor-pointer"
        >
          <ArrowLeft width={14} height={14} />
          {t("sokki.settings.title")}
        </button>
        <PageHeader
          title={t("sokki.settings.advanced")}
          description={t("sokki.settings.advancedDescription")}
        />
        <SettingsGroup title={t("sokki.settings.textInput")}>
          <PasteMethodSetting descriptionMode="inline" grouped />
          <ClipboardHandlingSetting descriptionMode="inline" grouped />
          <AutoSubmit descriptionMode="inline" grouped />
          <AppendTrailingSpace descriptionMode="inline" grouped />
        </SettingsGroup>
        <SettingsGroup title={t("sokki.settings.transcription")}>
          <FillerWordRemoval descriptionMode="inline" grouped />
          <VoiceActivityDetection descriptionMode="inline" grouped />
          <TranslateToEnglish descriptionMode="inline" grouped />
          <ModelUnloadTimeoutSetting descriptionMode="inline" grouped />
        </SettingsGroup>
        <SettingsGroup title={t("sokki.settings.application")}>
          <OverlayPosition />
          <AppDataDirectory descriptionMode="inline" grouped />
          <LogDirectory grouped />
        </SettingsGroup>
      </Page>
    );
  }

  return (
    <Page>
      <PageHeader title={t("sokki.settings.title")} />
      <SharingGroup />
      <SettingsGroup title={t("sokki.settings.appearance")}>
        <ThemeSelector descriptionMode="inline" grouped />
        <RecordingWindow />
        <AppLanguageSelector descriptionMode="inline" grouped />
      </SettingsGroup>
      <SettingsGroup title={t("sokki.settings.dictation")}>
        <ShortcutInput
          shortcutId="transcribe"
          grouped
          descriptionMode="inline"
          title={t("sokki.settings.startDictation")}
          description=""
        />
        <DictationMode />
        {activation !== "push_to_talk" && (
          <ShortcutInput
            shortcutId="cancel"
            grouped
            descriptionMode="inline"
            title={t("sokki.settings.discardDictation")}
            description=""
          />
        )}
      </SettingsGroup>
      <SettingsGroup title={t("sokki.settings.meetings")}>
        <ShortcutInput
          shortcutId="meeting"
          grouped
          descriptionMode="inline"
          title={t("sokki.settings.startMeeting")}
          description={t("sokki.settings.startMeetingDescription")}
        />
        <VideosToggle />
      </SettingsGroup>
      <SoundGroup />
      <SettingsGroup title={t("sokki.settings.application")}>
        <AutostartToggle descriptionMode="inline" grouped />
        <KeepRecordings />
      </SettingsGroup>
      <button
        type="button"
        onClick={() => setAdvanced(true)}
        className="vb-card flex items-center justify-between px-4 min-h-[52px] text-[14px] font-medium cursor-pointer hover:bg-wash text-start focus:outline-none focus-visible:ring-1 focus-visible:ring-logo-primary"
      >
        {t("sokki.settings.advanced")}
        <ChevronRight width={16} height={16} className="text-ink-3" />
      </button>
      <p className="px-1 text-xs text-ink-3">
        {t("sokki.settings.version", { version })} ·{" "}
        {t("sokki.settings.basedOn")}
      </p>
    </Page>
  );
};
