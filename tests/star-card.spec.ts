import { test, expect, type Page } from "@playwright/test";
import { open } from "./fake-app";

/**
 * The star card ("Gefällt dir Sokki?"): once, right after a meeting or
 * dictation finished at 5 hours. When it's due is Rust's call
 * (`milestones.rs`, with unit tests); here: the card appears on Sokki's
 * signal, its buttons, and that it gives way to recordings and other
 * messages. A browser with a fake app behind it; nothing starts Sokki.
 * Screenshots land in test-results/star-card/ (or $SHOT_DIR).
 */

const SHOTS = process.env.SHOT_DIR ?? "test-results/star-card";

async function shot(page: Page, name: string) {
  await page.waitForTimeout(400);
  await page.screenshot({ path: `${SHOTS}/${name}.png` });
}

/** What Sokki sends right after a meeting or dictation finished (once
 * the window is up and listening). */
async function finished(page: Page) {
  await expect(page.getByRole("navigation")).toBeVisible();
  await page.waitForTimeout(200);
  await page.evaluate(() => (window as any).__emit("sokki-star-ask", null));
}

const calls = (page: Page) =>
  page.evaluate(() => (window as any).__args as [string, any][]);

const card = (page: Page) =>
  page.getByRole("region", { name: /Gefällt dir Sokki\?|Enjoying Sokki\?/ });

test.describe("star card", () => {
  test("shows after a finished recording, not on start; the star opens sokki.org/github", async ({
    page,
  }) => {
    await open(page, {
      lang: "de",
      theme: "dark",
      extra: { get_star_ask: true },
    });
    await expect(page.getByText("Stunden mitgeschrieben")).toBeVisible();
    // Due, but nothing finished yet: no card when the window opens.
    await page.waitForTimeout(500);
    await expect(card(page)).toBeHidden();

    await finished(page);
    await expect(card(page)).toBeVisible();
    await expect(
      card(page).getByText(
        "Ein Stern auf GitHub hilft anderen, Sokki zu finden.",
      ),
    ).toBeVisible();
    await shot(page, "stern-dark-de");

    await card(page).getByRole("button", { name: "Stern auf GitHub" }).click();
    await expect(card(page)).toBeHidden();
    const c = await calls(page);
    expect(
      c.some(
        ([cmd, a]) =>
          cmd === "plugin:opener|open_url" &&
          a?.url === "https://sokki.org/github?s=star-card",
      ),
    ).toBe(true);
    expect(c.some(([cmd]) => cmd === "close_star_ask")).toBe(true);
  });

  test("“Nicht jetzt” closes it for good", async ({ page }) => {
    await open(page, {
      lang: "de",
      theme: "light",
      extra: { get_star_ask: true },
    });
    await finished(page);
    await expect(card(page)).toBeVisible();
    await shot(page, "stern-light-de");
    await card(page).getByRole("button", { name: "Nicht jetzt" }).click();
    await expect(card(page)).toBeHidden();
    const c = await calls(page);
    expect(c.some(([cmd]) => cmd === "close_star_ask")).toBe(true);
    expect(c.some(([cmd]) => cmd === "plugin:opener|open_url")).toBe(false);
  });

  test("its X closes it for good (English)", async ({ page }) => {
    await open(page, {
      lang: "en",
      theme: "dark",
      extra: { get_star_ask: true },
    });
    await finished(page);
    await expect(card(page)).toBeVisible();
    await expect(
      card(page).getByText("A star on GitHub helps others find it."),
    ).toBeVisible();
    await expect(
      card(page).getByRole("button", { name: "Star on GitHub" }),
    ).toBeVisible();
    await shot(page, "star-dark-en");
    await card(page).getByRole("button", { name: "Close" }).click();
    await expect(card(page)).toBeHidden();
    expect((await calls(page)).some(([c]) => c === "close_star_ask")).toBe(
      true,
    );
  });

  test("not when Sokki says it isn't due", async ({ page }) => {
    await open(page, { lang: "de", theme: "dark" });
    await finished(page);
    await page.waitForTimeout(500);
    await expect(card(page)).toBeHidden();
  });

  test("never while a meeting records", async ({ page }) => {
    await open(page, {
      lang: "de",
      theme: "dark",
      recording: true,
      extra: { get_star_ask: true },
    });
    await expect(page.getByRole("button", { name: "Beenden" })).toBeVisible();
    await finished(page);
    await page.waitForTimeout(500);
    await expect(card(page)).toBeHidden();
  });

  test("never together with a message; it waits until the message is answered", async ({
    page,
  }) => {
    await open(page, {
      lang: "de",
      theme: "dark",
      extra: {
        get_star_ask: true,
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
    const dialog = page.getByRole("alertdialog");
    await expect(dialog).toBeVisible();
    await finished(page);
    await page.waitForTimeout(500);
    await expect(card(page)).toBeHidden();
    // Answered: the message goes, and the star card follows.
    await page.evaluate(() => {
      const w = window as any;
      const invoke = w.__TAURI_INTERNALS__.invoke;
      w.__TAURI_INTERNALS__.invoke = (cmd: string, a: unknown) =>
        cmd === "pending_message" ? Promise.resolve(null) : invoke(cmd, a);
    });
    await dialog.getByRole("button", { name: "OK" }).click();
    await expect(dialog).toBeHidden();
    await expect(card(page)).toBeVisible();
  });

  test("never together with a notice", async ({ page }) => {
    await open(page, {
      lang: "de",
      theme: "dark",
      extra: { get_star_ask: true },
    });
    await expect(page.getByText("Stunden mitgeschrieben")).toBeVisible();
    await finished(page);
    await expect(card(page)).toBeVisible();
    // A notice pops up as a toast: the card steps aside.
    await page.evaluate(async () => {
      const m = await import("/src/stores/noticeStore.ts");
      m.useNoticeStore.getState().notify({ title: "Kopieren fehlgeschlagen" });
    });
    await expect(
      page.getByText("Kopieren fehlgeschlagen").last(),
    ).toBeVisible();
    await expect(card(page)).toBeHidden();
  });
});
