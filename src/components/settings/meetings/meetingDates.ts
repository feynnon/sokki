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

export function time(at: Date, locale: string): string {
  return new Intl.DateTimeFormat(locale, {
    hour: "2-digit",
    minute: "2-digit",
  }).format(at);
}

/** "Mo., 09:00" this week, "12. Nov., 09:00" before. */
export function weekdayTime(at: Date, locale: string): string {
  return `${new Intl.DateTimeFormat(locale, { weekday: "short" }).format(at)}, ${time(at, locale)}`;
}

function dateTime(at: Date, locale: string): string {
  return new Intl.DateTimeFormat(locale, {
    day: "numeric",
    month: "short",
    hour: "2-digit",
    minute: "2-digit",
  }).format(at);
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

/** When, as the list shows it: the time in today's and yesterday's group,
 * weekday and time this week, the date before. */
export function listWhen(at: Date, now: Date, locale: string): string {
  const g = dayGroup(at, now);
  if (g === "today" || g === "yesterday") return time(at, locale);
  if (g === "thisWeek") return weekdayTime(at, locale);
  return dateTime(at, locale);
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
