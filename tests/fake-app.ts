import type { Page } from "@playwright/test";

/**
 * A fake Sokki behind the main window, for browser tests: every Tauri call
 * answers from the fixtures below. Nothing starts Sokki and nothing opens
 * on screen (headless Chromium). The meeting titles, names and texts are
 * invented filler.
 */

export type Scenario = {
  lang: "de" | "en";
  theme: "dark" | "light";
  empty?: boolean;
  recording?: boolean;
  firstRun?: boolean;
  section?: string;
  /** First start: setup instead of the main window, resuming from `saved`
   * (what setup keeps in localStorage across a relaunch). */
  setup?: { saved?: Record<string, unknown> };
  /** macOS permissions at start; a request grants Microphone and
   * Accessibility, Screen only after a relaunch (as macOS does). */
  perms?: { mic: boolean; acc: boolean; scr: boolean };
  /** Another page than the main window, e.g. the call window. */
  path?: string;
  /** More fake answers, by command name; `{ __throw: x }` fails with x. */
  extra?: Record<string, unknown>;
};

function fixtures(s: Scenario) {
  const now = new Date();
  const at = (daysAgo: number, h: number, m = 0) => {
    const d = new Date(
      now.getFullYear(),
      now.getMonth(),
      now.getDate() - daysAgo,
      h,
      m,
    );
    // RFC 3339 with the local offset, as the app writes it.
    const off = -d.getTimezoneOffset();
    const sign = off >= 0 ? "+" : "-";
    const pad = (n: number) => String(Math.abs(n)).padStart(2, "0");
    return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}T${pad(d.getHours())}:${pad(d.getMinutes())}:00${sign}${pad(Math.trunc(off / 60))}:${pad(off % 60)}`;
  };
  const done = { kind: "done" };
  const meeting = (
    id: string,
    title: string | null,
    startedAt: string,
    minutes: number,
    app: string | null,
    status: object = done,
  ) => ({
    id,
    started_at: startedAt,
    duration_s: minutes * 60,
    status,
    tracks: ["mic.flac", "system.flac"],
    source_app: app,
    title,
    note_path: `/tmp/sokki-notes/${id}.md`,
    speakers: ["Du", "Lea", "Jonas"],
    warnings: [],
    summary_missing: null,
    audio_deleted: false,
    online: true,
  });
  const meetings = s.empty
    ? []
    : [
        ...(s.recording
          ? [
              meeting(
                "rec",
                null,
                new Date(Date.now() - 768_000).toISOString(),
                0,
                "Zoom",
                { kind: "recording" },
              ),
            ]
          : []),
        meeting("m1", "Wochenplanung Produkt", at(0, 10), 34, "Zoom"),
        meeting(
          "m2",
          "Kundengespräch Lieferzeiten",
          at(1, 15, 30),
          48,
          "Google Meet",
        ),
        meeting("m3", "1:1 mit Jonas", at(3, 9), 22, "Microsoft Teams"),
        meeting("m4", "Interview Nutzerforschung", at(9, 14), 41, "Zoom"),
        meeting("m5", null, at(12, 16, 15), 9, null),
      ];
  // Voices across meetings (Sprecher page); invented people.
  const line = (
    meeting_id: string,
    speaker: string,
    start_s: number,
    text: string,
    title: string,
    daysAgo: number,
  ) => ({
    meeting_id,
    meeting_title: title,
    started_at: at(daysAgo, 10),
    speaker,
    start_s,
    text,
    audio: true,
  });
  const voice = (
    id: string,
    name: string | null,
    meetings: number,
    daysAgo: number,
    extra: object = {},
  ) => ({
    id,
    name,
    is_user: false,
    meetings,
    unnamed: name ? 0 : meetings,
    last: at(daysAgo, 10),
    members: Array.from({ length: meetings }, (_, i) => ({
      meeting_id: `m${i + 1}`,
      speaker: name ?? "Speaker 2",
    })),
    emails: [],
    remembered: false,
    suggestion: null,
    lines: [],
    ...extra,
  });
  const voices = s.empty
    ? []
    : [
        voice("a", null, 13, 0, {
          suggestion: {
            name: "Mara Keller",
            email: "mara@example.com",
            invited: 11,
            of: 13,
          },
          lines: [
            line(
              "m1",
              "Speaker 2",
              312,
              "Die Preise kläre ich bis Mittwoch mit dem Vertrieb.",
              "Wochenplanung Produkt",
              0,
            ),
            line(
              "m2",
              "Speaker 3",
              95,
              "Das Angebot für den Kunden geht morgen raus.",
              "Kundengespräch Lieferzeiten",
              1,
            ),
            line(
              "m4",
              "Speaker 2",
              1210,
              "Ich übernehme die Folien für Freitag.",
              "Interview Nutzerforschung",
              9,
            ),
          ],
        }),
        voice("b", null, 12, 1, {
          suggestion: {
            name: "Tim Hoffmann",
            email: null,
            invited: 7,
            of: 12,
          },
        }),
        voice("c", null, 5, 6),
        voice("u", "Du", 17, 0, { is_user: true }),
        voice("l", "Lea Brandt", 9, 0, {
          remembered: true,
          emails: ["lea@example.com"],
        }),
        voice("j", "Jonas Weber", 4, 3, {
          unnamed: 2,
          members: [
            { meeting_id: "m1", speaker: "Jonas Weber" },
            { meeting_id: "m2", speaker: "Jonas Weber" },
            { meeting_id: "m3", speaker: "Speaker 2" },
            { meeting_id: "m4", speaker: "Speaker 3" },
          ],
        }),
      ];
  return {
    voices,
    settings: {
      onboarding_completed: !s.setup,
      app_language: s.lang,
      // A new install starts with the default (dark); `theme` then only
      // sets macOS's appearance.
      theme: s.setup ? "dark" : s.theme,
      debug_mode: false,
      selected_model: "parakeet",
      shortcut_activation: "hold_or_toggle",
      keyboard_implementation: "tauri",
      bindings: {
        transcribe: {
          id: "transcribe",
          name: "Transcribe",
          description: "",
          default_binding: "option+space",
          current_binding: "option+space",
        },
        meeting: {
          id: "meeting",
          name: "Meeting",
          description: "",
          default_binding: "option+m",
          current_binding: "option+m",
        },
        cancel: {
          id: "cancel",
          name: "Cancel",
          description: "",
          default_binding: "escape",
          current_binding: "escape",
        },
      },
      meetings: {
        notes_dir: null,
        user_name: "Du",
        diarization_enabled: true,
        glossary: ["Sokki", "Parakeet", "Obsidian", "Tauri"],
        detection_enabled: true,
        footer_enabled: true,
        phone_link_enabled: false,
        call_names_enabled: true,
        // On by default since 2026-11-25 (meeting/mail.rs).
        mail_notes_auto: true,
        mail_notes_auto_set: false,
        silence_hint_enabled: true,
      },
      videos: { enabled: true, display: null },
      custom_words: ["Sokki", "Parakeet", "Obsidian", "Tauri"],
    },
    meetings,
    note: {
      summary:
        "- Release 1.4 geht am Donnerstag raus, ohne Export.\n- Lea klärt bis Mittwoch die Preise mit dem Vertrieb.\n- Nächste Woche testen fünf Nutzer das Onboarding.",
      transcript:
        "[00:00] Lea: Gut, dann fangen wir an. Erstes Thema ist das Release.\n[00:06] Jonas: Ich würde den Export lieber auf 1.5 schieben.\n[00:12] Du: Einverstanden. Dann geht 1.4 am Donnerstag raus.\n[00:19] Lea: Und die Preise kläre ich bis Mittwoch mit dem Vertrieb.",
    },
    stats: {
      improve: true,
      support: false,
      buyers: true,
      shared: false,
      buyer: null,
      clips: s.empty ? 0 : 412,
      folder: "/tmp/voice",
      total_seconds: s.empty ? 0 : 31 * 3600 + 900,
      bytes: s.empty ? 0 : 2.4e9,
      seconds_by_language: s.empty ? {} : { de: 80000, en: 32000 },
      seconds_by_source: s.empty ? {} : { meeting: 98000, dictation: 14000 },
      donated_clips: 0,
      donated_seconds: 0,
    },
    totals: {
      seconds: s.empty ? 0 : 31 * 3600 + 900,
      words: s.empty ? 0 : 12480,
      meetings: s.empty ? 0 : 36,
    },
    history: s.empty
      ? []
      : [
          {
            id: 3,
            file_name: "a.wav",
            timestamp: Math.floor(Date.now() / 1000) - 600,
            saved: false,
            title: "",
            transcription_text:
              "Kannst du mir bis morgen die Zahlen für November schicken? Dann baue ich die Folien fertig.",
            post_processed_text: null,
            post_process_prompt: null,
            post_process_requested: false,
            source_name: null,
            language: "de",
            language_source: null,
            meeting_id: null,
            meeting_track: null,
          },
          {
            id: 2,
            file_name: "b.wav",
            timestamp: Math.floor(Date.now() / 1000) - 7200,
            saved: false,
            title: "",
            transcription_text:
              "Danke für das Gespräch heute, ich melde mich bis Freitag mit einem Vorschlag.",
            post_processed_text: null,
            post_process_prompt: null,
            post_process_requested: false,
            source_name: null,
            language: "de",
            language_source: null,
            meeting_id: null,
            meeting_track: null,
          },
        ],
  };
}

export async function open(page: Page, s: Scenario) {
  page.on("pageerror", (e) =>
    console.log("PAGEERROR", e.message.slice(0, 400)),
  );
  page.on(
    "console",
    (m) =>
      m.type() === "error" && console.log("CONSOLE", m.text().slice(0, 400)),
  );
  const data = fixtures(s);
  await page.addInitScript(
    ({ data, s }) => {
      const w = window as any;
      try {
        // A first run starts with nothing stored, once: a reload keeps
        // what the page stored since.
        if (s.firstRun) {
          if (!sessionStorage.getItem("cleared")) {
            localStorage.clear();
            sessionStorage.setItem("cleared", "1");
          }
        } else {
          localStorage.setItem("sokki.tip.dictations", "1");
          if (s.section) localStorage.setItem("sokki.section", s.section);
        }
        if (s.setup?.saved && !sessionStorage.getItem("seeded")) {
          localStorage.setItem(
            "sokki.onboarding",
            JSON.stringify(s.setup.saved),
          );
          sessionStorage.setItem("seeded", "1");
        }
      } catch {}
      // Every command the page sends, in order (no event plumbing).
      w.__calls = [] as string[];
      w.__args = [] as [string, unknown][];
      const perm = { ...(s.perms ?? { mic: true, acc: true, scr: true }) };
      w.__TAURI_OS_PLUGIN_INTERNALS__ = {
        platform: "macos",
        os_type: "macos",
        family: "unix",
        version: "26.0",
        arch: "aarch64",
        eol: "\n",
        exe_extension: "",
      };
      let nextId = 1;
      const listeners: Record<string, number[]> = {};
      w.__emit = (event: string, payload: unknown) => {
        for (const h of listeners[event] ?? [])
          w[`_${h}`]?.({ event, id: h, payload });
      };
      const answers: Record<string, (a: any) => unknown> = {
        get_app_settings: () => data.settings,
        get_retired_models: () => [],
        list_meetings: () => data.meetings,
        is_meeting_recording: () =>
          data.meetings.some((m: any) => m.status.kind === "recording"),
        get_meeting_note: () => data.note,
        voice_data_stats: () => data.stats,
        voice_data_community: () => ({ donors: 0, notice: null }),
        usage_totals: () => data.totals,
        get_history_entries: () => ({ entries: data.history, has_more: false }),
        meeting_llm_status: () => ({ state: "ready" }),
        list_voice_profiles: () => [],
        list_voices: () => ({
          voices: data.voices,
          once: data.voices.length ? 21 : 0,
        }),
        meeting_speaker_proposals: () => [],
        meeting_voice_offer: () => [],
        voice_offer_done: () => null,
        name_voice: (a) => (a.members as unknown[]).length + 1,
        set_person_emails: () => null,
        // A tenth of a second of silence.
        speaker_clip: () =>
          "data:audio/wav;base64,UklGRkQGAABXQVZFZm10IBAAAAABAAEAgD4AAAB9AAACABAAZGF0YSAGAAA=",
        delete_voice_profile: () => null,
        get_available_models: () => [],
        get_active_notices: () => [],
        get_available_microphones: () => [],
        get_available_output_devices: () => [],
        switch_meeting_microphone: () => null,
        get_current_model: () => "parakeet",
        "plugin:macos-permissions|check_accessibility_permission": () =>
          perm.acc,
        "plugin:macos-permissions|check_microphone_permission": () => perm.mic,
        "plugin:macos-permissions|request_accessibility_permission": () => {
          perm.acc = true;
        },
        "plugin:macos-permissions|request_microphone_permission": () => {
          perm.mic = true;
        },
        screen_permission: () => perm.scr,
        request_screen_permission: () => perm.scr,
        request_system_audio_permission: () => null,
        video_list: () => [],
        is_supporter_code: () => false,
        // Notes to everyone (meeting/mail.rs): nothing to send, no Gmail.
        meeting_mail_preview: () => ({
          event: null,
          to: [],
          skipped: [],
          subject: null,
          body: null,
          problem: "no_event",
          mailed: null,
          sending: false,
        }),
        mail_objections: () => [],
        mail_setup: () => ({ calendar: "allowed", accounts: [], error: null }),
        gmail_status: () => ({ available: false, connected: null }),
        sokki_line: (a) =>
          a.german
            ? "Von KI zusammengefasst. Lokal, kostenlos, Open Source. Diktier-App und Meeting-Notetaker: sokki.org"
            : "Summarized by AI. Local, free, open source. Dictation app and meeting notetaker: sokki.org",
        "plugin:app|version": () => "0.0.0-test",
        "plugin:os|locale": () => s.lang,
        "plugin:event|listen": (a) => {
          (listeners[a.event] ??= []).push(a.handler);
          return a.handler;
        },
      };
      w.__TAURI_INTERNALS__ = {
        metadata: {
          currentWindow: { label: "main" },
          currentWebview: { windowLabel: "main", label: "main" },
        },
        transformCallback: (cb: (x: unknown) => void, once = false) => {
          const id = nextId++;
          w[`_${id}`] = (x: unknown) => {
            if (once) delete w[`_${id}`];
            cb?.(x);
          };
          return id;
        },
        unregisterCallback: (id: number) => delete w[`_${id}`],
        convertFileSrc: (p: string) => p,
        invoke: async (cmd: string, args: any) => {
          if (!cmd.startsWith("plugin:event|")) {
            w.__calls.push(cmd);
            w.__args.push([cmd, args]);
          }
          if (s.extra && cmd in s.extra) {
            const v = s.extra[cmd] as any;
            if (v && typeof v === "object" && "__throw" in v) throw v.__throw;
            return v;
          }
          const f = answers[cmd];
          if (!f && w.__logUnknown) console.log("UNKNOWN " + cmd);
          return f ? f(args ?? {}) : null;
        },
      };
      w.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => {} };
    },
    { data, s },
  );
  await page.emulateMedia({ colorScheme: s.theme });
  await page.setViewportSize({ width: 1120, height: 720 });
  await page.goto(s.path ?? "/");
}
