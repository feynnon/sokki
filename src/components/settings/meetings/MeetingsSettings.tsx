import React, { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  AlertTriangle,
  ArrowLeft,
  FolderOpen,
  Loader2,
  Pencil,
  RotateCcw,
  Settings,
  Trash2,
} from "lucide-react";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";
import {
  languageModelName,
  selectedLanguageModel,
} from "../models/LanguageModels";
import { useModelsTab } from "@/stores/modelsTabStore";
import {
  ReaderToolbar,
  SummaryView,
  TranscriptView,
  useMeetingNote,
} from "./MeetingDetails";
import {
  byDay,
  dayHeader,
  meetingPlace,
  isDateTitle,
  minutes,
  otherSpeakers,
  readerWhen,
  time,
} from "./meetingDates";
import { useMeetingStore } from "@/stores/meetingStore";
import { useFormatLocale } from "./useFormatLocale";
import { PhoneLink } from "./PhoneLink";
import { MailSettings } from "./MailSettings";
import { SpeakerChips } from "./SpeakerChips";
import { LiveTranscript, SampleCard, SampleTag } from "./SampleMeeting";
import { SlackHuddles } from "./SlackHuddles";
import {
  commands,
  events,
  type LlmStatus,
  type MeetingRecord,
  type MeetingSettings as MeetingSettingsType,
} from "@/bindings";
import { useSettings } from "../../../hooks/useSettings";
import { formatDateTime } from "@/utils/dateFormat";
import { SettingsGroup } from "../../ui/SettingsGroup";
import { SettingContainer } from "../../ui/SettingContainer";
import { ToggleSwitch } from "../../ui/ToggleSwitch";
import { Switch } from "../../ui/Switch";
import { Input } from "../../ui/Input";
import { Button } from "../../ui/Button";
import { Tooltip } from "../../ui/Tooltip";

/** Opens Models → Language model (App switches section on this event). */
function openLanguageModels() {
  useModelsTab.getState().setTab("language");
  window.dispatchEvent(
    new CustomEvent("sokki-open-section", { detail: "models" }),
  );
}

/** A text field that saves when it loses focus or on Enter, not per keystroke. */
const SavedInput: React.FC<{
  value: string;
  onSave: (value: string) => void;
  placeholder?: string;
  type?: string;
  className?: string;
  ariaLabel?: string;
}> = ({ value, onSave, placeholder, type, className, ariaLabel }) => {
  const [draft, setDraft] = useState(value);
  useEffect(() => setDraft(value), [value]);
  const commit = () => {
    if (draft !== value) onSave(draft);
  };
  return (
    <Input
      value={draft}
      type={type}
      placeholder={placeholder}
      className={className}
      aria-label={ariaLabel}
      onChange={(e) => setDraft(e.target.value)}
      onBlur={commit}
      onKeyDown={(e) => {
        if (e.key === "Enter") commit();
      }}
    />
  );
};

/** Where "Support Sokki" leads; sokki.org/sponsor/ forwards to the sponsor page. */
const SUPPORT_URL = "https://sokki.org/sponsor/?s=line";

/**
 * The Sokki line under every summary, shown as it will read. On for
 * everyone; only a supporter (at least €20 paid, proven by a code) gets a
 * switch. Processing checks the stored code itself, so this only shows it.
 */
const SokkiLine: React.FC<{
  meetingSettings: MeetingSettingsType;
  save: (patch: Partial<MeetingSettingsType>) => Promise<void>;
}> = ({ meetingSettings, save }) => {
  const { t, i18n } = useTranslation();
  const [supporter, setSupporter] = useState(false);
  const [line, setLine] = useState("");
  const [entering, setEntering] = useState(false);
  const [invalid, setInvalid] = useState(false);
  const german = i18n.language.startsWith("de");
  useEffect(() => {
    void commands
      .isSupporterCode(meetingSettings.supporter_code)
      .then(setSupporter);
  }, [meetingSettings.supporter_code]);
  // The line drops "local, free" for a cloud model, so ask again on a change.
  useEffect(() => {
    void commands
      .sokkiLine(german)
      .then((l) => setLine(l ?? ""))
      .catch(() => setLine(""));
  }, [german, meetingSettings.llm_provider]);
  const enterCode = async (code: string) => {
    const c = code.trim();
    const ok = c !== "" && (await commands.isSupporterCode(c));
    setInvalid(c !== "" && !ok);
    if (ok) await save({ supporter_code: c });
  };
  const SITE = "sokki.org";
  const site = line.lastIndexOf(SITE);
  return (
    <div className="flex items-start justify-between gap-8 px-4 py-3">
      <div className="min-w-0 flex-1">
        <h3 className="text-[14px] font-medium leading-5">
          {t("meetings.settings.footer")}
        </h3>
        <p className="text-[12.5px] leading-[18px] text-ink-3 mt-0.5">
          {t("meetings.settings.footerDescription")}
        </p>
        <p className="mt-2 rounded-lg bg-wash px-3 py-2 text-[12.5px] leading-[18px] text-ink-2 select-text">
          {site < 0 ? (
            line
          ) : (
            <>
              {line.slice(0, site)}
              <span className="text-logo-primary">{SITE}</span>
              {line.slice(site + SITE.length)}
            </>
          )}
        </p>
        {supporter ? (
          <p className="text-[12.5px] leading-[18px] text-ink-3 mt-1.5">
            {t("meetings.settings.supporterThanks")}
          </p>
        ) : (
          <>
            <p className="text-[12.5px] leading-[18px] text-ink-3 mt-1.5">
              {t("meetings.settings.footerSupporters")}{" "}
              {!entering && (
                <button
                  type="button"
                  className="text-logo-primary hover:underline"
                  onClick={() => setEntering(true)}
                >
                  {t("meetings.settings.supporterEnter")}
                </button>
              )}
            </p>
            {entering && (
              <div className="mt-2 flex flex-wrap items-center gap-2">
                <SavedInput
                  value=""
                  placeholder="sokki-…"
                  ariaLabel={t("meetings.settings.supporterCode")}
                  className="w-[var(--vb-control-w)] h-[var(--vb-control-h)] text-[13px]"
                  onSave={(v) => void enterCode(v)}
                />
                <Button
                  variant="ghost"
                  size="sm"
                  onClick={() => void openUrl(SUPPORT_URL)}
                >
                  {t("meetings.settings.supporterLink")}
                </Button>
              </div>
            )}
            {invalid && (
              <p className="text-xs text-red-500 mt-1">
                {t("meetings.settings.supporterCodeInvalid")}
              </p>
            )}
          </>
        )}
      </div>
      {supporter && (
        <div className="shrink-0 pt-0.5">
          <Switch
            checked={meetingSettings.footer_enabled}
            onChange={(v) => void save({ footer_enabled: v })}
            ariaLabel={t("meetings.settings.footer")}
          />
        </div>
      )}
    </div>
  );
};

/** One line of status under a meeting. */
const StatusLine: React.FC<{ record: MeetingRecord }> = ({ record }) => {
  const { t } = useTranslation();
  const s = record.status;
  if (s.kind === "recording") {
    return (
      <span className="flex items-center gap-2 text-sm text-red-500">
        <span className="inline-block h-2 w-2 rounded-full bg-red-500 animate-pulse" />
        {t("meetings.recording")}
      </span>
    );
  }
  if (s.kind === "processing") {
    const pct = s.progress != null ? ` ${Math.round(s.progress * 100)}%` : "";
    return (
      <span className="flex flex-col gap-0.5">
        <span className="flex items-center gap-2 text-sm text-ink-3">
          <Loader2 width={14} height={14} className="animate-spin" />
          {t(`meetings.stage.${s.stage}`, { defaultValue: s.stage })}
          {pct}
        </span>
        <span className="ps-[22px] text-xs text-ink-3">
          {t(record.sample ? "meetings.sample.soon" : "meetings.notesWhen")}
        </span>
      </span>
    );
  }
  if (s.kind === "failed") {
    return (
      <span className="flex flex-wrap items-center gap-x-2 gap-y-1 text-sm text-amber-600 select-text">
        <AlertTriangle width={14} height={14} className="shrink-0" />
        {s.error}
        {!record.audio_deleted && (
          <TryAgain onClick={() => void commands.reprocessMeeting(record.id)} />
        )}
      </span>
    );
  }
  return null;
};

/** A small "Try again" link after a problem's sentence. */
const TryAgain: React.FC<{ onClick: () => void }> = ({ onClick }) => {
  const { t } = useTranslation();
  return (
    <button
      type="button"
      onClick={onClick}
      className="inline-flex items-center gap-1 text-xs font-medium text-logo-primary hover:underline"
    >
      <RotateCcw width={12} height={12} />
      {t("meetings.tryAgain")}
    </button>
  );
};

/** The problems a done meeting had, one short sentence each. A missing
 * summary is the last line and gets a "Try again". */
const Warnings: React.FC<{ record: MeetingRecord }> = ({ record }) => {
  if (record.warnings.length === 0) return null;
  const last = record.warnings.length - 1;
  return (
    <div className="space-y-0.5">
      {record.warnings.map((w, i) => (
        <div
          key={w}
          className="flex flex-wrap items-center gap-x-2 text-xs text-amber-600 select-text"
        >
          {w}
          {i === last &&
            record.summary_missing &&
            record.status.kind === "done" && (
              <TryAgain
                onClick={() => void commands.summarizeMeeting(record.id)}
              />
            )}
        </div>
      ))}
    </div>
  );
};

/** The current language model, with a way to the Models page to change it. */
const ModelSetting: React.FC<{ meetingSettings: MeetingSettingsType }> = ({
  meetingSettings,
}) => {
  const { t } = useTranslation();
  const name = languageModelName(
    selectedLanguageModel(meetingSettings),
    t("meetings.lm.off"),
  );
  return (
    <SettingContainer
      title={t("meetings.settings.model")}
      description={t("meetings.settings.modelDescription")}
      grouped
    >
      <div className="flex items-center gap-2">
        <span className="text-sm text-ink-2">{name}</span>
        <Button size="sm" variant="secondary" onClick={openLanguageModels}>
          {t("meetings.settings.change")}
        </Button>
      </div>
    </SettingContainer>
  );
};

/** Everything that can be set for meetings; kept apart from the list. */
const MeetingSettingsView: React.FC<{
  meetingSettings: MeetingSettingsType;
  save: (patch: Partial<MeetingSettingsType>) => Promise<void>;
  onBack: () => void;
}> = ({ meetingSettings, save, onBack }) => {
  const { t } = useTranslation();
  const notesDir = meetingSettings.notes_dir || "~/Documents/Sokki";
  const chooseFolder = async () => {
    const dir = await openDialog({ directory: true, multiple: false });
    if (typeof dir === "string" && dir) await save({ notes_dir: dir });
  };

  return (
    <div className="flex-1 min-h-0 overflow-y-auto px-8 py-6">
      <div className="max-w-3xl w-full mx-auto space-y-6">
        <Button
          variant="ghost"
          size="sm"
          onClick={onBack}
          className="inline-flex items-center gap-1.5"
        >
          <ArrowLeft width={14} height={14} />
          {t("meetings.back")}
        </Button>

        <SettingsGroup title={t("meetings.settings.notesTitle")}>
          <SettingContainer
            title={t("meetings.settings.notesDir")}
            description={t("meetings.settings.notesDirDescription")}
            grouped
          >
            <div className="flex items-center gap-2 min-w-0">
              <span
                className="text-sm text-ink-2 truncate max-w-56"
                title={notesDir}
              >
                {notesDir.replace(/^\/Users\/[^/]+/, "~")}
              </span>
              <Button
                size="sm"
                variant="secondary"
                onClick={() => void chooseFolder()}
                className="inline-flex items-center gap-1.5"
              >
                <FolderOpen width={14} height={14} />
                {t("meetings.settings.choose")}
              </Button>
            </div>
          </SettingContainer>
          <SettingContainer
            title={t("meetings.settings.userName")}
            description={t("meetings.settings.userNameDescription")}
            grouped
          >
            <SavedInput
              value={meetingSettings.user_name}
              className="w-[var(--vb-control-w)] h-[var(--vb-control-h)] text-[13px]"
              onSave={(v) => void save({ user_name: v.trim() || "Ich" })}
            />
          </SettingContainer>
          <ToggleSwitch
            checked={meetingSettings.detection_enabled}
            onChange={(v) => void save({ detection_enabled: v })}
            label={t("meetings.settings.detection")}
            description={t("meetings.settings.detectionDescription")}
            grouped
          />
          <ToggleSwitch
            checked={meetingSettings.chat_notice_enabled}
            onChange={(v) => void save({ chat_notice_enabled: v })}
            label={t("meetings.settings.chatNotice")}
            description={t("meetings.settings.chatNoticeDescription")}
            grouped
          />
          <ToggleSwitch
            checked={meetingSettings.call_names_enabled}
            onChange={(v) => void save({ call_names_enabled: v })}
            label={t("meetings.settings.callNames")}
            description={t("meetings.settings.callNamesDescription")}
            grouped
          />
          <ToggleSwitch
            checked={meetingSettings.silence_hint_enabled}
            onChange={(v) => void save({ silence_hint_enabled: v })}
            label={t("meetings.settings.silenceHint")}
            description={t("meetings.settings.silenceHintDescription")}
            grouped
          />
          <SlackHuddles meetingSettings={meetingSettings} save={save} />
          <SokkiLine meetingSettings={meetingSettings} save={save} />
        </SettingsGroup>

        <SettingsGroup
          title={t("meetings.settings.aiTitle")}
          description={t("meetings.settings.aiDescription")}
        >
          <ModelSetting meetingSettings={meetingSettings} />
        </SettingsGroup>

        <MailSettings meetingSettings={meetingSettings} save={save} />

        <PhoneLink meetingSettings={meetingSettings} save={save} />
      </div>
    </div>
  );
};

/** Where the meeting took place, in the app's words. */
function usePlace(record: MeetingRecord): string | null {
  const { t } = useTranslation();
  if (record.sample) return t("meetings.sample.place");
  return meetingPlace(record, {
    phone: t("meetings.place.phone"),
    online: t("meetings.place.online"),
    inPerson: t("meetings.place.inPerson"),
  });
}

/** The title to show: the meeting's own, else "Google Meet, 10:00" (Robin,
 * 2026-11-24): while it records or is processed, and for a note without a
 * summary, whose title would only repeat the date ("Meeting 2026-11-24
 * 10:00"). */
function useShownTitle(record: MeetingRecord): string {
  const { t } = useTranslation();
  const dates = useFormatLocale();
  const place = usePlace(record);
  if (record.title && !isDateTitle(record.title)) return record.title;
  if (record.sample) return t("meetings.sample.title");
  const at = time(new Date(Date.parse(record.started_at)), dates);
  return place
    ? t("meetings.placeholder", { place, time: at })
    : t("meetings.placeholderNoPlace", { time: at });
}

/** Who spoke besides the user, in words for screen readers: "Lea, Jonas
 * and 2 others". */
function useOthers(record: MeetingRecord, userName: string): string | null {
  const { t } = useTranslation();
  if (record.status.kind !== "done") return null;
  const { named, unnamed } = otherSpeakers(record.speakers, userName);
  const names = named.join(", ");
  if (named.length > 0 && unnamed > 0)
    return t("meetings.others.withOthers", { names, count: unnamed });
  if (named.length > 0) return names;
  if (unnamed > 0) return t("meetings.others.only", { count: unnamed });
  return null;
}

/** Who spoke besides the user, on the right of a row (variant V2, Robin
 * 2026-11-24): a circle with the initial of up to two named people, then
 * how many more; every name in a tooltip. */
const SpeakerCircles: React.FC<{ record: MeetingRecord; userName: string }> = ({
  record,
  userName,
}) => {
  const label = useOthers(record, userName);
  if (!label) return null;
  const me = userName.trim().toLowerCase();
  const all = record.speakers.filter((s) => s.trim().toLowerCase() !== me);
  const { named } = otherSpeakers(record.speakers, userName);
  const initials = named
    .slice(0, 2)
    .map((n) => Array.from(n.trim())[0]?.toUpperCase() ?? "?");
  const rest = all.length - initials.length;
  const circles =
    rest > 0
      ? [...initials, initials.length ? `+${rest}` : String(rest)]
      : initials;
  return (
    <Tooltip text={all.join(", ")} placement="below">
      <span className="flex shrink-0" aria-label={label} role="img">
        {circles.map((c, i) => (
          <span
            key={i}
            aria-hidden="true"
            className="size-5 grid place-items-center rounded-full border-2 bg-line text-[10px] font-semibold leading-none text-ink-2"
            style={{
              marginInlineStart: i ? -6 : 0,
              borderColor: "var(--vb-list)",
            }}
          >
            {c}
          </span>
        ))}
      </span>
    </Tooltip>
  );
};

/** One row of the list: title, then time · place · length (red while it
 * records; the day's header has the date); who spoke on the right. */
const ListItem: React.FC<{
  record: MeetingRecord;
  selected: boolean;
  userName: string;
  onSelect: () => void;
}> = ({ record, selected, userName, onSelect }) => {
  const { t, i18n } = useTranslation();
  const dates = useFormatLocale();
  const at = new Date(Date.parse(record.started_at));
  const rec = record.status.kind === "recording";
  const place = usePlace(record);
  const shown = useShownTitle(record);
  const meta = rec
    ? [t("meetings.recording"), place].filter(Boolean).join(" · ")
    : [time(at, dates), place, minutes(record.duration_s, i18n.language)]
        .filter(Boolean)
        .join(" · ");
  const guess =
    !record.title || isDateTitle(record.title) || record.title_provisional;
  return (
    <button
      type="button"
      aria-current={selected ? "true" : undefined}
      onClick={onSelect}
      className={`flex items-center gap-2 w-full px-2.5 py-2 rounded-lg text-start cursor-pointer focus:outline-none focus-visible:ring-1 focus-visible:ring-logo-primary ${selected ? "bg-line" : "hover:bg-wash"}`}
    >
      <span className="flex flex-col gap-px min-w-0 flex-1">
        <span
          className={`text-[13px] truncate ${guess ? "font-medium text-ink-2" : "font-semibold text-text"}`}
        >
          {shown}
        </span>
        <span
          className={`text-[12px] truncate ${rec ? "font-medium" : "text-ink-3"}`}
          style={rec ? { color: "var(--vb-rec)" } : undefined}
        >
          {meta}
        </span>
      </span>
      <SpeakerCircles record={record} userName={userName} />
    </button>
  );
};

/** The reader's title. A click turns it into a field; Enter (or leaving
 * the field) saves, Esc cancels. A typed title is kept for good. */
const Title: React.FC<{
  record: MeetingRecord;
  title: string;
  onError: (e: string | null) => void;
}> = ({ record, title, onError }) => {
  const { t } = useTranslation();
  const [draft, setDraft] = useState<string | null>(null);
  // Shown at once, until the record catches up.
  const [saved, setSaved] = useState<string | null>(null);
  useEffect(() => setSaved(null), [record.title]);
  const shown = saved ?? title;
  const guess =
    saved === null &&
    (!record.title || isDateTitle(record.title) || record.title_provisional);
  const heading =
    "font-display text-title font-[650] leading-[1.15] tracking-[-0.015em]";

  const save = async () => {
    if (draft === null) return;
    const next = draft.split(/\s+/).join(" ").trim();
    setDraft(null);
    if (!next || next === shown) return;
    setSaved(next);
    const r = await commands.setMeetingTitle(record.id, next);
    if (r.status === "error") {
      setSaved(null);
      onError(r.error);
    }
  };

  if (draft !== null) {
    return (
      <div>
        <input
          id="meeting-title"
          autoFocus
          value={draft}
          aria-label={t("meetings.titleEdit")}
          onChange={(e) => setDraft(e.target.value)}
          onFocus={(e) => e.target.select()}
          onBlur={() => void save()}
          onKeyDown={(e) => {
            if (e.key === "Enter") void save();
            if (e.key === "Escape") setDraft(null);
          }}
          className={`${heading} w-full rounded-[10px] border border-logo-primary bg-wash px-2.5 py-1 -mx-2.5 -my-1 text-text focus:outline-none`}
        />
        <p className="mt-1.5 text-[12px] text-ink-3">
          {t("meetings.titleEditHint")}
        </p>
      </div>
    );
  }
  return (
    <h1 className={`${heading} group flex flex-wrap items-center gap-x-2.5`}>
      <button
        type="button"
        onClick={() => setDraft(shown)}
        title={t("meetings.titleEdit")}
        className={`text-start select-text cursor-text rounded-md focus:outline-none focus-visible:ring-1 focus-visible:ring-logo-primary ${guess ? "text-ink-2" : "text-text"}`}
      >
        {shown}
      </button>
      {record.title_provisional && saved === null && (
        <span className="text-[11px] font-medium tracking-normal font-sans text-ink-2 bg-wash rounded-md px-[7px] py-[3px]">
          {t("meetings.titleProvisional")}
        </span>
      )}
      <Pencil
        width={16}
        height={16}
        aria-hidden="true"
        className="text-ink-3 opacity-0 group-hover:opacity-100 transition-opacity"
      />
    </h1>
  );
};

/** The right side: one meeting with its summary and transcript. */
const Reader: React.FC<{
  record: MeetingRecord;
  hints: React.ReactNode;
  onChanged: () => void;
}> = ({ record, hints, onChanged }) => {
  const { t, i18n } = useTranslation();
  const dates = useFormatLocale();
  const { parts, error: noteError } = useMeetingNote(record);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => setError(null), [record.id]);
  const at = new Date(Date.parse(record.started_at));
  const title = useShownTitle(record);
  const rec = record.status.kind === "recording";
  const place = usePlace(record);
  const meta = [
    readerWhen(at, new Date(), dates, (g) => t(`meetings.group.${g}`)),
    rec ? null : minutes(record.duration_s, i18n.language),
    place,
  ]
    .filter(Boolean)
    .join(" · ");
  const shown = error ?? noteError;

  return (
    <div className="flex-1 min-w-0 flex flex-col">
      {rec ? (
        <div className="h-[52px] shrink-0" />
      ) : (
        <ReaderToolbar
          record={record}
          title={title}
          date={formatDateTime(String(at.getTime() / 1000), i18n.language)}
          onDeleted={onChanged}
          onError={setError}
        />
      )}
      <div className="flex-1 min-h-0 overflow-y-auto px-12 pt-1 pb-6">
        <div className="max-w-[720px] flex flex-col gap-3">
          {hints}
          <div>
            {record.sample && (
              <div className="mb-2 flex">
                <SampleTag />
              </div>
            )}
            <Title record={record} title={title} onError={setError} />
            <div className="mt-1 text-[13px] text-ink-3">{meta}</div>
          </div>
          {rec && record.sample && <LiveTranscript record={record} />}
          <SpeakerChips record={record} onDone={onChanged} />
          {!rec && <StatusLine record={record} />}
          <Warnings record={record} />
          {shown && <p className="text-xs text-red-500 select-text">{shown}</p>}
          {parts && (
            <>
              <h2 className="mt-3 text-[13px] font-semibold text-text">
                {t("meetings.details.summary")}
              </h2>
              {parts.summary ? (
                <SummaryView markdown={parts.summary} />
              ) : (
                <p className="text-[13px] text-ink-3">
                  {t("meetings.details.noSummary")}
                </p>
              )}
              <h2 className="mt-3 text-[13px] font-semibold text-text">
                {t("meetings.details.transcript")}
              </h2>
              <TranscriptView text={parts.transcript} />
            </>
          )}
        </div>
      </div>
    </div>
  );
};

/** Meetings as a split view, like Mail: the list (288 px) grouped by day,
 * and the selected meeting beside it (design C1). Start and stop live in
 * the recording bar under every view. */
export const MeetingsSettings: React.FC = () => {
  const { t } = useTranslation();
  const dates = useFormatLocale();
  const { settings, refreshSettings } = useSettings();
  const meetings = useMeetingStore((s) => s.meetings);
  const loaded = useMeetingStore((s) => s.loaded);
  const reload = useMeetingStore((s) => s.reload);
  const selectedId = useMeetingStore((s) => s.selectedId);
  const select = useMeetingStore((s) => s.select);
  const [view, setView] = useState<"list" | "settings">("list");
  const [llm, setLlm] = useState<LlmStatus | null>(null);
  const meetingSettings = settings?.meetings as MeetingSettingsType | undefined;

  useEffect(() => {
    if (view !== "list") return;
    void commands.meetingLlmStatus(false).then((r) => {
      if (r.status === "ok") setLlm(r.data);
    });
  }, [view, meetingSettings]);

  const save = async (patch: Partial<MeetingSettingsType>) => {
    if (!meetingSettings) return;
    await commands.setMeetingSettings({ ...meetingSettings, ...patch });
    await refreshSettings();
  };

  if (!meetingSettings) return null;

  if (view === "settings") {
    return (
      <MeetingSettingsView
        meetingSettings={meetingSettings}
        save={save}
        onBack={() => setView("list")}
      />
    );
  }

  const needsSetup =
    llm && !["ready", "off"].includes(llm.state) ? llm.state : null;

  const startOllama = async () => {
    await commands.startOllama();
    const r = await commands.meetingLlmStatus(false);
    if (r.status === "ok") setLlm(r.data);
  };

  const now = new Date();
  // A running meeting first, then newest first.
  const ordered = [...meetings].sort(
    (a, b) =>
      Number(b.status.kind === "recording") -
        Number(a.status.kind === "recording") ||
      Date.parse(b.started_at) - Date.parse(a.started_at),
  );
  const selected =
    ordered.find((m) => m.id === selectedId) ?? ordered[0] ?? null;
  // One group per day, its header with the date (Robin, 2026-11-25).
  const groups = byDay(ordered, (m) => new Date(Date.parse(m.started_at))).map(
    ({ day, items }) => ({
      day,
      items,
      header: dayHeader(new Date(day), now, dates, (g, date) =>
        t(`meetings.group.${g}On`, { date }),
      ),
    }),
  );

  const hints = (
    <>
      {needsSetup === "not_running" && (
        <div className="flex flex-wrap items-center gap-x-2 gap-y-1 text-sm text-amber-600">
          <AlertTriangle width={14} height={14} className="shrink-0" />
          {t("meetings.lm.notRunning")}
          <Button
            size="sm"
            variant="secondary"
            onClick={() => void startOllama()}
          >
            {t("meetings.lm.startOllama")}
          </Button>
        </div>
      )}
      {needsSetup && needsSetup !== "not_running" && (
        <button
          type="button"
          onClick={openLanguageModels}
          className="flex items-center gap-2 text-sm text-amber-600 text-start"
        >
          <AlertTriangle width={14} height={14} className="shrink-0" />
          {t("meetings.setupHint")}
        </button>
      )}
    </>
  );

  return (
    <div className="flex-1 min-h-0 flex">
      <div
        className="w-72 shrink-0 flex flex-col border-e border-line"
        style={{ background: "var(--vb-list)" }}
      >
        <div className="h-[52px] shrink-0 flex items-center justify-between ps-4 pe-3">
          <h1 className="font-display text-[17px] font-[650]">
            {t("meetings.title")}
          </h1>
          <button
            type="button"
            onClick={() => setView("settings")}
            title={t("meetings.settings.title")}
            aria-label={t("meetings.settings.title")}
            className="size-7 flex items-center justify-center rounded-lg text-ink-2 hover:bg-wash hover:text-text cursor-pointer focus:outline-none focus-visible:ring-1 focus-visible:ring-logo-primary"
          >
            <Settings width={16} height={16} />
          </button>
        </div>
        <div className="flex-1 min-h-0 overflow-y-auto px-2 pb-3">
          {loaded && ordered.length === 0 && (
            <p className="px-2 py-3 text-[13px] text-ink-3">
              {t("meetings.empty")}
            </p>
          )}
          {groups.map(({ day, items, header }) => (
            <section key={day} aria-label={header}>
              <h2 className="px-2 pt-3 pb-1 text-[12px] font-medium text-ink-3">
                {header}
              </h2>
              {items.map((m) => (
                <ListItem
                  key={m.id}
                  record={m}
                  userName={meetingSettings.user_name}
                  selected={selected?.id === m.id}
                  onSelect={() => select(m.id)}
                />
              ))}
            </section>
          ))}
        </div>
      </div>
      {selected ? (
        <Reader
          key={selected.id}
          record={selected}
          hints={hints}
          onChanged={() => void reload()}
        />
      ) : (
        <div className="flex-1 min-w-0 overflow-y-auto px-12 pt-[52px] pb-6 flex flex-col gap-4">
          {/* The card says what the sample needs, setup hints included. */}
          {loaded && (
            <SampleCard
              llmReady={llm?.state === "ready"}
              onSetUp={openLanguageModels}
            />
          )}
        </div>
      )}
    </div>
  );
};
