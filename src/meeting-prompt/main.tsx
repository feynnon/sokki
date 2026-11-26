import React, { useState } from "react";
import ReactDOM from "react-dom/client";
import { useTranslation } from "react-i18next";
import { Monitor, Users } from "lucide-react";
import "@/i18n";
import { commands } from "@/bindings";
import "./prompt.css";

const params = new URLSearchParams(window.location.search);
const kindParam = params.get("kind");
const kind =
  kindParam === "ended" || kindParam === "auto" ? kindParam : "started";
const app = params.get("app") ?? "";
const offerScreen = params.get("screen") === "1";

/**
 * Asks when a call starts (Not now / Record, optionally with the screen) or
 * ends (Keep recording / Stop), or says a meeting started by itself
 * (Turn off auto start / Discard / Keep). The answer goes back to meeting/prompt.rs.
 */
const Prompt: React.FC = () => {
  const { t } = useTranslation();
  const [screen, setScreen] = useState(false);
  const answer = (value: string) => void commands.meetingPromptAnswer(value);

  const started = kind === "started";
  const auto = kind === "auto";
  const title = auto
    ? t("meetings.prompt.autoTitle")
    : t(
        started ? "meetings.prompt.startedTitle" : "meetings.prompt.endedTitle",
        { app },
      );
  const hint = auto
    ? t("meetings.prompt.autoHint")
    : t(started ? "meetings.prompt.startedHint" : "meetings.prompt.endedHint");
  return (
    <div className="prompt" role="dialog" aria-labelledby="prompt-title">
      <div className="head">
        <span className="logo" aria-hidden>
          <Users width={16} height={16} />
        </span>
        <div>
          <div className="title" id="prompt-title">
            {title}
          </div>
          <div className="hint">{hint}</div>
        </div>
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
        {auto ? (
          <>
            <button
              type="button"
              className="button quiet"
              onClick={() => answer("off")}
            >
              {t("meetings.prompt.turnOff")}
            </button>
            <span className="spacer" />
            <button
              type="button"
              className="button"
              onClick={() => answer("discard")}
            >
              {t("meetings.prompt.discard")}
            </button>
            <button
              type="button"
              className="button primary"
              onClick={() => answer("keep")}
            >
              {t("meetings.prompt.keepAuto")}
            </button>
          </>
        ) : started ? (
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
    <Prompt />
  </React.StrictMode>,
);
