import { test, expect, type Page } from "@playwright/test";
import { open } from "./fake-app";

/**
 * The dictionary pill (src-tauri/src/dictionary.rs): after a typed word the Mac
 * underlines, the overlay offers to add it to the Wörterbuch; one click (or
 * ⌃⌥W) does it, and an added or kept word shows with an Undo. Headless, with a
 * fake app behind the overlay page; the words are made up. Screenshots land in
 * test-results/word-pill/ (or $SHOT_DIR).
 */

const SHOTS = process.env.SHOT_DIR ?? "test-results/word-pill";

/** The pill's window (OVERLAY_WORD_WIDTH x OVERLAY_HEIGHT in overlay.rs). */
const WINDOW = { width: 400, height: 50 };

async function showPill(
  page: Page,
  kind: "offer" | "added" | "kept",
  word: string,
  theme: "dark" | "light" = "dark",
) {
  await open(page, { lang: "de", theme, path: "/src/overlay/index.html" });
  await page.setViewportSize(WINDOW);
  await page.evaluate(
    (p) => {
      (window as any).__emit("overlay-word", p);
      (window as any).__emit("show-overlay", "word");
    },
    { kind, word },
  );
  await expect(page.getByTestId("word-pill")).toBeVisible();
  await page.waitForTimeout(300);
}

async function calls(page: Page) {
  return page.evaluate(() => (window as any).__args as [string, any][]);
}

test("offers the word, and one click adds it", async ({ page }) => {
  await showPill(page, "offer", "Tarnisio");
  const pill = page.getByTestId("word-pill");
  await expect(pill).toContainText("Tarnisio ins Wörterbuch?");
  await expect(pill).toContainText("⌃⌥W");
  await page.screenshot({ path: `${SHOTS}/1-offer-dark.png` });
  // the whole pill fits its window
  const box = await pill.boundingBox();
  expect(box!.width).toBeLessThanOrEqual(WINDOW.width);
  await page.getByRole("button", { name: "Hinzufügen", exact: true }).click();
  expect(await calls(page)).toContainEqual([
    "dictionary_add_word",
    { word: "Tarnisio" },
  ]);
});

test("the X never offers the word again", async ({ page }) => {
  await showPill(page, "offer", "Quorbit");
  await page.getByRole("button", { name: "Nicht hinzufügen" }).click();
  expect(await calls(page)).toContainEqual([
    "dictionary_decline_word",
    { word: "Quorbit" },
  ]);
});

test("an added word shows with an Undo", async ({ page }) => {
  await showPill(page, "added", "Tarnisio", "light");
  await expect(page.getByTestId("word-pill")).toContainText(
    "Tarnisio im Wörterbuch",
  );
  await page.screenshot({ path: `${SHOTS}/2-added-light.png` });
  await page.getByRole("button", { name: "Rückgängig" }).click();
  expect(await calls(page)).toContainEqual([
    "dictionary_remove_word",
    { word: "Tarnisio" },
  ]);
});

test("a fix undone with Backspace shows the word kept", async ({ page }) => {
  await showPill(page, "kept", "Lumbra");
  await expect(page.getByTestId("word-pill")).toContainText(
    "Lumbra bleibt so, im Wörterbuch",
  );
  await page.screenshot({ path: `${SHOTS}/3-kept-dark.png` });
});

test("a long word still fits its window", async ({ page }) => {
  await showPill(page, "offer", "Donaudampfschifffahrtsgesellschaft");
  const box = await page.getByTestId("word-pill").boundingBox();
  expect(box!.width).toBeLessThanOrEqual(WINDOW.width);
  await expect(
    page.getByRole("button", { name: "Hinzufügen", exact: true }),
  ).toBeVisible();
  await page.screenshot({ path: `${SHOTS}/4-long-word.png` });
});
