import React, { useState } from "react";
import ReactDOM from "react-dom/client";
import { useTranslation } from "react-i18next";
import { Monitor } from "lucide-react";
import "@/i18n";
import { commands } from "@/bindings";
import { SokkiMark } from "@/components/icons/SokkiLogo";
import appIcon from "@/assets/sokki-app-icon.png";
import "./prompt.css";

const params = new URLSearchParams(window.location.search);
const kindParam = params.get("kind");
const kind =
  kindParam === "ended" || kindParam === "auto" ? kindParam : "started";
const app = params.get("app") ?? "";
const offerScreen = params.get("screen") === "1";

const answer = (value: string) => void commands.meetingPromptAnswer(value);

/**
 * A meeting started by itself (canvas M2.2): one line like a macOS
 * notification, with an Auto-Start switch and an X over the corner that
 * shows with the mouse. The pill shows the recording, so nothing here says
 * it goes on. The switch flips at once and answers "off" a moment later,
 * so the user sees it go off; the X answers "close".
 */
const AutoCard: React.FC = () => {
  const { t } = useTranslation();
  const [on, setOn] = useState(true);
  const turnOff = () => {
    if (!on) return;
    setOn(false);
    window.setTimeout(() => answer("off"), 700);
  };
  return (
    <div className="auto-stage">
      <div className="auto-card" role="dialog" aria-labelledby="prompt-title">
        <button
          type="button"
          className="auto-close"
          aria-label={t("meetings.prompt.close")}
          onClick={() => answer("close")}
        >
          <svg width="9" height="9" viewBox="0 0 24 24" aria-hidden>
            <path
              d="M6 6l12 12M18 6 6 18"
              stroke="currentColor"
              strokeWidth="3.2"
              strokeLinecap="round"
            />
          </svg>
        </button>
        <span className="auto-icon" aria-hidden>
          <SokkiMark size={18} />
        </span>
        <span className="auto-title" id="prompt-title">
          {t("meetings.prompt.autoTitle")}
        </span>
        <button
          type="button"
          className="auto-switch"
          role="switch"
          aria-checked={on}
          onClick={turnOff}
        >
          {t("meetings.prompt.autoSwitch")}
          <span className="track" aria-hidden>
            <span className="knob" />
          </span>
        </button>
      </div>
    </div>
  );
};

/**
 * Asks when a call starts (Not now / Record, optionally with the screen) or
 * ends (Keep recording / Stop). The answer goes back to meeting/prompt.rs.
 */
const Prompt: React.FC = () => {
  const { t } = useTranslation();
  const [screen, setScreen] = useState(false);

  const started = kind === "started";
  const title = t(
    started ? "meetings.prompt.startedTitle" : "meetings.prompt.endedTitle",
    { app },
  );
  const hint = t(
    started ? "meetings.prompt.startedHint" : "meetings.prompt.endedHint",
  );
  return (
    <div className="prompt" role="dialog" aria-labelledby="prompt-title">
      {/* Who asks, like a macOS notification: app icon and name first
          (Robin, 2026-11-27: "Da muss unbedingt unser Icon rein, und sokki
          mit Namen, sonst checkt man das nicht!"; canvas Prompt-Sokki-1). */}
      <div className="sender">
        <img src={appIcon} width={16} height={16} alt="" />
        <span>{"Sokki"}</span>
        <span className="when">{t("meetings.prompt.now")}</span>
      </div>
      <div className="head">
        <div className="title" id="prompt-title">
          {title}
        </div>
        <div className="hint">{hint}</div>
      </div>
      {started && offerScreen && (
        <div className="option">
          <span className="option-label">
            <Monitor width={16} height={16} aria-hidden />
            {t("meetings.prompt.screen")}
          </span>
          <button
            type="button"
            className="switch"
            role="switch"
            aria-checked={screen}
            aria-label={t("meetings.prompt.screen")}
            onClick={() => setScreen((on) => !on)}
          >
            <span className="knob" />
          </button>
        </div>
      )}
      <div className="actions">
        {started ? (
          <>
            <button
              type="button"
              className="button"
              onClick={() => answer("not_now")}
            >
              {t("meetings.prompt.notNow")}
            </button>
            <button
              type="button"
              className="button primary"
              onClick={() => answer(screen ? "record_screen" : "record")}
            >
              {t("meetings.prompt.record")}
            </button>
          </>
        ) : (
          <>
            <button
              type="button"
              className="button"
              onClick={() => answer("keep")}
            >
              {t("meetings.prompt.keep")}
            </button>
            <button
              type="button"
              className="button primary"
              onClick={() => answer("stop")}
            >
              {t("meetings.prompt.stop")}
            </button>
          </>
        )}
      </div>
    </div>
  );
};

window.addEventListener("contextmenu", (e) => e.preventDefault());

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    {kind === "auto" ? <AutoCard /> : <Prompt />}
  </React.StrictMode>,
);
