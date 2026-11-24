import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { commands } from "@/bindings";

/** The Mac's region and clock setting, asked once per run, as soon as the
 * meeting pages load, so the first list already shows it. */
let known: string | null | undefined;
const asked: Promise<string | null> = commands
  .dateFormatLocale()
  .then((l) => {
    try {
      // Only a locale Intl understands.
      if (l) new Intl.DateTimeFormat(l);
      known = l;
    } catch {
      known = null;
    }
    return known;
  })
  .catch(() => (known = null));

/** The locale for dates and times: the Mac's region ("24.11., 15:20" for
 * en_DE, "11/24, 3:20 PM" for en_US), not the app language (Robin's Mac:
 * English app, German region, 2026-11-24). Without one, the app language. */
export function useFormatLocale(): string {
  const { i18n } = useTranslation();
  const [locale, setLocale] = useState(known);
  useEffect(() => {
    if (known === undefined) void asked.then(setLocale);
  }, []);
  return locale ?? i18n.language;
}
