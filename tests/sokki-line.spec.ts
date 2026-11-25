import { test, expect, type Page } from "@playwright/test";
import { open } from "./fake-app";

/**
 * The Sokki line row in Settings › Meetings (design B, 2026-11-24): the
 * preview of the real line, "Code eingeben", a wrong and a valid supporter
 * code. Headless, with the fake app from fake-app.ts. Screenshots
 * land in test-results/sokki-line/ (or $SHOT_DIR).
 */

const SHOTS = process.env.SHOT_DIR ?? "test-results/sokki-line";
const VALID = "sokki-eyJ2IjoxLCJ2aWEiOiJ0ZXN0IiwiYXQiOiIyMDI2LTExLTI0In0.test";
const LINE_DE =
  "Von KI zusammengefasst. Lokal, kostenlos, Open Source. Diktier-App und Meeting-Notetaker: sokki.org";
const LINE_DE_CLOUD =
  "Von KI zusammengefasst. Open Source. Diktier-App und Meeting-Notetaker: sokki.org";

async function openRow(page: Page, line = LINE_DE, supporter = false) {
  await open(page, {
    lang: "de",
    theme: "dark",
    section: "meetings",
    extra: {
      sokki_line: line,
      // The fake app accepts only VALID, and the stored code when a supporter.
      ...(supporter ? { is_supporter_code: true } : {}),
    },
  });
  await page.evaluate((valid) => {
    const w = window as any;
    const invoke = w.__TAURI_INTERNALS__.invoke;
    w.__TAURI_INTERNALS__.invoke = async (cmd: string, args: any) =>
      cmd === "is_supporter_code" && !w.__supporter
        ? args?.code === valid
        : invoke(cmd, args);
    w.__supporter = false;
  }, VALID);
  if (supporter)
    await page.evaluate(() => ((window as any).__supporter = true));
  await page.getByRole("button", { name: "Meeting-Einstellungen" }).click();
  const row = page
    .locator("div.px-4")
    .filter({ has: page.getByRole("heading", { name: "Sokki-Zeile" }) })
    .last();
  await row.scrollIntoViewIfNeeded();
  return row;
}

async function shot(
  page: Page,
  row: ReturnType<Page["locator"]>,
  name: string,
) {
  await page.waitForTimeout(300);
  await row.screenshot({ path: `${SHOTS}/${name}.png` });
}

test.describe("Sokki line setting", () => {
  test("shows the real line and who can switch it off", async ({ page }) => {
    const row = await openRow(page);
    await expect(
      row.getByText("Steht unter jeder Zusammenfassung:"),
    ).toBeVisible();
    await expect(
      row.getByText(LINE_DE.replace(" sokki.org", ""), { exact: false }),
    ).toBeVisible();
    await expect(
      row.getByText("Ab 20 € Unterstützung kannst du sie ausschalten."),
    ).toBeVisible();
    await expect(row.getByRole("checkbox")).toHaveCount(0);
    await expect(row.getByRole("textbox")).toHaveCount(0);
    await shot(page, row, "1-not-a-supporter");
  });

  test("a cloud model gets the shorter line", async ({ page }) => {
    const row = await openRow(page, LINE_DE_CLOUD);
    await expect(
      row.getByText("Von KI zusammengefasst. Open Source.", { exact: false }),
    ).toBeVisible();
    await expect(
      row.getByText("Lokal, kostenlos", { exact: false }),
    ).toHaveCount(0);
    await shot(page, row, "2-cloud-model");
  });

  test("Code eingeben opens the field; a wrong code says so", async ({
    page,
  }) => {
    const row = await openRow(page);
    await row.getByRole("button", { name: "Code eingeben" }).click();
    await expect(
      row.getByRole("button", { name: "Code eingeben" }),
    ).toHaveCount(0);
    const field = row.getByRole("textbox", { name: "Unterstützer-Code" });
    await expect(field).toBeVisible();
    await expect(
      row.getByRole("button", { name: "Sokki unterstützen" }),
    ).toBeVisible();
    await shot(page, row, "3-code-field");
    await field.fill("sokki-eyJ2IjoxLCJ2a");
    await field.press("Enter");
    await expect(
      row.getByText("Dieser Code passt nicht. Kopiere ihn ganz, ab „sokki-“."),
    ).toBeVisible();
    await shot(page, row, "4-wrong-code");
  });

  test("a valid code saves it", async ({ page }) => {
    const row = await openRow(page);
    const saved: unknown[] = [];
    await page.exposeFunction("__saved", (s: unknown) => saved.push(s));
    await page.evaluate(() => {
      const w = window as any;
      const invoke = w.__TAURI_INTERNALS__.invoke;
      w.__TAURI_INTERNALS__.invoke = async (cmd: string, args: any) => {
        if (cmd === "set_meeting_settings")
          w.__saved(args?.settings?.supporter_code);
        return invoke(cmd, args);
      };
    });
    await row.getByRole("button", { name: "Code eingeben" }).click();
    const field = row.getByRole("textbox", { name: "Unterstützer-Code" });
    await field.fill(`  ${VALID}  `);
    await field.press("Enter");
    await expect.poll(() => saved).toContain(VALID);
    await expect(
      row.getByText("Dieser Code passt nicht", { exact: false }),
    ).toHaveCount(0);
  });

  test("a supporter gets a switch and a thank-you", async ({ page }) => {
    const row = await openRow(page, LINE_DE, true);
    await expect(row.getByText("Danke für deine Unterstützung.")).toBeVisible();
    await expect(
      row.getByRole("checkbox", { name: "Sokki-Zeile" }),
    ).toBeVisible();
    await expect(row.getByText("Ab 20 €", { exact: false })).toHaveCount(0);
    await shot(page, row, "5-supporter");
  });
});
