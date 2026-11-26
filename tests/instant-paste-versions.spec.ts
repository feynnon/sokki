import { test } from "@playwright/test";
import { open } from "./fake-app";

// The Sokki panel between the instant paste and the edit (design exploration, not a
// regression test): the real overlay, invented text. The four versions on the design
// canvas combine these with a drawn text field.
// Runs only with DESIGN=1: DESIGN=1 bunx playwright test tests/instant-paste-versions.spec.ts
const OUT = process.env.SHOT_DIR ?? "test-results/instant-paste-versions";

const CLEANED =
  "Kurz zum Stand für Samstag: Wir treffen uns um vier bei mir im Garten.\n\nFür die Party bräuchten wir noch:\n\n- den Grill, den kann Tom mitbringen.\n- Getränke, Bier und Saft, die besorg ich.";

test.skip(!process.env.DESIGN, "design screenshots only with DESIGN=1");

for (const [name, size, cleaned] of [
  ["panel-offen", { width: 400, height: 184 }, CLEANED],
  ["pille", { width: 400, height: 120 }, ""],
] as const) {
  test(name, async ({ page }) => {
    await open(page, { lang: "de", theme: "dark", path: "/src/overlay/index.html" });
    await page.setViewportSize(size);
    await page.evaluate(() => (window as any).__emit("show-overlay", "streaming"));
    if (cleaned)
      await page.evaluate(
        (c) =>
          (window as any).__emit("stream-text-event", {
            cleaned: c,
            committed: "",
            tentative: "",
            cleanup: true,
          }),
        cleaned,
      );
    await page.evaluate(() =>
      (window as any).__emit("stream-phase-event", { phase: "working", kind: "polishing" }),
    );
    await page.waitForTimeout(700);
    await page.screenshot({ path: `${OUT}/${name}.png` });
  });
}
