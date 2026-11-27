import { test, expect, type Page } from "@playwright/test";
import { open } from "./fake-app";

/**
 * Where a summary point was said (Robin, 2026-11-27, "Belege" 2 + 3 + 1):
 * a time mark at every cited point, its quote on hover with "Listen" (only
 * while the recording exists) and "Show in transcript", and a click on the
 * point jumps to the highlighted lines. Fake app, invented people.
 */

const SHOTS = process.env.SHOT_DIR ?? "test-results/summary-cites";

const transcript = [
  "[00:00] Mara: Guten Morgen zusammen.",
  "[03:12] Lena: Die Herbstaktion startet am 6. Oktober, das steht jetzt fest.",
  "[08:40] Jonas: Budget sind 4.000 Euro, davon 1.500 für Anzeigen.",
  "[12:22] Mara: Und wann muss das alles bei der Druckerei sein?",
  "[12:41] Lena: Die Druckerei braucht die Dateien bis Freitag zwölf Uhr, sonst liefern sie erst im November.",
  "[19:05] Jonas: Ich hole ein Angebot von der Agentur für die Fotos, bis Mittwoch.",
].join("\n");

const summary =
  "## Zusammenfassung\n\n- Die Herbstaktion startet am 6. Oktober.\n- Die Druckerei braucht die Dateien bis Freitag 12 Uhr, sonst wird erst im November geliefert.\n\n## Aufgaben\n\n- [ ] **Jonas**: Angebot der Agentur für die Fotos, bis Mittwoch";

/** As `get_meeting_note` answers: lines into `summary` and `transcript`. */
const cites = [
  { line: 2, lines: [1], start_s: [192] },
  { line: 3, lines: [4], start_s: [761] },
  { line: 7, lines: [5], start_s: [1145] },
];

const record = (audio_deleted: boolean) => ({
  id: "c1",
  started_at: new Date().toISOString(),
  duration_s: 31 * 60,
  status: { kind: "done" },
  tracks: ["mic.flac", "system.flac"],
  source_app: "Zoom",
  title: "Herbstaktion planen",
  note_path: "/tmp/sokki-notes/c1.md",
  speakers: ["Mara", "Lena", "Jonas"],
  warnings: [],
  summary_missing: null,
  audio_deleted,
  online: true,
});

async function summaryOf(page: Page, audioDeleted = false) {
  await open(page, {
    lang: "de",
    theme: "dark",
    section: "meetings",
    extra: {
      list_meetings: [record(audioDeleted)],
      get_meeting_note: { summary, transcript, combined: "", cites },
      // A tenth of a second of silence.
      meeting_clip:
        "data:audio/wav;base64,UklGRkQGAABXQVZFZm10IBAAAAABAAEAgD4AAAB9AAACABAAZGF0YSAGAAA=",
    },
  });
  await page.setViewportSize({ width: 1280, height: 800 });
  await page.getByRole("tab", { name: "AI-Zusammenfassung" }).click();
  const point = page
    .locator("[data-cited]")
    .filter({ hasText: "Die Druckerei braucht" });
  await expect(point).toBeVisible();
  return point;
}

test("every cited point shows when it was said", async ({ page }) => {
  await summaryOf(page);
  await expect(page.locator("[data-cite-mark]")).toHaveText([
    "03:12",
    "12:41",
    "19:05",
  ]);
  await page.screenshot({ path: `${SHOTS}/marks-de.png` });
});

test("hover shows the quote; Listen plays from there", async ({ page }) => {
  const point = await summaryOf(page);
  await point.hover();
  const quote = page.locator("[data-quote]");
  await expect(quote).toBeVisible();
  await expect(quote).toContainText("12:41 · Lena");
  await expect(quote).toContainText("sonst liefern sie erst im November");
  await page.screenshot({ path: `${SHOTS}/quote-de.png` });
  await quote.getByRole("button", { name: "Anhören" }).click();
  const args = await page.evaluate(() =>
    (window as any).__args.filter((a: any) => a[0] === "meeting_clip"),
  );
  // From the point's line to the next line's start.
  expect(args).toEqual([
    ["meeting_clip", { id: "c1", startS: 761, endS: 1145 }],
  ]);
});

test("without the recording there is no Listen", async ({ page }) => {
  const point = await summaryOf(page, true);
  await point.hover();
  const quote = page.locator("[data-quote]");
  await expect(quote).toBeVisible();
  await expect(quote.getByRole("button", { name: "Anhören" })).toHaveCount(0);
  await expect(
    quote.getByRole("button", { name: "Im Transkript zeigen" }),
  ).toBeVisible();
});

test("a click on the point jumps to its lines", async ({ page }) => {
  const point = await summaryOf(page);
  await point.click();
  await expect(page.getByRole("tab", { name: "Transkript" })).toHaveAttribute(
    "aria-selected",
    "true",
  );
  const focused = page.locator("[data-focused]");
  await expect(focused).toHaveCount(1);
  await expect(focused).toContainText("Die Druckerei braucht die Dateien");
  await expect(focused).toBeInViewport();
  // The tab's pill slides over.
  await page.waitForTimeout(400);
  await page.screenshot({ path: `${SHOTS}/jump-de.png` });
});

test("the time mark and Show in transcript jump too", async ({ page }) => {
  await summaryOf(page);
  await page.locator("[data-cite-mark]", { hasText: "19:05" }).click();
  await expect(page.locator("[data-focused]")).toContainText(
    "Angebot von der Agentur",
  );
  await page.getByRole("tab", { name: "AI-Zusammenfassung" }).click();
  await page
    .locator("[data-cited]")
    .filter({ hasText: "Herbstaktion" })
    .hover();
  await page.getByRole("button", { name: "Im Transkript zeigen" }).click();
  await expect(page.locator("[data-focused]")).toContainText(
    "startet am 6. Oktober",
  );
});
