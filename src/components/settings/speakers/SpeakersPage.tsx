import React, { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { Lock, Pause, Play } from "lucide-react";
import { commands, events, type VoiceInfo } from "@/bindings";
import { Button } from "@/components/ui/Button";
import { Input } from "@/components/ui/Input";
import { useFormatLocale } from "../meetings/useFormatLocale";
import { parseEmails, useVoiceClip } from "./voiceClip";

/** Voice A, B, … for voices without a name, in list order. */
function letters(voices: VoiceInfo[]): Map<string, string> {
  const out = new Map<string, string>();
  let i = 0;
  for (const v of voices) {
    if (v.name) continue;
    out.set(v.id, i < 26 ? String.fromCharCode(65 + i) : String(i + 1));
    i++;
  }
  return out;
}

function shortDate(iso: string, locale: string): string {
  const d = new Date(Date.parse(iso));
  return d.toLocaleDateString(locale, { day: "numeric", month: "numeric" });
}

/** The voices of all meetings, so the store reloads after a change. */
export function useVoices() {
  const [voices, setVoices] = useState<VoiceInfo[] | null>(null);
  const [once, setOnce] = useState(0);
  const load = useCallback(async () => {
    const r = await commands.listVoices();
    if (r.status === "ok") {
      setVoices(r.data.voices);
      setOnce(r.data.once);
    }
  }, []);
  useEffect(() => {
    void load();
    const unlisten = events.meetingChanged.listen(() => void load());
    return () => {
      void unlisten.then((fn) => fn());
    };
  }, [load]);
  return { voices, once, load, setVoices };
}

const Avatar: React.FC<{ label: string; named: boolean; big?: boolean }> = ({
  label,
  named,
  big,
}) => (
  <span
    aria-hidden="true"
    className={`grid place-items-center shrink-0 rounded-full font-semibold ${big ? "size-12 text-[17px]" : "size-8 text-[12px]"} ${named ? "text-white bg-background-ui" : "text-ink-2 border border-dashed border-ink-3"}`}
  >
    {label}
  </span>
);

/** The Sprecher page (design C, Robin 2026-11-25): every voice across
 * meetings on the left; on the right who it is and what it said. */
export const SpeakersPage: React.FC = () => {
  const { t } = useTranslation();
  const locale = useFormatLocale();
  const { voices, once, load, setVoices } = useVoices();
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const letterOf = useMemo(() => letters(voices ?? []), [voices]);

  const ordered = useMemo(() => {
    const v = voices ?? [];
    const unnamed = v.filter((x) => !x.name);
    const named = v.filter((x) => x.name);
    named.sort(
      (a, b) =>
        Number(b.is_user) - Number(a.is_user) ||
        (a.name ?? "").localeCompare(b.name ?? ""),
    );
    return { unnamed, named };
  }, [voices]);
  const selected =
    (voices ?? []).find((v) => v.id === selectedId) ??
    ordered.unnamed[0] ??
    ordered.named[0] ??
    null;

  const label = (v: VoiceInfo) =>
    v.is_user
      ? t("speakers.you")
      : (v.name ?? t("speakers.voice", { letter: letterOf.get(v.id) ?? "?" }));
  const initial = (v: VoiceInfo) =>
    v.name
      ? (Array.from(v.name.trim())[0]?.toUpperCase() ?? "?")
      : (letterOf.get(v.id) ?? "?");

  const Row: React.FC<{ v: VoiceInfo }> = ({ v }) => {
    const on = selected?.id === v.id;
    return (
      <button
        type="button"
        aria-current={on ? "true" : undefined}
        onClick={() => setSelectedId(v.id)}
        className={`flex items-center gap-2.5 w-full px-2.5 py-2 rounded-lg text-start cursor-pointer focus:outline-none focus-visible:ring-1 focus-visible:ring-logo-primary ${on ? "bg-line" : "hover:bg-wash"}`}
      >
        <Avatar label={initial(v)} named={!!v.name} />
        <span className="flex flex-col min-w-0 flex-1">
          <span className="text-[13px] font-semibold truncate">{label(v)}</span>
          <span className="text-[12px] text-ink-3 truncate">
            {v.emails.length > 0
              ? `${v.emails[0]} · ${t("speakers.meetings", { count: v.meetings })}`
              : t("speakers.meetings", { count: v.meetings })}
          </span>
        </span>
        {v.suggestion && (
          <span className="text-[12px] text-logo-primary truncate max-w-[84px]">
            {v.suggestion.name.split(" ")[0]}?
          </span>
        )}
        {v.name && v.unnamed > 0 && (
          <span
            className="size-2 rounded-full shrink-0 bg-logo-primary"
            aria-label={t("speakers.stillUnnamed", { count: v.unnamed })}
          />
        )}
      </button>
    );
  };

  return (
    <div className="flex-1 min-h-0 flex">
      <div
        className="w-72 shrink-0 flex flex-col border-e border-line"
        style={{ background: "var(--vb-list)" }}
      >
        <div className="h-[52px] shrink-0 flex items-center ps-4 pe-3">
          <h1 className="font-display text-[17px] font-[650]">
            {t("speakers.title")}
          </h1>
        </div>
        <div className="flex-1 min-h-0 overflow-y-auto px-2 pb-3">
          {voices && voices.length === 0 && (
            <p className="px-2 py-3 text-[13px] text-ink-3">
              {t("speakers.empty")}
            </p>
          )}
          {ordered.unnamed.length > 0 && (
            <section aria-label={t("speakers.unnamed")}>
              <h2 className="px-2 pt-3 pb-1 text-[12px] font-medium text-ink-3">
                {t("speakers.unnamed")}
              </h2>
              {ordered.unnamed.map((v) => (
                <Row key={v.id} v={v} />
              ))}
            </section>
          )}
          {ordered.named.length > 0 && (
            <section aria-label={t("speakers.named")}>
              <h2 className="px-2 pt-3 pb-1 text-[12px] font-medium text-ink-3">
                {t("speakers.named")}
              </h2>
              {ordered.named.map((v) => (
                <Row key={v.id} v={v} />
              ))}
            </section>
          )}
          {once > 0 && (
            <p className="px-2 pt-4 text-[12px] text-ink-3">
              {t("speakers.once", { count: once })}
            </p>
          )}
          <p className="flex items-start gap-1.5 px-2 pt-4 text-[12px] text-ink-3">
            <Lock width={12} height={12} className="shrink-0 mt-0.5" />
            {t("speakers.privacy")}
          </p>
        </div>
      </div>
      <div className="flex-1 min-w-0 overflow-y-auto px-12 pt-[52px] pb-6">
        {selected ? (
          <VoiceDetail
            key={selected.id}
            voice={selected}
            title={label(selected)}
            initial={initial(selected)}
            locale={locale}
            onNamed={(name) => {
              // Shown at once; the page reloads behind it.
              setVoices(
                (vs) =>
                  vs?.map((x) =>
                    x.id === selected.id ? { ...x, name, unnamed: 0 } : x,
                  ) ?? vs,
              );
              void load();
            }}
            onChanged={() => void load()}
          />
        ) : (
          <p className="max-w-[560px] text-[15px] text-ink-2">
            {t("speakers.intro")}
          </p>
        )}
      </div>
    </div>
  );
};

const VoiceDetail: React.FC<{
  voice: VoiceInfo;
  title: string;
  initial: string;
  locale: string;
  onNamed: (name: string) => void;
  onChanged: () => void;
}> = ({ voice, title, initial, locale, onNamed, onChanged }) => {
  const { t } = useTranslation();
  const clip = useVoiceClip();
  const [name, setName] = useState(voice.name ?? voice.suggestion?.name ?? "");
  const [emails, setEmails] = useState(
    (voice.emails.length
      ? voice.emails
      : voice.suggestion?.email
        ? [voice.suggestion.email]
        : []
    ).join(", "),
  );
  // Consent is the person's, so the box starts empty (GDPR Art. 7).
  const [consent, setConsent] = useState(false);
  const [busy, setBusy] = useState(false);
  const [done, setDone] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const unnamedMembers = voice.members.filter((m) =>
    /^(Speaker|Sprecher|Sprecherin) \d+$/.test(m.speaker),
  );
  const changedName = voice.name ? name.trim() !== voice.name : true;
  const first = name.trim().split(" ")[0] || "";

  const nameIt = async (members = voice.members) => {
    const n = name.trim();
    if (!n) return;
    setBusy(true);
    setError(null);
    onNamed(n);
    const r = await commands.nameVoice(
      members,
      n,
      parseEmails(emails),
      consent || voice.remembered,
    );
    setBusy(false);
    if (r.status === "ok") {
      setDone(t("speakers.namedIn", { count: r.data }));
      onChanged();
    } else {
      setError(r.error);
    }
  };

  const saveEmails = async () => {
    if (!voice.name) return;
    const r = await commands.setPersonEmails(voice.name, parseEmails(emails));
    if (r.status !== "ok") setError(r.error);
    else onChanged();
  };

  const forget = async () => {
    if (!voice.name) return;
    const r = await commands.deleteVoiceProfile(voice.name);
    if (r.status !== "ok") setError(r.error);
    onChanged();
  };

  return (
    <div className="max-w-[640px] flex flex-col gap-6">
      <div className="flex items-center gap-4">
        <Avatar label={initial} named={!!voice.name} big />
        <div className="flex flex-col">
          <h2 className="font-display text-[26px] font-[650] leading-tight">
            {title}
          </h2>
          <span className="text-[13px] text-ink-2">
            {t("speakers.inMeetings", {
              meetings: t("speakers.meetings", { count: voice.meetings }),
              date: new Date(Date.parse(voice.last)).toLocaleDateString(
                locale,
                { day: "numeric", month: "numeric" },
              ),
            })}
          </span>
        </div>
      </div>

      {!voice.is_user && (
        <div className="vb-card p-4 flex flex-col gap-3">
          <h3 className="font-display text-[17px] font-[650]">
            {voice.name ? t("speakers.name") : t("speakers.whoIsThis")}
          </h3>
          <div className="flex gap-2">
            <Input
              value={name}
              placeholder={t("speakers.name")}
              aria-label={t("speakers.name")}
              onChange={(e) => setName(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter" && changedName) void nameIt();
              }}
              className="flex-1"
            />
            {changedName && (
              <Button
                onClick={() => void nameIt()}
                disabled={!name.trim() || busy}
              >
                {t("speakers.nameButton")}
              </Button>
            )}
          </div>
          {voice.suggestion && !voice.name && (
            <div className="flex items-center gap-2 flex-wrap">
              <span className="text-[12px] text-ink-3">
                {t("speakers.suggestion")}
              </span>
              <button
                type="button"
                onClick={() => {
                  setName(voice.suggestion!.name);
                  if (voice.suggestion!.email && !emails)
                    setEmails(voice.suggestion!.email);
                }}
                className="inline-flex items-center gap-1.5 h-7 px-3 rounded-full text-[13px] cursor-pointer bg-wash border border-logo-primary"
              >
                {voice.suggestion.name}
                <span className="text-[12px] text-ink-3">
                  {t("speakers.invitedOf", {
                    invited: voice.suggestion.invited,
                    of: voice.suggestion.of,
                  })}
                </span>
              </button>
            </div>
          )}
          <label className="flex flex-col gap-1">
            <span className="text-[12px] text-ink-3">
              {t("speakers.emails")}
            </span>
            <Input
              value={emails}
              placeholder="lea@example.com, lea.privat@example.com"
              onChange={(e) => setEmails(e.target.value)}
              onBlur={() => void saveEmails()}
            />
            <span className="text-[12px] text-ink-3">
              {t("speakers.emailsHint")}
            </span>
          </label>
          {voice.remembered ? (
            <div className="flex items-center justify-between gap-3 text-[12px] text-ink-2">
              <span>{t("speakers.remembered")}</span>
              <button
                type="button"
                onClick={() => void forget()}
                className="text-[12px] text-red-400 hover:underline cursor-pointer"
              >
                {t("speakers.forget")}
              </button>
            </div>
          ) : (
            <label className="flex items-center gap-2 text-[12px] text-ink-2">
              <input
                type="checkbox"
                checked={consent}
                onChange={(e) => setConsent(e.target.checked)}
              />
              {first
                ? t("speakers.consent", { name: first })
                : t("speakers.consentAnyone")}
            </label>
          )}
          {voice.name && unnamedMembers.length > 0 && (
            <div className="flex items-center justify-between gap-3 text-[13px] text-ink-2">
              <span>
                {t("speakers.stillUnnamed", { count: unnamedMembers.length })}
              </span>
              <Button
                size="sm"
                variant="secondary"
                onClick={() => void nameIt(unnamedMembers)}
                disabled={busy}
              >
                {t("speakers.nameEverywhere")}
              </Button>
            </div>
          )}
          {done && <p className="text-[12px] text-ink-3">{done}</p>}
          {error && (
            <p className="text-[12px] text-red-500 select-text">{error}</p>
          )}
        </div>
      )}

      {voice.lines.length > 0 && (
        <section className="flex flex-col">
          <h3 className="text-[13px] text-ink-3 mb-1">
            {t("speakers.said", {
              name: voice.is_user ? t("speakers.you") : title,
            })}
          </h3>
          {voice.lines.map((l) => {
            const key = `${l.meeting_id}|${l.start_s}`;
            const on = clip.playing === key;
            return (
              <div
                key={key}
                className="grid grid-cols-[28px_1fr_auto] items-center gap-3 py-2.5 border-b border-line"
              >
                <button
                  type="button"
                  disabled={!l.audio}
                  title={l.audio ? t("speakers.play") : t("speakers.noAudio")}
                  aria-label={t("speakers.play")}
                  onClick={() =>
                    void clip.play(key, l.meeting_id, l.speaker, l.start_s)
                  }
                  className="size-7 grid place-items-center rounded-full border border-ink-3 text-text cursor-pointer disabled:opacity-40 disabled:cursor-default hover:bg-wash"
                >
                  {on ? (
                    <Pause width={12} height={12} />
                  ) : (
                    <Play width={12} height={12} className="ms-0.5" />
                  )}
                </button>
                <span className="font-mono text-[13px] text-text min-w-0">
                  {l.text}
                </span>
                <span className="text-[12px] text-ink-3 whitespace-nowrap">
                  {[l.meeting_title, shortDate(l.started_at, locale)]
                    .filter(Boolean)
                    .join(" · ")}
                </span>
              </div>
            );
          })}
          {clip.error && (
            <p className="mt-2 text-[12px] text-red-500">{clip.error}</p>
          )}
        </section>
      )}
    </div>
  );
};
