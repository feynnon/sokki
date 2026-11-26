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
});
