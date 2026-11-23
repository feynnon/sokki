import React, { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  AlertTriangle,
  ChevronUp,
  FileUp,
  Mic,
  Monitor,
  Video,
} from "lucide-react";
import { commands } from "@/bindings";
import { useSettings } from "@/hooks/useSettings";
import { useOsType } from "@/hooks/useOsType";
import { usePressAway } from "@/hooks/usePressAway";
import { keycapLabels } from "@/lib/utils/keyboard";
import { notify } from "@/stores/noticeStore";
import {
  NO_SOUND_AFTER_MS,
  levelHeight,
  recordingMeeting,
  useMeetingStore,
} from "@/stores/meetingStore";
import { pickAndTranscribeFiles } from "./settings/history/transcribeFiles";

/** "12:48", "1:02:03". */
export function clock(ms: number): string {
  const s = Math.max(0, Math.floor(ms / 1000));
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const sec = String(s % 60).padStart(2, "0");
  return h > 0 ? `${h}:${String(m).padStart(2, "0")}:${sec}` : `${m}:${sec}`;
}

/** The meeting shortcut as Mac menus print it: "⌥M". */
export function useMeetingKeys(): string {
  const { getSetting } = useSettings();
  const osType = useOsType();
  const binding = getSetting("bindings")?.meeting?.current_binding ?? "";
  return keycapLabels(binding, osType).join(osType === "macos" ? "" : "+");
}

const BAR_SHAPE = [6, 12, 16, 9, 5];
/** Shown next to "Transcribe file…"; handled in RecordBar. */
const OPEN_FILE_KEYS = "⌘O";

/** Five small bars that move with one track's level; flat when silent. */
const Meter: React.FC<{ level: number; flat: boolean }> = ({ level, flat }) => {
  const h = levelHeight(level);
  return (
    <span aria-hidden="true" className="flex items-end gap-[2px] h-4">
      {BAR_SHAPE.map((max, i) => (
        <i
          key={i}
          className="block w-[3px] rounded-[2px] transition-[height] duration-100"
          style={{
            height: flat ? 3 : Math.max(3, Math.round(max * h)),
            background: flat ? "var(--vb-flat)" : "var(--color-text)",
          }}
        />
      ))}
    </span>
  );
};

const Recording: React.FC = () => {
  const { t } = useTranslation();
  const meetings = useMeetingStore((s) => s.meetings);
  const busy = useMeetingStore((s) => s.busy);
  const stop = useMeetingStore((s) => s.stop);
  const mic = useMeetingStore((s) => s.mic);
  const system = useMeetingStore((s) => s.system);
  const micHeardAt = useMeetingStore((s) => s.micHeardAt);
  const systemHeardAt = useMeetingStore((s) => s.systemHeardAt);
  const record = recordingMeeting(meetings);
  const app = record?.source_app ?? null;
  const [now, setNow] = useState(Date.now());
  useEffect(() => {
    const timer = window.setInterval(() => setNow(Date.now()), 500);
    return () => window.clearInterval(timer);
  }, []);
  const startedAt = record ? Date.parse(record.started_at) : null;
  // Only with a call app: in a room there is no call audio to miss.
  const noSound =
    !!app &&
    now - systemHeardAt > NO_SOUND_AFTER_MS &&
    now - micHeardAt < NO_SOUND_AFTER_MS;

  return (
    <div
      role="status"
      className="h-14 shrink-0 flex items-center gap-5 ps-5 pe-4"
      style={{
        background: "var(--vb-rec-bar)",
        borderTop: "1px solid var(--vb-rec-line)",
      }}
    >
      <div
        className="flex items-center gap-2 text-[15px] font-semibold"
        style={{ color: "var(--vb-rec)" }}
      >
        <span
          className="vb-pulse size-2.5 shrink-0 rounded-full"
          style={{ background: "var(--vb-rec)" }}
        />
        {t("meetings.recording")}
        <span className="font-mono text-[13px] font-medium tabular-nums">
          {startedAt ? clock(now - startedAt) : "0:00"}
        </span>
        {app && <span className="font-normal text-ink-2">· {app}</span>}
      </div>
      <div className="flex items-center gap-2 text-[12px] text-ink-2">
        {t("sokki.bar.mic")}
        <Meter level={mic} flat={now - micHeardAt > 1000} />
      </div>
      {app && (
        <div className="flex items-center gap-2 text-[12px] text-ink-2">
          {app}
          <Meter level={system} flat={now - systemHeardAt > 1000} />
        </div>
      )}
      {noSound && (
        <div className="flex items-center gap-1.5 text-[13px] font-medium text-warning">
          <AlertTriangle width={16} height={16} className="shrink-0" />
          {t("sokki.bar.noSound", { app })}
        </div>
      )}
      <button
        type="button"
        onClick={() => void stop()}
        disabled={busy}
        className="ms-auto h-8 px-3.5 flex items-center gap-2 rounded-[10px] text-[13px] font-medium text-white cursor-pointer disabled:opacity-60 focus:outline-none focus-visible:ring-2 focus-visible:ring-white/60"
        style={{ background: "var(--vb-rec-fill)" }}
      >
        <span className="size-2.5 rounded-[2px] bg-white" />
        {t("sokki.bar.stop")}
      </button>
    </div>
  );
};

/** Asks for files and transcribes them into Diktate, which opens as soon
 * as files are picked. */
export async function transcribeFileIntoDictations(failed: string) {
  try {
    await pickAndTranscribeFiles(() =>
      window.dispatchEvent(
        new CustomEvent("sokki-open-section", { detail: "history" }),
      ),
    );
  } catch (error) {
    console.error("Failed to pick a file to transcribe:", error);
    notify({ title: failed });
  }
}

const Idle: React.FC = () => {
  const { t } = useTranslation();
  const { settings } = useSettings();
  const busy = useMeetingStore((s) => s.busy);
  const start = useMeetingStore((s) => s.start);
  const startWithScreen = useMeetingStore((s) => s.startWithScreen);
  const screenHint = useMeetingStore((s) => s.screenHint);
  const keys = useMeetingKeys();
  const [menu, setMenu] = useState(false);
  const menuRef = useRef<HTMLDivElement>(null);
  usePressAway(menu, menuRef, () => setMenu(false));
  const videos = settings?.videos;
  const videosOn = videos?.enabled ?? false;

  const item =
    "flex w-full items-center gap-2 rounded-lg px-2 py-1.5 text-[13px] text-start hover:bg-wash cursor-pointer focus:outline-none focus-visible:bg-wash";

  return (
    <div className="h-14 shrink-0 flex items-center gap-3 ps-5 pe-4 border-t border-line bg-background">
      <div className="flex items-center gap-2 text-[13px] font-medium text-ink-2">
        <span className="size-2.5 rounded-full border-2 border-ink-3" />
        {t("sokki.bar.idle")}
      </div>
      {screenHint && (
        <div className="flex items-center gap-1.5 text-[13px] text-warning">
          <AlertTriangle width={14} height={14} className="shrink-0" />
          {t("video.bar.screenNotAllowed")}
        </div>
      )}
      <span className="ms-auto font-mono text-[12px] text-ink-3">{keys}</span>
      <div ref={menuRef} className="relative flex">
        <button
          type="button"
          onClick={() => void start()}
          disabled={busy}
          title={keys ? `${t("meetings.start")} ${keys}` : undefined}
          className="h-8 ps-3.5 pe-3.5 flex items-center gap-1.5 rounded-s-[10px] bg-background-ui text-white text-[13px] font-medium cursor-pointer disabled:opacity-60 hover:bg-background-ui/90 focus:outline-none focus-visible:ring-2 focus-visible:ring-logo-primary"
        >
          <Mic width={16} height={16} />
          {t("meetings.start")}
        </button>
        <button
          type="button"
          aria-label={t("sokki.bar.more")}
          aria-haspopup="menu"
          aria-expanded={menu}
          disabled={busy}
          onClick={() => setMenu((m) => !m)}
          className="h-8 w-7 flex items-center justify-center rounded-e-[10px] bg-background-ui text-white border-s border-white/30 cursor-pointer disabled:opacity-60 hover:bg-background-ui/90 focus:outline-none focus-visible:ring-2 focus-visible:ring-logo-primary"
        >
          <ChevronUp width={16} height={16} />
        </button>
        {menu && (
          <div
            role="menu"
            className="absolute end-0 bottom-full z-30 mb-1.5 w-60 rounded-xl border border-line bg-background p-1 shadow-lg"
          >
            {videosOn && (
              <button
                type="button"
                role="menuitem"
                className={item}
                onClick={() => {
                  setMenu(false);
                  void startWithScreen(videos?.display ?? null);
                }}
              >
                <Monitor width={16} height={16} className="text-ink-2" />
                {t("meetings.startWithScreen")}
              </button>
            )}
            {videosOn && (
              <button
                type="button"
                role="menuitem"
                className={item}
                onClick={() => {
                  setMenu(false);
                  void commands.videoBarShow();
                }}
              >
                <Video width={16} height={16} className="text-ink-2" />
                {t("sokki.bar.recordVideo")}
              </button>
            )}
            <button
              type="button"
              role="menuitem"
              className={item}
              onClick={() => {
                setMenu(false);
                void transcribeFileIntoDictations(
                  t("settings.history.transcribeFailed"),
                );
              }}
            >
              <FileUp width={16} height={16} className="text-ink-2" />
              <span className="flex-1">{t("sokki.bar.transcribeFile")}</span>
              <span className="font-mono text-[12px] text-ink-3">
                {OPEN_FILE_KEYS}
              </span>
            </button>
          </div>
        )}
      </div>
    </div>
  );
};

/** The bar under every view: "Not recording" + "Start meeting", or red
 * with time, levels and "Stop" while a meeting records (design C). */
export const RecordBar: React.FC = () => {
  const { t } = useTranslation();
  const recording = useMeetingStore((s) => s.recording);

  // ⌘O transcribes a file from anywhere in the window.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (
        e.metaKey &&
        !e.shiftKey &&
        !e.altKey &&
        e.key.toLowerCase() === "o"
      ) {
        e.preventDefault();
        void transcribeFileIntoDictations(
          t("settings.history.transcribeFailed"),
        );
      }
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [t]);

  return recording ? <Recording /> : <Idle />;
};
