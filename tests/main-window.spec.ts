import { test, expect, type Page } from "@playwright/test";

/**
 * The main window (design C, 2026-11-21) in a browser with a fake app
 * behind it: every Tauri call answers from the fixtures below. Nothing
 * starts Sokki and nothing opens on screen (headless Chromium).
 * Screenshots land in test-results/main-window/ (or $SHOT_DIR).
 *
 * The meeting titles, names and texts are invented filler.
 */

const SHOTS = process.env.SHOT_DIR ?? "test-results/main-window";

type Scenario = {
  lang: "de" | "en";
  theme: "dark" | "light";
  empty?: boolean;
  recording?: boolean;
  firstRun?: boolean;
  section?: string;
  /** More fake answers, by command name. */
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
  return {
    settings: {
      onboarding_completed: true,
      app_language: s.lang,
      theme: s.theme,
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
      } catch {}
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
        get_available_models: () => [],
        get_active_notices: () => [],
        get_available_microphones: () => [],
        get_available_output_devices: () => [],
        get_current_model: () => "parakeet",
        "plugin:macos-permissions|check_accessibility_permission": () => true,
        "plugin:macos-permissions|check_microphone_permission": () => true,
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
          if (s.extra && cmd in s.extra) return s.extra[cmd];
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
  await page.goto("/");
}

async function shot(page: Page, name: string) {
  // Let hover and colour transitions settle.
  await page.waitForTimeout(300);
  await page.screenshot({ path: `${SHOTS}/${name}.png` });
}

test.describe("main window", () => {
  test("first launch opens Your data (F6): all-time tiles and the latest meeting", async ({
    page,
  }) => {
    await open(page, { lang: "de", theme: "dark" });
    await expect(
      page.getByRole("heading", { level: 1, name: "Deine Daten" }),
    ).toBeVisible();
    await expect(page.getByText("Alles auf diesem Mac, 2,4 GB.")).toBeVisible();
    await expect(page.getByText("Stunden mitgeschrieben")).toBeVisible();
    await expect(page.getByText("31", { exact: true })).toBeVisible();
    await expect(page.getByText("12.480")).toBeVisible();
    await expect(page.getByText("Wörter diktiert")).toBeVisible();
    await expect(
      page.getByRole("button", { name: "Zusammenfassung kopieren" }),
    ).toBeVisible();
    // "Nach Sprache" and the old message card are gone.
    await expect(page.getByText("Nach Sprache")).toBeHidden();
    await expect(page.getByText("Wochenplanung Produkt")).toBeVisible();
    await expect(
      page.getByText(
        "Du teilst Fehler und Nutzung, unter einer zufälligen ID. Keine Aufnahmen.",
      ),
    ).toBeVisible();
    await expect(page.getByText("Nimmt nicht auf")).toBeVisible();
    await expect(page.getByText("⌥M", { exact: true })).toBeVisible();
    const nav = page.getByRole("navigation");
    await expect(nav.getByRole("button").first()).toHaveText("Deine Daten");
    await expect(nav.getByText("Diktate")).toBeVisible();
    await shot(page, "f6-deine-daten-dark");
  });

  test("Your data in light mode", async ({ page }) => {
    await open(page, { lang: "de", theme: "light" });
    await expect(page.getByText("Stunden mitgeschrieben")).toBeVisible();
    await shot(page, "f6-deine-daten-light");
  });

  test("English", async ({ page }) => {
    await open(page, { lang: "en", theme: "dark" });
    await expect(page.getByText("hours transcribed")).toBeVisible();
    await expect(page.getByText("Not recording")).toBeVisible();
    await shot(page, "f6-your-data-en");
  });

  test("the dictations tip shows once after the update", async ({ page }) => {
    await open(page, { lang: "de", theme: "dark", firstRun: true });
    const tip = page.getByText("Deine Diktate findest du jetzt unter Diktate.");
    await expect(tip).toBeVisible();
    await shot(page, "tip-diktate");
    await page.getByRole("button", { name: "OK" }).click();
    await expect(tip).toBeHidden();
    await page.reload();
    await expect(page.getByText("Stunden mitgeschrieben")).toBeVisible();
    await expect(tip).toBeHidden();
  });

  test("empty Mac shows the two ways (E1) with the real shortcuts", async ({
    page,
  }) => {
    await open(page, {
      lang: "de",
      theme: "dark",
      empty: true,
      firstRun: true,
    });
    await expect(page.getByText("Noch nichts mitgeschrieben.")).toBeVisible();
    await expect(page.getByText("Zwei Wege")).toBeVisible();
    await expect(page.getByText("Leertaste")).toBeVisible();
    await expect(
      page.getByText(
        "Du teilst Fehler und Nutzung, unter einer zufälligen ID. Keine Aufnahmen.",
      ),
    ).toBeVisible();
    // No tip for someone without dictations.
    await expect(
      page.getByText("Deine Diktate findest du jetzt unter Diktate."),
    ).toBeHidden();
    await shot(page, "e1-leer");
    await page.getByRole("button", { name: "Ändern" }).nth(1).click();
    await expect(page.getByText("Tasten drücken …")).toBeVisible();
    await shot(page, "e1-leer-aendern");
    await page.keyboard.press("Escape");
    await expect(page.getByText("Tasten drücken …")).toBeHidden();
  });

  test("Meetings is a split view (C1) and reopens where you left off", async ({
    page,
  }) => {
    await open(page, { lang: "de", theme: "dark" });
    await page.getByRole("navigation").getByText("Meetings").click();
    await expect(page.getByRole("heading", { name: "Heute" })).toBeVisible();
    await expect(page.getByRole("heading", { name: "Gestern" })).toBeVisible();
    await expect(page.getByText("[00:06]").first()).toBeVisible();
    await expect(
      page.getByRole("button", { name: "Transkript kopieren" }),
    ).toBeVisible();
    await shot(page, "c1-meetings");
    await page.getByRole("button", { name: "Mehr", exact: true }).click();
    await expect(
      page.getByRole("menuitem", { name: "Im Finder zeigen" }),
    ).toBeVisible();
    await shot(page, "c1-meetings-mehr");
    await page.reload();
    await expect(page.getByRole("heading", { name: "Heute" })).toBeVisible();
  });

  test("a click on the latest meeting's row shows it in Meetings", async ({
    page,
  }) => {
    await open(page, { lang: "de", theme: "dark" });
    await page.getByText("Wochenplanung Produkt").click();
    await expect(
      page.getByRole("heading", { level: 1, name: "Wochenplanung Produkt" }),
    ).toBeVisible();
  });

  test("Change on the sharing line opens Settings at Sharing", async ({
    page,
  }) => {
    await open(page, { lang: "de", theme: "dark" });
    await page.getByRole("button", { name: "Ändern" }).click();
    await expect(
      page.getByRole("heading", { level: 1, name: "Einstellungen" }),
    ).toBeVisible();
    await expect(page.getByText("Hilf, Sokki zu verbessern")).toBeInViewport();
  });

  test("the gear opens the meeting settings", async ({ page }) => {
    await open(page, { lang: "de", theme: "dark", section: "meetings" });
    await page.getByRole("button", { name: "Meeting-Einstellungen" }).click();
    await expect(page.getByText("Notizordner")).toBeVisible();
  });

  test("the start menu opens upward with three items", async ({ page }) => {
    await open(page, { lang: "de", theme: "dark", section: "history" });
    await expect(page.getByRole("heading", { name: "Diktate" })).toBeVisible();
    await expect(
      page.getByText("Was du in anderen Apps diktiert hast."),
    ).toBeVisible();
    await page.getByRole("button", { name: "Weitere Aufnahmen" }).click();
    const items = page.getByRole("menuitem");
    await expect(items).toHaveText([
      "Meeting mit Bildschirm",
      "Video aufnehmen",
      "Datei transkribieren …⌘O",
    ]);
    await shot(page, "a2-diktate-menue");
  });

  test("recording: red bar, timer, levels, and the no-sound warning (C2, D1)", async ({
    page,
  }) => {
    await page.clock.install();
    await open(page, {
      lang: "de",
      theme: "dark",
      recording: true,
      section: "meetings",
    });
    await expect(page.getByText("Aufnahme läuft · Zoom")).toBeVisible();
    await expect(page.getByRole("button", { name: "Beenden" })).toBeVisible();
    await expect(page.getByRole("status").getByText("Mikrofon")).toBeVisible();
    // Both sides speak: meters move, no warning.
    for (let i = 0; i < 5; i++) {
      await page.evaluate(() =>
        (window as any).__emit("meeting-level", { mic: 0.05, system: 0.04 }),
      );
      await page.clock.runFor(100);
    }
    await expect(page.getByText("Von Zoom kommt kein Ton.")).toBeHidden();
    await shot(page, "c2-aufnahme");
    // Only the microphone for 21 s: the warning appears.
    for (let i = 0; i < 22; i++) {
      await page.evaluate(() =>
        (window as any).__emit("meeting-level", { mic: 0.05, system: 0 }),
      );
      await page.clock.runFor(1000);
    }
    await expect(page.getByText("Von Zoom kommt kein Ton.")).toBeVisible();
    await shot(page, "d1-kein-ton");
  });

  test("an important message is a window the user answers (D)", async ({
    page,
  }) => {
    const message = {
      id: 7,
      title: "Bitte aktualisiere Sokki.",
      text: "In 0.2.8 konnten sehr lange Meetings beim Verarbeiten verloren gehen. 0.2.10 behebt das. Deine Aufnahmen bleiben erhalten.",
      action_label: "Jetzt aktualisieren",
      action_url: null,
      action_update: true,
    };
    await open(page, {
      lang: "de",
      theme: "dark",
      extra: { pending_message: message },
    });
    const dialog = page.getByRole("alertdialog");
    await expect(dialog).toBeVisible();
    await expect(dialog.getByText("Bitte aktualisiere Sokki.")).toBeVisible();
    await expect(
      dialog.getByRole("button", { name: "Jetzt aktualisieren" }),
    ).toBeFocused();
    await shot(page, "d-nachricht");
    await dialog.getByRole("button", { name: "Später" }).click();
    await expect(dialog).toBeHidden();
  });

  test("no message window while a meeting records", async ({ page }) => {
    await open(page, {
      lang: "de",
      theme: "dark",
      recording: true,
      extra: {
        pending_message: {
          id: 7,
          title: "Bitte aktualisiere Sokki.",
          text: "…",
          action_label: null,
          action_url: null,
          action_update: false,
        },
      },
    });
    await expect(page.getByRole("button", { name: "Beenden" })).toBeVisible();
    await expect(page.getByRole("alertdialog")).toBeHidden();
  });
});
