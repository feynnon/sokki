import React from "react";
import { useTranslation } from "react-i18next";
import { AudioLines } from "lucide-react";
import { commands } from "@/bindings";
import { Button } from "../../ui/Button";
import { OnboardingShell, ScreenHeader, ScreenMain } from "../OnboardingShell";
import { useOnboardingStore } from "../onboardingStore";

interface VoiceProps {
  progress?: { index: number; count: number };
  onBack?: () => void;
  onContinue: () => void;
}

/** Verbatim: one question about "Your voice" (keep, be counted, be asked). */
export const Voice: React.FC<VoiceProps> = ({
  progress,
  onBack,
  onContinue,
}) => {
  const { t } = useTranslation();
  const { preview } = useOnboardingStore();
  const answer = async (yes: boolean) => {
    if (!preview) await commands.voiceDataSetOptIn(yes);
    onContinue();
  };
  return (
    <OnboardingShell
      progress={progress}
      onBack={onBack}
      hint={t("verbatim.voice.onboardingHint")}
      left={
        <Button
          variant="secondary"
          size="lg"
          onClick={() => void answer(false)}
        >
          {t("verbatim.voice.notNow")}
        </Button>
      }
      primary={
        <Button variant="primary" size="lg" onClick={() => void answer(true)}>
          {t("verbatim.voice.yes")}
        </Button>
      }
    >
      <ScreenMain
        header={
          <ScreenHeader
            title={t("verbatim.voice.onboardingTitle")}
            sub={t("verbatim.voice.onboardingSub")}
          />
        }
      >
        <div className="flex w-full gap-[16px] rounded-panel border border-line-strong px-[22px] py-[18px] text-start">
          <AudioLines
            size={22}
            aria-hidden="true"
            className="mt-[2px] shrink-0 text-logo-primary"
          />
          <ul className="flex flex-col gap-[8px] text-small text-ink-2 list-disc ps-[18px]">
            <li>{t("verbatim.voice.point1")}</li>
            <li>{t("verbatim.voice.point2")}</li>
            <li>{t("verbatim.voice.point3")}</li>
            <li>{t("verbatim.voice.point4")}</li>
          </ul>
        </div>
      </ScreenMain>
    </OnboardingShell>
  );
};
