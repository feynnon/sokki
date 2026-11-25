import React, { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { listen } from "@tauri-apps/api/event";
import {
  AlertTriangle,
  Check,
  ChevronUp,
  FileUp,
  Mic,
  Monitor,
  X,
} from "lucide-react";
import { commands } from "@/bindings";
import { useSettings } from "@/hooks/useSettings";
import { useOsType } from "@/hooks/useOsType";
import { usePressAway } from "@/hooks/usePressAway";
import { keycapLabels } from "@/lib/utils/keyboard";
import { notify } from "@/stores/noticeStore";
import {
  levelHeight,
  recordingMeeting,
  useMeetingStore,
  type SilenceHint,
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

/** "Sokki hears nothing": the microphone brings no speech, or the detected
 * call no sound (`meeting/silence.rs` decides, once per recording). The
 * microphone hint offers the other microphones; choosing one moves the
 * meeting to it within a second. Its X hides the hint for this meeting. */
const SilenceHintRow: React.FC<{
  hint: SilenceHint;
  mic: string | null;
  app: string | null;
  onDismiss: () => void;
}> = ({ hint, mic, app, onDismiss }) => {
  const { t } = useTranslation();
  const [menu, setMenu] = useState(false);
  const [devices, setDevices] = useState<string[] | null>(null);
  // Shown at once; the meeting reports the new name a second later.
  const [chosen, setChosen] = useState<string | null>(null);
  const menuRef = useRef<HTMLDivElement>(null);
  usePressAway(menu, menuRef, () => setMenu(false));
  const name = chosen ?? mic;
  useEffect(() => {
    if (chosen && chosen === mic) setChosen(null);
  }, [chosen, mic]);

  const openMenu = () => {
    setMenu((m) => !m);
    void commands.getAvailableMicrophones().then((r) => {
      if (r.status === "ok")
        setDevices(
          r.data.filter((d) => d.index !== "default").map((d) => d.name),
        );
    });
  };
  const choose = (device: string) => {
    setMenu(false);
    setChosen(device);
    void commands.switchMeetingMicrophone(device).then((r) => {
      if (r.status === "error") {
        setChosen(null);
        notify({ title: t("sokki.bar.switchFailed") });
      }
    });
  };
  const item =
    "flex w-full items-center gap-2 rounded-lg px-2 py-1.5 text-[13px] text-start hover:bg-wash cursor-pointer focus:outline-none focus-visible:bg-wash";

  return (
    <div
      data-testid="silence-hint"
      role="alert"
      className="h-11 shrink-0 flex items-center gap-2.5 ps-5 pe-4 text-[13px] font-medium text-warning bg-background"
      style={{ borderTop: "1px solid var(--vb-rec-line)" }}
    >
      <AlertTriangle width={16} height={16} className="shrink-0" />
      <span className="min-w-0 truncate">
        {hint === "call"
          ? t("sokki.bar.noSound", { app })
          : name
            ? t("sokki.bar.micSilent", { mic: name })
            : t("sokki.bar.micSilentUnnamed")}
      </span>
      {hint === "mic" && (
        <div ref={menuRef} className="relative shrink-0">
          <button
            type="button"
            aria-haspopup="menu"
            aria-expanded={menu}
            onClick={openMenu}
            className="h-7 px-2.5 flex items-center gap-1 rounded-lg border border-line bg-background text-[12px] font-medium text-text whitespace-nowrap cursor-pointer hover:bg-wash focus:outline-none focus-visible:ring-2 focus-visible:ring-logo-primary"
          >
            {t("sokki.bar.switchMic")}
            <ChevronUp width={14} height={14} />
          </button>
          {menu && (
            <div
              role="menu"
              className="absolute start-0 bottom-full z-30 mb-1.5 w-64 rounded-xl border border-line bg-background p-1 shadow-lg"
            >
              {(devices ?? []).map((d) => (
                <button
                  key={d}
                  type="button"
                  role="menuitem"
                  className={item}
                  onClick={() => choose(d)}
                >
                  <span className="w-4 shrink-0">
                    {d === name && <Check width={14} height={14} />}
                  </span>
                  <span className="flex-1 truncate font-normal text-text">
                    {d}
                  </span>
                </button>
              ))}
              {devices && devices.length === 0 && (
                <div className="px-2 py-1.5 text-[12px] text-ink-3">
                  {t("sokki.bar.noOtherMic")}
                </div>
              )}
            </div>
          )}
        </div>
      )}
      <button
        type="button"
        aria-label={t("sokki.bar.hideHint")}
        title={t("sokki.bar.hideHint")}
        onClick={onDismiss}
        className="ms-auto shrink-0 size-6 flex items-center justify-center rounded-md text-ink-3 cursor-pointer hover:bg-wash hover:text-text focus:outline-none focus-visible:ring-2 focus-visible:ring-logo-primary"
      >
        <X width={14} height={14} />
      </button>
    </div>
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
  const hint = useMeetingStore((s) => s.hint);
  const micName = useMeetingStore((s) => s.micName);
  // Hints the user closed in this meeting.
  const [hidden, setHidden] = useState<SilenceHint[]>([]);
  const record = recordingMeeting(meetings);
  const app = record?.source_app ?? null;
  const [now, setNow] = useState(Date.now());
  useEffect(() => {
    const timer = window.setInterval(() => setNow(Date.now()), 500);
    return () => window.clearInterval(timer);
  }, []);
  const startedAt = record ? Date.parse(record.started_at) : null;
  const shownHint = hint && !hidden.includes(hint) ? hint : null;

  return (
    <div className="shrink-0 flex flex-col">
      {shownHint && (
        <SilenceHintRow
          hint={shownHint}
          mic={micName}
          app={app}
          onDismiss={() => setHidden((h) => [...h, shownHint])}
        />
      )}
      <div
        role="status"
        className="h-14 shrink-0 flex items-center gap-5 ps-5 pe-4"
        style={{
          background: "var(--vb-rec-bar)",
          borderTop: "1px solid var(--vb-rec-line)",
        }}
      >
        <div
          className="shrink-0 whitespace-nowrap flex items-center gap-2 text-[15px] font-semibold"
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
        <div className="shrink-0 flex items-center gap-2 text-[12px] text-ink-2">
          {t("sokki.bar.mic")}
          <Meter level={mic} flat={now - micHeardAt > 1000} />
        </div>
        {app && (
          <div className="shrink-0 flex items-center gap-2 text-[12px] text-ink-2">
            {app}
            <Meter level={system} flat={now - systemHeardAt > 1000} />
          </div>
        )}
        <button
          type="button"
          onClick={() => void stop()}
          disabled={busy}
          className="ms-auto shrink-0 h-8 px-3.5 flex items-center gap-2 rounded-[10px] text-[13px] font-medium text-white cursor-pointer disabled:opacity-60 focus:outline-none focus-visible:ring-2 focus-visible:ring-white/60"
          style={{ background: "var(--vb-rec-fill)" }}
        >
          <span className="size-2.5 rounded-[2px] bg-white" />
          {t("sokki.bar.stop")}
        </button>
      </div>
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

/** How long the bar says "No speech, nothing saved." */
const NO_SPEECH_SHOWN_MS = 20_000;

/** A meeting without speech was just removed (`no_speech` in
 * src-tauri/src/meeting/process.rs): true for a moment. */
function useNoSpeech(): boolean {
  const [shown, setShown] = useState(false);
  useEffect(() => {
    let timer: ReturnType<typeof setTimeout> | undefined;
    const off = listen("meeting-no-speech", () => {
      setShown(true);
      clearTimeout(timer);
      timer = setTimeout(() => setShown(false), NO_SPEECH_SHOWN_MS);
    });
    return () => {
      clearTimeout(timer);
      void off.then((fn) => fn());
    };
  }, []);
  return shown;
}

const Idle: React.FC = () => {
  const { t } = useTranslation();
  const busy = useMeetingStore((s) => s.busy);
  const start = useMeetingStore((s) => s.start);
  const startWithScreen = useMeetingStore((s) => s.startWithScreen);
  const screenHint = useMeetingStore((s) => s.screenHint);
  const noSpeech = useNoSpeech();
  const keys = useMeetingKeys();
  const [menu, setMenu] = useState(false);
  const menuRef = useRef<HTMLDivElement>(null);
  usePressAway(menu, menuRef, () => setMenu(false));

  const item =
    "flex w-full items-center gap-2 rounded-lg px-2 py-1.5 text-[13px] text-start hover:bg-wash cursor-pointer focus:outline-none focus-visible:bg-wash";

  return (
    <div className="h-14 shrink-0 flex items-center gap-3 ps-5 pe-4 border-t border-line bg-background">
      <div className="shrink-0 whitespace-nowrap flex items-center gap-2 text-[13px] font-medium text-ink-2">
        <span className="size-2.5 rounded-full border-2 border-ink-3" />
        <span role="status">
          {noSpeech ? t("sokki.bar.noSpeech") : t("sokki.bar.idle")}
        </span>
      </div>
      {screenHint && (
        <div className="min-w-0 flex items-center gap-1.5 text-[12px] leading-tight text-warning">
          <AlertTriangle width={14} height={14} className="shrink-0" />
          <span className="line-clamp-2" title={t("meetings.screenNotAllowed")}>
            {t("meetings.screenNotAllowed")}
          </span>
        </div>
      )}
      <span className="ms-auto shrink-0 whitespace-nowrap font-mono text-[12px] text-ink-3">
        {keys}
      </span>
      <div ref={menuRef} className="relative flex shrink-0 whitespace-nowrap">
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
            <button
              type="button"
              role="menuitem"
              className={item}
              onClick={() => {
                setMenu(false);
                void startWithScreen();
              }}
            >
              <Monitor width={16} height={16} className="text-ink-2" />
              {t("meetings.startWithScreen")}
            </button>
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
