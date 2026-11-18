import React, { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { getVersion } from "@tauri-apps/api/app";
import { ChevronRight, ArrowLeft } from "lucide-react";
import { useSettings } from "@/hooks/useSettings";
import type { OverlayStyle } from "@/bindings";
import { SettingsGroup } from "@/components/ui/SettingsGroup";
import { SettingContainer } from "@/components/ui/SettingContainer";
import { ShortcutInput } from "../ShortcutInput";
import { ShortcutActivationSetting } from "../ShortcutActivation";
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
import { Page, PageHeader, Segmented } from "./parts";

/** Classic / Mini / None, as Superwhisper calls it. */
const RecordingWindow: React.FC = () => {
  const { t } = useTranslation();
  const { getSetting, updateSetting } = useSettings();
  const style = (getSetting("overlay_style") ?? "live") as OverlayStyle;
  return (
    <SettingContainer
      title={t("verbatim.settings.window")}
      description={t("verbatim.settings.windowDescription")}
      grouped
    >
      <Segmented<OverlayStyle>
        label={t("verbatim.settings.window")}
        value={style}
        onChange={(v) => void updateSetting("overlay_style", v)}
        options={[
          { value: "live", label: t("verbatim.settings.windowLive") },
          { value: "minimal", label: t("verbatim.settings.windowMini") },
          { value: "none", label: t("verbatim.settings.windowNone") },
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

/** Verbatim's settings: look, shortcuts, app; the rest behind Advanced. */
export const VerbatimSettingsPage: React.FC = () => {
  const { t } = useTranslation();
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
          {t("verbatim.settings.title")}
        </button>
        <PageHeader
          title={t("verbatim.settings.advanced")}
          description={t("verbatim.settings.advancedDescription")}
        />
        <SettingsGroup title={t("verbatim.settings.textInput")}>
          <PasteMethodSetting descriptionMode="inline" grouped />
          <ClipboardHandlingSetting descriptionMode="inline" grouped />
          <AutoSubmit descriptionMode="inline" grouped />
          <AppendTrailingSpace descriptionMode="inline" grouped />
        </SettingsGroup>
        <SettingsGroup title={t("verbatim.settings.transcription")}>
          <FillerWordRemoval descriptionMode="inline" grouped />
          <VoiceActivityDetection descriptionMode="inline" grouped />
          <TranslateToEnglish descriptionMode="inline" grouped />
          <ModelUnloadTimeoutSetting descriptionMode="inline" grouped />
        </SettingsGroup>
        <SettingsGroup title={t("verbatim.settings.application")}>
          <OverlayPosition />
          <AppDataDirectory descriptionMode="inline" grouped />
          <LogDirectory grouped />
        </SettingsGroup>
      </Page>
    );
  }

  return (
    <Page>
      <PageHeader title={t("verbatim.settings.title")} />
      <SettingsGroup title={t("verbatim.settings.appearance")}>
        <ThemeSelector descriptionMode="inline" grouped />
        <RecordingWindow />
        <AppLanguageSelector descriptionMode="inline" grouped />
      </SettingsGroup>
      <SettingsGroup title={t("verbatim.settings.shortcuts")}>
        <ShortcutInput
          shortcutId="transcribe"
          grouped
          descriptionMode="inline"
        />
        <ShortcutActivationSetting descriptionMode="inline" grouped />
        <ShortcutInput shortcutId="meeting" grouped descriptionMode="inline" />
        <ShortcutInput shortcutId="cancel" grouped descriptionMode="inline" />
      </SettingsGroup>
      <SettingsGroup title={t("verbatim.settings.application")}>
        <AutostartToggle descriptionMode="inline" grouped />
        <KeepRecordings />
      </SettingsGroup>
      <button
        type="button"
        onClick={() => setAdvanced(true)}
        className="vb-card flex items-center justify-between px-4 min-h-[52px] text-[14px] font-medium cursor-pointer hover:bg-wash text-start focus:outline-none focus-visible:ring-1 focus-visible:ring-logo-primary"
      >
        {t("verbatim.settings.advanced")}
        <ChevronRight width={16} height={16} className="text-ink-3" />
      </button>
      <p className="px-1 text-xs text-ink-3">
        {t("verbatim.settings.version", { version })} ·{" "}
        {t("verbatim.settings.basedOn")}
      </p>
    </Page>
  );
};
