import { test, expect, type Page } from "@playwright/test";

/**
 * The first meeting note without hunting for Models → Language model:
 * setup's summary-model screen, and the sample meeting on an empty
 * Meetings page. A browser with a fake app behind it (every Tauri call
 * answers from the fixtures below); nothing starts Sokki, nothing opens on
 * screen. Screenshots land in test-results/meeting-setup/ (or $SHOT_DIR).
 *
 * The people in the sample (Maya, Daniel, Priya) are invented.
 */

const SHOTS = process.env.SHOT_DIR ?? "test-results/meeting-setup";
const GiB = 2 ** 30;

type Scenario = {
  lang?: "de" | "en";
  theme?: "dark" | "light";
  /** Start in setup on this step (with these uses). */
  onboarding?: { step: string; meetings?: boolean };
  /** The Mac's memory in bytes. */
  memory?: number;
  /** The built-in model: "ready", "missing" or "unavailable". */
  builtin?: string;
  /** A download already running. */
  download?: { completed: number; total: number } | null;
  /** The selected summary model's state for the Meetings page. */
  llm?: string;
  meetings?: unknown[];
  /** The summary model set before setup ("local": Ollama, switched off). */
  provider?: string;
  /** Gmail (`gmail_status`); left out, the command doesn't exist. */
  gmail?: { available: boolean; connected: string | null };
  /** The Mac's region locale, e.g. "en-DE". */
  locale?: string;
};

const SAMPLE_LINES = [
  "Okay, I think we're all here. Thanks for joining, both of you. This is our last check-in before the checkout launch, so let's keep it short.",
  "Sounds good, Maya. Quick update from my side: the new checkout works on every phone we tested, except one thing. Apple Pay still fails on older iPhones.",
  "How long would it take to fix that? Honestly, at least a week. Apple has to review the change, and that's out of our hands.",
  "Then I'd launch without Apple Pay. Card payments work fine, and we can add Apple Pay in the next release.",
];

const SAMPLE_NOTE = {
  summary:
    "The team agreed to launch the new checkout on Thursday with card payments only; Apple Pay follows in the next release.\n\n## Decisions\n- Launch on Thursday without Apple Pay.\n\n## Action items\n- **Daniel**: fix the address field (apartment number) tomorrow and hide the Apple Pay button.\n- **Priya**: finish the empty-cart screen by Wednesday morning.\n- **Maya**: email the twenty beta testers on Friday, including a question about the empty cart.",
  transcript:
    "[00:00] Speaker 1: Okay, I think we're all here. Thanks for joining, both of you.\n[00:09] Speaker 2: Sounds good. Quick update from my side: the new checkout works on every phone we tested, except one thing.\n[00:18] Speaker 1: How long would it take to fix that?\n[00:20] Speaker 2: Honestly, at least a week.\n[00:25] Speaker 3: Then I'd launch without Apple Pay.",
};

async function open(page: Page, s: Scenario) {
  page.on("pageerror", (e) =>
    console.log("PAGEERROR", e.message.slice(0, 400)),
  );
  await page.addInitScript(
    ({ s, SAMPLE_LINES, SAMPLE_NOTE }) => {
      const w = window as any;
      try {
        if (!sessionStorage.getItem("seeded")) {
          localStorage.clear();
          sessionStorage.setItem("seeded", "1");
          localStorage.setItem("sokki.tip.dictations", "1");
          if (s.onboarding)
            localStorage.setItem(
              "sokki.onboarding",
              JSON.stringify({
                step: s.onboarding.step,
                uses: {
                  dictation: true,
                  meetings: s.onboarding.meetings ?? true,
                },
                systemSoundAsked: true,
              }),
            );
          else localStorage.setItem("sokki.section", "meetings");
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
      const calls: { cmd: string; args: any }[] = [];
      w.__calls = calls;
      let nextId = 1;
      const listeners: Record<string, number[]> = {};
      w.__emit = (event: string, payload: unknown) => {
        for (const h of listeners[event] ?? [])
          w[`_${h}`]?.({ event, id: h, payload });
      };
      const meetingSettings = {
        notes_dir: null,
        user_name: "Me",
        diarization_enabled: true,
        glossary: [],
        detection_enabled: true,
        footer_enabled: true,
        phone_link_enabled: false,
        chat_notice_enabled: false,
        summary_enabled: !s.provider,
        llm_provider: s.provider ?? "builtin",
        cloud_model: "",
        llm_base_url: "http://localhost:11434",
        llm_model: "qwen3.5:9b",
        keep_audio_days: 7,
      };
      const state = {
        builtin: s.builtin ?? "missing",
        download: s.download
          ? { status: "downloading", ...s.download }
          : (null as any),
        meetings: (s.meetings ?? []) as any[],
        live: [] as { start_s: number; text: string }[],
      };
      w.__state = state;
      const settings = {
        onboarding_completed: !s.onboarding,
        app_language: s.lang ?? "en",
        theme: s.theme ?? "dark",
        debug_mode: false,
        selected_model: "parakeet",
        shortcut_activation: "hold_or_toggle",
        keyboard_implementation: "tauri",
        spoken_languages: ["en"],
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
        },
        meetings: meetingSettings,
        post_process_api_keys: {},
        custom_words: [],
      };
      const answers: Record<string, (a: any) => unknown> = {
        get_app_settings: () => settings,
        get_retired_models: () => [],
        set_meeting_settings: (a) => {
          Object.assign(meetingSettings, a.settings);
          return null;
        },
        summary_model_setup: () => ({
          memory_bytes: s.memory ?? 16 * 2 ** 30,
          builtin: state.builtin,
          download: state.download,
        }),
        pull_meeting_llm: () => {
          state.download = { status: "downloading", completed: 0, total: 0 };
          // Never ends on its own in the test.
          return new Promise(() => {});
        },
        meeting_llm_status: () => ({ state: s.llm ?? "ready", detail: "" }),
        // A copy, as the real app's answer is: a new list each time.
        list_meetings: () => structuredClone(state.meetings),
        is_meeting_recording: () =>
          state.meetings.some((m: any) => m.status.kind === "recording"),
        start_sample_meeting: () => {
          state.meetings = [
            {
              id: "sample",
              started_at: new Date(Date.now() - 42_000).toISOString(),
              duration_s: 0,
              status: { kind: "recording" },
              tracks: [],
              source_app: "Sample",
              title: null,
              note_path: null,
              speakers: [],
              warnings: [],
              summary_missing: null,
              audio_deleted: false,
              online: null,
              sample: true,
            },
          ];
          state.live = SAMPLE_LINES.slice(0, 2).map((text, i) => ({
            start_s: [0.6, 9.4][i],
            text,
          }));
          return "sample";
        },
        stop_sample_meeting: () => {
          state.meetings = [];
          return null;
        },
        meeting_live_transcript: () => state.live,
        ...(s.gmail
          ? {
              gmail_status: () => s.gmail,
              gmail_connect: () => "maya@example.com",
            }
          : {}),
        date_format_locale: () => s.locale ?? "en-US",
        get_meeting_note: () => SAMPLE_NOTE,
        voice_data_stats: () => ({
          improve: false,
          support: false,
          buyers: false,
          shared: false,
          clips: 0,
          total_seconds: 0,
          seconds_by_language: {},
          seconds_by_source: {},
          bytes: 0,
          folder: "/tmp/voice",
        }),
        voice_data_community: () => ({ donors: 0, notice: null }),
        usage_totals: () => ({ seconds: 0, words: 0, meetings: 0 }),
        get_history_entries: () => ({ entries: [], has_more: false }),
        list_voice_profiles: () => [],
        get_available_models: () => [],
        get_active_notices: () => [],
        get_available_microphones: () => [],
        get_available_output_devices: () => [],
        get_current_model: () => "parakeet",
        screen_permission: () => true,
        "plugin:macos-permissions|check_accessibility_permission": () => true,
        "plugin:macos-permissions|check_microphone_permission": () => true,
        "plugin:app|version": () => "0.0.0-test",
        "plugin:os|locale": () => s.lang ?? "en",
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
          calls.push({ cmd, args });
          const f = answers[cmd];
          return f ? f(args ?? {}) : null;
        },
      };
      w.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => {} };
    },
    { s, SAMPLE_LINES, SAMPLE_NOTE },
  );
  await page.emulateMedia({ colorScheme: s.theme ?? "dark" });
  await page.setViewportSize({ width: 1120, height: 720 });
  await page.goto("/");
}

async function shot(page: Page, name: string) {
  await page.waitForTimeout(300);
  await page.screenshot({ path: `${SHOTS}/${name}.png` });
}

const called = (page: Page, cmd: string) =>
  page.evaluate(
    (cmd) => (window as any).__calls.filter((c: any) => c.cmd === cmd),
    cmd,
  );

test.describe("setup: the summary model", () => {
  test("a 16 GB Mac is offered the model with its size and memory, and setup goes on while it downloads", async ({
    page,
  }) => {
    await open(page, { onboarding: { step: "summary" }, memory: 16 * GiB });
    await expect(
      page.getByRole("heading", { name: "Notes for your meetings" }),
    ).toBeVisible();
    await expect(
      page.getByText("Needs about 5.6 GB. Your Mac has 16 GB."),
    ).toBeVisible();
    await expect(
      page.getByText("About 3 minutes after you hang up"),
    ).toBeVisible();
    await expect(page.getByText("Gemma 4 E4B")).toBeVisible();
    await shot(page, "setup-summary-16gb");
    await page.getByRole("button", { name: "Download 5.1 GB" }).click();
    // The next screen, at once; the download runs in the background.
    await expect(
      page.getByRole("heading", { name: "Your data" }),
    ).toBeVisible();
    expect(await called(page, "pull_meeting_llm")).toHaveLength(1);
    const saved = await called(page, "set_meeting_settings");
    // Already the built-in model and on: nothing to change.
    expect(saved).toHaveLength(0);
    await page.getByRole("button", { name: "Back" }).click();
    await expect(page.getByText("Downloads in the background")).toBeVisible();
    await page.evaluate(() =>
      (window as any).__emit("meeting-llm-pull", {
        status: "downloading",
        completed: 2.4e9,
        total: 5_075_824_832,
      }),
    );
    await expect(page.getByText("47% · 2.4 GB of 5.1 GB")).toBeVisible();
    await expect(page.getByRole("button", { name: "Continue" })).toBeVisible();
    await shot(page, "setup-summary-downloading");
  });

  test("a download picks the built-in model when another one was set", async ({
    page,
  }) => {
    await open(page, { onboarding: { step: "summary" }, provider: "local" });
    await page.getByRole("button", { name: "Download 5.1 GB" }).click();
    await expect(
      page.getByRole("heading", { name: "Your data" }),
    ).toBeVisible();
    const saved = await called(page, "set_meeting_settings");
    expect(saved.at(-1).args.settings).toMatchObject({
      llm_provider: "builtin",
      summary_enabled: true,
    });
  });

  test("an 8 GB Mac is told plainly and pointed to Ollama or an own key", async ({
    page,
  }) => {
    await open(page, { onboarding: { step: "summary" }, memory: 8 * GiB });
    await expect(
      page.getByText("Needs about 5.6 GB. Your Mac has 8 GB."),
    ).toBeVisible();
    await expect(
      page.getByText(
        "With 8 GB, that leaves too little for macOS and your apps. These work instead:",
      ),
    ).toBeVisible();
    await expect(page.getByText("Ollama", { exact: true })).toBeVisible();
    await expect(
      page.getByText("Claude or GPT. The transcript goes to that provider."),
    ).toBeVisible();
    await expect(
      page.getByRole("button", { name: "Download anyway" }),
    ).toBeVisible();
    await shot(page, "setup-summary-8gb");
    await page.getByRole("button", { name: "Continue" }).click();
    await expect(
      page.getByRole("heading", { name: "Your data" }),
    ).toBeVisible();
    expect(await called(page, "pull_meeting_llm")).toHaveLength(0);
  });

  test("a model already on this Mac just continues", async ({ page }) => {
    await open(page, { onboarding: { step: "summary" }, builtin: "ready" });
    await expect(page.getByText("On this Mac", { exact: true })).toBeVisible();
    await expect(page.getByRole("button", { name: /Download/ })).toHaveCount(0);
  });

  test("without Meetings, setup has no summary screen", async ({ page }) => {
    await open(page, { onboarding: { step: "summary", meetings: false } });
    await expect(page.locator("[data-onboarding]")).toBeVisible();
    await expect(
      page.getByRole("heading", { name: "Notes for your meetings" }),
    ).toHaveCount(0);
  });

  test("German, light", async ({ page }) => {
    await open(page, {
      lang: "de",
      theme: "light",
      onboarding: { step: "summary" },
      memory: 16 * GiB,
    });
    await expect(
      page.getByText("Braucht etwa 5,6 GB. Dein Mac hat 16 GB."),
    ).toBeVisible();
    await expect(
      page.getByText("Etwa 3 Minuten nach dem Auflegen"),
    ).toBeVisible();
    await shot(page, "setup-summary-de-light");
  });
});

test.describe("the sample meeting", () => {
  test("an empty Meetings page offers it; it plays with a live transcript, then shows the note", async ({
    page,
  }) => {
    await open(page, { builtin: "ready" });
    await expect(
      page.getByRole("heading", { name: "No meeting today? Hear a sample." }),
    ).toBeVisible();
    await expect(
      page.getByText("Your notes arrive about 3 minutes after you hang up."),
    ).toBeVisible();
    await shot(page, "sample-card");
    await page.getByRole("button", { name: "Play sample meeting" }).click();
    expect(await called(page, "start_sample_meeting")).toHaveLength(1);
    await expect(page.getByText(/Sample call · 0:4\d of 2:00/)).toBeVisible();
    await expect(page.getByText(SAMPLE_LINES[1])).toBeVisible();
    await expect(
      page.getByText("SAMPLE", { exact: false }).first(),
    ).toBeVisible();
    await shot(page, "sample-playing");

    // The call ends: processing, then the note.
    await page.evaluate(() => {
      const st = (window as any).__state;
      st.meetings[0] = {
        ...st.meetings[0],
        duration_s: 121,
        status: { kind: "processing", stage: "diarizing", progress: null },
      };
      (window as any).__emit("meeting-changed", { id: "sample" });
    });
    await expect(page.getByText("Finding speakers…")).toBeVisible();
    await expect(
      page.getByText("The sample's notes arrive in about 2 minutes."),
    ).toBeVisible();
    await shot(page, "sample-processing");
    await page.evaluate(() => {
      const st = (window as any).__state;
      st.meetings[0] = {
        ...st.meetings[0],
        status: { kind: "done" },
        title: "Checkout launch check-in",
        note_path: "/tmp/sample.md",
        speakers: ["Speaker 1", "Speaker 2", "Speaker 3"],
        online: true,
      };
      (window as any).__emit("meeting-changed", { id: "sample" });
    });
    await expect(
      page.getByRole("heading", { name: "Checkout launch check-in" }),
    ).toBeVisible();
    await expect(
      page.getByText("Launch on Thursday without Apple Pay."),
    ).toBeVisible();
    await expect(
      page.getByText("Speaker 3", { exact: true }).first(),
    ).toBeVisible();
    await shot(page, "sample-done");
  });

  test("Stop sample deletes it and brings the card back", async ({ page }) => {
    await open(page, { builtin: "ready" });
    await page.getByRole("button", { name: "Play sample meeting" }).click();
    await page.getByRole("button", { name: "Stop sample" }).click();
    expect(await called(page, "stop_sample_meeting")).toHaveLength(1);
    await expect(
      page.getByRole("button", { name: "Play sample meeting" }),
    ).toBeVisible();
  });

  test("while the summary model downloads, the sample waits for it", async ({
    page,
  }) => {
    await open(page, {
      llm: "model_missing",
      download: { completed: 1.7e9, total: 5_075_824_832 },
    });
    await expect(
      page.getByText("Ready once the summary model is in · 33% of 5.1 GB"),
    ).toBeVisible();
    await expect(
      page.getByRole("button", { name: "Play sample meeting" }),
    ).toBeDisabled();
    await shot(page, "sample-waiting");
  });

  test("without a summary model, the card offers the download", async ({
    page,
  }) => {
    await open(page, { llm: "model_missing" });
    await page.getByRole("button", { name: "Download 5.1 GB" }).click();
    expect(await called(page, "pull_meeting_llm")).toHaveLength(1);
  });
});

test.describe("setup: Gmail with Meetings", () => {
  const step = (page: Page) =>
    page.evaluate(
      () => JSON.parse(localStorage.getItem("sokki.onboarding") ?? "{}").step,
    );

  test("Permissions offers Gmail as an optional fifth row", async ({
    page,
  }) => {
    await open(page, {
      onboarding: { step: "permissions" },
      gmail: { available: true, connected: null },
      locale: "en-DE",
    });
    await expect(page.locator("b", { hasText: /^Gmail/ })).toBeVisible();
    await expect(page.getByText("Optional", { exact: true })).toBeVisible();
    await expect(page.getByRole("button", { name: "Connect" })).toBeEnabled();
    await shot(page, "setup-permissions-gmail");
  });

  test("going on without Gmail goes straight on, also in Germany", async ({
    page,
  }) => {
    // The § 201 notice promised a mail before the meeting, which Robin
    // dropped (2026-11-25): no notice any more.
    await open(page, {
      lang: "de",
      theme: "light",
      onboarding: { step: "permissions" },
      gmail: { available: true, connected: null },
      locale: "de-DE",
    });
    await expect(page.locator("b", { hasText: /^Gmail/ })).toBeVisible();
    await expect(
      page.getByText("Schickt allen nach dem Meeting die Notizen."),
    ).toBeVisible();
    await shot(page, "setup-permissions-gmail-de");
    await page.getByRole("button", { name: "Weiter" }).click();
    await expect(page.getByText(/§ 201/)).toHaveCount(0);
    await expect.poll(() => step(page)).toBe("model");
  });

  test("Gmail connected in the row, then on", async ({ page }) => {
    await open(page, {
      onboarding: { step: "permissions" },
      gmail: { available: true, connected: null },
      locale: "en-DE",
    });
    await page.getByRole("button", { name: "Connect" }).click();
    await expect(page.getByText("Connected", { exact: true })).toBeVisible();
    await page.getByRole("button", { name: "Continue" }).click();
    await expect.poll(() => step(page)).toBe("model");
  });

  test("a build without Gmail has no Gmail row", async ({ page }) => {
    await open(page, { onboarding: { step: "permissions" }, locale: "en-DE" });
    await expect(page.locator("[data-onboarding]")).toBeVisible();
    // Every macOS permission is given: with nothing to offer, setup skips
    // the screen.
    await expect(
      page.getByRole("heading", { name: /permissions, then you're set/ }),
    ).toHaveCount(0);
    await expect(page.locator("b", { hasText: /^Gmail/ })).toHaveCount(0);
  });

  test("Gmail asks for sending only, with the short notice", async ({
    page,
  }) => {
    await open(page, {
      onboarding: { step: "permissions" },
      gmail: { available: true, connected: null },
      locale: "en-US",
    });
    await expect(
      page.getByText(
        "Sokki asks Google to send email on your behalf and to see your address. Sokki can't read your mail. Notes go from this Mac straight to Gmail.",
      ),
    ).toBeVisible();
    await page.getByRole("button", { name: "Connect" }).click();
    await expect(page.getByText("Connected", { exact: true })).toBeVisible();
    expect(await called(page, "gmail_connect")).toHaveLength(1);
    await expect(page.getByText(/sender and subject/)).toHaveCount(0);
  });
});
