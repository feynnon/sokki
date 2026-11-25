import React, { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { X } from "lucide-react";
import {
  commands,
  type GmailStatus,
  type MailSetup,
  type MeetingSettings,
} from "@/bindings";
import { useSettings } from "../../../hooks/useSettings";
import { SettingsGroup } from "../../ui/SettingsGroup";
import { SettingContainer } from "../../ui/SettingContainer";
import { ToggleSwitch } from "../../ui/ToggleSwitch";
import { Input } from "../../ui/Input";
import { Button } from "../../ui/Button";
import { ErrorBoundary } from "../../ErrorBoundary";

/**
 * Gmail as the sender. One button; Google's required notice shows only
 * after it is clicked, right before Google's page opens (card
 * privacy-google), so the settings page itself stays one line.
 */
const GmailRow: React.FC<{ onChange: () => void }> = ({ onChange }) => {
  const { t } = useTranslation();
  const { refreshSettings } = useSettings();
  const [status, setStatus] = useState<GmailStatus | null>(null);
  const [asking, setAsking] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const load = async () => {
    try {
      setStatus((await commands.gmailStatus()) ?? null);
    } catch {
      setStatus(null);
    }
  };
  useEffect(() => {
    void load();
  }, []);
  if (!status?.available) return null;

  const connect = async () => {
    setBusy(true);
    setError(null);
    const r = await commands.gmailConnect();
    setBusy(false);
    setAsking(false);
    if (r?.status === "error") setError(r.error);
    await refreshSettings();
    void load();
    onChange();
  };
  const disconnect = async () => {
    setBusy(true);
    const r = await commands.gmailDisconnect();
    setBusy(false);
    if (r?.status === "error") setError(r.error);
    await refreshSettings();
    void load();
    onChange();
  };

  return (
    <>
      <SettingContainer
        title={t("meetings.mail.gmail")}
        description={status.connected ?? ""}
        descriptionMode="inline"
        grouped
      >
        {status.connected ? (
          <Button
            size="sm"
            variant="secondary"
            disabled={busy}
            onClick={() => void disconnect()}
          >
            {t("meetings.mail.gmailDisconnect")}
          </Button>
        ) : (
          <Button
            size="sm"
            variant="secondary"
            disabled={busy || asking}
            onClick={() => setAsking(true)}
          >
            {t("meetings.mail.gmailConnect")}
          </Button>
        )}
      </SettingContainer>
      {asking && !status.connected && (
        <div className="px-4 pb-3 flex flex-col gap-2">
          <p className="text-[12.5px] leading-[18px] text-ink-2">
            {t("meetings.mail.gmailNotice")}
          </p>
          <div className="flex justify-end gap-2">
            <Button
              size="sm"
              variant="secondary"
              disabled={busy}
              onClick={() => setAsking(false)}
            >
              {t("meetings.mail.cancel")}
            </Button>
            <Button size="sm" disabled={busy} onClick={() => void connect()}>
              {busy
                ? t("meetings.mail.gmailWaiting")
                : t("meetings.mail.gmailContinue")}
            </Button>
          </div>
        </div>
      )}
      {error && <p className="px-4 pb-3 text-[12.5px] text-error">{error}</p>}
    </>
  );
};

/**
 * Notes by mail: Gmail, the switch, and the people who said no. Only what
 * someone needs to act on; problems show only when there is one.
 */
const MailSettingsBox: React.FC<{
  meetingSettings: MeetingSettings;
  save: (patch: Partial<MeetingSettings>) => Promise<void>;
}> = ({ meetingSettings, save }) => {
  const { t } = useTranslation();
  const on = meetingSettings.mail_notes_auto;
  const [setup, setSetup] = useState<MailSetup | null>(null);
  const [objections, setObjections] = useState<string[]>([]);
  const [adding, setAdding] = useState(false);
  const [draft, setDraft] = useState("");

  const check = async () => {
    try {
      const r = await commands.mailSetup();
      if (r?.status === "ok" && r.data) setSetup(r.data);
    } catch {
      setSetup(null);
    }
  };
  const loadObjections = async () => {
    // A list that can't be read is an empty list: this box must never
    // take the settings page down.
    try {
      const list = await commands.mailObjections();
      setObjections(Array.isArray(list) ? list : []);
    } catch {
      setObjections([]);
    }
  };
  useEffect(() => {
    void loadObjections();
  }, []);

  const toggle = async (v: boolean) => {
    // Set by the person: a later default never changes it again.
    await save({ mail_notes_auto: v, mail_notes_auto_set: true });
    // Turning it on is the moment to ask for the calendar and for Mail.
    if (v) void check();
    else setSetup(null);
  };

  const add = async () => {
    const email = draft.trim();
    if (!email) return;
    const r = await commands.mailObjectionAdd(email);
    if (r?.status === "ok") {
      setDraft("");
      setAdding(false);
      void loadObjections();
    }
  };

  const problem = !setup
    ? null
    : setup.calendar !== "allowed"
      ? t("meetings.mail.noCalendar")
      : setup.error;

  return (
    <SettingsGroup title={t("meetings.mail.title")}>
      {/* The sender changed: an Apple Mail problem may no longer apply. */}
      <GmailRow onChange={() => (on ? void check() : undefined)} />
      <ToggleSwitch
        checked={on}
        onChange={(v) => void toggle(v)}
        label={t("meetings.mail.auto")}
        description={t("meetings.mail.autoHint")}
        descriptionMode="inline"
        grouped
      />
      {on && problem && (
        <p className="px-4 pb-3 text-[12.5px] text-error">{problem}</p>
      )}
      <SettingContainer
        title={t("meetings.mail.objections")}
        description=""
        layout="stacked"
        grouped
      >
        <div className="flex flex-wrap items-center gap-1.5">
          {objections.map((email) => (
            <span
              key={email}
              className="inline-flex items-center gap-1 h-7 pl-2.5 pr-1 rounded-lg bg-wash text-[13px]"
            >
              {email}
              <button
                type="button"
                aria-label={t("meetings.mail.remove")}
                className="size-5 inline-flex items-center justify-center rounded text-ink-3 hover:text-text cursor-pointer"
                onClick={() =>
                  void commands
                    .mailObjectionRemove(email)
                    .then(() => loadObjections())
                }
              >
                <X width={12} height={12} />
              </button>
            </span>
          ))}
          {adding ? (
            <Input
              autoFocus
              value={draft}
              placeholder={t("meetings.mail.addPlaceholder")}
              className="w-[220px] h-7 text-[13px]"
              onChange={(e) => setDraft(e.target.value)}
              onBlur={() => {
                if (!draft.trim()) setAdding(false);
              }}
              onKeyDown={(e) => {
                if (e.key === "Enter") void add();
                if (e.key === "Escape") setAdding(false);
              }}
            />
          ) : (
            <button
              type="button"
              className="h-7 px-2 rounded-lg text-[13px] text-ink-2 hover:bg-wash cursor-pointer"
              onClick={() => setAdding(true)}
            >
              {t("meetings.mail.add")}
            </button>
          )}
        </div>
      </SettingContainer>
    </SettingsGroup>
  );
};

/** The box, kept from taking the meeting settings page down if it fails. */
export const MailSettings: React.FC<{
  meetingSettings: MeetingSettings;
  save: (patch: Partial<MeetingSettings>) => Promise<void>;
}> = (props) => (
  <ErrorBoundary context="Notes by mail">
    <MailSettingsBox {...props} />
  </ErrorBoundary>
);
