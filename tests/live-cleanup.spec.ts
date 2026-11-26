import { test, expect, type Page } from "@playwright/test";
import { open } from "./fake-app";

/**
 * The live panel while a dictation is cleaned up as it is spoken
 * (`dictation_cleanup`): one text that grows. The cleaned part shows as
 * paragraphs and lists, the newest words follow it raw on the same line,
 * dimmed and upright, and are replaced in place once their piece is cleaned.
 * With the cleanup on, the panel is 184 px tall (six lines). Headless, with a fake app behind the
 * overlay page; the text is invented. Screenshots land in
 * test-results/live-cleanup/ (or $SHOT_DIR).
 */

const SHOTS = process.env.SHOT_DIR ?? "test-results/live-cleanup";

/** The live panel's window with the cleanup on (src-tauri/src/overlay.rs). */
const TALL = { width: 400, height: 184 };

async function showPanel(page: Page, size = TALL) {
  await open(page, {
    lang: "de",
    theme: "dark",
    path: "/src/overlay/index.html",
  });
  await page.setViewportSize(size);
  await page.evaluate(() =>
    (window as any).__emit("show-overlay", "streaming"),
  );
}

async function text(
  page: Page,
  cleaned: string,
  committed: string,
  tentative: string,
  cleanup = true,
) {
  await page.evaluate((p) => (window as any).__emit("stream-text-event", p), {
    cleaned,
    committed,
    tentative,
    cleanup,
  });
}

/** Whether the element's last line is inside the panel's text area. */
async function endInView(page: Page, selector: string) {
  return page.evaluate((sel) => {
    const el = document.querySelector(sel)!;
    const cap = document.querySelector(".stext-cap")!.getBoundingClientRect();
    const rects = Array.from(el.getClientRects()).filter((r) => r.width > 0);
    const last = rects[rects.length - 1];
    return !!last && last.bottom <= cap.bottom + 1 && last.top >= cap.top - 1;
  }, selector);
}

const LIST =
  "Für die Party bräuchten wir noch:\n\n- den Grill, den kann Tom mitbringen.\n- Getränke, Bier und Saft, die besorg ich.";

test("before the first piece is cleaned, everything shows raw", async ({
  page,
}) => {
  await showPanel(page);
  await text(
    page,
    "",
    "Also ähm für die Party bräuchten wir noch, also da wäre einmal der Grill.",
    "Den kann der Tom",
  );
  await expect(
    page.getByText("Also ähm für die Party", { exact: false }),
  ).toBeVisible();
  await expect(page.locator(".cleaned")).toHaveCount(0);
  // Raw words with the cleanup on: dimmed and upright.
  await expect(page.locator(".committed")).toHaveCSS("opacity", "0.5");
  await expect(page.locator(".committed")).toHaveCSS("font-style", "normal");
  await page.waitForTimeout(300);
  await page.screenshot({ path: `${SHOTS}/1-raw.png` });
});

test("the newest words follow the cleaned text on the same line", async ({
  page,
}) => {
  await showPanel(page);
  await text(
    page,
    LIST,
    "Und dann noch die Musik, ähm, die Box,",
    "die Lautsprecherbox meine ich",
  );
  const cleaned = page.locator(".cleaned");
  await expect(cleaned).toContainText("- Getränke, Bier und Saft");
  // Paragraphs and list items keep their line breaks, and stand upright.
  await expect(cleaned).toHaveCSS("white-space", "pre-wrap");
  await expect(cleaned).toHaveCSS("font-style", "normal");
  // One text: a space, not a new paragraph, before the raw words.
  const joined = await cleaned.evaluate((el) => el.textContent);
  expect(joined?.endsWith("die besorg ich. ")).toBe(true);
  await expect(page.locator(".committed")).toContainText(
    "Und dann noch die Musik",
  );
  // The raw words are dimmed, the cleaned text at full strength.
  await expect(page.locator(".committed")).toHaveCSS("opacity", "0.5");
  await expect(page.locator(".tentative")).toHaveCSS("opacity", "0.5");
  await expect(cleaned).toHaveCSS("opacity", "1");
  // The cleaned list is still in view with the raw words after it.
  expect(await endInView(page, ".cleaned")).toBe(true);
  expect(await endInView(page, ".tentative")).toBe(true);
  await page.waitForTimeout(300);
  await page.screenshot({ path: `${SHOTS}/2-raw-appended.png` });
});

test("once cleaned, the raw words are replaced in place", async ({ page }) => {
  await showPanel(page);
  await text(page, LIST, "Und dann noch die Musik, ähm, die Box,", "die Lautsprecherbox");
  await text(
    page,
    LIST + "\n- die Musikbox, die hat Anna.",
    "",
    "Und wer",
  );
  await expect(page.locator(".cleaned")).toContainText(
    "- die Musikbox, die hat Anna.",
  );
  await expect(page.locator(".committed")).toHaveText("");
  await expect(page.locator(".tentative")).toHaveText("Und wer");
  expect(await endInView(page, ".cleaned")).toBe(true);
  await page.waitForTimeout(300);
  await page.screenshot({ path: `${SHOTS}/3-replaced.png` });
});

test("in a long dictation the end of the cleaned text stays in view", async ({
  page,
}) => {
  await showPanel(page);
  const earlier =
    "Kurz zum Stand der Planung für Samstag. Wir treffen uns um vier bei mir im Garten, bei Regen in der Garage.\n\n";
  // About 30 raw words after it, the most a live piece leaves raw.
  await text(
    page,
    earlier + LIST,
    "Und dann noch die Musik, ähm, die Box, die Lautsprecherbox meine ich, die hat die Anna, und die bringt sie",
    "glaube ich am Nachmittag mit, wenn sie vorher noch",
  );
  await page.waitForTimeout(300);
  await page.screenshot({ path: `${SHOTS}/4-long.png` });
  expect(await endInView(page, ".cleaned")).toBe(true);
});

test("without the cleanup the panel keeps its three lines", async ({
  page,
}) => {
  await showPanel(page, { width: 400, height: 120 });
  await text(page, "", "Ein Satz ohne Aufräumen, wie bisher.", "und weiter", false);
  // Without the cleanup the live text stays italic at full strength.
  await expect(page.locator(".stext-cap")).toHaveCSS("font-style", "italic");
  await expect(page.locator(".committed")).toHaveCSS("opacity", "1");
  // Measured once the panel's open animation (0.44 s) is over: mid-animation
  // a frame can be taller (67 px in 1 of 30 parallel runs, 26.11.2026).
  await page.waitForTimeout(700);
  const [h, max] = await page
    .locator(".stext-cap")
    .evaluate((el) => [
      el.getBoundingClientRect().height,
      parseFloat(getComputedStyle(el).maxHeight),
    ]);
  expect(h).toBeLessThanOrEqual(64);
  expect(max).toBeLessThanOrEqual(64);
});
