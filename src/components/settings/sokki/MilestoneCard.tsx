import React, { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { openUrl } from "@tauri-apps/plugin-opener";
import { save } from "@tauri-apps/plugin-dialog";
import { commands, type Milestone } from "@/bindings";

/** Where "Chip in tokens" leads; sokki.org/sponsor/ forwards to GitHub Sponsors. */
const SPONSOR_URL = "https://sokki.org/sponsor/?s=stats";

/**
 * Off until GitHub Sponsors is approved and sokki.org/sponsor/ leads there.
 * While off, the card never shows, so no milestone is marked as shown and
 * users who pass 100 hours meanwhile still see it once it's on.
 */
export const SPONSORS_LIVE = false;

/** The shareable picture, 1200×630 (the size link previews use), as a PNG data URL. */
function drawPicture(hoursText: string, line: string): string {
  const w = 1200;
  const h = 630;
  const canvas = document.createElement("canvas");
  canvas.width = w;
  canvas.height = h;
  const c = canvas.getContext("2d");
  if (!c) return "";
  c.fillStyle = "#0e0f11";
  c.fillRect(0, 0, w, h);

  // Sokki's logo: seven bars, the middle one blue.
  const bars: [number, number, number][] = [
    [5, 11, 22],
    [13, 17, 22],
    [21, 23, 22],
    [29.5, 29, 26],
    [38, 23, 22],
    [46, 17, 22],
    [54, 11, 22],
  ];
  const s = 1.1;
  const ox = 72;
  const oy = 64;
  bars.forEach(([x, y, len], i) => {
    c.fillStyle = i === 3 ? "#8ab4ff" : "#ecedef";
    c.beginPath();
    c.roundRect(ox + x * s, oy + y * s, 5 * s, len * s, 2.5 * s);
    c.fill();
  });
  const font =
    "-apple-system, BlinkMacSystemFont, 'SF Pro Display', sans-serif";
  c.fillStyle = "#ecedef";
  c.font = `600 34px ${font}`;
  c.textBaseline = "middle";
  c.fillText("Sokki", ox + 82, oy + 36);

  c.textBaseline = "alphabetic";
  c.font = `700 150px ${font}`;
  c.fillText(hoursText, 72, 380);
  c.fillStyle = "#a1a1a6";
  c.font = `500 42px ${font}`;
  c.fillText(line, 76, 450);
  c.fillStyle = "#8ab4ff";
  c.font = `600 32px ${font}`;
  c.fillText("sokki.org", 76, 566);
  return canvas.toDataURL("image/png");
}

/**
 * At 100, 200, 500 and 1,000 hours transcribed: a picture to share and the
 * ask to sponsor Sokki. Once per milestone; "Sponsor" or "Not now" closes it.
 */
export const MilestoneCard: React.FC = () => {
  const { t } = useTranslation();
  const [milestone, setMilestone] = useState<Milestone | null>(null);
  const [picture, setPicture] = useState("");
  const [saved, setSaved] = useState(false);

  useEffect(() => {
    void commands.getMilestone().then((r) => {
      if (r.status === "ok" && r.data) setMilestone(r.data);
    });
  }, []);

  useEffect(() => {
    if (!milestone) return;
    setPicture(
      drawPicture(
        t("sokki.milestone.pictureHours", { hours: milestone.hours }),
        t("sokki.milestone.pictureLine"),
      ),
    );
  }, [milestone, t]);

  if (!milestone || !picture) return null;

  const close = () => {
    void commands.closeMilestone(milestone.hours);
    setMilestone(null);
  };

  const sponsor = () => {
    void openUrl(SPONSOR_URL);
    close();
  };

  const savePicture = async () => {
    const path = await save({
      defaultPath: `Sokki ${milestone.hours}.png`,
      filters: [{ name: "PNG", extensions: ["png"] }],
    });
    if (!path) return;
    const r = await commands.savePicture(path, picture.split(",")[1] ?? "");
    if (r.status === "ok") {
      setSaved(true);
      setTimeout(() => setSaved(false), 1800);
    }
  };

  return (
    <section className="mx-4 flex items-center gap-5 rounded-lg border border-line bg-background p-4">
      <img
        src={picture}
        alt=""
        className="w-[176px] shrink-0 rounded-md border border-line"
      />
      <div className="min-w-0 flex-1">
        <h2 className="text-[15px] font-semibold">
          {t("sokki.milestone.title", { hours: milestone.hours })}
        </h2>
        <p className="mt-1 text-[13px] text-ink-3">
          {t("sokki.milestone.body")}
        </p>
        <div className="mt-3 flex flex-wrap gap-2 text-[13px]">
          <button
            type="button"
            onClick={sponsor}
            className="cursor-pointer rounded-md border border-logo-primary bg-logo-primary px-2.5 py-1 font-medium text-background hover:opacity-90"
          >
            {t("sokki.milestone.sponsor")}
          </button>
          <button
            type="button"
            onClick={() => void savePicture()}
            className="cursor-pointer rounded-md border border-line px-2.5 py-1 hover:bg-wash"
          >
            {saved ? t("sokki.milestone.saved") : t("sokki.milestone.save")}
          </button>
          <button
            type="button"
            onClick={close}
            className="cursor-pointer rounded-md px-2.5 py-1 text-ink-3 hover:text-text"
          >
            {t("sokki.milestone.notNow")}
          </button>
        </div>
      </div>
    </section>
  );
};
