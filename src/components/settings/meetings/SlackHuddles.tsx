import React, { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Loader2 } from "lucide-react";
import {
  commands,
  type MeetingSettings as MeetingSettingsType,
  type SlackStatus,
} from "@/bindings";
import { Button } from "../../ui/Button";
import { Switch } from "../../ui/Switch";

/** Settings › Meetings › Slack: connect once, then the notes of every huddle
 * go into the huddle's thread (`src-tauri/src/meeting/slack.rs`). Drawn as
 * Robin approved it on 2026-11-24: a connect button, then "Connected to
 * <workspace>." with the switch and "Disconnect". */
export const SlackHuddles: React.FC<{
  meetingSettings: MeetingSettingsType;
  save: (patch: Partial<MeetingSettingsType>) => Promise<void>;
}> = ({ meetingSettings, save }) => {
  const { t } = useTranslation();
  const [status, setStatus] = useState<SlackStatus | null>(null);
  const [connecting, setConnecting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    void commands.slackStatus().then(setStatus);
  }, []);

  // Not set up in this build: no row at all.
  if (!status?.available) return null;

  const connect = async () => {
    setError(null);
    setConnecting(true);
    const r = await commands.slackConnect();
    setConnecting(false);
    if (r.status === "ok") {
      setStatus(r.data);
      // Connecting turns posting on; the stored settings already say so.
      await save({ slack_post_huddles: true });
    } else if (!r.error.includes("cancelled")) {
      setError(r.error);
    }
  };
  const disconnect = async () => {
    const r = await commands.slackDisconnect();
    if (r.status === "ok") setStatus(r.data);
  };

  return (
    <div className="px-4 py-3">
      <div className="flex items-start justify-between gap-8">
        <div className="min-w-0 flex-1">
          <h3 className="text-[14px] font-medium leading-5">
            {t("meetings.settings.slack")}
          </h3>
          <p className="text-[12.5px] leading-[18px] text-ink-3 mt-0.5">
            {status.connected
              ? t("meetings.settings.slackConnected", {
                  team: status.team || "Slack",
                })
              : t("meetings.settings.slackDescription")}
          </p>
        </div>
        {status.connected ? (
          <Button
            size="sm"
            variant="secondary"
            onClick={() => void disconnect()}
          >
            {t("meetings.settings.slackDisconnect")}
          </Button>
        ) : connecting ? (
          <div className="flex items-center gap-2">
            <span className="inline-flex items-center gap-1.5 text-[12.5px] text-ink-3">
              <Loader2 width={14} height={14} className="animate-spin" />
              {t("meetings.settings.slackWaiting")}
            </span>
            <Button
              size="sm"
              variant="ghost"
              onClick={() => void commands.slackCancelConnect()}
            >
              {t("meetings.settings.slackCancel")}
            </Button>
          </div>
        ) : (
          <Button size="sm" variant="primary" onClick={() => void connect()}>
            {t("meetings.settings.slackConnect")}
          </Button>
        )}
      </div>
      {status.connected && (
        <div className="mt-3 flex items-center justify-between gap-8">
          <span
            id="slack-post-label"
            className="text-[13px] leading-5 text-ink-2"
          >
            {t("meetings.settings.slackPost")}
          </span>
          <Switch
            checked={meetingSettings.slack_post_huddles}
            onChange={(v) => void save({ slack_post_huddles: v })}
            ariaLabelledBy="slack-post-label"
          />
        </div>
      )}
      {error && (
        <p className="text-[12.5px] leading-[18px] text-error mt-1.5">
          {t("meetings.settings.slackFailed", { error })}
        </p>
      )}
    </div>
  );
};
