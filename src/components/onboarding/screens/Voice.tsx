import React, { useState } from "react";
import { useTranslation } from "react-i18next";
import { commands } from "@/bindings";
import { Button } from "../../ui/Button";
import { OnboardingShell, ScreenHeader, ScreenMain } from "../OnboardingShell";
import { useOnboardingStore } from "../onboardingStore";
import {
  VoiceChoices,
  type VoiceChoice,
} from "../../settings/verbatim/VoiceChoices";

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
  const [choice, setChoice] = useState<VoiceChoice | null>(null);
  const answer = async (c: VoiceChoice) => {
    if (!preview) await commands.voiceDataSetChoice(c);
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
          onClick={() => void answer("off")}
        >
          {t("verbatim.voice.notNow")}
        </Button>
      }
      primary={
        <Button
          variant="primary"
          size="lg"
          disabled={!choice}
          onClick={() => choice && void answer(choice)}
        >
          {t("onboarding.shell.continue")}
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
        <div className="flex w-full flex-col gap-[12px] text-start">
          <VoiceChoices value={choice} onChange={setChoice} withOff={false} />
          <ul className="flex flex-col gap-[4px] text-small text-ink-3 list-disc ps-[18px]">
            <li>{t("verbatim.voice.point1")}</li>
            <li>{t("verbatim.voice.point3")}</li>
          </ul>
        </div>
      </ScreenMain>
    </OnboardingShell>
  );
};
