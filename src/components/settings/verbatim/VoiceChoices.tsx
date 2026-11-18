import React from "react";
import { Trans, useTranslation } from "react-i18next";

export type VoiceChoice = "off" | "sell" | "donate";

/** The three answers to "what happens with your voice", as radio cards.
 * Used on the "Your voice" page and on first start. */
export const VoiceChoices: React.FC<{
  value: VoiceChoice | null;
  onChange: (choice: VoiceChoice) => void;
  /** First start offers only the two yes answers; "Not now" is a button. */
  withOff?: boolean;
}> = ({ value, onChange, withOff = true }) => {
  const { t } = useTranslation();
  const options: VoiceChoice[] = withOff
    ? ["sell", "donate", "off"]
    : ["sell", "donate"];
  return (
    <div
      role="radiogroup"
      aria-label={t("verbatim.voice.choiceLabel")}
      className="flex flex-col gap-2"
    >
      {options.map((c) => {
        const on = value === c;
        return (
          <button
            key={c}
            type="button"
            role="radio"
            aria-checked={on}
            onClick={() => onChange(c)}
            className={`flex items-start gap-3 rounded-xl border px-4 py-3 text-start cursor-pointer transition-colors focus:outline-none focus-visible:ring-1 focus-visible:ring-logo-primary ${
              on
                ? "border-logo-primary bg-logo-primary/10"
                : "border-line hover:bg-wash"
            }`}
          >
            <span
              aria-hidden="true"
              className={`mt-[3px] grid size-4 shrink-0 place-items-center rounded-full border ${
                on ? "border-logo-primary" : "border-line-strong"
              }`}
            >
              {on && <span className="size-2 rounded-full bg-logo-primary" />}
            </span>
            <span className="flex flex-col gap-0.5">
              <span className="text-[14px] font-medium">
                <Trans
                  i18nKey={`verbatim.voice.choice.${c}.title`}
                  components={{ b: <strong className="font-semibold" /> }}
                />
              </span>
              <span className="text-[12.5px] leading-[18px] text-ink-3">
                <Trans
                  i18nKey={`verbatim.voice.choice.${c}.text`}
                  components={{
                    b: <strong className="font-semibold text-ink-2" />,
                  }}
                />
              </span>
            </span>
          </button>
        );
      })}
    </div>
  );
};
