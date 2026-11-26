import React from "react";
import { useTranslation } from "react-i18next";
import { AlertTriangle, Check, KeyRound, Server, Sparkles } from "lucide-react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { Button } from "../../ui/Button";
import { ProgressLine } from "../../shared/ProgressLine";
import { OnboardingShell, ScreenHeader, ScreenMain } from "../OnboardingShell";
import { useOnboardingStore } from "../onboardingStore";
import {
  BUILTIN_MODEL,
  LANGUAGE_MODELS,
} from "../../settings/models/LanguageModels";
import {
  SUMMARY_DOWNLOAD_BYTES,
  SUMMARY_NEEDS_BYTES,
  gigabytes,
  macMemory,
  useSummaryModel,
} from "@/hooks/useSummaryModel";

/** "Gemma 4 E4B", as Models → Language model names it. */
const BUILTIN_NAME =
  LANGUAGE_MODELS.find((m) => m.value === BUILTIN_MODEL)?.name ?? "";

interface SummaryProps {
  progress?: { index: number; count: number };
  onBack?: () => void;
  onContinue: () => void;
}

/** For Meetings: the language model that writes each meeting's summary.
 * Offered with its size and whether it fits this Mac's memory; it
 * downloads in the background while setup goes on. A Mac too small for
 * it is told so, with the other ways (Ollama, an own key). */
export const Summary: React.FC<SummaryProps> = ({
  progress,
  onBack,
  onContinue,
}) => {
  const { t, i18n } = useTranslation();
  const lang = i18n.language;
  const preview = useOnboardingStore((s) => s.preview);
  const model = useSummaryModel(preview);
  const { state, download, fits, memory } = model;
  const small = state !== "ready" && !fits;
  const none = state === "unavailable";
  const size = gigabytes(SUMMARY_DOWNLOAD_BYTES, lang);
  const needs = gigabytes(SUMMARY_NEEDS_BYTES, lang);

  const downloadAndGo = () => {
    void model.start();
    onContinue();
  };

  /* ---------------- the download line ---------------- */

  let line: React.ReactNode = null;
  if (state === "downloading" && download) {
    const verifying = download.status === "verifying";
    const value = download.total > 0 ? download.completed / download.total : 0;
    const percent = Math.min(99, Math.floor(value * 100));
    line = (
      <span className="mt-[12px] flex flex-col gap-[5px]">
        <ProgressLine
          value={verifying ? 1 : value}
          busy={verifying || !value}
        />
        <span
          role="status"
          className="flex justify-between gap-[12px] text-label tabular-nums text-ink-3"
        >
          <span className="truncate">
            {verifying
              ? t("onboarding.download.checking")
              : t("onboarding.summary.downloading", {
                  percent,
                  done: gigabytes(download.completed, lang),
                  total: size,
                })}
          </span>
          <span className="shrink-0">{t("onboarding.summary.background")}</span>
        </span>
      </span>
    );
  } else if (state === "ready") {
    line = (
      <span
        role="status"
        className="mt-[12px] flex items-center gap-[6px] text-small font-medium text-moss"
      >
        <Check size={14} strokeWidth={2.5} aria-hidden="true" />
        {t("onboarding.summary.ready")}
      </span>
    );
  } else if (state === "failed") {
    line = (
      <span role="status" className="mt-[12px] text-label text-warning">
        {t("onboarding.download.failed")}
      </span>
    );
  }

  /* ---------------- the card ---------------- */

  const memoryLine = none
    ? t("onboarding.summary.noBuiltin")
    : memory
      ? t("onboarding.summary.memory", { needs, has: macMemory(memory, lang) })
      : t("onboarding.summary.memoryUnknown", { needs });

  const card = (
    <div
      className={`flex w-full flex-col rounded-panel border-[1.5px] bg-background px-5 py-4 text-start text-small ${
        small || none ? "border-line" : "border-plum-pick"
      }`}
    >
      <span className="flex items-center gap-[12px]">
        <span className="grid size-10 shrink-0 place-items-center rounded-[10px] bg-logo-primary/20 text-logo-primary">
          <Sparkles size={20} aria-hidden="true" />
        </span>
        <span className="flex min-w-0 flex-col">
          <span className="font-display text-name font-[650] leading-[1.2] tracking-[-0.01em]">
            {BUILTIN_NAME}
          </span>
          <span className="text-ink-2">{t("onboarding.summary.local")}</span>
        </span>
        {!none && (
          <span className="ms-auto self-start text-label tabular-nums text-ink-3">
            {size}
          </span>
        )}
      </span>
      <span className="mt-[14px] grid grid-cols-[minmax(84px,max-content)_1fr] items-baseline gap-x-[12px] gap-y-[8px] border-t border-line pt-[12px]">
        <span className="text-ink-2">
          {t("onboarding.summary.memoryLabel")}
        </span>
        <span
          className={`flex items-center gap-[6px] ${small || none ? "text-warning" : "text-text"}`}
        >
          {small || none ? (
            <AlertTriangle size={13} className="shrink-0" aria-hidden="true" />
          ) : (
            memory > 0 && (
              <Check
                size={13}
                strokeWidth={2.5}
                className="shrink-0 text-moss"
                aria-hidden="true"
              />
            )
          )}
          {memoryLine}
        </span>
        <span className="text-ink-2">{t("onboarding.summary.notesLabel")}</span>
        <span>{t("onboarding.summary.notesWhen")}</span>
      </span>
      {line}
    </div>
  );

  /* ---------------- other ways, for a small Mac ---------------- */

  const other = (
    icon: React.ReactNode,
    name: string,
    detail: string,
    action?: React.ReactNode,
  ) => (
    <div className="flex items-center gap-[12px] rounded-card border-[1.5px] border-line px-4 py-[10px] text-start text-small">
      <span className="grid size-8 shrink-0 place-items-center rounded-[8px] bg-wash text-ink-2">
        {icon}
      </span>
      <span className="flex min-w-0 flex-col">
        <b className="font-semibold">{name}</b>
        <span className="text-ink-3">{detail}</span>
      </span>
      {action && <span className="ms-auto shrink-0">{action}</span>}
    </div>
  );
  const others = (small || none) && (
    <div className="mt-[14px] flex w-full flex-col gap-[8px]">
      <p className="m-0 text-start text-small text-ink-2">
        {none
          ? t("onboarding.summary.othersNone")
          : t("onboarding.summary.othersSmall", {
              has: macMemory(memory, lang),
            })}
      </p>
      {other(
        <Server size={16} aria-hidden="true" />,
        t("onboarding.summary.ollama"),
        t("onboarding.summary.ollamaLine"),
        <Button
          variant="outline"
          size="sm"
          onClick={() => void openUrl("https://ollama.com/download")}
        >
          {t("meetings.lm.getOllama")}
        </Button>,
      )}
      {other(
        <KeyRound size={16} aria-hidden="true" />,
        t("onboarding.summary.ownKey"),
        t("onboarding.summary.ownKeyLine"),
      )}
      <p className="m-0 text-start text-label text-ink-3">
        {t("onboarding.summary.where")}
      </p>
    </div>
  );

  /* ---------------- the footer ---------------- */

  let hint: React.ReactNode = "";
  let primary: React.ReactNode;
  const go = (label: string, onClick = onContinue) => (
    <Button
      variant="primary"
      size="lg"
      onClick={onClick}
      className="whitespace-nowrap"
    >
      {label}
    </Button>
  );
  const notNow = (
    <Button
      variant="outline"
      size="lg"
      onClick={onContinue}
      className="whitespace-nowrap"
    >
      {t("onboarding.summary.notNow")}
    </Button>
  );
  if (state === "ready" || state === "downloading") {
    primary = go(t("onboarding.shell.continue"));
  } else if (small || none) {
    primary = (
      <span className="flex items-center gap-[8px]">
        {!none && (
          <Button
            variant="outline"
            size="lg"
            onClick={downloadAndGo}
            className="whitespace-nowrap"
          >
            {t("onboarding.summary.downloadAnyway")}
          </Button>
        )}
        {go(t("onboarding.shell.continue"))}
      </span>
    );
  } else {
    hint = state === "failed" ? "" : t("onboarding.model.hint");
    primary = (
      <span className="flex items-center gap-[8px]">
        {notNow}
        {go(
          state === "failed"
            ? t("onboarding.summary.retry")
            : t("onboarding.model.download", { size }),
          downloadAndGo,
        )}
      </span>
    );
  }

  return (
    <OnboardingShell
      progress={progress}
      onBack={onBack}
      hint={hint}
      primary={primary}
    >
      <ScreenMain
        header={
          <ScreenHeader
            title={t("onboarding.summary.title")}
            sub={t("onboarding.summary.subtitle")}
          />
        }
      >
        <div className="mx-auto flex w-full max-w-[520px] flex-col">
          {card}
          {others}
        </div>
      </ScreenMain>
    </OnboardingShell>
  );
};
