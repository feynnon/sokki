import assert from "node:assert/strict";
import { DRAG_PX, pressOutcome } from "./press";

// A press that barely moves is a click: it opens the meeting.
assert.equal(pressOutcome({ x: 10, y: 10 }, { x: 10, y: 10 }), "click");
assert.equal(pressOutcome({ x: 10, y: 10 }, { x: 12, y: 12 }), "click");
// From 4 px on it is a drag: it moves the pill and opens nothing.
assert.equal(pressOutcome({ x: 0, y: 0 }, { x: DRAG_PX, y: 0 }), "drag");
assert.equal(pressOutcome({ x: 0, y: 0 }, { x: 0, y: -10 }), "drag");

console.log("press: ok");
