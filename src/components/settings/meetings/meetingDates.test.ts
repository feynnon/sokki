import assert from "node:assert/strict";
import { dayGroup, listWhen, minutes, weekStart } from "./meetingDates";

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

assert.equal(listWhen(new Date(2025, 10, 26, 10, 0), now, "de"), "10:00");
// ICU versions differ: "Mo., 09:00" or "Mo, 09:00".
assert.match(
  listWhen(new Date(2025, 10, 24, 9, 0), now, "de"),
  /^Mo\.?, 09:00$/,
);

console.log("meetingDates: ok");
