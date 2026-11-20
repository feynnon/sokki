import React, { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { CheckCircle2, Loader2 } from "lucide-react";
import { listen } from "@tauri-apps/api/event";
import { openUrl } from "@tauri-apps/plugin-opener";
import {
  commands,
  type LlmStatus,
  type MeetingSettings as MeetingSettingsType,
} from "@/bindings";
import { useSettings } from "@/hooks/useSettings";
import { Radio } from "@/components/ui/Radio";
import { Button } from "@/components/ui/Button";
import { Input } from "@/components/ui/Input";
import { Tag } from "@/components/shared/Tag";
import { CODE_CLASS } from "@/components/shared/modelTable/columns";

/** The built-in model's value ("builtin|…"): one model, set by Sokki
 * (`BUILTIN_MODEL` in crates/sokki-engine/src/llm.rs). */
export const BUILTIN_MODEL = "builtin|qwen3.5-9b";

/** The language models offered for meeting notes ("provider|model"). The
 * local ones are the model measured in KNOWN_ISSUES.md, built in or through
 * Ollama; the cloud ones are each provider's current models. Other models
 * can be set in the config file. */
export const LANGUAGE_MODELS: {
  value: string;
  name: string;
  lineKey: string;
  right: string;
  tagKey?: string;
}[] = [
  {
    value: BUILTIN_MODEL,
    name: "Qwen 3.5 9B",
    lineKey: "meetings.lm.qwen",
    right: "5.7 GB",
    tagKey: "meetings.lm.recommended",
  },
  {
    value: "local|qwen3.5:9b",
    name: "Qwen 3.5 9B via Ollama",
    lineKey: "meetings.lm.ollama",
    right: "Ollama",
  },
  {
    value: "anthropic|claude-sonnet-5-5",
    name: "Claude Sonnet 5.5",
    lineKey: "meetings.lm.sonnet",
    right: "Anthropic",
  },
  {
    value: "anthropic|claude-opus-5-5",
    name: "Claude Opus 5.5",
    lineKey: "meetings.lm.opus",
    right: "Anthropic",
  },
  {
    value: "openai|gpt-6.2-sol",
    name: "GPT-6.2 Sol",
    lineKey: "meetings.lm.gpt",
    right: "OpenAI",
  },
  {
    value: "openrouter|anthropic/claude-sonnet-5.5",
    name: "Claude Sonnet 5.5 via OpenRouter",
    lineKey: "meetings.lm.openrouter",
    right: "OpenRouter",
  },
];

export const PROVIDER_NAMES: Record<string, string> = {
  anthropic: "Anthropic",
  openai: "OpenAI",
  openrouter: "OpenRouter",
};

/** Default model per cloud provider when `cloud_model` is empty (keep in
 * sync with `default_cloud_model` in src-tauri/src/meeting/process.rs). */
const CLOUD_DEFAULT_MODEL: Record<string, string> = {
  anthropic: "claude-opus-5-5",
  openai: "gpt-6.2-sol",
  openrouter: "anthropic/claude-opus-5.5",
};

/** The current choice as "provider|model", or "off". */
export function selectedLanguageModel(m: MeetingSettingsType): string {
  if (!m.summary_enabled) return "off";
  const provider = m.llm_provider || "local";
  if (provider === "builtin") return BUILTIN_MODEL;
  if (provider === "local") return `local|${m.llm_model}`;
  return `${provider}|${m.cloud_model || CLOUD_DEFAULT_MODEL[provider] || ""}`;
}

/** A readable name for the current choice. */
export function languageModelName(value: string, offLabel: string): string {
  if (value === "off") return offLabel;
  const known = LANGUAGE_MODELS.find((o) => o.value === value);
  if (known) return `${known.name} · ${known.right}`;
  return value.split("|").slice(1).join("|");
}

/** Status of the selected model, and what to do to make it work. */
const SelectedStatus: React.FC<{ provider: string }> = ({ provider }) => {
  const { t, i18n } = useTranslation();
  const { settings, refreshSettings } = useSettings();
  const [status, setStatus] = useState<LlmStatus | null>(null);
  const [busy, setBusy] = useState(false);
  const [pull, setPull] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);
  const apiKeys = (settings?.post_process_api_keys ?? {}) as Record<
    string,
    string
  >;
  const [key, setKey] = useState(apiKeys[provider] ?? "");
  const providerName = PROVIDER_NAMES[provider] ?? provider;
  const cloud = provider in PROVIDER_NAMES;

  const refresh = useCallback(async (probe = false) => {
    const r = await commands.meetingLlmStatus(probe);
    if (r.status === "ok") setStatus(r.data);
  }, []);

  useEffect(() => {
    setError(null);
    setKey(apiKeys[provider] ?? "");
    void refresh();
  }, [provider, settings?.meetings, refresh]);

  useEffect(() => {
    const unlisten = listen<{ completed: number; total: number }>(
      "meeting-llm-pull",
      (e) => {
        if (e.payload.total > 0)
          setPull(Math.round((100 * e.payload.completed) / e.payload.total));
      },
    );
    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  const run = async (
    action: () => Promise<{ status: string; error?: string }>,
  ) => {
    setBusy(true);
    setError(null);
    try {
      const r = await action();
      if (r.status !== "ok") setError(r.error ?? "");
    } finally {
      setBusy(false);
      setPull(null);
      await refresh();
    }
  };

  const saveKey = async () => {
    if (key.trim() === (apiKeys[provider] ?? "")) return;
    await commands.changePostProcessApiKeySetting(provider, key.trim());
    await refreshSettings();
    if (key.trim()) await run(async () => ({ status: "ok" }));
    await refresh(true);
  };

  const state = status?.state;
  const spin = busy && (
    <Loader2 width={14} height={14} className="animate-spin" />
  );
  return (
    <div className="ms-[40px] me-[14px] mb-[10px] mt-[2px] space-y-2 text-small">
      {cloud && (
        <div className="flex items-center gap-2">
          <Input
            type="password"
            value={key}
            placeholder={t("meetings.lm.apiKeyPlaceholder", {
              provider: providerName,
            })}
            onChange={(e) => setKey(e.target.value)}
            onBlur={() => void saveKey()}
            onKeyDown={(e) => {
              if (e.key === "Enter") void saveKey();
            }}
            className="flex-1"
          />
        </div>
      )}
      <div className="flex items-center gap-3">
        {state === "ready" && (
          <span className="flex items-center gap-2 text-emerald-600">
            <CheckCircle2 width={15} height={15} />
            {!cloud
              ? t("meetings.lm.readyLocal")
              : t("meetings.lm.readyCloud", { provider: providerName })}
          </span>
        )}
        {state === "no_key" && (
          <span className="text-ink-2">
            {t("meetings.lm.noKey", { provider: providerName })}
          </span>
        )}
        {state === "no_ollama" && (
          <>
            <span className="flex-1 text-ink-2">
              {t("meetings.lm.noOllama")}
            </span>
            <Button
              size="sm"
              onClick={() => void openUrl("https://ollama.com/download")}
            >
              {t("meetings.lm.getOllama")}
            </Button>
            <Button
              size="sm"
              variant="secondary"
              onClick={() => void refresh()}
            >
              {t("meetings.lm.recheck")}
            </Button>
          </>
        )}
        {state === "not_running" && (
          <>
            <span className="flex-1 text-ink-2">
              {t("meetings.lm.notRunning")}
            </span>
            <Button
              size="sm"
              disabled={busy}
              className="inline-flex items-center gap-1.5"
              onClick={() => void run(() => commands.startOllama())}
            >
              {spin}
              {t("meetings.lm.startOllama")}
            </Button>
          </>
        )}
        {state === "model_missing" && (
          <>
            <span className="flex-1 text-ink-2 tabular-nums">
              {busy
                ? t("meetings.lm.downloading", { pct: pull ?? 0 })
                : t("meetings.lm.modelMissing", {
                    size: `${new Intl.NumberFormat(i18n.language).format(
                      provider === "builtin" ? 5.7 : 7.6,
                    )} GB`,
                  })}
            </span>
            <Button
              size="sm"
              disabled={busy}
              className="inline-flex items-center gap-1.5"
              onClick={() => void run(() => commands.pullMeetingLlm())}
            >
              {spin}
              {t("meetings.lm.download")}
            </Button>
          </>
        )}
        {state === "error" && (
          <>
            <span className="flex-1 select-text text-red-500">
              {status?.detail}
            </span>
            <Button
              size="sm"
              variant="secondary"
              onClick={() => void refresh(true)}
            >
              {t("meetings.lm.recheck")}
            </Button>
          </>
        )}
      </div>
      {error && <div className="select-text text-red-500">{error}</div>}
    </div>
  );
};

/** One model in the list, styled like the speech model rows. */
const Row: React.FC<{
  name: string;
  line: string;
  right?: string;
  tag?: string;
  selected: boolean;
  onPick: () => void;
}> = ({ name, line, right, tag, selected, onPick }) => (
  <div
    role="button"
    tabIndex={0}
    onClick={onPick}
    onKeyDown={(e) => {
      if (e.key === "Enter" || e.key === " ") {
        e.preventDefault();
        onPick();
      }
    }}
    className={`grid min-h-[50px] w-full grid-cols-[18px_1fr_auto] items-center gap-x-[14px] rounded-control px-[14px] py-[9px] text-start text-small leading-[1.5] text-text focus:outline-none focus-visible:ring-1 focus-visible:ring-logo-primary ${
      selected ? "cursor-default bg-line" : "cursor-pointer hover:bg-wash"
    }`}
  >
    <Radio checked={selected} />
    <span className="flex min-w-0 flex-col items-start">
      <span className="flex min-w-0 max-w-full items-center gap-[6px]">
        <b className="min-w-0 truncate font-[650]">{name}</b>
        {tag && <Tag>{tag}</Tag>}
      </span>
      <span className="max-w-full truncate text-ink-3" title={line}>
        {line}
      </span>
    </span>
    <span className="text-ink-3 tabular-nums">{right}</span>
  </div>
);

/** The Models page tab for the language model that summarises meetings. */
export const LanguageModels: React.FC = () => {
  const { t } = useTranslation();
  const { settings, refreshSettings } = useSettings();
  const m = settings?.meetings as MeetingSettingsType | undefined;
  if (!m) return null;
  const selected = selectedLanguageModel(m);

  const choose = async (value: string) => {
    if (value === selected) return;
    let patch: Partial<MeetingSettingsType>;
    if (value === "off") {
      patch = { summary_enabled: false };
    } else {
      const [p, ...rest] = value.split("|");
      const model = rest.join("|");
      patch =
        p === "builtin"
          ? {
              summary_enabled: true,
              llm_provider: "builtin",
            }
          : p === "local"
            ? {
                summary_enabled: true,
                llm_provider: "local",
                llm_model: model,
              }
            : {
                summary_enabled: true,
                llm_provider: p,
                cloud_model: model,
              };
    }
    await commands.setMeetingSettings({ ...m, ...patch });
    await refreshSettings();
  };

  const row = (
    value: string,
    props: Omit<React.ComponentProps<typeof Row>, "selected" | "onPick">,
  ) => (
    <React.Fragment key={value}>
      <Row
        {...props}
        selected={selected === value}
        onPick={() => void choose(value)}
      />
      {selected === value && value !== "off" && (
        <SelectedStatus provider={value.split("|")[0]} />
      )}
    </React.Fragment>
  );

  const known = LANGUAGE_MODELS.some((o) => o.value === selected);
  const section = (label: string, rows: React.ReactNode) => (
    <div className="space-y-[2px]">
      <div className={`h-[32px] flex items-center px-[14px] ${CODE_CLASS}`}>
        {label}
      </div>
      {rows}
    </div>
  );

  return (
    <div className="flex min-h-0 w-full flex-1 flex-col overflow-y-auto rounded-panel bg-background px-[6px] py-[6px]">
      <p className="px-[14px] pt-[6px] pb-[10px] text-small text-ink-3">
        {t("meetings.lm.intro")}
      </p>
      {section(
        t("meetings.lm.onThisMac"),
        LANGUAGE_MODELS.filter(
          (o) => !(o.value.split("|")[0] in PROVIDER_NAMES),
        ).map((o) =>
          row(o.value, {
            name: o.name,
            line: t(o.lineKey),
            right: o.right,
            tag: o.tagKey && t(o.tagKey),
          }),
        ),
      )}
      {section(
        t("meetings.lm.cloud"),
        LANGUAGE_MODELS.filter(
          (o) => o.value.split("|")[0] in PROVIDER_NAMES,
        ).map((o) =>
          row(o.value, {
            name: o.name,
            line: t(o.lineKey),
            right: o.right,
          }),
        ),
      )}
      {section(
        t("meetings.lm.other"),
        <>
          {!known &&
            selected !== "off" &&
            row(selected, {
              name: selected.split("|").slice(1).join("|"),
              line: t("meetings.lm.custom"),
            })}
          {row("off", {
            name: t("meetings.lm.off"),
            line: t("meetings.lm.offLine"),
          })}
        </>,
      )}
    </div>
  );
};
