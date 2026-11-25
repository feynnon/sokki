import React, { useState } from "react";
import { useTranslation } from "react-i18next";
import { X } from "lucide-react";
import { commands, type MeetingSettings } from "@/bindings";
import { useSettings } from "@/hooks/useSettings";
import { Input } from "@/components/ui/Input";
import { Page, PageHeader } from "./parts";

/** One list of names and terms for meetings and dictation. The summary
 * prompt lists them all. Dictation swaps in only words of 6+ letters
 * (`swap_words`): shorter ones turned "Lied" into "Lead" and "mit" into
 * "Meta" in tests. Below the list: what Sokki learned from the user's
 * writing ("Keep what I write") and how the speech model hears it. */
export const SokkiVocabularyPage: React.FC = () => {
  const { t } = useTranslation();
  const { settings, refreshSettings } = useSettings();
  const [draft, setDraft] = useState("");
  const meetings = settings?.meetings as MeetingSettings | undefined;
  if (!meetings) return null;
  const words = meetings.glossary;
  const learned = settings?.vocabulary_learned ?? [];
  const heardAs = settings?.vocabulary_heard_as ?? [];

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
      {learned.length > 0 && (
        <section className="vb-card p-4 flex flex-col gap-3">
          <h2 className="text-[13px] text-ink-3">
            {t("sokki.vocabulary.learned")}
          </h2>
          <ul className="flex flex-wrap gap-2">
            {learned.map((w) => (
              <li
                key={w}
                className="inline-flex items-center gap-1 rounded-full bg-wash border border-logo-primary pl-3 pr-1.5 h-7 text-[13px]"
              >
                {w}
                <button
                  type="button"
                  aria-label={t("sokki.vocabulary.remove", { word: w })}
                  onClick={() =>
                    void commands
                      .vocabularyForgetLearned(w)
                      .then(() => refreshSettings())
                  }
                  className="grid size-5 place-items-center rounded-full text-ink-3 hover:text-text hover:bg-line cursor-pointer"
                >
                  <X width={12} height={12} />
                </button>
              </li>
            ))}
          </ul>
        </section>
      )}
      {heardAs.length > 0 && (
        <section className="vb-card p-4 flex flex-col gap-3">
          <h2 className="text-[13px] text-ink-3">
            {t("sokki.vocabulary.heardAs")}
          </h2>
          <ul className="flex flex-wrap gap-2">
            {heardAs.map((h) => (
              <li
                key={h.heard}
                className="inline-flex items-center gap-1 rounded-full bg-wash border border-line pl-3 pr-1.5 h-7 text-[13px]"
              >
                <span className="font-mono text-[12px] text-ink-3">
                  {h.heard}
                </span>
                <span className="text-ink-3">→</span>
                {h.word}
                <button
                  type="button"
                  aria-label={t("sokki.vocabulary.remove", { word: h.heard })}
                  onClick={() =>
                    void commands
                      .vocabularyForgetHeardAs(h.heard)
                      .then(() => refreshSettings())
                  }
                  className="grid size-5 place-items-center rounded-full text-ink-3 hover:text-text hover:bg-line cursor-pointer"
                >
                  <X width={12} height={12} />
                </button>
              </li>
            ))}
          </ul>
          <p className="text-[12px] text-ink-3">
            {t("sokki.vocabulary.heardAsNote")}
          </p>
        </section>
      )}
    </Page>
  );
};
