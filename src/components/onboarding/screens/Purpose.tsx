import React from "react";
import { useTranslation } from "react-i18next";
import {
  AudioLines,
  Check,
  Keyboard,
  Mail,
  Mic,
  Monitor,
  Users,
  Volume2,
} from "lucide-react";
import { Button } from "../../ui/Button";
import { OnboardingShell, ScreenHeader, ScreenMain } from "../OnboardingShell";
import { useOnboardingStore, type Uses } from "../onboardingStore";

interface PurposeProps {
  progress?: { index: number; count: number };
  onBack?: () => void;
  onContinue: (uses: Uses) => void;
  /** Meetings also asks for Gmail (optional), when this build has it. */
  gmail?: boolean;
}

/** "What will you use Sokki for?": Dictation and/or Meetings. The next
 * screen asks only for the permissions these need. */
export const Purpose: React.FC<PurposeProps> = ({
  progress,
  onBack,
  onContinue,
  gmail = false,
}) => {
  const { t } = useTranslation();
  const { uses, set } = useOnboardingStore();
  const mic = { key: "microphone", icon: Mic };
  const cards: {
    key: keyof Uses;
    icon: React.ReactNode;
    needs: { key: string; icon: typeof Mic }[];
  }[] = [
    {
      key: "dictation",
      icon: <AudioLines size={20} aria-hidden="true" />,
      needs: [mic, { key: "accessibility", icon: Keyboard }],
    },
    {
      key: "meetings",
      icon: <Users size={20} aria-hidden="true" />,
      needs: [
        mic,
        { key: "systemSound", icon: Volume2 },
        { key: "screen", icon: Monitor },
        ...(gmail ? [{ key: "gmail", icon: Mail }] : []),
      ],
    },
  ];
  const none = !uses.dictation && !uses.meetings;
  return (
    <OnboardingShell
      progress={progress}
      onBack={onBack}
      hint={none ? t("onboarding.purpose.pickOne") : ""}
      primary={
        <Button
          variant="primary"
          size="lg"
          disabled={none}
          onClick={() => onContinue(uses)}
        >
          {t("onboarding.shell.continue")}
        </Button>
      }
    >
      <ScreenMain
        header={<ScreenHeader title={t("onboarding.purpose.title")} />}
      >
        <div className="mx-auto grid w-full max-w-[480px] grid-cols-2 gap-3 text-start">
          {cards.map(({ key, icon, needs }) => {
            const on = uses[key];
            return (
              <button
                key={key}
                type="button"
                aria-pressed={on}
                onClick={() => set({ uses: { ...uses, [key]: !on } })}
                className={`flex flex-col items-start gap-[6px] rounded-panel border p-4 text-start transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-logo-primary ${
                  on
                    ? "border-logo-primary bg-logo-primary/10 shadow-[inset_0_0_0_1px_var(--color-logo-primary)]"
                    : "border-line-strong hover:bg-wash"
                }`}
              >
                <span className="mb-2 flex w-full items-start justify-between">
                  <span
                    className={`grid size-10 place-items-center rounded-[10px] ${
                      on
                        ? "bg-logo-primary/20 text-logo-primary"
                        : "bg-wash text-ink-2"
                    }`}
                  >
                    {icon}
                  </span>
                  <span
                    className={`grid size-[22px] place-items-center rounded-full border-[1.5px] ${
                      on
                        ? "border-logo-primary bg-logo-primary text-background"
                        : "border-line-strong text-transparent"
                    }`}
                  >
                    <Check size={13} strokeWidth={3} aria-hidden="true" />
                  </span>
                </span>
                <span className="font-display text-row font-[650] leading-[1.3]">
                  {t(`onboarding.purpose.${key}`)}
                </span>
                <span className="mt-3 w-full border-t border-line pt-[10px] text-[12px] text-ink-3">
                  <b className="mb-[6px] block font-semibold text-ink-2">
                    {t("onboarding.purpose.asksFor")}
                  </b>
                  <span className="flex flex-col gap-[6px]">
                    {needs.map(({ key: need, icon: Icon }) => (
                      <span key={need} className="flex items-center gap-2">
                        <Icon size={14} aria-hidden="true" />
                        {t(`onboarding.permissions.${need}.name`)}
                      </span>
                    ))}
                  </span>
                </span>
              </button>
            );
          })}
        </div>
        <p className="mt-4 mb-0 text-small text-ink-3">
          {t("onboarding.purpose.later")}
        </p>
      </ScreenMain>
    </OnboardingShell>
  );
};
