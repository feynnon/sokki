import React, { useState } from "react";
import { useTranslation } from "react-i18next";
import { commands } from "@/bindings";
import { Button } from "../../ui/Button";
import { OnboardingShell, ScreenHeader, ScreenMain } from "../OnboardingShell";
import { useOnboardingStore } from "../onboardingStore";
import {
  DataChoices,
  type DataChoiceValues,
} from "../../settings/sokki/DataChoices";

interface VoiceProps {
  progress?: { index: number; count: number };
  onBack?: () => void;
  onContinue: () => void;
}

/** Sokki: "Your data". Both boxes start unticked; nothing is sent before
 * Continue. */
export const Voice: React.FC<VoiceProps> = ({
  progress,
  onBack,
  onContinue,
}) => {
  const { t } = useTranslation();
  const { preview } = useOnboardingStore();
  const [choices, setChoices] = useState<DataChoiceValues>({
    improve: false,
    support: false,
    tasks: false,
  });
  const next = async () => {
    if (!preview) {
      await commands.voiceDataSetChoices(
        choices.improve,
        choices.support,
        choices.tasks,
      );
    }
    onContinue();
  };
  return (
    <OnboardingShell
      progress={progress}
      onBack={onBack}
      hint={t("sokki.voice.onboardingHint")}
      primary={
        <Button variant="primary" size="lg" onClick={() => void next()}>
          {t("onboarding.shell.continue")}
        </Button>
      }
    >
      <ScreenMain
        header={
          <ScreenHeader
            title={t("sokki.voice.title")}
            sub={t("sokki.voice.intro")}
          />
        }
      >
        <div className="flex w-full flex-col text-start">
          <DataChoices
            value={choices}
            onChange={setChoices}
            variant="checkbox"
          />
        </div>
      </ScreenMain>
    </OnboardingShell>
  );
};
