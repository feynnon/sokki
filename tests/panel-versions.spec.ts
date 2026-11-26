import { test } from "@playwright/test";
import { open } from "./fake-app";

// Four looks for the raw words after the cleaned text (design exploration,
// not a regression test): same invented text, same 400 × 184 panel, CSS only.
// Robin chose 2-gedimmt on 26.11.2026; it is built (`.stext-cap.cleanup`).
// Runs only with DESIGN=1: DESIGN=1 bunx playwright test tests/panel-versions.spec.ts
const OUT = process.env.SHOT_DIR ?? "test-results/panel-versions";

const LIST =
  "Kurz zum Stand für Samstag: Wir treffen uns um vier bei mir im Garten.\n\nFür die Party bräuchten wir noch:\n\n- den Grill, den kann Tom mitbringen.\n- Getränke, Bier und Saft, die besorg ich.";
const RAW = "Und dann noch die Musik, ähm, die Box, die Lautsprecherbox meine ich,";
const TENT = "die hat die Anna";
const DONE = LIST + "\n- die Musikbox, die hat Anna.";

const VERSIONS: Record<string, string> = {
  "1-kursiv": "",
  "2-gedimmt": `.stext-cap .committed, .stext-cap .tentative { font-style: normal !important; opacity: .5; }`,
  "3-unterstrichen": `.stext-cap .committed, .stext-cap .tentative { font-style: normal !important;
     text-decoration: underline dotted; text-decoration-color: rgba(255,255,255,.45); text-underline-offset: 3px; }`,
  "4-markiert": `.stext-cap .committed, .stext-cap .tentative { font-style: normal !important;
     background: rgba(138,180,255,.16); border-radius: 4px; box-decoration-break: clone; -webkit-box-decoration-break: clone; padding: 0 2px; }`,
};

test.skip(!process.env.DESIGN, "design screenshots only with DESIGN=1");

for (const [name, css] of Object.entries(VERSIONS)) {
  for (const [state, cleaned, committed, tentative] of [
    ["sprechen", LIST, RAW, TENT],
    ["aufgeraeumt", DONE, "", "Und wer"],
  ] as const) {
    test(`${name} ${state}`, async ({ page }) => {
      await open(page, { lang: "de", theme: "dark", path: "/src/overlay/index.html" });
      await page.setViewportSize({ width: 400, height: 184 });
      if (css) await page.addStyleTag({ content: css });
      await page.evaluate(() => (window as any).__emit("show-overlay", "streaming"));
      await page.evaluate((p) => (window as any).__emit("stream-text-event", p), { cleaned, committed, tentative });
      await page.waitForTimeout(600);
      await page.screenshot({ path: `${OUT}/${name}-${state}.png` });
    });
  }
}
