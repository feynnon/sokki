import React, { useState } from "react";
import { Trans, useTranslation } from "react-i18next";
import { relaunch } from "@tauri-apps/plugin-process";
import { Keyboard, Mail, Mic, Monitor, Volume2 } from "lucide-react";
import { Button } from "../../ui/Button";
import { Spinner } from "../../ui/Spinner";
import { TextLink } from "../../ui/TextLink";
import { DoneMark } from "../../shared/DoneMark";
import { OnboardingShell, ScreenHeader, ScreenMain } from "../OnboardingShell";
import { useOnboardingStore, type Uses } from "../onboardingStore";
import type { Permissions as PermissionsState } from "../usePermissions";
import type { useGmail } from "../useGmail";

interface PermissionRowProps {
  icon: React.ReactNode;
  title: string;
  why: string;
  status: PermissionsState["microphone"];
  /** Its turn: the rows are allowed in order, so only this one is live. */
  next: boolean;
  action: string;
  /** While waiting, what to do in the OS, in place of `why`. */
  instruction?: string;
  /** In place of "Allowed" (system sound can only be asked, not checked). */
  grantedLabel?: string;
  /** A small tag after the name, e.g. "Optional". */
  tag?: string;
  /** Small print under `why`, beside the button (Google's notice). */
  note?: string;
  /** Less room above and below, when five rows share the screen. */
  compact?: boolean;
  onRequest: () => void;
}

/** One permission row; the status column's min width stops text re-wrapping. */
const PermissionRow: React.FC<PermissionRowProps> = ({
  icon,
  title,
  why,
  status,
  next,
  action,
  instruction,
  grantedLabel,
  tag,
  note,
  compact,
  onRequest,
}) => {
  const { t } = useTranslation();
  const waiting = status === "waiting";
  return (
    <div
      className={`grid items-center gap-[18px] [&+&]:border-t [&+&]:border-line ${compact ? "grid-cols-[38px_1fr_minmax(112px,auto)] py-[6px]" : "grid-cols-[46px_1fr_minmax(112px,auto)] py-[18px]"}`}
    >
      {/* A label, not a key: neutral, and "Allowed" alone shows the grant. */}
      <span
        className={`grid place-items-center rounded-[11px] bg-wash text-ink-2 ${compact ? "size-[38px]" : "size-[46px]"}`}
      >
        {icon}
      </span>
      <div>
        <b className="flex items-center gap-[8px] font-display text-row font-[650] leading-[1.3]">
          {title}
          {tag && (
            <span className="rounded-md bg-wash px-[6px] py-[1px] font-ui text-label font-medium text-ink-2">
              {tag}
            </span>
          )}
        </b>
        {/* Five rows: a granted row is just its name, to make room. */}
        {!(compact && status === "granted") && (
          <p
            className={`mt-[2px] mb-0 text-small leading-[1.45] text-pretty ${
              waiting && instruction ? "text-text" : "text-ink-2"
            }`}
          >
            {(waiting && instruction) || why}
          </p>
        )}
      </div>
      {status === "granted" ? (
        <span className="inline-flex items-center justify-self-end gap-[6px] text-small font-semibold text-moss">
          <DoneMark />
          {grantedLabel ?? t("onboarding.permissions.allowed")}
        </span>
      ) : waiting ? (
        <span className="inline-flex items-center justify-self-end gap-[6px] text-small text-ink-3">
          <Spinner className="shrink-0" />
          {t("onboarding.permissions.waiting")}
        </span>
      ) : (
        <Button
          variant="primary"
          size="md"
          disabled={!next}
          onClick={onRequest}
          className="justify-self-end"
        >
          {action}
        </Button>
      )}
      {/* Under the whole row, beside and below the button. */}
      {note && (
        <p className="col-start-2 col-end-4 -mt-[12px] mb-0 text-label leading-[1.4] text-ink-3 text-pretty">
          {note}
        </p>
      )}
    </div>
  );
};

interface PermissionsProps {
  permissions: PermissionsState;
  /** What Sokki is for: decides which permissions are asked. */
  uses: Uses;
  /** In the flow: its place and Back. Standalone (a returning user): neither. */
  progress?: { index: number; count: number };
  onBack?: () => void;
  onContinue: () => void;
  /** Gmail, offered with Meetings when this build can send through it. */
  gmail?: ReturnType<typeof useGmail>;
}

/** Number words for the title and hint, as in "Four permissions". */
const COUNT = ["one", "two", "three", "four"] as const;

/**
 * The permissions the picks need, granted in order: the microphone, then on
 * macOS Accessibility (dictation), system sound and the screen (meetings).
 * The screen comes last: allowing it makes macOS reopen Sokki.
 */
export const Permissions: React.FC<PermissionsProps> = ({
  permissions,
  uses,
  progress,
  onBack,
  onContinue,
  gmail,
}) => {
  const { t } = useTranslation();
  const { welcomeBack, preview, set } = useOnboardingStore();
  const isMac = permissions.platform === "macos";
  type Row = "microphone" | "accessibility" | "systemSound" | "screen";
  const rows: Row[] = [
    "microphone",
    ...(isMac && uses.dictation ? (["accessibility"] as const) : []),
    ...(isMac && uses.meetings ? (["systemSound", "screen"] as const) : []),
  ];
  const allGranted = permissions.allGrantedFor(uses);
  const count = COUNT[rows.length - 1];
  const forWhat =
    uses.dictation && uses.meetings
      ? "forBoth"
      : uses.meetings
        ? "forMeetings"
        : "forDictation";
  const next = rows.find((r) => permissions[r] !== "granted");
  const withGmail = !!gmail?.available && uses.meetings && isMac;
  const compact = withGmail && rows.length >= 4;
  const go = () => {
    if (welcomeBack) set({ welcomeBack: false });
    onContinue();
  };
  // Asking for the microphone opens the OS settings on Windows, and on macOS
  // once it has asked: then the row says what to turn on there.
  const [micInSettings, setMicInSettings] = useState(false);
  const turnOn = t(
    isMac
      ? "onboarding.permissions.turnOnMac"
      : "onboarding.permissions.turnOnWindows",
  );

  // macOS keeps an Accessibility entry tied to the app's signature. After
  // Sokki's signature changed (0.2.13: Developer ID instead of "Sokki Dev"),
  // the old entry stays in the list, switching it on changes nothing, and
  // only removing it (–) and adding Sokki again (+) works (tccd: "Failed to
  // match existing code requirement", Robin, 2026-11-25). Say so to a
  // returning user (standalone: no `progress`) and after a round in System
  // Settings that didn't help.
  const staleAccessibility =
    isMac && (permissions.accessibilityRetry || (!progress && !preview));

  const relaunchSokki = async () => {
    if (preview) return;
    set({ step: "permissions", welcomeBack: true });
    await relaunch();
  };

  const showRelaunch =
    isMac &&
    rows.includes("accessibility") &&
    permissions.accessibility === "granted" &&
    !(rows.includes("screen") && permissions.screen !== "granted");
  const relaunchLine = (
    <Trans
      i18nKey="onboarding.permissions.relaunch"
      components={{ relaunch: <TextLink onClick={relaunchSokki} /> }}
    />
  );

  return (
    <OnboardingShell
      progress={progress}
      onBack={onBack}
      hint={
        allGranted
          ? compact && showRelaunch
            ? relaunchLine
            : ""
          : t(
              rows.length === 1
                ? "onboarding.permissions.hintMicrophone"
                : rows.length === 2
                  ? "onboarding.permissions.hintBoth"
                  : `onboarding.permissions.hint_${count}`,
            )
      }
      primary={
        <Button variant="primary" size="lg" disabled={!allGranted} onClick={go}>
          {t("onboarding.shell.continue")}
        </Button>
      }
    >
      <ScreenMain
        header={
          <ScreenHeader
            above={
              welcomeBack && (
                <div className="flex animate-rise items-center justify-center gap-[10px] text-small font-semibold text-moss">
                  <DoneMark />
                  {t("onboarding.permissions.welcomeBack")}
                </div>
              )
            }
            title={t(`onboarding.permissions.title_${count}`)}
            sub={t(
              isMac
                ? `onboarding.permissions.${forWhat}`
                : "onboarding.permissions.subtitleWindows",
            )}
          />
        }
      >
        <div className="flex w-full flex-col rounded-panel border border-line-strong px-[22px] py-[4px] text-start">
          <PermissionRow
            icon={<Mic size={20} aria-hidden="true" />}
            title={t("onboarding.permissions.microphone.name")}
            why={t("onboarding.permissions.microphone.why")}
            status={permissions.microphone}
            next={next === "microphone"}
            action={t(
              permissions.microphoneAsked && isMac
                ? "onboarding.permissions.openSettings"
                : "onboarding.permissions.allow",
            )}
            instruction={micInSettings ? turnOn : undefined}
            onRequest={() => {
              setMicInSettings(!isMac || permissions.microphoneAsked);
              permissions.requestMicrophone();
            }}
            compact={compact}
          />
          {rows.includes("accessibility") && (
            <PermissionRow
              icon={<Keyboard size={20} aria-hidden="true" />}
              title={t("onboarding.permissions.accessibility.name")}
              why={
                staleAccessibility
                  ? t("onboarding.permissions.accessibility.stale")
                  : t("onboarding.permissions.accessibility.why")
              }
              status={permissions.accessibility}
              next={next === "accessibility"}
              action={t("onboarding.permissions.allow")}
              instruction={
                staleAccessibility
                  ? t("onboarding.permissions.accessibility.stale")
                  : turnOn
              }
              onRequest={permissions.requestAccessibility}
              compact={compact}
            />
          )}
          {rows.includes("systemSound") && (
            <PermissionRow
              icon={<Volume2 size={20} aria-hidden="true" />}
              title={t("onboarding.permissions.systemSound.name")}
              why={t("onboarding.permissions.systemSound.why")}
              status={permissions.systemSound}
              grantedLabel={t("onboarding.permissions.asked")}
              next={next === "systemSound"}
              action={t("onboarding.permissions.allow")}
              onRequest={permissions.requestSystemSound}
              compact={compact}
            />
          )}
          {rows.includes("screen") && (
            <PermissionRow
              icon={<Monitor size={20} aria-hidden="true" />}
              title={t("onboarding.permissions.screen.name")}
              why={t("onboarding.permissions.screen.why")}
              status={permissions.screen}
              next={next === "screen"}
              action={t("onboarding.permissions.allow")}
              instruction={turnOn}
              onRequest={permissions.requestScreen}
              compact={compact}
            />
          )}
          {withGmail && gmail && (
            <PermissionRow
              icon={<Mail size={20} aria-hidden="true" />}
              title={t("onboarding.permissions.gmail.name")}
              tag={t("onboarding.permissions.optional")}
              why={gmail.error ?? t("onboarding.permissions.gmail.why")}
              status={
                gmail.connected ? "granted" : gmail.busy ? "waiting" : "needed"
              }
              grantedLabel={t("onboarding.permissions.gmail.connected")}
              // Google's review needs this beside the button, before its
              // page opens (card privacy-google).
              note={
                !next && !gmail.connected
                  ? t("onboarding.permissions.gmail.noticeSend")
                  : undefined
              }
              next={!next}
              action={t("onboarding.permissions.gmail.connect")}
              instruction={t("onboarding.permissions.gmail.inBrowser")}
              compact={compact}
              onRequest={() => void gmail.connect()}
            />
          )}
        </div>
        {rows.includes("screen") && permissions.screen !== "granted" && (
          <p className="mt-[18px] mb-0 text-small text-ink-3">
            {t("onboarding.permissions.screen.reopen")}
          </p>
        )}
        {!compact && (
          <p
            className={`mt-[18px] mb-0 text-small text-ink-3 ${showRelaunch ? "visible" : "invisible"}`}
          >
            {relaunchLine}
          </p>
        )}
      </ScreenMain>
    </OnboardingShell>
  );
};
