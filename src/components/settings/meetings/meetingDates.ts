/** Where a meeting sits in the list: today, yesterday, this week (since
 * Monday) or earlier. Local time. */
export type DayGroup = "today" | "yesterday" | "thisWeek" | "earlier";

function midnight(d: Date): number {
  return new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();
}

/** Monday 00:00 of `now`'s week, local time. */
export function weekStart(now: Date): number {
  const back = (now.getDay() + 6) % 7;
  return new Date(
    now.getFullYear(),
    now.getMonth(),
    now.getDate() - back,
  ).getTime();
}

export function dayGroup(at: Date, now: Date): DayGroup {
  const today = midnight(now);
  const t = at.getTime();
  if (t >= today) return "today";
  const yesterday = new Date(
    now.getFullYear(),
    now.getMonth(),
    now.getDate() - 1,
  ).getTime();
  if (t >= yesterday) return "yesterday";
  if (t >= weekStart(now)) return "thisWeek";
  return "earlier";
}

/** "09:05" / "15:20" with a 24-hour clock, "9:05 AM" with a 12-hour one
 * (not "09:05 AM"). */
export function time(at: Date, locale: string): string {
  const cycle = new Intl.DateTimeFormat(locale, {
    hour: "numeric",
  }).resolvedOptions().hourCycle;
  const twelve = cycle === "h11" || cycle === "h12";
  return new Intl.DateTimeFormat(locale, {
    hour: twelve ? "numeric" : "2-digit",
    minute: "2-digit",
  }).format(at);
}

/** "Mo., 09:00" this week, "12. Nov., 09:00" before. */
export function weekdayTime(at: Date, locale: string): string {
  return `${new Intl.DateTimeFormat(locale, { weekday: "short" }).format(at)}, ${time(at, locale)}`;
}

/** "34 Min." / "34 min"; under a minute counts as one. */
export function minutes(seconds: number, locale: string): string {
  const m = Math.max(1, Math.round(seconds / 60));
  return new Intl.NumberFormat(locale, {
    style: "unit",
    unit: "minute",
    unitDisplay: "short",
  }).format(m);
}

/** The day as the list shows it, in the region's own order: "24.11."
 * (Germany), "11/24" (US), "24/11" (UK), with the year when it isn't this
 * year's ("24.11.25"). */
export function listDate(at: Date, now: Date, locale: string): string {
  return new Intl.DateTimeFormat(locale, {
    day: "numeric",
    month: "2-digit",
    year: at.getFullYear() === now.getFullYear() ? undefined : "2-digit",
  }).format(at);
}

/** When, as the list shows it: the date first, then the time ("24.11. ·
 * 10:00"), in every group (Robin, 2026-11-24). */
export function listWhen(at: Date, now: Date, locale: string): string[] {
  return [listDate(at, now, locale), time(at, locale)];
}

/** Where a meeting took place, as the list and the reader show it: the
 * call app or service, else "Online" or "In person" once processing knows
 * whether someone spoke on the call side. `null` while that is unknown. */
export function meetingPlace(
  r: { source_app: string | null; online?: boolean | null },
  words: { phone: string; online: string; inPerson: string },
): string | null {
  if (r.source_app === "Phone") return words.phone;
  if (r.source_app) return r.source_app;
  if (r.online === true) return words.online;
  if (r.online === false) return words.inPerson;
  return null;
}

/** The title a note gets without a summary ("Meeting 2026-11-24 10:00",
 * `meeting_core::note::note_title`): it only repeats the date. */
export function isDateTitle(title: string): boolean {
  return /^Meeting \d{4}-\d{2}-\d{2} \d{2}:\d{2}$/.test(title);
}

/** Who spoke, for the list: named people other than the user, then how
 * many unnamed ("Speaker 3") there were. */
export function otherSpeakers(
  speakers: string[],
  userName: string,
): { named: string[]; unnamed: number } {
  const me = userName.trim().toLowerCase();
  const others = speakers.filter((s) => s.trim().toLowerCase() !== me);
  const generic = (s: string) => /^Speaker \d+$/.test(s);
  return {
    named: others.filter((s) => !generic(s)),
    unnamed: others.filter(generic).length,
  };
}

/** When, as the reader shows it: "Heute, 10:00" (`dayName` gives the
 * group's own word for today and yesterday). */
export function readerWhen(
  at: Date,
  now: Date,
  locale: string,
  dayName: (g: "today" | "yesterday") => string,
): string {
  const g = dayGroup(at, now);
  if (g === "today" || g === "yesterday")
    return `${dayName(g)}, ${time(at, locale)}`;
  if (g === "thisWeek") return weekdayTime(at, locale);
  return new Intl.DateTimeFormat(locale, {
    day: "numeric",
    month: "long",
    year: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  }).format(at);
}
