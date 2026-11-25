import { test, expect, type Page } from "@playwright/test";
import { open } from "./fake-app";

/**
 * The reader's toolbar at narrow windows (Robin, 2026-11-25: buttons
 * wrapped and spilled out of their borders). Buttons never wrap; when the
 * labels don't fit, the secondary ones turn into icons with a tooltip.
 * Fake app, invented people.
 */

const SHOTS = process.env.SHOT_DIR ?? "test-results/reader-toolbar";

const ready = {
  meeting_mail_preview: {
    event: "Weekly Nordlicht",
    to: [
      { name: "Mira Muster", email: "mira@example.com" },
      { name: "Tim Berger", email: "tim@example.com" },
    ],
    skipped: [],
    subject: "Notizen: Weekly Nordlicht",
    body: "…",
    problem: null,
    mailed: null,
    sending: false,
  },
};

async function toolbar(page: Page, lang: "de" | "en", width: number) {
  await open(page, { lang, theme: "dark", section: "meetings", extra: ready });
  await page.setViewportSize({ width, height: 720 });
  const send = page.getByRole("button", {
    name: lang === "de" ? "An alle senden" : "Send to everyone",
  });
  await expect(send).toBeVisible();
  return send;
}

/** Every toolbar button is one line high and its content stays inside. */
async function noSpill(page: Page) {
  const bad = await page.evaluate(() => {
    const out: string[] = [];
    const bar = document.querySelector("[data-reader-toolbar]");
    if (!bar) return ["no toolbar"];
    for (const b of Array.from(bar.querySelectorAll("button"))) {
      const r = b.getBoundingClientRect();
      if (
        r.height > 30 ||
        b.scrollWidth > b.clientWidth + 1 ||
        b.scrollHeight > b.clientHeight + 1
      )
        out.push(
          `${b.textContent || b.getAttribute("aria-label")}: ${Math.round(r.width)}x${Math.round(r.height)} scroll ${b.scrollWidth}`,
        );
    }
    const br = bar.getBoundingClientRect();
    if (bar.scrollWidth > bar.clientWidth + 1)
      out.push(`bar overflows ${bar.scrollWidth}>${bar.clientWidth}`);
    out.push(`(bar ${Math.round(br.width)})`);
    return out;
  });
  return bad;
}

for (const lang of ["de", "en"] as const) {
  for (const width of [820, 900, 1000, 1120, 1280]) {
    test(`${lang} at ${width}px: nothing wraps or spills`, async ({ page }) => {
      await toolbar(page, lang, width);
      const bad = await noSpill(page);
      console.log(lang, width, bad.join(" | "));
      expect(bad.filter((b) => !b.startsWith("("))).toEqual([]);
      await page.waitForTimeout(200);
      await page.screenshot({ path: `${SHOTS}/${lang}-${width}.png` });
      await page
        .locator("[data-reader-toolbar]")
        .screenshot({ path: `${SHOTS}/bar-${lang}-${width}.png` });
    });
  }
}

test("narrow: icons carry their names; wide again: labels come back", async ({
  page,
}) => {
  await toolbar(page, "de", 820);
  const summary = page.getByRole("button", {
    name: "Zusammenfassung kopieren",
  });
  await expect(summary).toHaveAttribute("title", "Zusammenfassung kopieren");
  await expect(summary).toHaveText("");
  await expect(
    page.getByRole("button", { name: "An alle senden" }),
  ).toHaveAttribute("title", /^An alle senden: /);
  await expect(page.getByText("Transkript kopieren")).toBeVisible();
  await page.setViewportSize({ width: 1280, height: 720 });
  await expect(summary).toHaveText("Zusammenfassung kopieren");
  await page.setViewportSize({ width: 820, height: 720 });
  await expect(summary).toHaveText("");
});
