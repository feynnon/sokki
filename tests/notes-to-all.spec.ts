import { test, expect, type Page } from "@playwright/test";
import { open } from "./fake-app";

/**
 * "Notes by mail" in the meeting settings (meeting/mail.rs). Short on
 * purpose (Robin, 2026-11-25: "don't make me think"): labels and buttons,
 * Google's notice only after "Connect". Addresses are invented.
 */

const SHOTS = process.env.SHOT_DIR ?? "test-results/notes-to-all";

async function shot(page: Page, name: string) {
  await page.waitForTimeout(300);
  await page.screenshot({ path: `${SHOTS}/${name}.png` });
}

async function settings(page: Page, extra: Record<string, unknown> = {}) {
  await open(page, { lang: "de", theme: "dark", section: "meetings", extra });
  await page.getByRole("button", { name: "Meeting-Einstellungen" }).click();
  const box = page.getByRole("heading", { name: "Notizen per Mail" });
  await box.scrollIntoViewIfNeeded();
  await expect(box).toBeVisible();
}

test.describe("notes by mail", () => {
  test("without Gmail: a switch and the no-mail list, nothing to read", async ({
    page,
  }) => {
    await settings(page, { mail_objections: ["olaf@kunde.example"] });
    await expect(
      page.getByText("Nach jedem Meeting an alle senden"),
    ).toBeVisible();
    await expect(page.getByText("olaf@kunde.example")).toBeVisible();
    await expect(page.getByRole("button", { name: "+ Adresse" })).toBeVisible();
    // Gmail is hidden while Google's review is pending.
    await expect(page.getByText("Gmail", { exact: true })).toHaveCount(0);
    await shot(page, "einstellungen-ohne-gmail");
  });

  test("Gmail: Google's notice only after Verbinden, then on to Google", async ({
    page,
  }) => {
    await settings(page, {
      gmail_status: { available: true, connected: null },
    });
    const notice = page.getByText("Sokki bittet Google", { exact: false });
    await expect(notice).toHaveCount(0);
    await page.getByRole("button", { name: "Verbinden" }).click();
    await expect(notice).toBeVisible();
    await expect(
      page.getByRole("button", { name: "Weiter zu Google" }),
    ).toBeVisible();
    await shot(page, "einstellungen-gmail-hinweis");
    const calls = await page.evaluate(() => (window as any).__calls);
    expect(calls).not.toContain("gmail_connect");
  });

  test("Gmail connected: the address and Trennen", async ({ page }) => {
    await settings(page, {
      gmail_status: { available: true, connected: "mira@beispiel.example" },
    });
    await expect(page.getByText("mira@beispiel.example")).toBeVisible();
    await expect(page.getByRole("button", { name: "Trennen" })).toBeVisible();
    await shot(page, "einstellungen-gmail-verbunden");
  });
});

test.describe("notes to everyone, on by default", () => {
  test("the switch is on and says what goes; off is remembered", async ({
    page,
  }) => {
    await settings(page);
    const toggle = page.getByRole("checkbox", {
      name: "Nach jedem Meeting an alle senden",
    });
    await expect(toggle).toBeChecked();
    await expect(
      page.getByText(
        "Zusammenfassung und Aufgaben, kein Transkript, an alle, die dabei waren.",
      ),
    ).toBeVisible();
    await shot(page, "standard-an");
    await toggle.click({ force: true });
    const saved = await page.evaluate(() =>
      (window as any).__args
        .map(([, a]: [string, any]) => JSON.stringify(a ?? {}))
        .filter((a: string) => a.includes("mail_notes_auto")),
    );
    expect(saved.join(" ")).toContain('"mail_notes_auto_set":true');
    expect(saved.join(" ")).toContain('"mail_notes_auto":false');
  });

  test("the Gmail notice asks for sending only", async ({ page }) => {
    await settings(page, {
      gmail_status: { available: true, connected: null },
    });
    await page.getByRole("button", { name: "Verbinden" }).click();
    await expect(
      page.getByText(
        "Sokki bittet Google, in deinem Namen Mails zu senden und deine Adresse zu sehen. Sokki kann deine Mails nicht lesen. Die Notizen gehen von diesem Mac direkt an Gmail.",
      ),
    ).toBeVisible();
    await expect(
      page.getByText(/Ankündigung|Absender und Betreff/),
    ).toHaveCount(0);
  });
});
