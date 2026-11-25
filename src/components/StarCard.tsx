import React, { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { listen } from "@tauri-apps/api/event";
import { openUrl } from "@tauri-apps/plugin-opener";
import { Star, X } from "lucide-react";
import { commands } from "@/bindings";
import { useMeetingStore } from "@/stores/meetingStore";
import { useNoticeStore } from "@/stores/noticeStore";
import { useCardStore } from "@/stores/cardStore";

/** sokki.org/github counts the click (`s` says where it came from), then
 * leads to the repository. */
export const STAR_URL = "https://sokki.org/github?s=star-card";

/**
 * "Enjoying Sokki?": asks once for a star on GitHub, after a meeting or
 * dictation finished and Sokki has written down `STAR_HOURS` hours
 * (`milestones.rs`), before the first donation ask. Never on the first
 * start, never while something records, never together with another card,
 * message or notice. Any answer closes it for good.
 */
export const StarCard: React.FC<{ firstStart: boolean }> = ({ firstStart }) => {
  const { t } = useTranslation();
  const [due, setDue] = useState(false);
  const recording = useMeetingStore((s) => s.recording);
  const notices = useNoticeStore((s) => s.toasts.length > 0 || s.panelOpen);
  const others = useCardStore((s) => s.shown.length > 0);

  const load = useCallback(async () => {
    const r = await commands.getStarAsk();
    if (r.status === "ok" && r.data) setDue(true);
  }, []);

  useEffect(() => {
    if (firstStart) return;
    // Sent right after a meeting or dictation finished, when it's due.
    const off = listen("sokki-star-ask", () => void load());
    // The window was closed then: it shows when the window opens again.
    const onFocus = () => void load();
    window.addEventListener("focus", onFocus);
    return () => {
      off.then((fn) => fn());
      window.removeEventListener("focus", onFocus);
    };
  }, [firstStart, load]);

  if (!due || firstStart || recording || notices || others) return null;

  const close = () => {
    setDue(false);
    void commands.closeStarAsk();
  };
  const star = () => {
    void openUrl(STAR_URL);
    close();
  };

  // Like T3 Code's card (Robin, 25.11.): an outlined star beside the title,
  // a quiet "Not now", the round X on the corner.
  return (
    <section
      aria-labelledby="sokki-star-title"
      className="fixed bottom-16 end-6 z-40 w-[360px] max-w-[calc(100vw-48px)] rounded-[12px] border border-line ps-4 pe-4 pt-3.5 pb-3 shadow-[0_16px_40px_rgba(0,0,0,0.35)] motion-safe:animate-rise"
      style={{ background: "var(--vb-card)" }}
    >
      <button
        type="button"
        onClick={close}
        aria-label={t("sokki.star.close")}
        className="absolute -top-2.5 -end-2.5 flex h-6 w-6 cursor-pointer items-center justify-center rounded-full border border-line-strong text-ink-2 transition-colors hover:text-text focus:outline-none focus-visible:ring-1 focus-visible:ring-logo-primary"
        style={{ background: "var(--vb-card)" }}
      >
        <X width={12} height={12} strokeWidth={2.25} />
      </button>
      <h2
        id="sokki-star-title"
        className="flex items-center gap-2 text-[15px] leading-[1.3] font-semibold"
      >
        <Star
          aria-hidden="true"
          width={17}
          height={17}
          strokeWidth={2}
          className="shrink-0 text-warning"
        />
        {t("sokki.star.title")}
      </h2>
      <p className="mt-1 text-[13.5px] text-ink-2">{t("sokki.star.body")}</p>
      <div className="mt-3 flex items-center justify-end gap-1">
        <button
          type="button"
          onClick={close}
          className="h-8 px-3 rounded-[10px] text-[13px] text-text hover:bg-wash cursor-pointer focus:outline-none focus-visible:ring-1 focus-visible:ring-logo-primary"
        >
          {t("sokki.star.notNow")}
        </button>
        <button
          type="button"
          onClick={star}
          className="h-8 px-3.5 rounded-[10px] bg-background-ui text-white text-[13px] font-medium hover:bg-background-ui/90 cursor-pointer focus:outline-none focus-visible:ring-2 focus-visible:ring-logo-primary"
        >
          {t("sokki.star.star")}
        </button>
      </div>
    </section>
  );
};
