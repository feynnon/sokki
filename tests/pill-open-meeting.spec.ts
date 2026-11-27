import { test, expect, type Page } from "@playwright/test";
import { open } from "./fake-app";

/**
 * A click on the recording pill opens the meeting's page (Robin,
 * 2026-11-27; design "Pille öffnen 3"). The main window gets
 * `open-meeting` from Rust and shows that meeting at once, from any
 * section, the meeting settings included; the pill page tells a click
 * (open) from a drag (move) and from stop. Fake app, invented meetings.
 */

const SHOTS = process.env.SHOT_DIR ?? "test-results/pill-open-meeting";

const args = (page: Page, cmd: string) =>
  page.evaluate(
    (c) =>
      ((window as any).__args as [string, any][])
        .filter(([name]) => name === c)
        .map(([, a]) => a),
    cmd,
  );

const clickPill = (page: Page, id: string | null) =>
  page.evaluate(
    (id) =>
      (window as any).__emit("open-meeting", {
        id,
        source: "pill",
        clicked_at_ms: Date.now(),
      }),
    id,
  );

test.describe("pill opens the meeting", () => {
  test("from another section, straight onto the recording meeting", async ({
    page,
  }) => {
    await open(page, {
      lang: "de",
      theme: "dark",
      recording: true,
      section: "history",
    });
    const content = page.locator("[data-section]");
    await expect(content).toHaveAttribute("data-section", "history");
    await clickPill(page, "rec");
    await expect(content).toHaveAttribute("data-section", "meetings");
    await expect(page.locator("[data-meeting-page]")).toHaveAttribute(
      "data-meeting-page",
      "rec",
    );
    // The page reports what it shows, for the log line.
    await expect
      .poll(async () => (await args(page, "meeting_page_shown")).length)
      .toBe(1);
    const [shown] = await args(page, "meeting_page_shown");
    expect(shown).toMatchObject({
      source: "pill",
      asked: "rec",
      section: "meetings",
      shown: "rec",
    });
    expect(shown.ms).toBeLessThan(1000);
    await page.screenshot({ path: `${SHOTS}/opened.png` });
  });

  test("another meeting selected: the pill switches to the recording one", async ({
    page,
  }) => {
    await open(page, {
      lang: "de",
      theme: "dark",
      recording: true,
      section: "meetings",
    });
    await page.getByText("Kundengespräch Lieferzeiten").first().click();
    await expect(page.locator("[data-meeting-page]")).toHaveAttribute(
      "data-meeting-page",
      "m2",
    );
    await clickPill(page, "rec");
    await expect(page.locator("[data-meeting-page]")).toHaveAttribute(
      "data-meeting-page",
      "rec",
    );
  });

  test("from the meeting settings, back to the meeting", async ({ page }) => {
    await open(page, {
      lang: "de",
      theme: "dark",
      recording: true,
      section: "meetings",
    });
    await page.getByRole("button", { name: "Einstellungen" }).first().click();
    await expect(page.locator("[data-meeting-page]")).toHaveCount(0);
    await clickPill(page, "rec");
    await expect(page.locator("[data-meeting-page]")).toHaveAttribute(
      "data-meeting-page",
      "rec",
    );
  });
});

test.describe("the pill itself", () => {
  const pill = (page: Page) =>
    open(page, {
      lang: "de",
      theme: "dark",
      recording: true,
      path: "/src/meeting-indicator/index.html",
    });

  test("a click on the waveform or time opens the meeting", async ({
    page,
  }) => {
    await pill(page);
    const body = page.getByRole("button", { name: "Meeting öffnen" });
    await expect(body).toBeVisible();
    await body.click();
    const calls: string[] = await page.evaluate(() => (window as any).__calls);
    expect(calls).toContain("open_meeting_from_pill");
    expect(calls).not.toContain("stop_meeting_recording");
  });

  test("a drag moves the pill and opens nothing", async ({ page }) => {
    await pill(page);
    const box = (await page
      .getByRole("button", { name: "Meeting öffnen" })
      .boundingBox())!;
    await page.mouse.move(box.x + 10, box.y + 10);
    await page.mouse.down();
    await page.mouse.move(box.x + 30, box.y + 12, { steps: 4 });
    await page.mouse.up();
    const calls: string[] = await page.evaluate(() => (window as any).__calls);
    expect(calls).toContain("plugin:window|start_dragging");
    expect(calls).not.toContain("open_meeting_from_pill");
  });

  test("stop stays stop", async ({ page }) => {
    await pill(page);
    await page.getByRole("button", { name: "Aufnahme beenden" }).click();
    const calls: string[] = await page.evaluate(() => (window as any).__calls);
    expect(calls).toContain("stop_meeting_recording");
    expect(calls).not.toContain("open_meeting_from_pill");
  });

  test("hover over the pill: lighter, and the hint after half a second", async ({
    page,
  }) => {
    await pill(page);
    // Rust sends hover (a background app's page gets none of its own).
    await page.evaluate(() =>
      (window as any).__emit("meeting-indicator-hover-pill", true),
    );
    await expect(page.locator(".body.is-hover")).toHaveCount(1);
    await expect(page.locator(".tip-open")).toHaveText("Meeting öffnen");
    await page.screenshot({ path: `${SHOTS}/pill-hover.png` });
    await page.evaluate(() =>
      (window as any).__emit("meeting-indicator-hover-pill", false),
    );
    await expect(page.locator(".tip-open")).toHaveCount(0);
  });
});
