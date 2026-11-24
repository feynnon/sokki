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
  return {
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
        get_available_models: () => [],
        get_active_notices: () => [],
        get_available_microphones: () => [],
        get_available_output_devices: () => [],
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

  test("the start menu opens upward with two items", async ({ page }) => {
    await open(page, { lang: "de", theme: "dark", section: "history" });
    await expect(page.getByRole("heading", { name: "Diktate" })).toBeVisible();
    await expect(
      page.getByText("Was du in anderen Apps diktiert hast."),
    ).toBeVisible();
    await page.getByRole("button", { name: "Weitere Aufnahmen" }).click();
    const items = page.getByRole("menuitem");
    await expect(items).toHaveText([
      "Meeting mit Bildschirm",
      "Datei transkribieren …⌘O",
    ]);
    await shot(page, "a2-diktate-menue");
  });

  test("Meeting with screen starts Sokki's own call window", async ({
    page,
  }) => {
    await open(page, { lang: "en", theme: "dark", section: "meetings" });
    await page.getByRole("button", { name: "More recordings" }).click();
    await page.getByRole("menuitem", { name: "Meeting with screen" }).click();
    await expect
      .poll(() => page.evaluate(() => (window as any).__calls))
      .toContain("start_meeting_with_screen");
  });

  test("without Screen Recording the bar says how to allow it, on one line each", async ({
    page,
  }) => {
    await open(page, {
      lang: "en",
      theme: "dark",
      section: "meetings",
      extra: { start_meeting_with_screen: { __throw: "screen_not_allowed" } },
    });
    await page.setViewportSize({ width: 820, height: 600 });
    await page.getByRole("button", { name: "More recordings" }).click();
    await page.getByRole("menuitem", { name: "Meeting with screen" }).click();
    await expect(
      page.getByText("Allow Screen Recording for Sokki in System Settings"),
    ).toBeVisible();
    // The button keeps its label on one line.
    const start = page.getByRole("button", { name: /^Start meeting/ });
    expect((await start.boundingBox())!.height).toBeLessThan(40);
    await shot(page, "c1-screen-not-allowed");
  });

  test("a meeting's screen video opens from its … menu", async ({ page }) => {
    await open(page, {
      lang: "en",
      theme: "dark",
      section: "meetings",
      extra: {
        video_list: [
          {
            folder: "/tmp/sokki-videos/m1",
            meeting_id: "m1",
            started_at: "2026-11-24T10:00:00+01:00",
            duration_s: 2040,
            screen: null,
            rendered: "screen.mp4",
            status: "ready",
          },
        ],
      },
    });
    await page.getByRole("button", { name: "More", exact: true }).click();
    await page.getByRole("menuitem", { name: "Screen video" }).click();
    await expect
      .poll(() => page.evaluate(() => (window as any).__calls))
      .toContain("video_reveal");
  });

  test("a meeting without a screen video has no Screen video item", async ({
    page,
  }) => {
    await open(page, { lang: "en", theme: "dark", section: "meetings" });
    await page.getByRole("button", { name: "More", exact: true }).click();
    await expect(
      page.getByRole("menuitem", { name: "Show in Finder" }),
    ).toBeVisible();
    await expect(
      page.getByRole("menuitem", { name: "Screen video" }),
    ).toHaveCount(0);
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

/** First start, with the real setup flow (not the debug preview). */
test.describe("setup", () => {
  const none = { mic: false, acc: false, scr: false };
  const calls = (page: Page) =>
    page.evaluate(() => (window as any).__calls as string[]);
  const row = (page: Page, name: string) =>
    page
      .locator("li, div")
      .filter({ has: page.getByText(name, { exact: true }) })
      .filter({ has: page.getByRole("button") })
      .last();

  test("starts with What for, dark even when macOS is light, no Back", async ({
    page,
  }) => {
    await open(page, { lang: "en", theme: "light", setup: {}, perms: none });
    await expect(
      page.getByRole("heading", { name: "What will you use Sokki for?" }),
    ).toBeVisible();
    await expect(page.getByRole("button", { name: "Back" })).toHaveCount(0);
    const bg = await page.evaluate(() =>
      getComputedStyle(document.documentElement)
        .getPropertyValue("--color-background")
        .trim()
        .toLowerCase(),
    );
    expect(bg).toBe("#17181b");
    await shot(page, "setup-1-purpose-light-mac");
  });

  test("Dictation only: two permissions, each button asks macOS, then on", async ({
    page,
  }) => {
    await open(page, { lang: "en", theme: "dark", setup: {}, perms: none });
    await page.getByRole("button", { name: /^Meetings/ }).click();
    await page.getByRole("button", { name: "Continue" }).click();
    await expect(
      page.getByRole("heading", { name: "Two permissions, then you're set" }),
    ).toBeVisible();
    await expect(page.getByText("System sound", { exact: true })).toHaveCount(
      0,
    );
    await expect(page.getByText("Screen", { exact: true })).toHaveCount(0);
    await page.getByRole("button", { name: "Allow" }).first().click();
    await expect
      .poll(() => calls(page))
      .toContain("plugin:macos-permissions|request_microphone_permission");
    await expect(page.getByText("Allowed").first()).toBeVisible({
      timeout: 5000,
    });
    await page.getByRole("button", { name: "Allow" }).first().click();
    await expect
      .poll(() => calls(page))
      .toContain("plugin:macos-permissions|request_accessibility_permission");
    await expect(page.getByText("Allowed")).toHaveCount(2, { timeout: 5000 });
    await shot(page, "setup-2-dictation-allowed");
    await page.getByRole("button", { name: "Continue" }).click();
    await expect(
      page.getByRole("heading", { name: "Two permissions, then you're set" }),
    ).toHaveCount(0);
    // Dictation needs no Screen: the check stops once both are allowed.
    const n = (await calls(page)).filter(
      (c) => c === "screen_permission",
    ).length;
    await page.waitForTimeout(2500);
    const m = (await calls(page)).filter(
      (c) => c === "screen_permission",
    ).length;
    expect(m - n).toBe(0);
  });

  test("Meetings only: microphone, system sound, screen; Screen needs a relaunch", async ({
    page,
  }) => {
    await open(page, { lang: "en", theme: "dark", setup: {}, perms: none });
    await page.getByRole("button", { name: /^Dictation/ }).click();
    await page.getByRole("button", { name: "Continue" }).click();
    await expect(
      page.getByRole("heading", { name: "Three permissions, then you're set" }),
    ).toBeVisible();
    await expect(page.getByText("Accessibility", { exact: true })).toHaveCount(
      0,
    );
    for (const [i, cmd] of [
      "plugin:macos-permissions|request_microphone_permission",
      "request_system_audio_permission",
      "request_screen_permission",
    ].entries()) {
      await page.getByRole("button", { name: "Allow" }).first().click();
      await expect.poll(() => calls(page)).toContain(cmd);
      // System sound says "Asked": macOS can't tell whether it was allowed.
      if (i < 2)
        await expect(page.getByText(/^(Allowed|Asked)$/)).toHaveCount(i + 1, {
          timeout: 5000,
        });
    }
    // macOS reopens Sokki after Screen: the flow must come back here.
    const saved = await page.evaluate(() =>
      JSON.parse(localStorage.getItem("sokki.onboarding") ?? "null"),
    );
    expect(saved.step).toBe("permissions");
    expect(saved.relaunchPending).toBe(true);
    // Not yet: "Welcome back" belongs to the start after the relaunch.
    await expect(page.getByText("Welcome back.", { exact: false })).toHaveCount(
      0,
    );
    await page.reload();
    await expect(
      page.getByText("Welcome back.", { exact: false }),
    ).toBeVisible();
    expect(saved.uses).toEqual({ dictation: false, meetings: true });
    await shot(page, "setup-2-meetings-screen-waiting");
  });

  test("after macOS reopens Sokki, setup is back on Permissions, all allowed", async ({
    page,
  }) => {
    await open(page, {
      lang: "en",
      theme: "dark",
      setup: {
        saved: {
          step: "permissions",
          uses: { dictation: false, meetings: true },
          systemSoundAsked: true,
          welcomeBack: true,
        },
      },
      perms: { mic: true, acc: false, scr: true },
    });
    await expect(
      page.getByRole("heading", { name: "Three permissions, then you're set" }),
    ).toBeVisible();
    await expect(page.getByText(/^(Allowed|Asked)$/)).toHaveCount(3);
    await expect(page.getByRole("button", { name: "Continue" })).toBeEnabled();
    await shot(page, "setup-2-welcome-back");
  });

  test("with Dictation, Your data leads to the practice with your shortcut", async ({
    page,
  }) => {
    await open(page, {
      lang: "en",
      theme: "dark",
      setup: {
        saved: { step: "voice", uses: { dictation: true, meetings: true } },
      },
    });
    await expect(
      page.getByRole("heading", { name: "Your data" }),
    ).toBeVisible();
    await page.getByRole("button", { name: "Continue" }).click();
    await expect(
      page.getByRole("heading", { name: "Give it a try!" }),
    ).toBeVisible();
    await expect(page.getByText(/^Hold/)).toBeVisible();
    await shot(page, "setup-5-try");
    await page.getByRole("button", { name: "Skip" }).click();
    await expect(
      page.getByRole("heading", { name: "You're all set" }),
    ).toBeVisible({ timeout: 5000 });
    await shot(page, "setup-6-all-set");
    await page.getByRole("button", { name: "Done" }).click();
    await expect.poll(() => calls(page)).toContain("complete_onboarding");
  });

  test("Meetings only ends at Your data", async ({ page }) => {
    await open(page, {
      lang: "en",
      theme: "dark",
      setup: {
        saved: { step: "voice", uses: { dictation: false, meetings: true } },
      },
    });
    await page.getByRole("button", { name: "Continue" }).click();
    await expect.poll(() => calls(page)).toContain("complete_onboarding");
    await expect(
      page.getByRole("heading", { name: "Give it a try!" }),
    ).toHaveCount(0);
  });
});

/** The small window when a call starts or ends (meeting/prompt.rs), at its
 * real size. */
test.describe("call window", () => {
  const answers = (page: Page) =>
    page.evaluate(() =>
      ((window as any).__args as [string, any][])
        .filter(([c]) => c === "meeting_prompt_answer")
        .map(([, a]) => a.answer),
    );

  for (const lang of ["en", "de"] as const)
    test(`call started, with "Record screen too" (${lang})`, async ({
      page,
    }) => {
      await open(page, {
        lang,
        theme: "dark",
        path: "/src/meeting-prompt/index.html?kind=started&screen=1&app=Zoom",
      });
      await page.setViewportSize({ width: 360, height: 168 });
      await expect(page.getByRole("dialog")).toBeVisible();
      await expect(
        page.getByText("Zoom", { exact: false }).first(),
      ).toBeVisible();
      // Everything fits in the window Rust opens (360 × 168).
      const size = await page.evaluate(() => ({
        w: document.documentElement.scrollWidth,
        h: document.documentElement.scrollHeight,
      }));
      expect(size.w).toBeLessThanOrEqual(360);
      expect(size.h).toBeLessThanOrEqual(168);
      await shot(page, `prompt-started-${lang}`);
      await page.getByRole("switch").click();
      await expect(page.getByRole("switch")).toHaveAttribute(
        "aria-checked",
        "true",
      );
      await page
        .getByRole("button", { name: lang === "en" ? "Record" : "Aufnehmen" })
        .click();
      await expect.poll(() => answers(page)).toEqual(["record_screen"]);
    });

  test("call started, switch off: Record and Not now", async ({ page }) => {
    await open(page, {
      lang: "en",
      theme: "dark",
      path: "/src/meeting-prompt/index.html?kind=started&screen=1&app=Zoom",
    });
    await page.getByRole("button", { name: "Record" }).click();
    await page.getByRole("button", { name: "Not now" }).click();
    await expect.poll(() => answers(page)).toEqual(["record", "not_now"]);
  });

  test("call ended: Keep recording or Stop, in 360 × 124", async ({ page }) => {
    await open(page, {
      lang: "en",
      theme: "dark",
      path: "/src/meeting-prompt/index.html?kind=ended&app=Google%20Meet",
    });
    await page.setViewportSize({ width: 360, height: 124 });
    await expect(page.getByRole("switch")).toHaveCount(0);
    const h = await page.evaluate(() => document.documentElement.scrollHeight);
    expect(h).toBeLessThanOrEqual(124);
    await shot(page, "prompt-ended-en");
    await page.getByRole("button", { name: "Keep recording" }).click();
    await page.getByRole("button", { name: "Stop" }).click();
    await expect.poll(() => answers(page)).toEqual(["keep", "stop"]);
  });
});
