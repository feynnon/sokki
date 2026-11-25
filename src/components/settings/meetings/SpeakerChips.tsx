import React, { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Pause, Play } from "lucide-react";
import {
  commands,
  type MeetingRecord,
  type NameCandidate,
  type SpeakerProposal,
} from "@/bindings";
import { Button } from "@/components/ui/Button";
import { Input } from "@/components/ui/Input";
import { parseEmails, useVoiceClip } from "../speakers/voiceClip";

const PLACEHOLDER = /^(Speaker|Sprecher|Sprecherin) \d+$/;

/** "Lena?", "Lena oder Jonas?", "Lena, Jonas +1?" */
function guess(c: NameCandidate[], or: string): string {
  const first = c.map((x) => x.name.split(" ")[0]);
  if (first.length === 1) return `${first[0]}?`;
  if (first.length === 2) return `${first[0]} ${or} ${first[1]}?`;
  return `${first[0]}, ${first[1]} +${first.length - 2}?`;
}

/** The speakers of a meeting as chips. A click names one; Sokki proposes
 * who it may be (same voice elsewhere, who was invited) and names the last
 * one by itself once only one name is left (board card speaker-names). */
export const SpeakerChips: React.FC<{
  record: MeetingRecord;
  onDone: () => void;
}> = ({ record, onDone }) => {
  const { t } = useTranslation();
  const [proposals, setProposals] = useState<SpeakerProposal[]>([]);
  const [editing, setEditing] = useState<string | null>(null);
  // Names shown at once, before the note catches up.
  const [given, setGiven] = useState<Record<string, string>>({});
  const key = `${record.id}|${record.speakers.join(",")}|${record.status.kind}`;
  useEffect(() => {
    setGiven({});
    if (record.status.kind !== "done") return;
    let live = true;
    void commands.meetingSpeakerProposals(record.id).then((r) => {
      if (live && r.status === "ok") setProposals(r.data);
    });
    return () => {
      live = false;
    };
  }, [key]);

  if (record.speakers.length === 0 || record.status.kind !== "done")
    return null;
  const or = t("speakers.or");
  const unnamed = record.speakers.some((s) => PLACEHOLDER.test(s));

  return (
    <div className="flex flex-col gap-2">
      <div className="flex flex-wrap gap-1.5">
        {record.speakers.map((sp) => {
          const shown = given[sp] ?? sp;
          const p = proposals.find((x) => x.speaker === sp);
          const suggest = !given[sp] && p && p.candidates.length > 0;
          return (
            <button
              key={sp}
              type="button"
              aria-expanded={editing === sp}
              title={t("meetings.renameSpeaker")}
              onClick={() => setEditing(editing === sp ? null : sp)}
              className={`h-[22px] px-2 inline-flex items-center rounded-md text-[11px] font-medium cursor-pointer focus:outline-none focus-visible:ring-1 focus-visible:ring-logo-primary ${suggest ? "text-text bg-wash border border-logo-primary" : "bg-wash text-ink-2 hover:text-text"}`}
            >
              {suggest ? guess(p.candidates, or) : shown}
            </button>
          );
        })}
      </div>
      {unnamed && !editing && (
        <p className="text-[12px] text-ink-3">{t("speakers.hint")}</p>
      )}
      {editing && (
        <NamePanel
          key={editing}
          record={record}
          speaker={editing}
          candidates={
            proposals.find((x) => x.speaker === editing)?.candidates ?? []
          }
          onNamed={(name) => {
            setGiven((g) => ({ ...g, [editing]: name }));
            setEditing(null);
          }}
          onDone={onDone}
        />
      )}
    </div>
  );
};

const NamePanel: React.FC<{
  record: MeetingRecord;
  speaker: string;
  candidates: NameCandidate[];
  onNamed: (name: string) => void;
  onDone: () => void;
}> = ({ record, speaker, candidates, onNamed, onDone }) => {
  const { t } = useTranslation();
  const clip = useVoiceClip();
  const [other, setOther] = useState(candidates.length === 0);
  const [name, setName] = useState(PLACEHOLDER.test(speaker) ? "" : speaker);
  const [emails, setEmails] = useState("");
  // Consent is the person's, so the box starts empty (GDPR Art. 7).
  const [consent, setConsent] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const audio = !record.audio_deleted;
  const first = name.trim().split(" ")[0];

  const give = async (to: string, mails: string[]) => {
    const n = to.trim();
    if (!n || n === speaker) return;
    onNamed(n);
    const r = await commands.nameVoice(
      [{ meeting_id: record.id, speaker }],
      n,
      mails,
      consent,
    );
    if (r.status === "ok") onDone();
    else setError(r.error);
  };

  return (
    <div className="vb-card p-3 flex flex-col gap-2.5 max-w-[520px]">
      <div className="flex items-center gap-2">
        {audio && (
          <button
            type="button"
            aria-label={t("speakers.play")}
            title={t("speakers.play")}
            onClick={() => void clip.play("chip", record.id, speaker, null)}
            className="size-7 grid place-items-center rounded-full text-white bg-background-ui cursor-pointer shrink-0"
          >
            {clip.playing ? (
              <Pause width={12} height={12} />
            ) : (
              <Play width={12} height={12} className="ms-0.5" />
            )}
          </button>
        )}
        <span className="text-[13px] text-ink-2">
          {t("speakers.is", { speaker })}
        </span>
      </div>
      {candidates.length > 0 && (
        <div className="flex flex-wrap gap-1.5">
          {candidates.map((c) => (
            <button
              key={c.name}
              type="button"
              onClick={() => void give(c.name, c.email ? [c.email] : [])}
              className="h-8 px-3 inline-flex items-center gap-1.5 rounded-full bg-wash border border-line hover:border-logo-primary text-[13px] cursor-pointer"
            >
              {c.name}
              <span className="text-[11px] text-ink-3">
                {c.source === "voice"
                  ? t("speakers.fromVoice")
                  : t("speakers.invited")}
              </span>
            </button>
          ))}
          {!other && (
            <button
              type="button"
              onClick={() => setOther(true)}
              className="h-8 px-3 rounded-full text-[13px] text-ink-2 hover:text-text cursor-pointer"
            >
              {t("speakers.someoneElse")}
            </button>
          )}
        </div>
      )}
      {other && (
        <div className="flex flex-col gap-2">
          <div className="flex gap-2">
            <Input
              autoFocus
              value={name}
              placeholder={t("speakers.name")}
              aria-label={t("speakers.name")}
              onChange={(e) => setName(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") void give(name, parseEmails(emails));
              }}
              className="flex-1"
            />
            <Button
              size="sm"
              onClick={() => void give(name, parseEmails(emails))}
              disabled={!name.trim()}
            >
              {t("speakers.nameButton")}
            </Button>
          </div>
          <Input
            value={emails}
            placeholder={t("speakers.emails")}
            aria-label={t("speakers.emails")}
            onChange={(e) => setEmails(e.target.value)}
          />
        </div>
      )}
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
      {(error || clip.error) && (
        <p className="text-[12px] text-red-500 select-text">
          {error ?? clip.error}
        </p>
      )}
    </div>
  );
};
