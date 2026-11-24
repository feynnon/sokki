import assert from "node:assert/strict";
import {
  dayGroup,
  isDateTitle,
  listDate,
  listWhen,
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

// The date first in every group, then the time.
assert.deepEqual(listWhen(new Date(2025, 10, 26, 10, 0), now, "de"), [
  "26.11.",
  "10:00",
]);
assert.deepEqual(listWhen(new Date(2025, 10, 24, 9, 0), now, "de"), [
  "24.11.",
  "09:00",
]);
// The Mac's region decides, not the app language: Robin's Mac is en_DE.
const nine = new Date(2025, 10, 26, 9, 5);
assert.deepEqual(listWhen(nine, now, "en-DE"), ["26.11.", "09:05"]);
assert.deepEqual(listWhen(nine, now, "de-DE"), ["26.11.", "09:05"]);
// A US Mac: its own order and a 12-hour clock without the leading zero
// (ICU versions differ in the space before "AM").
assert.equal(listDate(nine, now, "en-US"), "11/26");
assert.match(time(nine, "en-US"), /^9:05\sAM$/);
// The 24-hour switch in System Settings ("en-US-u-hc-h23").
assert.equal(time(nine, "en-US-u-hc-h23"), "09:05");
assert.equal(listDate(new Date(2024, 10, 26), now, "en-US"), "11/26/24");
assert.equal(listDate(new Date(2024, 10, 26), now, "de"), "26.11.24");

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
