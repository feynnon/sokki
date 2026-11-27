import assert from "node:assert/strict";
import { atEnd, END_SLACK } from "./followEnd";

// A 1000 px list in a 300 px box: the end is at scrollTop 700.
assert.equal(atEnd(1000, 700, 300), true);
assert.equal(atEnd(1000, 699.5, 300), true, "sub-pixel positions count");
assert.equal(atEnd(1000, 700 - END_SLACK, 300), true);
assert.equal(atEnd(1000, 700 - END_SLACK - 1, 300), false);
assert.equal(atEnd(1000, 0, 300), false, "the top of a long list");
// Shorter than the box: always at the end.
assert.equal(atEnd(200, 0, 300), true);
assert.equal(atEnd(0, 0, 0), true, "empty");

console.log("followEnd: ok");
