// "Delete what was shared" and uploads within the free plan's daily KV
// limits: node --test counter/test/forget.test.mjs
// D1 is a real SQLite (node:sqlite) with schema.sql, so the SQL itself is
// tested; KV is a Map that, like Cloudflare, fails beyond a daily limit.
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { DatabaseSync } from "node:sqlite";
import worker, { drainAudio, moveToR2, takeKv, storedBytes, secondsToNextDay } from "../src/index.js";

const SCHEMA = readFileSync(new URL("../schema.sql", import.meta.url), "utf8");

/** D1's API over SQLite: prepare/bind/first/all/run and batch. */
function sqliteD1() {
  const db = new DatabaseSync(":memory:");
  db.exec(SCHEMA);
  const stmt = (sql, args = []) => ({
    bind: (...a) => stmt(sql, a),
    first: async () => db.prepare(sql).get(...args) ?? null,
    all: async () => ({ results: db.prepare(sql).all(...args) }),
    run: async () => (db.prepare(sql).run(...args), { success: true }),
    _run: () => db.prepare(sql).run(...args),
  });
  return {
    db,
    prepare: (sql) => stmt(sql),
    batch: async (list) => {
      db.exec("BEGIN");
      try {
        for (const s of list) s._run();
        db.exec("COMMIT");
      } catch (e) {
        db.exec("ROLLBACK");
        throw e;
      }
      return [];
    },
  };
}

/** Workers KV with Cloudflare's free-plan behaviour: errors past `limit` a day. */
function fakeKV(limit = 1000) {
  const store = new Map();
  const used = { write: 0, delete: 0, list: 0 };
  const take = (kind) => {
    if (++used[kind] > limit) throw new Error(`KV ${kind} limit exceeded for the day (429)`);
  };
  return {
    store,
    used,
    put: async (k, v) => (take("write"), store.set(k, v)),
    delete: async (k) => (take("delete"), store.delete(k)),
    get: async (k) => store.get(k) ?? null,
    list: async ({ prefix = "", limit: n = 1000 }) => {
      take("list");
      const keys = [...store.keys()].filter((k) => k.startsWith(prefix)).slice(0, n);
      return { keys: keys.map((name) => ({ name })), list_complete: true };
    },
  };
}

const ID = "0123456789abcdef0123456789abcdef";
const OTHER = "fedcba9876543210fedcba9876543210";

function env(opts = {}) {
  return { DB: sqliteD1(), AUDIO: fakeKV(opts.kvLimit), ...opts.vars };
}

/** An install that shared `n` clips: rows in D1 and files in KV. */
function seed(e, id, n) {
  e.DB.db.prepare("INSERT INTO installs (id, clips, seconds, updated, donate) VALUES (?, ?, '{}', 0, 1)").run(id, n);
  const ins = e.DB.db.prepare(
    "INSERT INTO donations (id, clip, language, source, seconds, bytes, text, received) VALUES (?, ?, 'de', 'meeting', 10, 100, 'hallo', 0)",
  );
  for (let i = 0; i < n; i++) {
    const clip = `meeting-20261125-${String(i).padStart(9, "0")}-abcd`;
    ins.run(id, clip);
    e.AUDIO.store.set(`${id}/${clip}.flac`, new Uint8Array(100));
  }
}

/** A new UTC day: Cloudflare's and the counter's daily counts start over. */
function nextDay(e) {
  e.DB.db.exec("DELETE FROM kv_ops");
  for (const k of Object.keys(e.AUDIO.used)) e.AUDIO.used[k] = 0;
}

const count = (e, sql, ...a) => Object.values(e.DB.db.prepare(sql).get(...a))[0];
const filesOf = (e, id) => [...e.AUDIO.store.keys()].filter((k) => k.startsWith(`${id}/`)).length;

async function call(e, path, body) {
  const waits = [];
  const res = await worker.fetch(
    new Request(`https://counter${path}`, { method: "POST", body: JSON.stringify(body) }),
    e,
    { waitUntil: (p) => waits.push(p) },
  );
  await Promise.all(waits);
  return { status: res.status, body: await res.json(), headers: res.headers };
}

test("a small install is deleted completely within the request", async () => {
  const e = env();
  seed(e, ID, 30);
  const r = await call(e, "/forget", { id: ID });
  assert.equal(r.status, 200);
  assert.deepEqual(r.body, { ok: true, pending: 0 });
  assert.equal(filesOf(e, ID), 0);
  assert.equal(count(e, "SELECT COUNT(*) FROM donations WHERE id = ?", ID), 0);
  assert.equal(count(e, "SELECT COUNT(*) FROM installs WHERE id = ?", ID), 0);
});

test("1,500 clips: no error, transcripts gone at once, audio over two days, nothing left", async () => {
  const e = env();
  seed(e, ID, 1500);
  const r = await call(e, "/forget", { id: ID });
  assert.equal(r.status, 200, "never an error");
  assert.equal(r.body.pending, 1300, "200 within the request, the rest queued");
  // Transcripts and counts are gone right away.
  assert.equal(count(e, "SELECT COUNT(*) FROM donations WHERE id = ?", ID), 0);
  assert.equal(count(e, "SELECT COUNT(*) FROM installs WHERE id = ?", ID), 0);
  // Day 1: hourly runs until the budget (900) is used.
  for (let h = 0; h < 24; h++) await drainAudio(e);
  assert.equal(filesOf(e, ID), 600, "900 deleted on day 1");
  assert.ok(e.AUDIO.used.delete <= 900, "never past Cloudflare's limit");
  nextDay(e);
  for (let h = 0; h < 3; h++) await drainAudio(e);
  assert.equal(filesOf(e, ID), 0);
  assert.equal(count(e, "SELECT COUNT(*) FROM forget_audio"), 0);
  // The last look in KV, five minutes after the last delete, then the id is done.
  e.DB.db.exec("UPDATE forget_sweep SET touched = 0");
  await drainAudio(e);
  assert.equal(count(e, "SELECT COUNT(*) FROM forget_sweep"), 0);
});

test("a file D1 never knew is found by the last look and deleted", async () => {
  const e = env();
  seed(e, ID, 3);
  e.AUDIO.store.set(`${ID}/dictation-20261125-000000001-ffff.flac`, new Uint8Array(10));
  await call(e, "/forget", { id: ID });
  assert.equal(filesOf(e, ID), 1, "the stray is still there");
  e.DB.db.exec("UPDATE forget_sweep SET touched = 0");
  await drainAudio(e); // lists, queues the stray
  await drainAudio(e); // deletes it
  assert.equal(filesOf(e, ID), 0);
  e.DB.db.exec("UPDATE forget_sweep SET touched = 0");
  await drainAudio(e);
  assert.equal(count(e, "SELECT COUNT(*) FROM forget_sweep"), 0);
});

test("when KV refuses deletes anyway, nothing is lost and the queue waits for tomorrow", async () => {
  const e = env({ kvLimit: 50 }); // someone used KV by hand today
  seed(e, ID, 120);
  const r = await call(e, "/forget", { id: ID });
  assert.equal(r.status, 200);
  assert.equal(r.body.pending, 70);
  assert.equal(filesOf(e, ID), 70);
  const before = e.AUDIO.used.delete;
  await drainAudio(e);
  assert.equal(e.AUDIO.used.delete, before, "no more tries today");
  nextDay(e);
  await drainAudio(e);
  assert.equal(filesOf(e, ID), 20, "another 50 the next day");
  nextDay(e);
  await drainAudio(e);
  assert.equal(filesOf(e, ID), 0);
});

test("two installs deleting at once share the budget and both finish", async () => {
  const e = env();
  seed(e, ID, 700);
  seed(e, OTHER, 700);
  await call(e, "/forget", { id: ID });
  await call(e, "/forget", { id: OTHER });
  for (let day = 0; day < 3; day++) {
    for (let h = 0; h < 24; h++) await drainAudio(e);
    assert.ok(e.AUDIO.used.delete <= 900);
    nextDay(e);
  }
  assert.equal(filesOf(e, ID) + filesOf(e, OTHER), 0);
});

test("audio waiting to be deleted still counts in the store", async () => {
  const e = env({ vars: { KV_DELETE_BUDGET: "0" } });
  seed(e, ID, 10);
  await call(e, "/forget", { id: ID });
  assert.equal(await storedBytes(e), 1000);
});

test("takeKv grants what fits under the budget, then nothing", async () => {
  const e = env({ vars: { KV_WRITE_BUDGET: "10" } });
  assert.equal(await takeKv(e, "write", 7), 7);
  assert.equal(await takeKv(e, "write", 7), 3);
  assert.equal(await takeKv(e, "write", 1), 0);
});

test("secondsToNextDay counts to 00:00 UTC", () => {
  assert.equal(secondsToNextDay(Date.UTC(2026, 10, 25, 23, 0, 0) / 1000), 3600);
});

// ----- Uploads -----

const FLAC = Buffer.concat([Buffer.from("fLaC"), Buffer.alloc(100)]).toString("base64");
const clip = (i) => ({
  id: ID,
  clip: `dictation-20261125-${String(i).padStart(9, "0")}-abcd`,
  language: "de",
  source: "dictation",
  seconds: 3,
  text: "hallo",
  audio: FLAC,
});

test("uploads past the daily write budget get 'try later' until the next day", async () => {
  const e = env({ vars: { KV_WRITE_BUDGET: "3" } });
  e.DB.db.prepare("INSERT INTO installs (id, clips, seconds, updated, donate) VALUES (?, 0, '{}', 0, 1)").run(ID);
  for (let i = 0; i < 3; i++) assert.equal((await call(e, "/donate", clip(i))).status, 200);
  const r = await call(e, "/donate", clip(3));
  assert.equal(r.status, 503);
  assert.equal(r.body.error, "try later");
  assert.ok(r.body.retry_after > 0 && r.body.retry_after <= 86400);
  assert.equal(r.headers.get("Retry-After"), String(r.body.retry_after));
  assert.equal(e.AUDIO.store.size, 3, "the refused clip wasn't stored");
  assert.equal(count(e, "SELECT COUNT(*) FROM donations"), 3, "nor its row");
  nextDay(e);
  assert.equal((await call(e, "/donate", clip(3))).status, 200, "the next day it goes");
});

test("an upload KV refuses anyway gets 'try later', not an error", async () => {
  const e = env({ kvLimit: 1 });
  e.DB.db.prepare("INSERT INTO installs (id, clips, seconds, updated, donate) VALUES (?, 0, '{}', 0, 1)").run(ID);
  assert.equal((await call(e, "/donate", clip(0))).status, 200);
  assert.equal((await call(e, "/donate", clip(1))).status, 503);
  assert.equal(count(e, "SELECT COUNT(*) FROM donations"), 1);
});

// ----- R2 (binding FILES) -----

/** R2: deletes free and in batches of up to 1,000 keys; counts writes. */
function fakeR2() {
  const store = new Map();
  const used = { put: 0, delete: 0, list: 0 };
  return {
    store,
    used,
    put: async (k, v) => (used.put++, store.set(k, v)),
    delete: async (keys) => {
      const list = Array.isArray(keys) ? keys : [keys];
      if (list.length > 1000) throw new Error("R2: at most 1000 keys per delete");
      used.delete++;
      list.forEach((k) => store.delete(k));
    },
    list: async ({ prefix = "", limit = 1000 }) => {
      used.list++;
      const keys = [...store.keys()].filter((k) => k.startsWith(prefix)).slice(0, limit);
      return { objects: keys.map((key) => ({ key })), truncated: false };
    },
  };
}

function r2env(opts = {}) {
  return { ...env(opts), FILES: fakeR2() };
}

const r2FilesOf = (e, id) => [...e.FILES.store.keys()].filter((k) => k.startsWith(`${id}/`)).length;

test("with R2, uploads go to R2 and never touch KV", async () => {
  const e = r2env({ vars: { KV_WRITE_BUDGET: "0" } });
  e.DB.db.prepare("INSERT INTO installs (id, clips, seconds, updated, donate) VALUES (?, 0, '{}', 0, 1)").run(ID);
  for (let i = 0; i < 5; i++) assert.equal((await call(e, "/donate", clip(i))).status, 200);
  assert.equal(e.FILES.store.size, 5);
  assert.equal(e.AUDIO.store.size, 0);
  assert.equal(e.AUDIO.used.write, 0);
});

test("with R2, the R2 write budget gives 'try later'", async () => {
  const e = r2env({ vars: { R2_WRITE_BUDGET: "2" } });
  e.DB.db.prepare("INSERT INTO installs (id, clips, seconds, updated, donate) VALUES (?, 0, '{}', 0, 1)").run(ID);
  assert.equal((await call(e, "/donate", clip(0))).status, 200);
  assert.equal((await call(e, "/donate", clip(1))).status, 200);
  assert.equal((await call(e, "/donate", clip(2))).status, 503);
});

test("with R2, 3,000 files are deleted within the request", async () => {
  const e = r2env();
  seed(e, ID, 3000);
  // seed() put the files into KV; these live in R2.
  for (const k of e.AUDIO.store.keys()) e.FILES.store.set(k, 1);
  e.AUDIO.store.clear();
  const r = await call(e, "/forget", { id: ID });
  assert.deepEqual(r.body, { ok: true, pending: 0 });
  assert.equal(r2FilesOf(e, ID), 0);
  assert.equal(e.AUDIO.used.delete, 0, "no KV deletes needed");
  assert.equal(e.FILES.used.delete, 3, "1,000 per call");
});

test("with R2, files still in KV from before are found and deleted too", async () => {
  const e = r2env();
  seed(e, ID, 300); // all still in KV
  const r = await call(e, "/forget", { id: ID });
  assert.equal(r.body.pending, 100, "200 KV deletes in the request");
  for (let h = 0; h < 2; h++) await drainAudio(e);
  assert.equal(filesOf(e, ID), 0);
  e.DB.db.exec("UPDATE forget_sweep SET touched = 0");
  await drainAudio(e);
  assert.equal(count(e, "SELECT COUNT(*) FROM forget_sweep"), 0);
});

test("the hourly run moves old KV files to R2 within the delete budget", async () => {
  const e = r2env({ vars: { KV_DELETE_BUDGET: "250" } });
  seed(e, ID, 300);
  e.AUDIO.store.set(`${OTHER}/dictation-20261125-000000001-ffff.flac`, 1); // nobody's
  const before = [...e.AUDIO.store.entries()].find(([k]) => k.startsWith(ID));
  for (let h = 0; h < 5; h++) await moveToR2(e);
  assert.equal(e.AUDIO.store.size, 51, "250 a day");
  nextDay(e);
  for (let h = 0; h < 3; h++) await moveToR2(e);
  assert.equal(e.AUDIO.store.size, 0, "KV empty");
  assert.equal(r2FilesOf(e, ID), 300, "every file of a kept recording moved");
  assert.equal(r2FilesOf(e, OTHER), 0, "a file without a recording is only deleted");
  assert.equal(e.FILES.store.get(before[0]), before[1], "same bytes");
  assert.ok(await e.DB.prepare("SELECT 1 FROM cache WHERE key = 'kv_moved'").first());
  const lists = e.AUDIO.used.list;
  await moveToR2(e);
  assert.equal(e.AUDIO.used.list, lists, "no more listing once done");
});

test("a recording deleted while moving isn't copied to R2", async () => {
  const e = r2env();
  seed(e, ID, 50);
  e.DB.db.exec("DELETE FROM donations");
  await moveToR2(e);
  assert.equal(e.FILES.store.size, 0);
  assert.equal(e.AUDIO.store.size, 0);
});
