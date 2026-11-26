import { test, expect } from "@playwright/test";
import { open } from "./fake-app";

/**
 * Own notes (Robin, 2026-11-26): while recording, a notes field on the
 * left and the live text on the right ("1 · Nebeneinander"); afterwards
 * Notizen | Kombiniert | AI-Zusammenfassung | Transkript, and the toolbar
 * in design C (grey copy buttons with icons, blue "An alle senden").
 * Fake app, invented meeting.
 */

const SHOTS = process.env.SHOT_DIR ?? "test-results/own-notes";

const live = [
  { start_s: 190, text: "Für den Herbst haben wir viertausend Euro." },
  { start_s: 578, text: "Die Druckerei braucht die Dateien bis Freitag." },
];

test("while recording: notes left, live text right, saved as typed", async ({
  page,
}) => {
  await open(page, {
    lang: "de",
    theme: "dark",
    section: "meetings",
    recording: true,
    extra: { meeting_live_transcript: live },
  });
  await page.setViewportSize({ width: 1280, height: 800 });
  const notes = page.getByRole("textbox", { name: "Deine Notizen" });
  await expect(notes).toBeVisible();
  await expect(notes).toBeFocused();
  const column = page.locator("[data-live-column]");
  await expect(column).toContainText("Live-Text");
  await expect(column).toContainText("03:10");
  await expect(column).toContainText("Druckerei braucht die Dateien");
  // Side by side: the live text is to the right of the notes.
  const n = (await notes.boundingBox())!;
  const c = (await column.boundingBox())!;
  expect(c.x).toBeGreaterThan(n.x + n.width - 1);

  await notes.pressSequentially("Budget?");
  await notes.press("Enter");
  await notes.pressSequentially("Druckerei bis Fr");
  await page.screenshot({ path: `${SHOTS}/recording-de.png` });
  // Saved a moment after the last key.
  await expect
    .poll(() => page.evaluate(() => (window as any).__notes))
    .toEqual(expect.objectContaining({}));
  await expect
    .poll(() =>
      page.evaluate(() => Object.values((window as any).__notes ?? {})[0]),
    )
    .toBe("Budget?\nDruckerei bis Fr");
});

const combined =
  "- Budget?\n  - 4.000 € gesamt, davon 1.500 € für Anzeigen\n- Druckerei bis Fr\n  - Dateien bis Freitag 12 Uhr";

async function done(page: import("@playwright/test").Page, withNotes: boolean) {
  await open(page, {
    lang: "de",
    theme: "dark",
    section: "meetings",
    extra: {
      get_meeting_note: {
        summary:
          "## Zusammenfassung\n\n- Die Herbstaktion startet am 6. Oktober.",
        transcript: "[00:00] Lea: Für den Herbst haben wir viertausend Euro.",
        combined: withNotes ? combined : "",
      },
      get_meeting_notes: withNotes ? "Budget?\nDruckerei bis Fr" : "",
    },
  });
  await page.setViewportSize({ width: 1280, height: 800 });
}

test("afterwards: four views, Kombiniert first when notes were typed", async ({
  page,
}) => {
  await done(page, true);
  const tabs = page.getByRole("tablist", { name: "Ansichten" });
  await expect(tabs.getByRole("tab")).toHaveText([
    "Notizen",
    "Kombiniert",
    "AI-Zusammenfassung",
    "Transkript",
  ]);
  await expect(tabs.getByRole("tab", { name: "Kombiniert" })).toHaveAttribute(
    "aria-selected",
    "true",
  );
  const view = page.locator("[data-combined]");
  await expect(view).toContainText("Budget?");
  await expect(view).toContainText("4.000 € gesamt");
  await page.screenshot({ path: `${SHOTS}/combined-de.png` });

  await tabs.getByRole("tab", { name: "Notizen" }).click();
  await expect(
    page.getByRole("textbox", { name: "Deine Notizen" }),
  ).toHaveValue("Budget?\nDruckerei bis Fr");
  await tabs.getByRole("tab", { name: "AI-Zusammenfassung" }).click();
  await expect(
    page.getByText("Die Herbstaktion startet am 6. Oktober."),
  ).toBeVisible();
  await tabs.getByRole("tab", { name: "Transkript" }).click();
  await expect(page.getByText("viertausend Euro")).toBeVisible();
  // The choice is remembered.
  await page.reload();
  await expect(page.getByRole("tab", { name: "Transkript" })).toHaveAttribute(
    "aria-selected",
    "true",
  );
});

test("afterwards without notes: no Kombiniert, the summary first", async ({
  page,
}) => {
  await done(page, false);
  const tabs = page.getByRole("tablist", { name: "Ansichten" });
  await expect(tabs.getByRole("tab")).toHaveText([
    "Notizen",
    "AI-Zusammenfassung",
    "Transkript",
  ]);
  await expect(
    tabs.getByRole("tab", { name: "AI-Zusammenfassung" }),
  ).toHaveAttribute("aria-selected", "true");
});

test("toolbar: copy buttons keep their place, each with an icon", async ({
  page,
}) => {
  await done(page, true);
  const bar = page.locator("[data-reader-toolbar]");
  for (const name of ["Transkript kopieren", "Zusammenfassung kopieren"]) {
    const b = bar.getByRole("button", { name });
    await expect(b).toBeVisible();
    await expect(b.locator("svg")).toHaveCount(1);
  }
  await page.screenshot({
    path: `${SHOTS}/toolbar-de.png`,
    clip: { x: 640, y: 0, width: 640, height: 60 },
  });
});
