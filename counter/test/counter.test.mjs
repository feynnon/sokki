// Unit tests for the counter: node --test counter/test/counter.test.mjs
// A stand-in D1 answers the few queries these paths make.
import { test } from "node:test";
import assert from "node:assert/strict";
import worker, { shouldRelay, posthogEvents, storedBytes } from "../src/index.js";

function fakeDB(answers = {}) {
  const calls = [];
  const stmt = (sql) => {
    const s = {
      sql,
      args: [],
      bind: (...a) => ((s.args = a), s),
      first: async () => {
        calls.push(sql);
        for (const [k, v] of Object.entries(answers)) if (sql.includes(k)) return v;
        return null;
      },
      run: async () => (calls.push(sql), {}),
      all: async () => (calls.push(sql), { results: [] }),
    };
    return s;
  };
  return { calls, prepare: stmt, batch: async (list) => list.forEach((s) => calls.push(s.sql)) };
}

const ID = "0123456789abcdef0123456789abcdef";
const oldReport = {
  id: ID,
  app: { version: "0.2.12", model: "parakeet" },
  activity: { dictations: 3, meetings: [] },
  problems: ["[sokki][WARN] something"],
};
const newReport = { ...oldReport, usage: { events: [], counts: { dictation_typed: 3 } } };

async function postUsage(report) {
  // knownId: an id from before the proof of work, so no pow is needed here.
  const env = {
    DB: fakeDB({ "FROM installs WHERE id": { 1: 1 } }),
    POSTHOG_KEY: "phc_test",
    POSTHOG_HOST: "https://posthog.invalid",
  };
  const sent = [];
  const realFetch = globalThis.fetch;
  globalThis.fetch = async (url, init) => (sent.push({ url, body: JSON.parse(init.body) }), new Response("{}"));
  const waits = [];
  try {
    const res = await worker.fetch(
      new Request("https://counter/usage", { method: "POST", body: JSON.stringify(report) }),
      env,
      { waitUntil: (p) => waits.push(p) },
    );
    await Promise.all(waits);
    return { status: res.status, sent, calls: env.DB.calls };
  } finally {
    globalThis.fetch = realFetch;
  }
}

test("an old app's report stays in D1 and never reaches PostHog", async () => {
  const r = await postUsage(oldReport);
  assert.equal(r.status, 200);
  assert.ok(r.calls.some((s) => s.includes("INSERT INTO usage")), "stored in D1");
  assert.equal(r.sent.length, 0);
});

test("a report with the usage part goes to PostHog, without anything unknown", async () => {
  const r = await postUsage({
    ...newReport,
    usage: { ...newReport.usage, events: [{ event: "evil", at: 1795000000, props: { text: "secret" } }] },
  });
  assert.equal(r.status, 200);
  assert.equal(r.sent.length, 1);
  const names = r.sent[0].body.batch.map((e) => e.event);
  assert.deepEqual(names, ["app_daily", "app_problem"]);
  assert.equal(r.sent[0].body.batch[0].properties.$lib, "sokki-app");
  assert.ok(!JSON.stringify(r.sent[0].body).includes("secret"));
});

test("shouldRelay wants an object under usage", () => {
  assert.equal(shouldRelay(oldReport), false);
  assert.equal(shouldRelay({ usage: [] }), false);
  assert.equal(shouldRelay({ usage: null }), false);
  assert.equal(shouldRelay(newReport), true);
});

test("posthogEvents drops events outside the list", () => {
  const out = posthogEvents(ID, { ...newReport, usage: { events: [{ event: "crash", at: 1795000000, props: { place: "src/x.rs:1" } }, { event: "nope", at: 1795000000 }] } }, 1795000100);
  assert.deepEqual(out.map((e) => e.event), ["app_daily", "crash", "app_problem"]);
});

test("storedBytes sums once, then reads the running total", async () => {
  const empty = fakeDB({ "SUM(bytes)": { b: 1234 } });
  assert.equal(await storedBytes({ DB: empty }), 1234);
  assert.ok(empty.calls.some((s) => s.includes("SUM(bytes)")));
  const cached = fakeDB({ "key = 'audio_bytes'": { value: "5678" } });
  assert.equal(await storedBytes({ DB: cached }), 5678);
  assert.ok(!cached.calls.some((s) => s.includes("SUM(bytes)")));
});
