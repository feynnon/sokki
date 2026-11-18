import React from "react";
import { useTranslation } from "react-i18next";
import {
  Cog,
  FlaskConical,
  History,
  Info,
  Mic,
  Sparkles,
  Cpu,
  SlidersHorizontal,
  House,
  BookOpen,
  Volume2,
  Settings as SettingsIcon,
  AudioLines,
} from "lucide-react";
import { platform } from "@tauri-apps/plugin-os";
import HandyTextLogo from "./icons/HandyTextLogo";
import HandyHand from "./icons/HandyHand";
import { SokkiLogo } from "./icons/SokkiLogo";
import { IS_SOKKI } from "../lib/brand";
import { useSettings } from "../hooks/useSettings";
import {
  GeneralSettings,
  AdvancedSettings,
  HistorySettings,
  MeetingsSettings,
  AboutSettings,
  PostProcessingSettings,
  ModelsSettings,
  SokkiSettingsPage,
  SokkiSoundPage,
  SokkiVocabularyPage,
  SokkiVoicePage,
} from "./settings";

export type SidebarSection = keyof typeof SECTIONS_CONFIG;

/** Sokki's sidebar: what you use, then how the app behaves (debug last). */
const SOKKI_ORDER: SidebarSection[][] = [
  ["history", "meetings", "vocabulary", "voice"],
  ["settings", "sound", "models"],
  ["debug"],
];

interface IconProps {
  width?: number | string;
  height?: number | string;
  size?: number | string;
  className?: string;
  [key: string]: any;
}

interface SectionConfig {
  labelKey: string;
  icon: React.ComponentType<IconProps>;
  /** `null` when App renders the section itself (Debug, with its handlers). */
  component: React.ComponentType | null;
  enabled: (settings: any) => boolean;
}

export const SECTIONS_CONFIG = {
  general: {
    labelKey: "sidebar.general",
    icon: IS_SOKKI ? SlidersHorizontal : HandyHand,
    component: GeneralSettings,
    enabled: () => !IS_SOKKI,
  },
  history: {
    labelKey: IS_SOKKI ? "sokki.nav.home" : "sidebar.history",
    icon: IS_SOKKI ? House : History,
    component: HistorySettings,
    enabled: () => true,
  },
  meetings: {
    labelKey: "sidebar.meetings",
    icon: Mic,
    component: MeetingsSettings,
    // Meeting recording uses macOS system audio capture.
    enabled: () => platform() === "macos",
  },
  models: {
    labelKey: "sidebar.models",
    icon: Cpu,
    component: ModelsSettings,
    enabled: () => true,
  },
  advanced: {
    labelKey: "sidebar.advanced",
    icon: Cog,
    component: AdvancedSettings,
    enabled: () => !IS_SOKKI,
  },
  vocabulary: {
    labelKey: "sokki.nav.vocabulary",
    icon: BookOpen,
    component: SokkiVocabularyPage,
    enabled: () => IS_SOKKI,
  },
  voice: {
    labelKey: "sokki.nav.voice",
    icon: AudioLines,
    component: SokkiVoicePage,
    enabled: () => IS_SOKKI,
  },
  settings: {
    labelKey: "sokki.nav.settings",
    icon: SettingsIcon,
    component: SokkiSettingsPage,
    enabled: () => IS_SOKKI,
  },
  sound: {
    labelKey: "sokki.nav.sound",
    icon: Volume2,
    component: SokkiSoundPage,
    enabled: () => IS_SOKKI,
  },
  postprocessing: {
    labelKey: "sidebar.postProcessing",
    icon: Sparkles,
    component: PostProcessingSettings,
    enabled: (settings) =>
      !IS_SOKKI && (settings?.post_process_enabled ?? false),
  },
  debug: {
    labelKey: "sidebar.debug",
    icon: FlaskConical,
    component: null,
    enabled: (settings) => settings?.debug_mode ?? false,
  },
  about: {
    labelKey: "sidebar.about",
    icon: Info,
    component: AboutSettings,
    enabled: () => !IS_SOKKI,
  },
} as const satisfies Record<string, SectionConfig>;

interface SidebarProps {
  activeSection: SidebarSection;
  onSectionChange: (section: SidebarSection) => void;
}

export const Sidebar: React.FC<SidebarProps> = ({
  activeSection,
  onSectionChange,
}) => {
  const { t } = useTranslation();
  const { settings } = useSettings();

  const availableSections = Object.entries(SECTIONS_CONFIG)
    .filter(([_, config]) => config.enabled(settings))
    .map(([id, config]) => ({ id: id as SidebarSection, ...config }));

  if (IS_SOKKI) {
    const byId = new Map(availableSections.map((s) => [s.id, s]));
    const groups = SOKKI_ORDER.map((ids) =>
      ids.flatMap((id) => {
        const s = byId.get(id);
        return s ? [s] : [];
      }),
    );
    return (
      <nav
        aria-label="Sections"
        className="flex flex-col w-48 shrink-0 h-full px-3 pb-3"
        style={{ background: "var(--vb-sidebar)" }}
      >
        <SokkiLogo className="px-2 pt-5 pb-6" />
        <div className="flex flex-col gap-5">
          {groups.map((group, i) => (
            <div key={i} className="flex flex-col gap-0.5">
              {group.map((section) => {
                const Icon = section.icon;
                const isActive = activeSection === section.id;
                return (
                  <button
                    type="button"
                    key={section.id}
                    aria-current={isActive ? "page" : undefined}
                    onClick={() => onSectionChange(section.id)}
                    className={`flex items-center gap-2.5 h-8 px-2.5 w-full rounded-lg text-start text-[13.5px] cursor-pointer transition-colors focus:outline-none focus-visible:ring-1 focus-visible:ring-logo-primary ${
                      isActive
                        ? "bg-line text-text font-medium"
                        : "text-ink-2 hover:bg-wash hover:text-text"
                    }`}
                  >
                    <Icon width={16} height={16} className="shrink-0" />
                    <span className="truncate">{t(section.labelKey)}</span>
                  </button>
                );
              })}
            </div>
          ))}
        </div>
      </nav>
    );
  }

  return (
    <div className="flex flex-col w-40 h-full border-e border-line items-center px-2">
      {IS_SOKKI ? (
        <SokkiLogo className="my-5" />
      ) : (
        <HandyTextLogo width={120} className="m-4" />
      )}
      <div className="flex flex-col w-full items-center gap-1 pt-2 border-t border-line">
        {availableSections.map((section) => {
          const Icon = section.icon;
          const isActive = activeSection === section.id;

          return (
            <div
              key={section.id}
              className={`flex gap-2 items-center p-2 w-full rounded-lg cursor-pointer transition-colors ${
                isActive
                  ? IS_SOKKI
                    ? "bg-line"
                    : "bg-logo-primary/80"
                  : "hover:bg-line hover:opacity-100 opacity-85"
              }`}
              onClick={() => onSectionChange(section.id)}
            >
              <Icon width={24} height={24} className="shrink-0" />
              <p
                className="text-sm font-medium truncate"
                title={t(section.labelKey)}
              >
                {t(section.labelKey)}
              </p>
            </div>
          );
        })}
      </div>
    </div>
  );
};
