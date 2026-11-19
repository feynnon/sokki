import React, { useState } from "react";
import { useTranslation } from "react-i18next";
import { X } from "lucide-react";
import { commands, type MeetingSettings } from "@/bindings";
import { useSettings } from "@/hooks/useSettings";
import { Input } from "@/components/ui/Input";
import { Page, PageHeader } from "./parts";

/** One list of names and terms. It feeds the meeting correction, which only
 * swaps a word after the language model and a sound check agree. Dictation's
 * direct word swap is left out on purpose: tested with these words it turned
 * "Led" and "Lied" into "Lead" and "Mach" into "Noxi". */
export const SokkiVocabularyPage: React.FC = () => {
  const { t } = useTranslation();
  const { settings, refreshSettings } = useSettings();
  const [draft, setDraft] = useState("");
  const meetings = settings?.meetings as MeetingSettings | undefined;
  if (!meetings) return null;
  const words = meetings.glossary;

  const save = async (next: string[]) => {
    const unique = Array.from(
      new Set(next.map((w) => w.trim()).filter(Boolean)),
    );
    unique.sort((a, b) =>
      a.localeCompare(b, undefined, { sensitivity: "base" }),
    );
    await commands.setMeetingSettings({ ...meetings, glossary: unique });
    await refreshSettings();
  };
  const add = () => {
    const parts = draft
      .split(",")
      .map((w) => w.trim())
      .filter(Boolean);
    if (parts.length === 0) return;
    setDraft("");
    void save([...words, ...parts]);
  };

  return (
    <Page>
      <PageHeader
        title={t("sokki.vocabulary.title")}
        description={t("sokki.vocabulary.description")}
      />
      <div className="vb-card p-4 flex flex-col gap-4">
        <div className="flex gap-2">
          <Input
            value={draft}
            placeholder={t("sokki.vocabulary.placeholder")}
            onChange={(e) => setDraft(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") add();
            }}
            className="flex-1"
            aria-label={t("sokki.vocabulary.add")}
          />
        </div>
        {words.length === 0 ? (
          <p className="text-[13px] text-ink-3">
            {t("sokki.vocabulary.empty")}
          </p>
        ) : (
          <ul
            className="flex flex-wrap gap-2"
            aria-label={t("sokki.vocabulary.title")}
          >
            {words.map((w) => (
              <li
                key={w}
                className="inline-flex items-center gap-1 rounded-full bg-wash border border-line pl-3 pr-1.5 h-7 text-[13px]"
              >
                {w}
                <button
                  type="button"
                  aria-label={t("sokki.vocabulary.remove", { word: w })}
                  onClick={() => void save(words.filter((x) => x !== w))}
                  className="grid size-5 place-items-center rounded-full text-ink-3 hover:text-text hover:bg-line cursor-pointer"
                >
                  <X width={12} height={12} />
                </button>
              </li>
            ))}
          </ul>
        )}
      </div>
    </Page>
  );
};
