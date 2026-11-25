import { test, expect, type Page } from "@playwright/test";
import { open } from "./fake-app";

/**
 * The main window (design C, 2026-11-21) in a browser with a fake app
 * behind it: every Tauri call answers from the fixtures below. Nothing
 * starts Sokki and nothing opens on screen (headless Chromium).
 * Screenshots land in test-results/main-window/ (or $SHOT_DIR).
 *
 * The meeting titles, names and texts are invented filler.
 */

const SHOTS = process.env.SHOT_DIR ?? "test-results/main-window";

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

  test("recording: red bar, timer, levels, and the silence hints (C2, D1–D3)", async ({
    page,
  }) => {
    await page.clock.install();
    await open(page, {
      lang: "de",
      theme: "dark",
      recording: true,
      section: "meetings",
      extra: {
        get_available_microphones: [
          { index: "default", name: "Default", is_default: true },
          { index: "1", name: "MacBook Pro-Mikrofon", is_default: false },
          { index: "2", name: "AirPods Pro", is_default: false },
        ],
      },
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
    await expect(page.getByText("Sokki hört nichts von Zoom.")).toBeHidden();
    await shot(page, "c2-aufnahme");
    // Rust decides the hint (meeting/silence.rs); the bar shows what it says.
    const level = (hint: string | null, micName = "MacBook Pro-Mikrofon") =>
      page.evaluate(
        ([hint, micName]) =>
          (window as any).__emit("meeting-level", {
            // The call hint: the user speaks, the call track is dead; the
            // mic hint: the call speaks, the microphone brings nothing.
            mic: hint === "call" ? 0.05 : 0,
            system: hint === "call" ? 0 : 0.04,
            system_dead_ms: hint === "call" ? 40_000 : 0,
            hint,
            mic_name: micName,
          }),
        [hint, micName] as const,
      );
    await level("call");
    await expect(page.getByText("Sokki hört nichts von Zoom.")).toBeVisible();
    await shot(page, "d1-kein-ton");
    await level("mic");
    const hint = page.getByTestId("silence-hint");
    await expect(
      hint.getByText(
        "Sokki hört keine Sprache von „MacBook Pro-Mikrofon“. Richtiges Mikrofon?",
      ),
    ).toBeVisible();
    await shot(page, "d2-keine-sprache");
    // Switching: the menu lists the microphones, the choice shows at once.
    await hint.getByRole("button", { name: "Mikrofon wechseln" }).click();
    await expect(
      page.getByRole("menuitem", { name: "AirPods Pro" }),
    ).toBeVisible();
    await shot(page, "d3-mikrofon-wechseln");
    await page.getByRole("menuitem", { name: "AirPods Pro" }).click();
    await expect(
      hint.getByText(
        "Sokki hört keine Sprache von „AirPods Pro“. Richtiges Mikrofon?",
      ),
    ).toBeVisible();
    const args = await page.evaluate(() => (window as any).__args);
    expect(args).toContainEqual([
      "switch_meeting_microphone",
      { name: "AirPods Pro" },
    ]);
    // Speech arrives: Rust drops the hint.
    await level(null, "AirPods Pro");
    await expect(hint).toBeHidden();
    // The X hides a hint for the rest of the meeting.
    await level("call");
    await page.getByRole("button", { name: "Ausblenden" }).click();
    await expect(page.getByText("Sokki hört nichts von Zoom.")).toBeHidden();
    await level("call");
    await page.clock.runFor(1000);
    await expect(page.getByText("Sokki hört nichts von Zoom.")).toBeHidden();
  });

  test("recording: the no-speech hint in English", async ({ page }) => {
    await open(page, {
      lang: "en",
      theme: "dark",
      recording: true,
      section: "meetings",
      extra: {
        get_available_microphones: [
          { index: "default", name: "Default", is_default: true },
          { index: "1", name: "MacBook Pro Microphone", is_default: false },
          { index: "2", name: "AirPods Pro", is_default: false },
        ],
      },
    });
    await expect(page.getByText("Recording · Zoom")).toBeVisible();
    await page.evaluate(() =>
      (window as any).__emit("meeting-level", {
        mic: 0,
        system: 0.04,
        hint: "mic",
        mic_name: "MacBook Pro Microphone",
      }),
    );
    const hint = page.getByTestId("silence-hint");
    await expect(
      hint.getByText(
        "Sokki hears no speech from “MacBook Pro Microphone”. Right microphone?",
      ),
    ).toBeVisible();
    await shot(page, "d2-no-speech-en");
    await hint.getByRole("button", { name: "Switch microphone" }).click();
    await expect(
      page.getByRole("menuitem", { name: "MacBook Pro Microphone" }),
    ).toBeVisible();
    await shot(page, "d3-switch-microphone-en");
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
