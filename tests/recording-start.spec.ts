import { test, expect } from "@playwright/test";
import { open } from "./fake-app";

/**
 * Settings › Meetings, group "Aufnahme starten" (canvas version C, Robin's
 * pick 2026-11-26): the call switch, the auto start switch and the last
 * three meetings that started by themselves. Headless, with the fake app
 * from fake-app.ts; made-up titles. Screenshots land in
 * test-results/recording-start/ (or $SHOT_DIR).
 */

const SHOTS = process.env.SHOT_DIR ?? "test-results/recording-start";

const now = new Date();
const at = (daysAgo: number, h: number, m: number) => {
  const d = new Date(now);
  d.setDate(d.getDate() - daysAgo);
  d.setHours(h, m, 0, 0);
  return d.toISOString();
};
const RECENT = [
  {
    meeting_id: "a",
    started_at: at(0, 14, 1),
    outcome: "kept",
    talk_s: 720,
    title: "Planung Winter",
  },
  {
    meeting_id: "b",
    started_at: at(1, 16, 40),
    outcome: "deleted",
    talk_s: 62,
    title: null,
  },
  {
    meeting_id: "c",
    started_at: at(3, 10, 12),
    outcome: "stopped",
    talk_s: null,
    title: "Kaffee mit Jana",
  },
  {
    meeting_id: "d",
    started_at: at(4, 9, 0),
    outcome: "kept",
    talk_s: 300,
    title: "Nicht gezeigt",
  },
];

test("Aufnahme starten: both switches on top, the last three auto starts", async ({
  page,
}) => {
  await open(page, {
    lang: "de",
    theme: "dark",
    section: "meetings",
    extra: { auto_meeting_recent: RECENT },
  });
  await page.getByRole("button", { name: "Meeting-Einstellungen" }).click();
  await expect(page.getByText("Aufnahme starten")).toBeVisible();
  // The group comes before "Notizen".
  const start = await page.getByText("Aufnahme starten").boundingBox();
  const notes = await page.getByText("Notizen", { exact: true }).boundingBox();
  expect(start!.y).toBeLessThan(notes!.y);
  await expect(page.getByText("Bei Calls fragen")).toBeVisible();
  await expect(page.getByText("Gespräche von selbst aufnehmen")).toBeVisible();
  const list = page.getByRole("list", { name: "Zuletzt von selbst gestartet" });
  await expect(list.getByRole("listitem")).toHaveCount(3);
  await expect(list).toContainText("Planung Winter");
  await expect(list).toContainText("12 Min.");
  await expect(list).toContainText("gelöscht, zu kurz");
  await expect(list).toContainText("von dir gestoppt");
  await expect(list).not.toContainText("Nicht gezeigt");
  await page.waitForTimeout(300);
  await page.screenshot({
    path: `${SHOTS}/recording-start-de.png`,
    fullPage: true,
  });

  // Turning it off here records that the person chose it.
  const sw = page.getByRole("checkbox", {
    name: "Gespräche von selbst aufnehmen",
  });
  await sw.click({ force: true });
  const saved = await page.evaluate(() =>
    ((window as any).__args as [string, any][])
      .filter(([c]) => c === "set_meeting_settings")
      .map(([, a]) => a.settings ?? a),
  );
  const last = saved[saved.length - 1];
  expect(last.auto_start_enabled).toBe(false);
  expect(last.auto_start_set).toBe(true);
});

test("Aufnahme starten without any auto start yet: no list", async ({
  page,
}) => {
  await open(page, {
    lang: "de",
    theme: "dark",
    section: "meetings",
    extra: { auto_meeting_recent: [] },
  });
  await page.getByRole("button", { name: "Meeting-Einstellungen" }).click();
  await expect(page.getByText("Gespräche von selbst aufnehmen")).toBeVisible();
  await expect(page.getByText("Zuletzt von selbst gestartet")).toHaveCount(0);
});
