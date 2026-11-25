import { create } from "zustand";
import { listen } from "@tauri-apps/api/event";
import { commands, events, type MeetingRecord } from "@/bindings";

/** Below this RMS (-55 dBFS, the pill's floor) a track counts as silent. */
export const SILENCE_RMS = 10 ** (-55 / 20);
/** Which track Sokki hears nothing from; Rust decides when
 * (`meeting/silence.rs`). The call hint replaces the 20-s test that
 * Robin approved on 2026-11-23, with the same text. */
export type SilenceHint = "mic" | "call";

/** A level as a bar height 0…1, as the pill draws it. */
export function levelHeight(rms: number): number {
  if (rms <= 0) return 0;
  const db = 20 * Math.log10(rms);
  return Math.min(1, Math.max(0, (db + 55) / 40)) ** 0.6;
}

interface MeetingState {
  /** Newest first, as the app lists them. */
  meetings: MeetingRecord[];
  loaded: boolean;
  recording: boolean;
  /** A start or stop is on its way. */
  busy: boolean;
  /** "Meeting with screen" was refused: screen recording not allowed. */
  screenHint: boolean;
  /** The meeting shown in the Meetings reader. */
  selectedId: string | null;
  mic: number;
  system: number;
  /** While recording: when each track last had sound (ms), or the start. */
  micHeardAt: number;
  systemHeardAt: number;
  /** How long the call track has been exact zeros or empty (ms). */
  systemDeadMs: number;
  /** "Sokki hears nothing" while it applies (`meeting/silence.rs` decides:
   * no speech from the microphone, or no sound from a detected call). */
  hint: SilenceHint | null;
  /** The microphone the meeting records from. */
  micName: string | null;
  reload: () => Promise<void>;
  start: () => Promise<void>;
  stop: () => Promise<void>;
  startWithScreen: () => Promise<void>;
  select: (id: string | null) => void;
}

export const useMeetingStore = create<MeetingState>((set, get) => ({
  meetings: [],
  loaded: false,
  recording: false,
  busy: false,
  screenHint: false,
  selectedId: null,
  mic: 0,
  system: 0,
  micHeardAt: 0,
  systemHeardAt: 0,
  systemDeadMs: 0,
  hint: null,
  micName: null,

  reload: async () => {
    const [list, recording] = await Promise.all([
      commands.listMeetings(),
      commands.isMeetingRecording(),
    ]);
    const was = get().recording;
    const now = Date.now();
    set({
      ...(list.status === "ok" ? { meetings: list.data } : {}),
      loaded: true,
      recording,
      ...(recording && !was ? { micHeardAt: now, systemHeardAt: now } : {}),
      ...(!recording
        ? { mic: 0, system: 0, systemDeadMs: 0, hint: null, micName: null }
        : {}),
    });
  },

  start: async () => {
    if (get().busy || get().recording) return;
    set({ busy: true, screenHint: false });
    try {
      const r = await commands.startMeetingRecording();
      // Recording shows as soon as the app says so, not after the list.
      if (r.status === "ok") {
        const now = Date.now();
        set({ recording: true, micHeardAt: now, systemHeardAt: now });
      } else console.error(r.error);
      await get().reload();
    } finally {
      set({ busy: false });
    }
  },

  stop: async () => {
    if (get().busy || !get().recording) return;
    set({ busy: true });
    try {
      const r = await commands.stopMeetingRecording();
      if (r.status === "ok") set({ recording: false, mic: 0, system: 0 });
      else console.error(r.error);
      await get().reload();
    } finally {
      set({ busy: false });
    }
  },

  /** "Meeting with screen": the meeting plus a full video of the screen. */
  startWithScreen: async () => {
    if (get().busy || get().recording) return;
    set({ busy: true, screenHint: false });
    try {
      const r = await commands.startMeetingWithScreen();
      set({
        screenHint: r.status === "error" && r.error === "screen_not_allowed",
      });
      if (r.status === "error") console.error(r.error);
      await get().reload();
    } finally {
      set({ busy: false });
    }
  },

  select: (id) => set({ selectedId: id }),
}));

/** The running meeting, if one records. */
export const recordingMeeting = (meetings: MeetingRecord[]) =>
  meetings.find((m) => m.status.kind === "recording") ?? null;

/** Shows a meeting in Meetings (App switches section on this event). */
export function openMeeting(id: string) {
  useMeetingStore.getState().select(id);
  window.dispatchEvent(
    new CustomEvent("sokki-open-section", { detail: "meetings" }),
  );
}

/** Follows the app: meeting changes and, while recording, the levels.
 * Returns the cleanup. Mounted once by App. */
export function watchMeetings(): () => void {
  const store = useMeetingStore;
  void store.getState().reload();
  const offChanged = events.meetingChanged.listen(
    () => void store.getState().reload(),
  );
  const offLevel = listen<{
    mic: number;
    system: number;
    system_dead_ms?: number;
    hint?: SilenceHint | null;
    mic_name?: string | null;
  }>("meeting-level", (e) => {
    if (!store.getState().recording) return;
    const now = Date.now();
    const { mic, system, system_dead_ms, hint, mic_name } = e.payload;
    store.setState((s) => ({
      mic,
      system,
      systemDeadMs: system_dead_ms ?? 0,
      hint: hint ?? null,
      micName: mic_name ?? null,
      micHeardAt: mic > SILENCE_RMS ? now : s.micHeardAt,
      systemHeardAt: system > SILENCE_RMS ? now : s.systemHeardAt,
    }));
  });
  return () => {
    offChanged.then((fn) => fn());
    offLevel.then((fn) => fn());
  };
}
