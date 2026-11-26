import React, { useEffect, useRef, useState } from "react";
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
  // Where the card hangs: under the clicked chip, kept inside the column.
  const [at, setAt] = useState({ left: 0, arrow: 16 });
  const box = useRef<HTMLDivElement>(null);
  // Names shown at once, before the note catches up.
  const [given, setGiven] = useState<Record<string, string>>({});
  const key = `${record.id}|${record.speakers.join(",")}|${record.status.kind}`;
  useEffect(() => {
    setGiven({});
    if (record.status.kind !== "done") return;
    let live = true;
    // A list that can't be read is no proposals: the chips still show.
    void commands
      .meetingSpeakerProposals(record.id)
      .then((r) => {
        if (live && r?.status === "ok")
          setProposals(Array.isArray(r.data) ? r.data : []);
      })
      .catch(() => {
        if (live) setProposals([]);
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
    <div ref={box} className="flex flex-col gap-2 relative">
      <div className="flex flex-wrap gap-1.5">
        {record.speakers.map((sp) => {
          const shown = given[sp] ?? sp;
          const p = proposals.find((x) => x.speaker === sp);
          const suggest = !given[sp] && p && (p.candidates ?? []).length > 0;
          return (
            <button
              key={sp}
              type="button"
              aria-expanded={editing === sp}
              title={t("meetings.renameSpeaker")}
              onClick={(e) => {
                const chip = e.currentTarget;
                const room = (box.current?.clientWidth ?? PANEL_W) - PANEL_W;
                const left = Math.max(0, Math.min(chip.offsetLeft, room));
                const middle = chip.offsetLeft + chip.offsetWidth / 2 - left;
                setAt({ left, arrow: Math.max(12, middle - 5) });
                setEditing(editing === sp ? null : sp);
              }}
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
          onClose={() => setEditing(null)}
          at={at}
        />
      )}
      {!unnamed && !editing && <VoiceOffer key={key} record={record} />}
    </div>
  );
};

const PANEL_W = 300;

/** A small card hanging from the clicked chip, over the note: who this
 * voice is (proposals, or a name and email addresses), "Benennen" at the
 * bottom right. A click outside or Esc closes it (Robin chose 6a,
 * 26 Nov 2026). */
const NamePanel: React.FC<{
  record: MeetingRecord;
  speaker: string;
  candidates: NameCandidate[];
  onNamed: (name: string) => void;
  onDone: () => void;
  onClose: () => void;
  at: { left: number; arrow: number };
}> = ({ record, speaker, candidates, onNamed, onDone, onClose, at }) => {
  const { t } = useTranslation();
  const clip = useVoiceClip();
  const [other, setOther] = useState(candidates.length === 0);
  const [name, setName] = useState(PLACEHOLDER.test(speaker) ? "" : speaker);
  const [emails, setEmails] = useState("");
  const [error, setError] = useState<string | null>(null);
  const audio = !record.audio_deleted;
  const card = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const down = (e: MouseEvent) => {
      const target = e.target as Node;
      // A chip toggles the card itself.
      const chip = (target as Element).closest?.("[aria-expanded]");
      if (card.current && !card.current.contains(target) && !chip) onClose();
    };
    const key = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    document.addEventListener("mousedown", down);
    document.addEventListener("keydown", key);
    return () => {
      document.removeEventListener("mousedown", down);
      document.removeEventListener("keydown", key);
    };
  }, [onClose]);

  const give = async (to: string, mails: string[]) => {
    const n = to.trim();
    if (!n || n === speaker) return;
    onNamed(n);
    const r = await commands.nameVoice(
      [{ meeting_id: record.id, speaker }],
      n,
      mails,
      // Keeping the voice is asked once, for everyone, at the end
      // (VoiceOffer).
      false,
    );
    if (r.status === "ok") onDone();
    else setError(r.error);
  };

  const submit = () => void give(name, parseEmails(emails));
  const onEnter = (e: React.KeyboardEvent) => {
    if (e.key === "Enter") submit();
  };

  return (
    <div
      ref={card}
      role="dialog"
      aria-label={t("speakers.is", { speaker })}
      style={{ left: at.left, width: PANEL_W }}
      className="vb-card p-3 flex flex-col gap-2.5 absolute z-20 top-[30px] border border-line-strong shadow-[0_16px_40px_rgba(0,0,0,0.45)]"
    >
      <span
        aria-hidden="true"
        style={{ left: at.arrow }}
        className="absolute -top-[6px] size-[10px] rotate-45 bg-[var(--vb-card)] border-l border-t border-line-strong"
      />
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
        <>
          <Input
            autoFocus
            value={name}
            placeholder={t("speakers.name")}
            aria-label={t("speakers.name")}
            onChange={(e) => setName(e.target.value)}
            onKeyDown={onEnter}
            className="w-full"
          />
          <Input
            value={emails}
            placeholder={t("speakers.emails")}
            aria-label={t("speakers.emails")}
            onChange={(e) => setEmails(e.target.value)}
            onKeyDown={onEnter}
            className="w-full"
          />
          <div className="flex justify-end">
            <Button size="sm" onClick={submit} disabled={!name.trim()}>
              {t("speakers.nameButton")}
            </Button>
          </div>
        </>
      )}
      {(error || clip.error) && (
        <p className="text-[12px] text-red-500 select-text">
          {error ?? clip.error}
        </p>
      )}
    </div>
  );
};

/** Once every speaker has a name: one question whether Sokki may keep the
 * new voices, with a box per person that starts empty, since consent is
 * theirs (GDPR Art. 7, 9). Answered once per meeting. */
const VoiceOffer: React.FC<{ record: MeetingRecord }> = ({ record }) => {
  const { t } = useTranslation();
  const [names, setNames] = useState<string[]>([]);
  const [ticked, setTicked] = useState<string[]>([]);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    let live = true;
    void commands
      .meetingVoiceOffer(record.id)
      .then((r) => {
        if (live && r?.status === "ok")
          setNames(Array.isArray(r.data) ? r.data : []);
      })
      .catch(() => {});
    return () => {
      live = false;
    };
  }, [record.id]);
  if (names.length === 0) return null;

  const answer = async (keep: string[]) => {
    // The card goes at once; the voices are saved in the background.
    setNames([]);
    for (const n of keep) {
      const r = await commands.nameVoice(
        [{ meeting_id: record.id, speaker: n }],
        n,
        [],
        true,
      );
      if (r.status !== "ok") {
        setError(r.error);
        return;
      }
    }
    await commands.voiceOfferDone(record.id);
  };

  return (
    <div className="vb-card p-3 flex flex-col gap-2 max-w-[520px]">
      <div className="text-[13px] font-medium">{t("speakers.offerTitle")}</div>
      <div className="text-[12px] text-ink-3">{t("speakers.offerHint")}</div>
      {names.map((n) => (
        <label key={n} className="flex items-center gap-2 text-[13px]">
          <input
            type="checkbox"
            checked={ticked.includes(n)}
            onChange={(e) =>
              setTicked((x) =>
                e.target.checked ? [...x, n] : x.filter((y) => y !== n),
              )
            }
          />
          {n}
        </label>
      ))}
      <div className="flex gap-2 pt-1">
        <Button
          size="sm"
          disabled={ticked.length === 0}
          onClick={() => void answer(ticked)}
        >
          {t("speakers.offerKeep")}
        </Button>
        <Button size="sm" variant="secondary" onClick={() => void answer([])}>
          {t("speakers.offerLater")}
        </Button>
      </div>
      {error && <p className="text-[12px] text-red-500 select-text">{error}</p>}
    </div>
  );
};
