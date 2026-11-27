import { test, expect, type Page } from "@playwright/test";
import { open } from "./fake-app";

/**
 * The live text of a running meeting shows the newest line (Robin,
 * 2026-11-27): it opens at the end, follows new lines, stops when the
 * person scrolls up to read, and "Zum Neuesten" brings it back. Fake app,
 * invented lines.
 */

const SHOTS = process.env.SHOT_DIR ?? "test-results/live-tail";

const line = (i: number) => ({
  start_s: i * 7,
  text: `Zeile ${i}: Wir besprechen den Plan für die nächste Woche.`,
});
const lines = (n: number) => Array.from({ length: n }, (_, i) => line(i));

async function setLive(page: Page, n: number) {
  await page.evaluate((l) => {
    (window as any).__live = { rec: l };
  }, lines(n));
}

/** Pixels between the box's bottom and its last line. */
const gap = (page: Page) =>
  page
    .locator("[data-live-lines]")
    .evaluate((b) => b.scrollHeight - b.scrollTop - b.clientHeight);

async function openRecording(page: Page, n: number) {
  await open(page, {
    lang: "de",
    theme: "dark",
    section: "meetings",
    recording: true,
  });
  await setLive(page, n);
  await page.setViewportSize({ width: 1280, height: 800 });
}

test("opens at the newest line and follows new ones", async ({ page }) => {
  await openRecording(page, 60);
  const box = page.locator("[data-live-lines]");
  await expect(box).toContainText("Zeile 59:");
  // Long enough to scroll, and scrolled to the end.
  expect(await box.evaluate((b) => b.scrollHeight > b.clientHeight * 2)).toBe(
    true,
  );
  await expect.poll(() => gap(page)).toBeLessThan(2);
  await expect(page.locator("[data-jump-latest]")).toHaveCount(0);

  await setLive(page, 70);
  await expect(box).toContainText("Zeile 69:");
  await expect.poll(() => gap(page)).toBeLessThan(2);
  await page.screenshot({ path: `${SHOTS}/following-de.png` });
});

test("scrolling up stops following; the button brings it back", async ({
  page,
}) => {
  await openRecording(page, 60);
  const box = page.locator("[data-live-lines]");
  await expect(box).toContainText("Zeile 59:");
  await expect.poll(() => gap(page)).toBeLessThan(2);

  await box.hover();
  await page.mouse.wheel(0, -600);
  await expect.poll(() => gap(page)).toBeGreaterThan(300);
  const jump = page.getByRole("button", { name: "Zum Neuesten" });
  await expect(jump).toBeVisible();
  const top = await box.evaluate((b) => b.scrollTop);

  // New lines don't move what the person reads.
  await setLive(page, 70);
  await expect(box).toContainText("Zeile 69:");
  expect(await box.evaluate((b) => b.scrollTop)).toBe(top);
  await page.screenshot({ path: `${SHOTS}/reading-de.png` });

  await jump.click();
  await expect.poll(() => gap(page)).toBeLessThan(2);
  await expect(jump).toHaveCount(0);
  await setLive(page, 80);
  await expect(box).toContainText("Zeile 79:");
  await expect.poll(() => gap(page)).toBeLessThan(2);
});

test("scrolling back down resumes following", async ({ page }) => {
  await openRecording(page, 60);
  const box = page.locator("[data-live-lines]");
  await expect(box).toContainText("Zeile 59:");
  await expect.poll(() => gap(page)).toBeLessThan(2);
  await box.hover();
  await page.mouse.wheel(0, -600);
  await expect(page.locator("[data-jump-latest]")).toBeVisible();
  await page.mouse.wheel(0, 5000);
  await expect(page.locator("[data-jump-latest]")).toHaveCount(0);
  await setLive(page, 70);
  await expect(box).toContainText("Zeile 69:");
  await expect.poll(() => gap(page)).toBeLessThan(2);
});

test("only the open meeting's live text", async ({ page }) => {
  await open(page, {
    lang: "de",
    theme: "dark",
    section: "meetings",
    recording: true,
  });
  // Lines of another meeting are never shown here.
  await page.evaluate(() => {
    (window as any).__live = { other: [{ start_s: 1, text: "Fremder Text" }] };
  });
  const box = page.locator("[data-live-lines]");
  await expect(box).toContainText("Hört zu");
  await page.waitForTimeout(1500);
  await expect(box).not.toContainText("Fremder Text");
});
