import { test, expect, type Page } from "@playwright/test";
import { open } from "./fake-app";

/**
 * The meeting list (Robin, 2026-11-25): one header per day with its date,
 * no week groups, no date in the rows; and the record bar's note when a
 * meeting without speech was removed. Fake app, invented meetings.
 */

const SHOTS = process.env.SHOT_DIR ?? "test-results/meeting-list";

/** A day's date as the browser formats it ("25. Nov."), `daysAgo` back. */
function dayDate(page: Page, daysAgo: number): Promise<string> {
  return page.evaluate((back) => {
    const now = new Date();
    const d = new Date(now.getFullYear(), now.getMonth(), now.getDate() - back);
    return new Intl.DateTimeFormat("de", {
      day: "numeric",
      month: "short",
      year: d.getFullYear() === now.getFullYear() ? undefined : "numeric",
    }).format(d);
  }, daysAgo);
}

test.describe("meeting list", () => {
  test("one header per day, with the date; rows without it", async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1120, height: 720 });
    await open(page, { lang: "de", theme: "dark", section: "meetings" });
    const list = page.locator("section");
    // The fake meetings were today, yesterday, 3, 9 and 12 days ago.
    const expected = [
      `Heute, ${await dayDate(page, 0)}`,
      `Gestern, ${await dayDate(page, 1)}`,
      await dayDate(page, 3),
      await dayDate(page, 9),
      await dayDate(page, 12),
    ];
    await expect(list.getByRole("heading", { level: 2 })).toHaveText(expected);
    await expect(page.getByText("Diese Woche")).toHaveCount(0);
    await expect(page.getByText("Älter")).toHaveCount(0);
    // The row: time · place · length, no date in front.
    await expect(page.getByText("10:00 · Zoom · 34 Min.")).toBeVisible();
    await expect(page.getByText(/\d+\.\d+\. · \d/)).toHaveCount(0);
    await page.waitForTimeout(300);
    await page.screenshot({ path: `${SHOTS}/meetings-days.png` });
  });

  test("English: Today and Yesterday carry the date too", async ({ page }) => {
    await open(page, { lang: "en", theme: "light", section: "meetings" });
    const first = page.locator("section").getByRole("heading", { level: 2 });
    await expect(first.first()).toHaveText(/^Today, \S/);
    await expect(first.nth(1)).toHaveText(/^Yesterday, \S/);
  });

  test("a meeting without speech: the bar says nothing was saved", async ({
    page,
  }) => {
    await page.clock.install();
    await page.setViewportSize({ width: 1120, height: 720 });
    await open(page, { lang: "de", theme: "dark", section: "meetings" });
    await expect(page.getByText("Nimmt nicht auf")).toBeVisible();
    await page.evaluate(() =>
      (window as any).__emit("meeting-no-speech", "meeting-test"),
    );
    await expect(
      page.getByText("Keine Sprache, nichts gespeichert."),
    ).toBeVisible();
    await page.waitForTimeout(300);
    await page.screenshot({ path: `${SHOTS}/bar-no-speech.png` });
    // For a moment only.
    await page.clock.runFor(21_000);
    await expect(
      page.getByText("Keine Sprache, nichts gespeichert."),
    ).toBeHidden();
    await expect(page.getByText("Nimmt nicht auf")).toBeVisible();
  });
});
