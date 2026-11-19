import React, { useState } from "react";
import { useTranslation } from "react-i18next";
import { Check } from "lucide-react";
import { Button } from "@/components/ui/Button";
import { Dialog } from "@/components/ui/Dialog";
import { Switch } from "@/components/ui/Switch";

export interface DataChoiceValues {
  improve: boolean;
  /** "Ask me when there's a buyer". */
  buyers: boolean;
  /** "Share all my recordings". */
  support: boolean;
}

type Sheet = "shared" | "support";

const SHARED_ITEMS = ["usage", "data", "problems", "mac"] as const;

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

/** What a user can agree to. First start shows "Help improve" and the buyer
 * question as checkboxes (a rounded row title over small text); Settings
 * adds "Share all my recordings", all as switches in its own row style. A
 * sheet's link ends a row's text. */
export const DataChoices: React.FC<{
  value: DataChoiceValues;
  onChange: (value: DataChoiceValues) => void;
  variant: "checkbox" | "switch";
}> = ({ value, onChange, variant }) => {
  const { t } = useTranslation();
  const [sheet, setSheet] = useState<Sheet | null>(null);
  const all: { key: keyof DataChoiceValues; sheet?: Sheet }[] = [
    { key: "improve", sheet: "shared" },
    { key: "buyers" },
    { key: "support", sheet: "support" },
  ];
  const rows =
    variant === "switch" ? all : all.filter((r) => r.key !== "support");
  const set = (key: keyof DataChoiceValues, on: boolean) =>
    onChange({ ...value, [key]: on });
  const sheetLink = (key: keyof DataChoiceValues, to?: Sheet) =>
    to && (
      <button
        type="button"
        onClick={(e) => {
          e.stopPropagation();
          setSheet(to);
        }}
        // Enter and Space open the sheet, not tick the box around it.
        onKeyDown={(e) => e.stopPropagation()}
        className="cursor-pointer text-logo-primary hover:underline focus:outline-none focus-visible:underline"
      >
        {t(`sokki.voice.${key}.link`)}
      </button>
    );

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
                {t(`sokki.voice.${r.key}.text`)} {sheetLink(r.key, r.sheet)}
              </span>
            </span>
          );
          const rowClass = "py-4 [&+&]:border-t [&+&]:border-line";
          if (variant === "switch") {
            // A Settings row: same type and padding as SettingContainer.
            return (
              <div
                key={r.key}
                className="flex min-h-[60px] items-center justify-between gap-8 px-4 py-3 text-start [&+&]:border-t [&+&]:border-line"
              >
                <span className="min-w-0 flex-1">
                  <h3
                    id={labelId}
                    className="text-[14px] font-medium leading-5"
                  >
                    {t(`sokki.voice.${r.key}.title`)}
                  </h3>
                  <span className="mt-0.5 block text-[12.5px] leading-[18px] text-ink-3">
                    {t(`sokki.voice.${r.key}.text`)} {sheetLink(r.key, r.sheet)}
                  </span>
                </span>
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
