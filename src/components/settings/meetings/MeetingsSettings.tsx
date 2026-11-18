import React, { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  AlertTriangle,
  ArrowLeft,
  FileText,
  FolderOpen,
  Loader2,
  RotateCcw,
  Settings2,
  Trash2,
} from "lucide-react";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import {
  languageModelName,
  selectedLanguageModel,
} from "../models/LanguageModels";
import { useModelsTab } from "@/stores/modelsTabStore";
import { IS_SOKKI } from "@/lib/brand";
import { MeetingDetails } from "./MeetingDetails";
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
import { Input } from "../../ui/Input";
import { Button } from "../../ui/Button";
import { Dropdown } from "../../ui/Dropdown";

const KEEP_AUDIO_CHOICES = [0, 7, 30];

/** Opens Models → Language model (App switches section on this event). */
function openLanguageModels() {
  useModelsTab.getState().setTab("language");
  window.dispatchEvent(
    new CustomEvent("handy-open-section", { detail: "models" }),
  );
}

function formatDuration(seconds: number): string {
  const m = Math.round(seconds / 60);
  return m < 1 ? "<1 min" : `${m} min`;
}

/** A text field that saves when it loses focus or on Enter, not per keystroke. */
const SavedInput: React.FC<{
  value: string;
  onSave: (value: string) => void;
  placeholder?: string;
  type?: string;
  className?: string;
}> = ({ value, onSave, placeholder, type, className }) => {
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
      onChange={(e) => setDraft(e.target.value)}
      onBlur={commit}
      onKeyDown={(e) => {
        if (e.key === "Enter") commit();
      }}
    />
  );
};

/** Speaker names of a meeting; click one to rename it (and remember the voice). */
const Speakers: React.FC<{ record: MeetingRecord; onDone: () => void }> = ({
  record,
  onDone,
}) => {
  const { t } = useTranslation();
  const [editing, setEditing] = useState<string | null>(null);
  const [name, setName] = useState("");
  const [remember, setRemember] = useState(true);
  const [error, setError] = useState<string | null>(null);
  if (record.speakers.length === 0 || record.status.kind !== "done")
    return null;

  const save = async () => {
    if (!editing) return;
    const result = await commands.renameMeetingSpeaker(
      record.id,
      editing,
      name,
      remember,
    );
    if (result.status === "ok") {
      setEditing(null);
      setError(null);
      onDone();
    } else {
      setError(result.error);
    }
  };

  return (
    <div className="space-y-2">
      <div className="flex flex-wrap gap-1">
        {record.speakers.map((sp) => (
          <button
            key={sp}
            type="button"
            className="text-xs px-2 py-0.5 rounded-full border border-line hover:bg-wash"
            title={t("meetings.renameSpeaker")}
            onClick={() => {
              setEditing(sp);
              setName(sp.startsWith("Speaker ") ? "" : sp);
              setError(null);
            }}
          >
            {sp}
          </button>
        ))}
      </div>
      {editing && (
        <div className="flex flex-wrap items-center gap-2">
          <span className="text-xs text-ink-3">{editing} →</span>
          <Input
            autoFocus
            value={name}
            placeholder={t("meetings.newName")}
            onChange={(e) => setName(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") void save();
              if (e.key === "Escape") setEditing(null);
            }}
          />
          <label className="flex items-center gap-1 text-xs">
            <input
              type="checkbox"
              checked={remember}
              onChange={(e) => setRemember(e.target.checked)}
            />
            {t("meetings.rememberVoice")}
          </label>
          <Button size="sm" onClick={() => void save()} disabled={!name.trim()}>
            {t("meetings.save")}
          </Button>
        </div>
      )}
      {error && <div className="text-xs text-red-500">{error}</div>}
    </div>
  );
};

/** Remembered voices, with delete. */
const VoiceProfiles: React.FC = () => {
  const { t } = useTranslation();
  const [names, setNames] = useState<string[]>([]);
  const load = useCallback(async () => {
    const r = await commands.listVoiceProfiles();
    if (r.status === "ok") setNames(r.data);
  }, []);
  useEffect(() => {
    void load();
    const unlisten = events.meetingChanged.listen(() => void load());
    return () => {
      unlisten.then((fn) => fn());
    };
  }, [load]);
  return (
    <SettingsGroup title={t("meetings.voices.title")}>
      <div className="px-4 py-3 space-y-2">
        <p className="text-sm text-ink-3">{t("meetings.voices.description")}</p>
        {names.length === 0 && (
          <p className="text-sm text-ink-3">{t("meetings.voices.empty")}</p>
        )}
        <div className="flex flex-wrap gap-2">
          {names.map((n) => (
            <span
              key={n}
              className="flex items-center gap-1 text-sm px-2 py-0.5 rounded-full border border-line"
            >
              {n}
              <button
                type="button"
                title={t("meetings.voices.forget")}
                onClick={() =>
                  void commands.deleteVoiceProfile(n).then(() => load())
                }
              >
                <Trash2 width={12} height={12} />
              </button>
            </span>
          ))}
        </div>
      </div>
    </SettingsGroup>
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
      <span className="flex items-center gap-2 text-sm text-ink-3">
        <Loader2 width={14} height={14} className="animate-spin" />
        {t(`meetings.stage.${s.stage}`, { defaultValue: s.stage })}
        {pct}
      </span>
    );
  }
  if (s.kind === "failed") {
    return (
      <span className="flex items-start gap-2 text-sm text-red-500 select-text">
        <AlertTriangle width={14} height={14} className="mt-0.5 shrink-0" />
        {s.error}
      </span>
    );
  }
  return null;
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
  const notesDir = meetingSettings.notes_dir || "~/Documents/Handy Meetings";
  const chooseFolder = async () => {
    const dir = await openDialog({ directory: true, multiple: false });
    if (typeof dir === "string" && dir) await save({ notes_dir: dir });
  };
  const keep = meetingSettings.keep_audio_days;
  const keepChoices = KEEP_AUDIO_CHOICES.includes(keep)
    ? KEEP_AUDIO_CHOICES
    : [...KEEP_AUDIO_CHOICES, keep].sort((a, b) => a - b);

  return (
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
            onSave={(v) => void save({ user_name: v.trim() || "Ich" })}
          />
        </SettingContainer>
        {!IS_SOKKI && (
          <SettingContainer
            title={t("meetings.settings.dictionary")}
            description={t("meetings.settings.dictionaryDescription")}
            grouped
            layout="stacked"
          >
            <SavedInput
              value={meetingSettings.glossary.join(", ")}
              placeholder="Kalasar, AVDO, Claude Code"
              className="w-full"
              onSave={(v) =>
                void save({
                  glossary: v
                    .split(",")
                    .map((w) => w.trim())
                    .filter((w) => w.length > 0),
                })
              }
            />
          </SettingContainer>
        )}
        <ToggleSwitch
          checked={meetingSettings.detection_enabled}
          onChange={(v) => void save({ detection_enabled: v })}
          label={t("meetings.settings.detection")}
          description={t("meetings.settings.detectionDescription")}
          grouped
        />
        {!IS_SOKKI && (
          <SettingContainer
            title={t("meetings.settings.keepAudio")}
            description={t("meetings.settings.keepAudioDescription")}
            grouped
          >
            <Dropdown
              selectedValue={String(keep)}
              onSelect={(v) => void save({ keep_audio_days: Number(v) })}
              options={keepChoices.map((d) => ({
                value: String(d),
                label:
                  d === 0
                    ? t("meetings.settings.keepAudioNone")
                    : t("meetings.settings.keepAudioDays", { count: d }),
              }))}
            />
          </SettingContainer>
        )}
      </SettingsGroup>

      <SettingsGroup
        title={t("meetings.settings.aiTitle")}
        description={t("meetings.settings.aiDescription")}
      >
        <ModelSetting meetingSettings={meetingSettings} />
      </SettingsGroup>

      <VoiceProfiles />
    </div>
  );
};

export const MeetingsSettings: React.FC = () => {
  const { t, i18n } = useTranslation();
  const { settings, refreshSettings } = useSettings();
  const [meetings, setMeetings] = useState<MeetingRecord[]>([]);
  const [recording, setRecording] = useState(false);
  const [busy, setBusy] = useState(false);
  const [view, setView] = useState<"list" | "settings">("list");
  const [llm, setLlm] = useState<LlmStatus | null>(null);
  const meetingSettings = settings?.meetings as MeetingSettingsType | undefined;

  const reload = useCallback(async () => {
    const result = await commands.listMeetings();
    if (result.status === "ok") setMeetings(result.data);
    setRecording(await commands.isMeetingRecording());
  }, []);

  useEffect(() => {
    void reload();
    const unlisten = events.meetingChanged.listen(() => void reload());
    return () => {
      unlisten.then((fn) => fn());
    };
  }, [reload]);

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

  const toggleRecording = async () => {
    setBusy(true);
    try {
      const result = recording
        ? await commands.stopMeetingRecording()
        : await commands.startMeetingRecording();
      if (result.status !== "ok") console.error(result.error);
      await reload();
    } finally {
      setBusy(false);
    }
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

  return (
    <div className="max-w-3xl w-full mx-auto space-y-6">
      <div
        className={`${IS_SOKKI ? "px-1" : "px-4"} flex items-start justify-between gap-4`}
      >
        {IS_SOKKI ? (
          <div className="min-w-0">
            <h1 className="text-[22px] font-semibold tracking-[-0.01em]">
              {t("meetings.title")}
            </h1>
            <p className="text-[13px] text-ink-3 mt-1 max-w-[60ch]">
              {t("meetings.description")}
            </p>
          </div>
        ) : (
          <p className="text-sm text-ink-3">{t("meetings.description")}</p>
        )}
        <div className="flex items-center gap-2 shrink-0">
          <Button
            variant={recording ? "danger" : "primary"}
            size="md"
            onClick={toggleRecording}
            disabled={busy}
          >
            {recording ? t("meetings.stop") : t("meetings.start")}
          </Button>
          <Button
            variant="secondary"
            size="md"
            onClick={() => setView("settings")}
            title={t("meetings.settings.title")}
          >
            <Settings2 width={16} height={16} />
          </Button>
        </div>
      </div>

      {needsSetup && (
        <button
          type="button"
          onClick={openLanguageModels}
          className="mx-4 flex items-center gap-2 text-sm text-amber-600 text-start"
        >
          <AlertTriangle width={14} height={14} className="shrink-0" />
          {t("meetings.setupHint")}
        </button>
      )}

      <SettingsGroup title={t("meetings.listTitle")}>
        {meetings.length === 0 && (
          <div className="px-4 py-3 text-sm text-ink-3">
            {t("meetings.empty")}
          </div>
        )}
        {meetings.map((m) => (
          <div key={m.id} className="px-4 py-3 space-y-1">
            <div className="flex items-center justify-between gap-2">
              <div className="min-w-0">
                <div className="text-sm font-medium truncate">
                  {m.title ?? t("meetings.untitled")}
                </div>
                <div className="text-xs text-ink-3">
                  {formatDateTime(
                    String(Date.parse(m.started_at) / 1000),
                    i18n.language,
                  )}{" "}
                  · {formatDuration(m.duration_s)}
                  {m.source_app && ` · ${m.source_app}`}
                  {m.online != null &&
                    ` · ${m.online ? t("meetings.online") : t("meetings.singleMic")}`}
                </div>
              </div>
              <div className="flex items-center gap-1 shrink-0">
                {m.note_path && (
                  <Button
                    variant="ghost"
                    size="sm"
                    onClick={() => void commands.openMeetingNote(m.id)}
                    title={t("meetings.openNote")}
                  >
                    <FileText width={14} height={14} />
                  </Button>
                )}
                {!m.audio_deleted &&
                  (m.status.kind === "done" || m.status.kind === "failed") && (
                    <Button
                      variant="ghost"
                      size="sm"
                      onClick={() => void commands.reprocessMeeting(m.id)}
                      title={t("meetings.reprocess")}
                    >
                      <RotateCcw width={14} height={14} />
                    </Button>
                  )}
                <Button
                  variant="danger-ghost"
                  size="sm"
                  onClick={() =>
                    void commands.deleteMeeting(m.id).then(() => reload())
                  }
                  title={t("meetings.delete")}
                >
                  <Trash2 width={14} height={14} />
                </Button>
              </div>
            </div>
            <StatusLine record={m} />
            <Speakers record={m} onDone={() => void reload()} />
            {m.status.kind === "done" && m.note_path && (
              <MeetingDetails
                id={m.id}
                noteKey={`${m.title}|${m.speakers.join(",")}|${m.note_path}`}
              />
            )}
            {m.warnings.map((w) => (
              <div key={w} className="text-xs text-amber-600 select-text">
                {w}
              </div>
            ))}
          </div>
        ))}
      </SettingsGroup>
    </div>
  );
};
