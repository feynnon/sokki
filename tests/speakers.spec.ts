import { test, expect, type Page } from "@playwright/test";
import { open } from "./fake-app";

/**
 * Speakers with real names (board card speaker-names): the Sprecher page
 * (design C) and the names Sokki proposes in a meeting. All people are
 * invented.
 */

const SHOTS = process.env.SHOT_DIR ?? "test-results/speakers";

async function shot(page: Page, name: string) {
  await page.waitForTimeout(300);
  await page.screenshot({ path: `${SHOTS}/${name}.png` });
}

const args = (page: Page, cmd: string) =>
  page.evaluate(
    (cmd) =>
      ((window as any).__args as [string, any][])
        .filter(([c]) => c === cmd)
        .map(([, a]) => a),
    cmd,
  );

test.describe("Sprecher page", () => {
  test("a voice without a name: suggestion, what it said, name it", async ({
    page,
  }) => {
    await open(page, { lang: "de", theme: "dark", section: "speakers" });
    await expect(
      page.getByRole("heading", { name: "Sprecher", exact: true }),
    ).toBeVisible();
    await expect(
      page.getByRole("heading", { name: "Stimme A", exact: true }),
    ).toBeVisible();
    await expect(page.getByText("Wer ist das?")).toBeVisible();
    await expect(page.getByText("in 11 von 13 eingeladen")).toBeVisible();
    await expect(page.getByText("Was Stimme A gesagt hat")).toBeVisible();
    // Consent is the person's: never ticked for them.
    const consent = page.getByRole("checkbox");
    await expect(consent).not.toBeChecked();
    await shot(page, "sprecher-stimme-a");

    await page.getByRole("button", { name: "Abspielen" }).first().click();
    const clip = await args(page, "speaker_clip");
    expect(clip[0]).toMatchObject({
      id: "m1",
      speaker: "Speaker 2",
      startS: 312,
    });

    await page.getByRole("button", { name: "Benennen" }).click();
    const named = await args(page, "name_voice");
    expect(named[0].name).toBe("Mara Keller");
    expect(named[0].emails).toEqual(["mara@example.com"]);
    expect(named[0].consent).toBe(false);
    expect(named[0].members).toHaveLength(13);
  });

  test("a named voice: emails, and naming the meetings it's still missing in", async ({
    page,
  }) => {
    await open(page, { lang: "de", theme: "dark", section: "speakers" });
    await page.getByRole("button", { name: /Jonas Weber/ }).click();
    await expect(page.getByText("In 2 Meetings noch „Speaker“")).toBeVisible();
    await page.getByRole("button", { name: "Überall benennen" }).click();
    const named = await args(page, "name_voice");
    expect(named[0].name).toBe("Jonas Weber");

    await page.getByRole("button", { name: /Lea Brandt/ }).click();
    await expect(
      page.getByText("Sokki erkennt diese Stimme wieder."),
    ).toBeVisible();
    const field = page.getByPlaceholder(
      "lea@example.com, lea.privat@example.com",
    );
    await field.fill("lea@example.com, Lea.Privat@example.com");
    await field.blur();
    const set = await args(page, "set_person_emails");
    expect(set.at(-1)).toEqual({
      name: "Lea Brandt",
      emails: ["lea@example.com", "lea.privat@example.com"],
    });
    await shot(page, "sprecher-lea");
  });

  test("English", async ({ page }) => {
    await open(page, { lang: "en", theme: "dark", section: "speakers" });
    await expect(
      page.getByRole("heading", { name: "Voice A", exact: true }),
    ).toBeVisible();
    await shot(page, "speakers-en");
  });
});

test.describe("meeting settings", () => {
  test("names from the call: a switch that says Meet shows captions", async ({
    page,
  }) => {
    await open(page, { lang: "de", theme: "dark", section: "meetings" });
    await page.getByRole("button", { name: "Meeting-Einstellungen" }).click();
    const label = page.getByText("Namen aus dem Call", { exact: true });
    await label.scrollIntoViewIfNeeded();
    await expect(label).toBeVisible();
    await expect(
      page.getByText("schaltet Sokki dafür die Untertitel ein", {
        exact: false,
      }),
    ).toBeVisible();
    await shot(page, "einstellungen-namen-aus-dem-call");
  });
});

test.describe("names in a meeting", () => {
  test("huddle of three: both names proposed, one click", async ({ page }) => {
    const now = new Date().toISOString();
    await open(page, {
      lang: "de",
      theme: "dark",
      section: "meetings",
      extra: {
        list_meetings: [
          {
            id: "h1",
            started_at: now,
            duration_s: 18 * 60,
            status: { kind: "done" },
            tracks: ["mic.flac", "system.flac"],
            source_app: "Slack",
            title: "Launch-Plan Dezember",
            note_path: "/tmp/sokki-notes/h1.md",
            speakers: ["Du", "Speaker 2", "Speaker 3"],
            warnings: [],
            summary_missing: null,
            audio_deleted: false,
            online: true,
          },
        ],
        get_meeting_note: {
          summary: "- Speaker 2 schickt die Folien.",
          transcript:
            "[00:04] Speaker 2: Ich schick die Folien.\n[00:09] Speaker 3: Danke.",
        },
        meeting_speaker_proposals: [
          {
            speaker: "Speaker 2",
            candidates: [
              {
                name: "Lena Vogt",
                email: "lena@example.com",
                source: "invited",
              },
              {
                name: "Jonas Weber",
                email: "jonas@example.com",
                source: "invited",
              },
            ],
          },
          {
            speaker: "Speaker 3",
            candidates: [
              {
                name: "Lena Vogt",
                email: "lena@example.com",
                source: "invited",
              },
              {
                name: "Jonas Weber",
                email: "jonas@example.com",
                source: "invited",
              },
            ],
          },
        ],
      },
    });
    const chips = page.getByRole("button", { name: "Lena oder Jonas?" });
    await expect(chips).toHaveCount(2);
    await expect(
      page.getByText("Klick auf einen Sprecher", { exact: false }),
    ).toBeVisible();
    await shot(page, "huddle-vorschlag");
    await chips.first().click();
    await expect(page.getByText("Speaker 2 ist …")).toBeVisible();
    // Naming asks nothing about the voice; that comes once, at the end.
    await expect(page.getByRole("checkbox")).toHaveCount(0);
    await shot(page, "huddle-auswahl");
    await page.getByRole("button", { name: /^Lena Vogt/ }).click();
    const named = await args(page, "name_voice");
    expect(named[0]).toEqual({
      members: [{ meeting_id: "h1", speaker: "Speaker 2" }],
      name: "Lena Vogt",
      emails: ["lena@example.com"],
      consent: false,
    });
    // Shown at once, before the note catches up.
    await expect(page.getByRole("button", { name: "Lena Vogt" })).toBeVisible();
  });

  test("everyone named: one question whether to keep their voices", async ({
    page,
  }) => {
    await open(page, {
      lang: "de",
      theme: "dark",
      section: "meetings",
      extra: {
        list_meetings: [
          {
            id: "h2",
            started_at: new Date().toISOString(),
            duration_s: 18 * 60,
            status: { kind: "done" },
            tracks: ["mic.flac", "system.flac"],
            source_app: "Slack",
            title: "Launch-Plan Dezember",
            note_path: "/tmp/sokki-notes/h2.md",
            speakers: ["Du", "Lena Vogt", "Jonas Weber"],
            warnings: [],
            summary_missing: null,
            audio_deleted: false,
            online: true,
          },
        ],
        get_meeting_note: {
          summary: "- Lena Vogt schickt die Folien.",
          transcript:
            "[00:04] Lena Vogt: Ich schick die Folien.\n[00:09] Jonas Weber: Danke.",
        },
        meeting_voice_offer: ["Lena Vogt", "Jonas Weber"],
      },
    });
    await expect(page.getByText("Künftig von selbst erkennen?")).toBeVisible();
    const lena = page.getByRole("checkbox", { name: "Lena Vogt" });
    await expect(lena).not.toBeChecked();
    await expect(
      page.getByRole("checkbox", { name: "Jonas Weber" }),
    ).not.toBeChecked();
    const keep = page.getByRole("button", { name: "Merken" });
    await expect(keep).toBeDisabled();
    await shot(page, "huddle-am-ende");
    await lena.check();
    await keep.click();
    await expect(page.getByText("Künftig von selbst erkennen?")).toHaveCount(0);
    await expect
      .poll(async () => (await args(page, "voice_offer_done")).length)
      .toBe(1);
    const kept = await args(page, "name_voice");
    expect(kept).toEqual([
      {
        members: [{ meeting_id: "h2", speaker: "Lena Vogt" }],
        name: "Lena Vogt",
        emails: [],
        consent: true,
      },
    ]);
  });

  test("someone else: a small card at the chip, Benennen at the bottom right", async ({
    page,
  }) => {
    await open(page, {
      lang: "de",
      theme: "dark",
      section: "meetings",
      extra: {
        list_meetings: [
          {
            id: "h3",
            started_at: new Date().toISOString(),
            duration_s: 18 * 60,
            status: { kind: "done" },
            tracks: ["mic.flac", "system.flac"],
            source_app: "Slack",
            title: "Launch-Plan Dezember",
            note_path: "/tmp/sokki-notes/h3.md",
            speakers: ["Du", "Speaker 2", "Jonas Weber"],
            warnings: [],
            summary_missing: null,
            audio_deleted: false,
            online: true,
          },
        ],
        get_meeting_note: {
          summary: "- Speaker 2 schickt die Folien bis Freitag.",
          transcript: "[00:04] Speaker 2: Ich schick die Folien bis Freitag.",
        },
        meeting_speaker_proposals: [
          {
            speaker: "Speaker 2",
            candidates: [
              {
                name: "Lena Vogt",
                email: "lena@example.com",
                source: "invited",
              },
            ],
          },
        ],
      },
    });
    const chip = page.getByRole("button", { name: "Lena?" });
    await chip.click();
    const card = page.getByRole("dialog", { name: "Speaker 2 ist …" });
    await expect(card).toBeVisible();
    // A click outside closes it; so does Esc.
    await page.getByRole("heading", { name: "Launch-Plan Dezember" }).click();
    await expect(card).toHaveCount(0);
    await chip.click();
    await page.keyboard.press("Escape");
    await expect(card).toHaveCount(0);

    await chip.click();
    await card.getByRole("button", { name: "Jemand anderes …" }).click();
    await card
      .getByRole("textbox", { name: "Name", exact: true })
      .fill("Mia Sand");
    const mail = card.getByRole("textbox", { name: "E-Mail-Adressen" });
    await mail.fill("mia@example.com");
    const button = card.getByRole("button", { name: "Benennen" });
    const [b, m, c] = await Promise.all([
      button.boundingBox(),
      mail.boundingBox(),
      card.boundingBox(),
    ]);
    // Below the last field, at the card's right edge, and the card is small.
    expect(b!.y).toBeGreaterThan(m!.y + m!.height);
    expect(Math.abs(b!.x + b!.width - (m!.x + m!.width))).toBeLessThan(2);
    expect(c!.width).toBeLessThan(320);
    await shot(page, "jemand-anderes");
    await mail.press("Enter");
    const named = await args(page, "name_voice");
    expect(named[0]).toEqual({
      members: [{ meeting_id: "h3", speaker: "Speaker 2" }],
      name: "Mia Sand",
      emails: ["mia@example.com"],
      consent: false,
    });
    await expect(card).toHaveCount(0);
  });

  test("chips on a second row: the card hangs under the clicked chip", async ({
    page,
  }) => {
    // Made-up names, long enough that the chips wrap.
    const people = [
      "Du",
      "Annika Brandtmüller",
      "Konstantin Oberhauser",
      "Friederike Wallenstein",
      "Maximilian Hohenberger",
      "Theresa Lichtenfeld",
      "Bartholomäus Grünwald",
      "Speaker 8",
    ];
    await open(page, {
      lang: "de",
      theme: "dark",
      section: "meetings",
      extra: {
        list_meetings: [
          {
            id: "w1",
            started_at: new Date().toISOString(),
            duration_s: 45 * 60,
            status: { kind: "done" },
            tracks: ["mic.flac", "system.flac"],
            source_app: "Zoom",
            title: "Großes Planungstreffen",
            note_path: "/tmp/sokki-notes/w1.md",
            speakers: people,
            warnings: [],
            summary_missing: null,
            audio_deleted: false,
            online: true,
          },
        ],
        get_meeting_note: {
          summary: "- Speaker 8 bucht den Raum.",
          transcript: "[00:04] Speaker 8: Ich buche den Raum.",
        },
        meeting_speaker_proposals: [
          {
            speaker: "Speaker 8",
            candidates: [
              {
                name: "Lena Vogt",
                email: "lena@example.com",
                source: "invited",
              },
            ],
          },
        ],
      },
    });
    const first = page.getByRole("button", { name: "Du", exact: true });
    const chip = page.getByRole("button", { name: "Lena?" });
    const [f, c0] = await Promise.all([
      first.boundingBox(),
      chip.boundingBox(),
    ]);
    // The test only means something when the chip really sits lower.
    expect(c0!.y).toBeGreaterThan(f!.y + f!.height);

    await chip.click();
    const card = page.getByRole("dialog", { name: "Speaker 8 ist …" });
    await expect(card).toBeVisible();
    const arrow = card.locator("span[aria-hidden=true]").first();
    const [c, k, a] = await Promise.all([
      chip.boundingBox(),
      card.boundingBox(),
      arrow.boundingBox(),
    ]);
    // Below the chip, not over it; the arrow points at this chip.
    expect(k!.y).toBeGreaterThanOrEqual(c!.y + c!.height);
    expect(k!.y - (c!.y + c!.height)).toBeLessThan(14);
    const tip = a!.x + a!.width / 2;
    expect(tip).toBeGreaterThan(c!.x);
    expect(tip).toBeLessThan(c!.x + c!.width);
    // And it stays inside the window.
    const width = page.viewportSize()!.width;
    expect(k!.x).toBeGreaterThanOrEqual(0);
    expect(k!.x + k!.width).toBeLessThanOrEqual(width);
    await shot(page, "zweite-reihe");
  });
});
