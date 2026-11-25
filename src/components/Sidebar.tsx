import React from "react";
import { useTranslation } from "react-i18next";
import {
  FlaskConical,
  Mic,
  Cpu,
  MessageSquareText,
  BookOpen,
  Volume2,
  Settings as SettingsIcon,
  AudioLines,
  Heart,
  Users,
} from "lucide-react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { platform } from "@tauri-apps/plugin-os";
import { SokkiLogo } from "./icons/SokkiLogo";
import { useSettings } from "../hooks/useSettings";
import { useMeetingStore } from "@/stores/meetingStore";
import {
  HistorySettings,
  MeetingsSettings,
  ModelsSettings,
  SokkiSettingsPage,
  SokkiSoundPage,
  SokkiVocabularyPage,
  SokkiVoicePage,
  SpeakersPage,
} from "./settings";

/** Sokki's "Sponsor" entry: sokki.org/sponsor/ forwards to GitHub Sponsors. */
const SPONSOR_URL = "https://sokki.org/sponsor/?s=sidebar";

export type SidebarSection = keyof typeof SECTIONS_CONFIG;

/** Sokki's sidebar: your data first (the start view), what you use, then
 * how the app behaves (debug last). Design C4, Robin 2026-11-21. */
const SOKKI_ORDER: SidebarSection[][] = [
  ["voice", "meetings", "speakers", "history", "vocabulary"],
  // Sound lives on the Settings page; Models keeps its own place.
  ["settings", "models"],
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
  history: {
    labelKey: "sokki.nav.dictations",
    icon: MessageSquareText,
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
  speakers: {
    labelKey: "sokki.nav.speakers",
    icon: Users,
    component: SpeakersPage,
    // Voices come from meetings, which are macOS only.
    enabled: () => platform() === "macos",
  },
  models: {
    labelKey: "sidebar.models",
    icon: Cpu,
    component: ModelsSettings,
    enabled: () => true,
  },
  vocabulary: {
    labelKey: "sokki.nav.vocabulary",
    icon: BookOpen,
    component: SokkiVocabularyPage,
    enabled: () => true,
  },
  voice: {
    labelKey: "sokki.nav.voice",
    icon: AudioLines,
    component: SokkiVoicePage,
    enabled: () => true,
  },
  settings: {
    labelKey: "sokki.nav.settings",
    icon: SettingsIcon,
    component: SokkiSettingsPage,
    enabled: () => true,
  },
  sound: {
    labelKey: "sokki.nav.sound",
    icon: Volume2,
    component: SokkiSoundPage,
    enabled: () => true,
  },
  debug: {
    labelKey: "sidebar.debug",
    icon: FlaskConical,
    component: null,
    enabled: (settings) => settings?.debug_mode ?? false,
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
  const recording = useMeetingStore((s) => s.recording);

  const availableSections = Object.entries(SECTIONS_CONFIG)
    .filter(([_, config]) => config.enabled(settings))
    .map(([id, config]) => ({ id: id as SidebarSection, ...config }));

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
                  {section.id === "meetings" && recording && (
                    <span
                      role="img"
                      aria-label={t("meetings.recording")}
                      className="vb-pulse ms-auto size-2 shrink-0 rounded-full"
                      style={{ background: "var(--vb-rec)" }}
                    />
                  )}
                </button>
              );
            })}
          </div>
        ))}
      </div>
      <button
        type="button"
        onClick={() => void openUrl(SPONSOR_URL)}
        className="group mt-auto flex items-center gap-2.5 h-8 px-2.5 w-full rounded-lg text-start text-[13.5px] cursor-pointer transition-colors text-ink-2 hover:bg-wash hover:text-text focus:outline-none focus-visible:ring-1 focus-visible:ring-logo-primary"
      >
        <Heart
          width={16}
          height={16}
          className="shrink-0 transition-colors group-hover:text-[#db61a2]"
        />
        <span className="truncate">{t("sokki.nav.sponsor")}</span>
      </button>
    </nav>
  );
};
