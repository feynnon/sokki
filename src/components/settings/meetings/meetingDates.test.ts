import assert from "node:assert/strict";
import {
  byDay,
  dayGroup,
  dayHeader,
  headerDate,
  isDateTitle,
  meetingPlace,
  minutes,
  otherSpeakers,
  time,
  weekStart,
} from "./meetingDates";

// Wednesday 26 Nov 2025, 15:00 local.
const now = new Date(2025, 10, 26, 15, 0);

assert.equal(weekStart(now), new Date(2025, 10, 24).getTime());
assert.equal(
  weekStart(new Date(2025, 10, 30, 20)),
  new Date(2025, 10, 24).getTime(),
);
assert.equal(
  weekStart(new Date(2025, 10, 24, 0, 1)),
  new Date(2025, 10, 24).getTime(),
);

assert.equal(dayGroup(new Date(2025, 10, 26, 0, 5), now), "today");
assert.equal(dayGroup(new Date(2025, 10, 25, 23, 59), now), "yesterday");
assert.equal(dayGroup(new Date(2025, 10, 24, 9), now), "thisWeek");
assert.equal(dayGroup(new Date(2025, 10, 23, 22), now), "earlier");
// On a Monday, yesterday (Sunday) is still "Yesterday", not last week.
assert.equal(
  dayGroup(new Date(2025, 10, 23, 22), new Date(2025, 10, 24, 8)),
  "yesterday",
);

assert.equal(minutes(20, "de"), "1 Min.");
assert.equal(minutes(34 * 60, "de"), "34 Min.");
assert.equal(minutes(34 * 60, "en"), "34 min");

// One header per day, always with the date (Robin, 2026-11-25).
const named = (g: "today" | "yesterday", date: string) =>
  `${g === "today" ? "Heute" : "Gestern"}, ${date}`;
assert.equal(
  dayHeader(new Date(2025, 10, 26, 10), now, "de", named),
  "Heute, 26. Nov.",
);
assert.equal(
  dayHeader(new Date(2025, 10, 25, 9), now, "de", named),
  "Gestern, 25. Nov.",
);
assert.equal(
  dayHeader(new Date(2025, 10, 23, 9), now, "de", named),
  "23. Nov.",
);
assert.equal(dayHeader(new Date(2025, 10, 2, 9), now, "de", named), "2. Nov.");
// The Mac's region decides, not the app language: Robin's Mac is en_DE.
assert.equal(headerDate(new Date(2025, 10, 23), now, "en-DE"), "23. Nov");
assert.equal(headerDate(new Date(2025, 10, 23), now, "en-US"), "Nov 23");
assert.equal(headerDate(new Date(2025, 10, 23), now, "en-GB"), "23 Nov");
// Older than this year: with the year.
assert.equal(headerDate(new Date(2024, 10, 23), now, "de"), "23. Nov. 2024");
assert.equal(headerDate(new Date(2024, 10, 23), now, "en-US"), "Nov 23, 2024");

// A day per group, newest first; a day's own order is kept (a recording
// meeting first).
const at = (d: number, h: number) => ({ at: new Date(2025, 10, d, h) });
const days = byDay(
  [at(26, 9), at(26, 15), at(25, 23), at(23, 1), at(25, 8)],
  (x) => x.at,
);
assert.deepEqual(
  days.map((g) => [
    new Date(g.day).getDate(),
    g.items.map((x) => x.at.getHours()),
  ]),
  [
    [26, [9, 15]],
    [25, [23, 8]],
    [23, [1]],
  ],
);
// A meeting just after midnight belongs to its own day.
assert.equal(byDay([at(26, 0), at(25, 23)], (x) => x.at).length, 2);

const nine = new Date(2025, 10, 26, 9, 5);
// A US Mac: a 12-hour clock without the leading zero (ICU versions differ
// in the space before "AM").
assert.match(time(nine, "en-US"), /^9:05\sAM$/);
// The 24-hour switch in System Settings ("en-US-u-hc-h23").
assert.equal(time(nine, "en-US-u-hc-h23"), "09:05");
assert.equal(time(nine, "en-DE"), "09:05");

const words = { phone: "Handy", online: "Online", inPerson: "Vor Ort" };
assert.equal(
  meetingPlace({ source_app: "Zoom", online: false }, words),
  "Zoom",
);
assert.equal(meetingPlace({ source_app: "Phone" }, words), "Handy");
assert.equal(meetingPlace({ source_app: null, online: true }, words), "Online");
assert.equal(
  meetingPlace({ source_app: null, online: false }, words),
  "Vor Ort",
);
assert.equal(meetingPlace({ source_app: null, online: null }, words), null);

assert.deepEqual(
  otherSpeakers(["Speaker 1", "Robin", "Lea", "Speaker 4"], "robin"),
  { named: ["Lea"], unnamed: 2 },
);

assert.ok(isDateTitle("Meeting 2026-11-24 14:42"));
assert.ok(!isDateTitle("Meeting mit Lea"));

console.log("meetingDates: ok");
