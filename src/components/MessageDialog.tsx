import React, { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { listen } from "@tauri-apps/api/event";
import { openUrl } from "@tauri-apps/plugin-opener";
import { commands, type MessageView } from "@/bindings";
import { useMeetingStore } from "@/stores/meetingStore";

/**
 * An important message from Sokki: a window in the app the user answers
 * (design D, Robin 2026-11-23: "einfach nur wichtige Nachrichten"). One at
 * a time; never while a meeting records and never on the first start.
 * "Later" brings it back after a day; its action or "OK" ends it; past its
 * last day it never shows.
 */
export const MessageDialog: React.FC<{ firstStart: boolean }> = ({
  firstStart,
}) => {
  const { t } = useTranslation();
  const recording = useMeetingStore((s) => s.recording);
  const [message, setMessage] = useState<MessageView | null>(null);
  const primary = useRef<HTMLButtonElement>(null);

  const load = useCallback(async () => {
    const r = await commands.pendingMessage();
    setMessage(r.status === "ok" ? r.data : null);
  }, []);

  useEffect(() => {
    if (firstStart) return;
    void load();
    const offNew = listen("sokki-message", () => void load());
    // Opening the window again shows a message put off until now.
    const onFocus = () => void load();
    window.addEventListener("focus", onFocus);
    return () => {
      offNew.then((fn) => fn());
      window.removeEventListener("focus", onFocus);
    };
  }, [firstStart, load]);

  const shown = message && !recording && !firstStart;
  useEffect(() => {
    if (shown) primary.current?.focus();
  }, [shown]);

  if (!shown || !message) return null;

  const answer = (a: "later" | "done") => {
    setMessage(null);
    void commands.answerMessage(message.id, a);
  };
  const act = () => {
    answer("done");
    if (message.action_url) void openUrl(message.action_url);
    else if (message.action_update)
      // Installs when one is ready (never while something records or is
      // processed); otherwise looks for it now.
      void commands.updateNow().then((r) => {
        if (r.status !== "ok") void commands.triggerUpdateCheck();
      });
  };

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/45"
      onKeyDown={(e) => {
        if (e.key === "Escape") answer("later");
      }}
    >
      <div
        role="alertdialog"
        aria-modal="true"
        aria-labelledby="sokki-message-title"
        aria-describedby="sokki-message-text"
        className="w-[440px] max-w-[calc(100vw-32px)] p-6 rounded-[14px] border border-line flex flex-col gap-1.5 shadow-[0_24px_60px_rgba(0,0,0,0.55)]"
        style={{ background: "var(--vb-card)" }}
      >
        <h2
          id="sokki-message-title"
          className="font-display text-[17px] leading-[1.3] font-[650]"
        >
          {message.title}
        </h2>
        <p
          id="sokki-message-text"
          className="text-body text-ink-2 select-text whitespace-pre-line"
        >
          {message.text}
        </p>
        <div className="mt-4 flex justify-end gap-2">
          {message.action_label && (
            <button
              type="button"
              onClick={() => answer("later")}
              className="h-8 px-3.5 rounded-[10px] border border-line-strong text-[13px] text-text hover:bg-wash cursor-pointer focus:outline-none focus-visible:ring-1 focus-visible:ring-logo-primary"
            >
              {t("sokki.message.later")}
            </button>
          )}
          <button
            ref={primary}
            type="button"
            onClick={message.action_label ? act : () => answer("done")}
            className="h-8 px-3.5 rounded-[10px] bg-background-ui text-white text-[13px] font-medium hover:bg-background-ui/90 cursor-pointer focus:outline-none focus-visible:ring-2 focus-visible:ring-logo-primary"
          >
            {message.action_label ?? t("sokki.message.ok")}
          </button>
        </div>
      </div>
    </div>
  );
};
