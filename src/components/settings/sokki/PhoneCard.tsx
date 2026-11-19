import React, { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { openUrl } from "@tauri-apps/plugin-opener";
import { commands } from "@/bindings";

/** Where the card sends people; `s` says which surface the visit came from. */
const GET_URL = "https://sokki.org/get?s=mac-card";
const DISMISSED_KEY = "sokki.phoneCard.dismissed";
/** Dictations before the card appears, unless a meeting note comes first. */
const DICTATIONS_BEFORE_CARD = 20;

function dismissed(): boolean {
  try {
    return localStorage.getItem(DISMISSED_KEY) === "1";
  } catch {
    return false;
  }
}

/**
 * "Sokki on your phone too": shown on Home only once Sokki has been used a
 * bit (the first finished meeting note or the 20th dictation), never at
 * setup, and not when a phone is already paired. "Not now" hides it for good.
 */
export const PhoneCard: React.FC = () => {
  const { t } = useTranslation();
  const [qr, setQr] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    if (dismissed()) return;
    let cancelled = false;
    (async () => {
      const [meetings, history, link] = await Promise.all([
        commands.listMeetings(),
        commands.getHistoryEntries(null, 1),
        commands.getPhoneLinkInfo(),
      ]);
      if (link.status === "ok" && link.data.phones.length > 0) return;
      const meetingDone =
        meetings.status === "ok" &&
        meetings.data.some((m) => m.status.kind === "done");
      // Entry ids only grow, so the newest id counts every dictation so far.
      const newest =
        history.status === "ok" ? (history.data.entries[0]?.id ?? 0) : 0;
      if (!meetingDone && newest < DICTATIONS_BEFORE_CARD) return;
      const svg = await commands.qrCodeSvg(GET_URL);
      if (!cancelled && svg.status === "ok") setQr(svg.data);
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  if (!qr) return null;

  const copy = async () => {
    try {
      await navigator.clipboard.writeText(GET_URL);
      setCopied(true);
      setTimeout(() => setCopied(false), 1800);
    } catch {
      /* the link stays visible to select */
    }
  };

  const email = () => {
    const subject = encodeURIComponent(t("sokki.phoneCard.mailSubject"));
    const body = encodeURIComponent(
      `${t("sokki.phoneCard.mailBody")}\n\n${GET_URL}`,
    );
    openUrl(`mailto:?subject=${subject}&body=${body}`);
  };

  const notNow = () => {
    try {
      localStorage.setItem(DISMISSED_KEY, "1");
    } catch {
      /* without storage it comes back next start; harmless */
    }
    setQr(null);
  };

  return (
    <section className="mx-4 flex items-center gap-5 rounded-lg border border-line bg-background p-4">
      <div
        className="h-[112px] w-[112px] shrink-0 overflow-hidden rounded-md bg-white p-1 [&_svg]:h-full [&_svg]:w-full"
        // Generated in Rust from a fixed link; contains no user input.
        dangerouslySetInnerHTML={{ __html: qr }}
      />
      <div className="min-w-0 flex-1">
        <h2 className="text-[15px] font-semibold">
          {t("sokki.phoneCard.title")}
        </h2>
        <p className="mt-1 text-[13px] text-ink-3">
          {t("sokki.phoneCard.body")}
        </p>
        <p className="mt-1 select-text text-[12px] text-ink-3">
          {t("sokki.phoneCard.scan")} · sokki.org/get
        </p>
        <div className="mt-3 flex flex-wrap gap-2 text-[13px]">
          <button
            type="button"
            onClick={copy}
            className="cursor-pointer rounded-md border border-line px-2.5 py-1 hover:bg-wash"
          >
            {copied ? t("sokki.phoneCard.copied") : t("sokki.phoneCard.copy")}
          </button>
          <button
            type="button"
            onClick={email}
            className="cursor-pointer rounded-md border border-line px-2.5 py-1 hover:bg-wash"
          >
            {t("sokki.phoneCard.email")}
          </button>
          <button
            type="button"
            onClick={notNow}
            className="cursor-pointer rounded-md px-2.5 py-1 text-ink-3 hover:text-text"
          >
            {t("sokki.phoneCard.notNow")}
          </button>
        </div>
      </div>
    </section>
  );
};
