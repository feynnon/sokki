/** How the reader names a meeting's day: today, yesterday, this week
 * (since Monday) or earlier. Local time. */
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

/** The day's date as a list header shows it, in the region's own format:
 * "25. Nov." (Germany), "Nov 25" (US), "25 Nov" (UK), with the year when
 * it isn't this year's ("23. Nov. 2025"). */
export function headerDate(at: Date, now: Date, locale: string): string {
  return new Intl.DateTimeFormat(locale, {
    day: "numeric",
    month: "short",
    year: at.getFullYear() === now.getFullYear() ? undefined : "numeric",
  }).format(at);
}

/** A day's header in the list, always with its date (Robin, 2026-11-25):
 * "Heute, 25. Nov.", "Gestern, 24. Nov.", then "23. Nov." (`named` puts
 * the group's word for today and yesterday around the date). */
export function dayHeader(
  at: Date,
  now: Date,
  locale: string,
  named: (g: "today" | "yesterday", date: string) => string,
): string {
  const g = dayGroup(at, now);
  const date = headerDate(at, now, locale);
  return g === "today" || g === "yesterday" ? named(g, date) : date;
}

/** The meetings by day, one group per local day, newest day first; the
 * order within a day is kept. `day` is that day's midnight. */
export function byDay<T>(
  items: T[],
  startedAt: (x: T) => Date,
): { day: number; items: T[] }[] {
  const days = new Map<number, T[]>();
  for (const x of items) {
    const d = midnight(startedAt(x));
    const list = days.get(d);
    if (list) list.push(x);
    else days.set(d, [x]);
  }
  return [...days.entries()]
    .sort((a, b) => b[0] - a[0])
    .map(([day, items]) => ({ day, items }));
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
