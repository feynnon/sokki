import { test, expect, type Page } from "@playwright/test";
import { open } from "./fake-app";

/**
 * The live panel while a dictation is cleaned up as it is spoken
 * (`dictation_cleanup`): finished paragraphs show cleaned, the newest part
 * raw. Headless, with a fake app behind the overlay page; the text is
 * invented. Screenshots land in test-results/live-cleanup/ (or $SHOT_DIR).
 */

const SHOTS = process.env.SHOT_DIR ?? "test-results/live-cleanup";

async function showPanel(page: Page) {
  await open(page, {
    lang: "de",
    theme: "dark",
    path: "/src/overlay/index.html",
  });
  // The overlay window's size while live text shows (src-tauri/src/overlay.rs).
  await page.setViewportSize({ width: 400, height: 120 });
  await page.evaluate(() =>
    (window as any).__emit("show-overlay", "streaming"),
  );
}

async function text(
  page: Page,
  cleaned: string,
  committed: string,
  tentative: string,
) {
  await page.evaluate((p) => (window as any).__emit("stream-text-event", p), {
    cleaned,
    committed,
    tentative,
  });
}

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
  await page.waitForTimeout(300);
  await page.screenshot({ path: `${SHOTS}/raw.png` });
});

test("cleaned paragraphs show as soon as they are done, the newest part raw", async ({
  page,
}) => {
  await showPanel(page);
  await text(
    page,
    "Für die Party bräuchten wir noch:\n\n- den Grill, den kann Tom mitbringen.\n- Getränke, Bier und Saft, die besorg ich.",
    "Und dann noch die Musik, ähm, die Box,",
    "die Lautsprecherbox meine ich",
  );
  const cleaned = page.locator(".cleaned");
  await expect(cleaned).toContainText("- den Grill, den kann Tom mitbringen.");
  // Paragraphs and list items keep their line breaks.
  await expect(cleaned).toHaveCSS("white-space", "pre-wrap");
  await expect(page.locator(".committed")).toContainText(
    "Und dann noch die Musik",
  );
  await expect(page.locator(".tentative")).toContainText("Lautsprecherbox");
  await page.waitForTimeout(300);
  await page.screenshot({ path: `${SHOTS}/cleaned.png` });
  await expect(cleaned).toHaveCSS("font-style", "normal");
});

test("right after a piece is cleaned, its paragraphs are in view", async ({
  page,
}) => {
  await showPanel(page);
  await text(
    page,
    "Für die Party bräuchten wir noch:\n\n- den Grill, den kann Tom mitbringen.\n- Getränke, Bier und Saft, die besorg ich.",
    "",
    "Und dann noch",
  );
  await expect(page.locator(".cleaned")).toContainText("Getränke");
  await page.waitForTimeout(300);
  await page.screenshot({ path: `${SHOTS}/just-cleaned.png` });
});
