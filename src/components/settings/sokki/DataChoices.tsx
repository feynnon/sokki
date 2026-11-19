import React, { useState } from "react";
import { useTranslation } from "react-i18next";
import { Check } from "lucide-react";
import { Button } from "@/components/ui/Button";
import { Dialog } from "@/components/ui/Dialog";
import { Switch } from "@/components/ui/Switch";

export interface DataChoiceValues {
  improve: boolean;
  support: boolean;
}

type Sheet = "shared" | "support";

const SHARED_ITEMS = [
  "mistakes",
  "usage",
  "problems",
  "data",
  "conditions",
  "mac",
] as const;

/** "What's shared" and "What this means", opened from the choices' links. */
const Sheets: React.FC<{ sheet: Sheet | null; onClose: () => void }> = ({
  sheet,
  onClose,
}) => {
  const { t } = useTranslation();
  const ok = (
    <div className="flex justify-end">
      <Button variant="primary" size="sm" onClick={onClose}>
        {t("sokki.voice.ok")}
      </Button>
    </div>
  );
  return (
    <>
      <Dialog
        open={sheet === "shared"}
        title={t("sokki.voice.sharedSheet.title")}
        closeLabel={t("common.close")}
        onOpenChange={(open) => !open && onClose()}
      >
        <div className="flex flex-col gap-3 text-body leading-[1.5]">
          <ul className="flex flex-col gap-1.5 list-disc ps-5">
            {SHARED_ITEMS.map((k) => (
              <li key={k}>{t(`sokki.voice.sharedSheet.${k}`)}</li>
            ))}
          </ul>
          <p className="text-small leading-[1.45] text-ink-2">
            {t("sokki.voice.sharedSheet.footer")}
          </p>
          {ok}
        </div>
      </Dialog>
      <Dialog
        open={sheet === "support"}
        title={t("sokki.voice.support.title")}
        closeLabel={t("common.close")}
        onOpenChange={(open) => !open && onClose()}
      >
        <div className="flex flex-col gap-3 text-body leading-[1.5]">
          <p>{t("sokki.voice.supportSheet.text")}</p>
          <p className="text-small leading-[1.45] text-ink-2">
            {t("sokki.voice.supportSheet.footer")}
          </p>
          {ok}
        </div>
      </Dialog>
    </>
  );
};

/** The two things a user can agree to share. First start shows them as
 * checkboxes, Settings as switches; both as rows split by a hairline: a
 * rounded row title over small text, the sheet's link at its end. */
export const DataChoices: React.FC<{
  value: DataChoiceValues;
  onChange: (value: DataChoiceValues) => void;
  variant: "checkbox" | "switch";
}> = ({ value, onChange, variant }) => {
  const { t } = useTranslation();
  const [sheet, setSheet] = useState<Sheet | null>(null);
  const rows: { key: keyof DataChoiceValues; sheet: Sheet }[] = [
    { key: "improve", sheet: "shared" },
    { key: "support", sheet: "support" },
  ];
  const set = (key: keyof DataChoiceValues, on: boolean) =>
    onChange({ ...value, [key]: on });

  return (
    <>
      <div className="flex flex-col">
        {rows.map((r) => {
          const on = value[r.key];
          const labelId = `data-choice-${r.key}`;
          const text = (
            <span className="block">
              <b
                id={labelId}
                className="block font-display text-row font-[650] leading-[1.3]"
              >
                {t(`sokki.voice.${r.key}.title`)}
              </b>
              <span className="mt-[2px] block text-small leading-[1.45] text-ink-2 text-pretty">
                {t(`sokki.voice.${r.key}.text`)}{" "}
                <button
                  type="button"
                  onClick={(e) => {
                    e.stopPropagation();
                    setSheet(r.sheet);
                  }}
                  // Enter and Space open the sheet, not tick the box around it.
                  onKeyDown={(e) => e.stopPropagation()}
                  className="cursor-pointer text-logo-primary hover:underline focus:outline-none focus-visible:underline"
                >
                  {t(`sokki.voice.${r.key}.link`)}
                </button>
              </span>
            </span>
          );
          const rowClass = "py-4 [&+&]:border-t [&+&]:border-line";
          if (variant === "switch") {
            return (
              <div
                key={r.key}
                className={`flex items-center gap-4 text-start ${rowClass}`}
              >
                <span className="grow">{text}</span>
                <Switch
                  checked={on}
                  onChange={(c) => set(r.key, c)}
                  ariaLabelledBy={labelId}
                />
              </div>
            );
          }
          return (
            <div
              key={r.key}
              role="checkbox"
              aria-checked={on}
              aria-labelledby={labelId}
              tabIndex={0}
              onClick={() => set(r.key, !on)}
              onKeyDown={(e) => {
                if (e.key === " " || e.key === "Enter") {
                  e.preventDefault();
                  set(r.key, !on);
                }
              }}
              className={`grid cursor-pointer grid-cols-[18px_1fr] items-start gap-[14px] text-start focus:outline-none focus-visible:ring-1 focus-visible:ring-logo-primary rounded-[4px] ${rowClass}`}
            >
              <span
                aria-hidden="true"
                className={`mt-[3px] grid size-[18px] place-items-center rounded-[5px] border ${
                  on
                    ? "border-logo-primary bg-logo-primary text-white"
                    : "border-line-strong"
                }`}
              >
                {on && <Check width={13} height={13} strokeWidth={3} />}
              </span>
              {text}
            </div>
          );
        })}
      </div>
      <Sheets sheet={sheet} onClose={() => setSheet(null)} />
    </>
  );
};
