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

type Sheet = "shared" | "support" | null;

const SHARED_ITEMS = [
  "mistakes",
  "usage",
  "problems",
  "data",
  "conditions",
  "mac",
] as const;

/** "What's shared" and "What this means", opened from the choices' links. */
const Sheets: React.FC<{ sheet: Sheet; onClose: () => void }> = ({
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
        <div className="flex flex-col gap-3 text-[13.5px] leading-[20px]">
          <ul className="flex flex-col gap-1.5 list-disc ps-5">
            {SHARED_ITEMS.map((k) => (
              <li key={k}>{t(`sokki.voice.sharedSheet.${k}`)}</li>
            ))}
          </ul>
          <p className="text-ink-3">{t("sokki.voice.sharedSheet.footer")}</p>
          {ok}
        </div>
      </Dialog>
      <Dialog
        open={sheet === "support"}
        title={t("sokki.voice.support.title")}
        closeLabel={t("common.close")}
        onOpenChange={(open) => !open && onClose()}
      >
        <div className="flex flex-col gap-3 text-[13.5px] leading-[20px]">
          <p>{t("sokki.voice.supportSheet.text")}</p>
          <p className="text-ink-3">{t("sokki.voice.supportSheet.footer")}</p>
          {ok}
        </div>
      </Dialog>
    </>
  );
};

/** The two things a user can agree to share. First start shows them as
 * checkboxes with the full text, Settings as switches with a short one. */
export const DataChoices: React.FC<{
  value: DataChoiceValues;
  onChange: (value: DataChoiceValues) => void;
  variant: "checkbox" | "switch";
}> = ({ value, onChange, variant }) => {
  const { t } = useTranslation();
  const [sheet, setSheet] = useState<Sheet>(null);
  const rows = [
    {
      key: "improve" as const,
      title: t("sokki.voice.improve.title"),
      text: t(
        variant === "checkbox"
          ? "sokki.voice.improve.text"
          : "sokki.voice.improve.short",
      ),
      link: t("sokki.voice.improve.link"),
      sheet: "shared" as const,
    },
    {
      key: "support" as const,
      title: t("sokki.voice.support.title"),
      text: t("sokki.voice.support.text"),
      link: t("sokki.voice.support.link"),
      sheet: "support" as const,
    },
  ];
  const set = (key: keyof DataChoiceValues, on: boolean) =>
    onChange({ ...value, [key]: on });

  return (
    <>
      <div className="flex flex-col gap-2">
        {rows.map((r) => {
          const on = value[r.key];
          const labelId = `data-choice-${r.key}`;
          const linkButton = (
            <button
              type="button"
              onClick={(e) => {
                e.stopPropagation();
                setSheet(r.sheet);
              }}
              // Enter and Space open the sheet, not tick the box around it.
              onKeyDown={(e) => e.stopPropagation()}
              className="self-start text-[12.5px] text-logo-primary hover:underline cursor-pointer"
            >
              {r.link}
            </button>
          );
          if (variant === "switch") {
            return (
              <div
                key={r.key}
                className="flex items-center gap-4 rounded-xl border border-line px-4 py-3"
              >
                <span className="flex grow flex-col gap-0.5">
                  <span id={labelId} className="text-[14px] font-medium">
                    {r.title}
                  </span>
                  <span className="text-[12.5px] leading-[18px] text-ink-3">
                    {r.text}
                  </span>
                  {linkButton}
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
              className={`flex items-start gap-3 rounded-xl border px-4 py-3 text-start cursor-pointer transition-colors focus:outline-none focus-visible:ring-1 focus-visible:ring-logo-primary ${
                on
                  ? "border-logo-primary bg-logo-primary/10"
                  : "border-line hover:bg-wash"
              }`}
            >
              <span
                aria-hidden="true"
                className={`mt-[2px] grid size-[18px] shrink-0 place-items-center rounded-[5px] border ${
                  on
                    ? "border-logo-primary bg-logo-primary text-white"
                    : "border-line-strong"
                }`}
              >
                {on && <Check width={13} height={13} strokeWidth={3} />}
              </span>
              <span className="flex flex-col gap-0.5">
                <span id={labelId} className="text-[14px] font-medium">
                  {r.title}
                </span>
                <span className="text-[12.5px] leading-[18px] text-ink-3">
                  {r.text}
                </span>
                {linkButton}
              </span>
            </div>
          );
        })}
      </div>
      <Sheets sheet={sheet} onClose={() => setSheet(null)} />
    </>
  );
};
