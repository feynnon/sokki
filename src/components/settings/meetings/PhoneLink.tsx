import React, { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Copy, Mail, RefreshCw, Trash2 } from "lucide-react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { inviteMailto } from "../../../../shared/invite";
import { commands, type MeetingSettings, type PhoneLinkInfo } from "@/bindings";
import { SettingsGroup } from "../../ui/SettingsGroup";
import { SettingContainer } from "../../ui/SettingContainer";
import { ToggleSwitch } from "../../ui/ToggleSwitch";
import { Button } from "../../ui/Button";

/** Lets the Sokki phone app send recordings to this Mac. */
export const PhoneLink: React.FC<{
  meetingSettings: MeetingSettings;
  save: (patch: Partial<MeetingSettings>) => Promise<void>;
}> = ({ meetingSettings, save }) => {
  const { t } = useTranslation();
  const [info, setInfo] = useState<PhoneLinkInfo | null>(null);
  const [copied, setCopied] = useState(false);
  const load = async () => {
    const r = await commands.getPhoneLinkInfo();
    if (r.status === "ok") setInfo(r.data);
  };
  useEffect(() => {
    void load();
    if (!meetingSettings.phone_link_enabled) return;
    // A phone may pair while this page is open.
    const timer = setInterval(() => void load(), 3000);
    return () => clearInterval(timer);
  }, [meetingSettings.phone_link_enabled]);

  const copy = async () => {
    if (!info) return;
    try {
      await navigator.clipboard.writeText(info.ticket);
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    } catch {
      /* the code stays selectable */
    }
  };

  return (
    <SettingsGroup
      title={t("meetings.phone.title")}
      description={t("meetings.phone.description")}
    >
      <ToggleSwitch
        checked={meetingSettings.phone_link_enabled}
        onChange={(v) => void save({ phone_link_enabled: v })}
        label={t("meetings.phone.enable")}
        description={t("meetings.phone.enableDescription")}
        grouped
      />
      <SettingContainer
        title={t("meetings.phone.invite")}
        description={t("meetings.phone.inviteDescription")}
        grouped
      >
        <Button
          size="sm"
          variant="secondary"
          className="inline-flex items-center gap-1.5"
          onClick={() => void openUrl(inviteMailto("mac"))}
        >
          <Mail width={13} height={13} />
          {t("meetings.phone.inviteButton")}
        </Button>
      </SettingContainer>
      {meetingSettings.phone_link_enabled && info && (
        <>
          <SettingContainer
            title={t("meetings.phone.pair")}
            description={t("meetings.phone.pairDescription")}
            layout="stacked"
            grouped
          >
            <div className="flex flex-wrap items-center gap-4">
              <div
                className="w-[168px] h-[168px] rounded-md bg-white p-1 [&>svg]:w-full [&>svg]:h-full"
                aria-label={t("meetings.phone.pair")}
                // Generated in Rust from the ticket; contains no user input.
                dangerouslySetInnerHTML={{ __html: info.qr_svg }}
              />
              <div className="flex flex-col gap-2">
                <Button
                  size="sm"
                  variant="secondary"
                  className="inline-flex items-center gap-1.5"
                  onClick={() => void copy()}
                >
                  <Copy width={13} height={13} />
                  {copied ? t("meetings.phone.copied") : t("meetings.phone.copy")}
                </Button>
                <span className="text-xs text-mid-gray">
                  {t("meetings.phone.code")}:{" "}
                  <span className="font-mono tracking-wider select-text">
                    {info.code}
                  </span>
                </span>
                <Button
                  size="sm"
                  variant="secondary"
                  className="inline-flex items-center gap-1.5"
                  title={t("meetings.phone.codeDescription")}
                  onClick={() =>
                    void commands.renewPhoneLinkCode().then(() => load())
                  }
                >
                  <RefreshCw width={13} height={13} />
                  {t("meetings.phone.renew")}
                </Button>
              </div>
            </div>
          </SettingContainer>
          <SettingContainer
            title={t("meetings.phone.phones")}
            description={info.phones.length ? "" : t("meetings.phone.noPhones")}
            layout="stacked"
            grouped
          >
            <div className="flex flex-col gap-1.5">
              {info.phones.map((p) => (
                <div key={p.id} className="flex items-center justify-between gap-3">
                  <span className="text-sm">
                    {p.name || t("meetings.phone.phone")}
                    <span className="text-xs text-mid-gray ms-2">
                      {t("meetings.phone.pairedOn", {
                        date: new Date(p.paired_at).toLocaleDateString(),
                      })}
                    </span>
                  </span>
                  <Button
                    size="sm"
                    variant="secondary"
                    className="inline-flex items-center gap-1.5"
                    onClick={() =>
                      void commands.removePairedPhone(p.id).then(() => load())
                    }
                  >
                    <Trash2 width={13} height={13} />
                    {t("meetings.phone.remove")}
                  </Button>
                </div>
              ))}
            </div>
          </SettingContainer>
        </>
      )}
    </SettingsGroup>
  );
};
