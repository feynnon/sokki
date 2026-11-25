import React from "react";
import { useTranslation } from "react-i18next";
import { Dialog } from "../ui";
import { MarkdownContent } from "./MarkdownContent";
import { Autocorrect } from "../settings/Autocorrect";
import type { ReleaseNote } from "./releaseNotes";

/** Settings a release note can show as a switch, with a line
 * `{{toggle:<setting>}}`, so a new option can be turned on right there. */
const TOGGLES: Record<string, React.ReactNode> = {
  autocorrect_enabled: <Autocorrect descriptionMode="inline" />,
};

type Part =
  | { kind: "text"; text: string }
  | { kind: "toggle"; setting: string };

const splitToggles = (markdown: string): Part[] => {
  const parts: Part[] = [];
  let text: string[] = [];
  for (const line of markdown.split("\n")) {
    const m = line.trim().match(/^\{\{toggle:([a-z_]+)\}\}$/);
    if (m && m[1] in TOGGLES) {
      if (text.length) parts.push({ kind: "text", text: text.join("\n") });
      parts.push({ kind: "toggle", setting: m[1] });
      text = [];
    } else if (!m) {
      text.push(line);
    }
  }
  if (text.length) parts.push({ kind: "text", text: text.join("\n") });
  return parts;
};

interface WhatsNewModalProps {
  note: ReleaseNote;
  open: boolean;
  onDismiss: () => void;
}

export const WhatsNewModal: React.FC<WhatsNewModalProps> = ({
  note,
  open,
  onDismiss,
}) => {
  const { t } = useTranslation();

  return (
    <Dialog
      open={open}
      title={t("whatsNew.title", { version: note.version })}
      closeLabel={t("common.close")}
      onOpenChange={(nextOpen) => {
        if (!nextOpen) onDismiss();
      }}
    >
      {splitToggles(note.markdown).map((part, i) =>
        part.kind === "text" ? (
          <MarkdownContent key={i} markdown={part.text} />
        ) : (
          <div key={i} className="my-3 rounded-lg border border-line">
            {TOGGLES[part.setting]}
          </div>
        ),
      )}
    </Dialog>
  );
};
