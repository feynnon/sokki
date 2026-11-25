// Sokki's counter.
//
// Installs where "Help improve Sokki" or "Share all my recordings" is on
// report a random id and how many seconds of their own data they keep per
// language; anyone can read the total. With "Help improve" on, an install
// also sends one daily report (usage counts, problems, the Mac's model and
// macOS version; never names, audio or anyone's words), which goes on to
// PostHog EU for charts without the user's IP address. Sharers upload
// every recording of theirs (FLAC + transcript), unchanged. A user who answers a buyer message with "I'm interested" sends
// the id, language and hours. Nothing here stores IP addresses.
//
//   POST /report    {"id", "pow", "clips": 12, "seconds": {"de": 3600}, "donate": false}
//   POST /usage     {"id", "pow", ...daily report...}
//   POST /donate    {"id", "pow", "clip", "language", "source", "seconds", "text", "audio": "<base64 FLAC>"}
//                   audio into R2 (FILES), or Workers KV (AUDIO) without it
//   POST /interest  {"id", "pow", "notice": 1700000000, "language": "de", "hours": 12}
//   POST /tasks     {"id", "pow", "on": true, "languages": ["de"]}  "tell me about paid tasks"
//   POST /leave     {"id", "pow"}   out of the count (both choices off)
//   POST /forget    {"id", "pow"}   everything stored under the id, audio included,
//                   and the install's events at PostHog -> {"ok", "pending"}:
//                   audio files still being deleted (KV's daily limit)
//   GET  /total     -> {"installs", "donors", "total_seconds", "seconds_by_language",
//                       "notice", "notice_id"}  (refreshed at most every 10 minutes)
//   GET  /visit?p=/get/&s=note   one view of a sokki.org page: the page and
//                       the tag of the link that led there (?s=), per day.
//                       Nothing about the visitor.
//
// Against fake data (an open-source app can't hold a secret):
// - every id needs a proof of work: sha256("sokki:<id>:<pow>") must start
//   with POW_BITS zero bits, about a second of work per new id, so inventing
//   thousands of installs costs real time; ids from before this check are
//   accepted without one;
// - writes are rate limited per IP address (counted in memory by Cloudflare,
//   never stored);
// - hours can only grow as fast as real time passes;
// - uploads only for sharers, FLAC only, and the audio store has a cap.

const ID = /^[0-9a-f]{32}$/;
const POW = /^[0-9a-f]{1,16}$/;
const LANG = /^([a-z]{2}|other)$/;
const CLIP = /^(dictation|meeting)-\d{8}-\d{9}-[0-9a-f]{4}$/;
// One person can't plausibly have kept more than this; stops silly numbers.
const MAX_SECONDS = 2000 * 3600;
// Leading zero bits of the proof of work. The app and this must agree.
const POW_BITS = 24;
// The audio store stops taking uploads here: 900 MB in Workers KV (1 GB
// free), 9 GB in R2 (10 GB free); AUDIO_CAP_MB in wrangler.toml overrides.
function audioCap(env) {
  const mb = Number(env.AUDIO_CAP_MB);
  return (Number.isFinite(mb) && mb > 0 ? mb : env.FILES ? 9000 : 900) * 1024 * 1024;
}
const MAX_CLIP_BYTES = 5 * 1024 * 1024;

const cors = {
  "Access-Control-Allow-Origin": "*",
  "Access-Control-Allow-Methods": "GET, POST, OPTIONS",
  "Access-Control-Allow-Headers": "Content-Type",
};

const json = (body, status = 200) =>
  new Response(JSON.stringify(body), {
    status,
    headers: { "Content-Type": "application/json", ...cors },
  });

const now = () => Math.floor(Date.now() / 1000);

async function powBits(id, pow) {
  const data = new TextEncoder().encode(`sokki:${id}:${pow}`);
  const hash = new Uint8Array(await crypto.subtle.digest("SHA-256", data));
  let bits = 0;
  for (const byte of hash) {
    if (byte === 0) {
      bits += 8;
      continue;
    }
    bits += Math.clz32(byte) - 24;
    break;
  }
  return bits;
}

// Ids that reached the counter before the proof of work existed.
async function knownId(env, id) {
  const row = await env.DB.prepare(
    `SELECT 1 FROM installs WHERE id = ?1
     UNION SELECT 1 FROM usage WHERE id = ?1
     UNION SELECT 1 FROM interest WHERE id = ?1 LIMIT 1`,
  )
    .bind(id)
    .first();
  return row !== null;
}

// The body with a valid id and proof of work, or an error response.
async function authorised(req, env) {
  const body = await req.json().catch(() => null);
  if (!body || !ID.test(body.id)) return { error: json({ error: "bad id" }, 400) };
  if (typeof body.pow === "string" && POW.test(body.pow)) {
    if ((await powBits(body.id, body.pow)) >= POW_BITS) return { body };
    return { error: json({ error: "bad proof of work" }, 403) };
  }
  if (await knownId(env, body.id)) return { body };
  return { error: json({ error: "proof of work needed" }, 403) };
}

function cleanSeconds(raw) {
  if (!raw || typeof raw !== "object") return null;
  const out = {};
  let total = 0;
  for (const [lang, secs] of Object.entries(raw).slice(0, 20)) {
    if (!LANG.test(lang) || typeof secs !== "number" || !(secs >= 0)) return null;
    out[lang] = Math.round(secs * 10) / 10;
    total += secs;
  }
  return total <= MAX_SECONDS ? { out, total } : null;
}

const sum = (o) => Object.values(o).reduce((a, b) => a + b, 0);

async function report(req, env) {
  const { body, error } = await authorised(req, env);
  if (error) return error;
  const seconds = cleanSeconds(body.seconds);
  const clips = Number.isInteger(body.clips) && body.clips >= 0 ? body.clips : null;
  if (!seconds || clips === null) return json({ error: "bad numbers" }, 400);
  const t = now();
  // Nobody records more of their own voice than time passes (plus an hour
  // of slack for clocks and rounding).
  const prev = await env.DB.prepare("SELECT seconds, updated FROM installs WHERE id = ?1")
    .bind(body.id)
    .first();
  if (prev && seconds.total > sum(JSON.parse(prev.seconds)) + (t - prev.updated) + 3600) {
    return json({ error: "implausible growth" }, 422);
  }
  const donate = body.donate === true ? 1 : 0;
  await env.DB.prepare(
    `INSERT INTO installs (id, clips, seconds, updated, donate) VALUES (?1, ?2, ?3, ?4, ?5)
     ON CONFLICT(id) DO UPDATE SET clips = ?2, seconds = ?3, updated = ?4, donate = ?5`,
  )
    .bind(body.id, clips, JSON.stringify(seconds.out), t, donate)
    .run();
  return json({ ok: true });
}

async function leave(req, env) {
  const { body, error } = await authorised(req, env);
  if (error) return error;
  await env.DB.prepare("DELETE FROM installs WHERE id = ?1").bind(body.id).run();
  return json({ ok: true });
}

/**
 * "Delete what was shared". Everything in D1 (counts, reports, transcripts)
 * goes at once. The audio files are queued in `forget_audio`: those in R2
 * are deleted right here (free, 1,000 per call); those still in Workers KV
 * within the day's budget of KV deletes (free plan: 1,000 a day), some right
 * here, the rest by the hourly run, until none is left.
 * Answers {"ok": true, "pending": n}: n audio files still to delete; the app
 * then says "being deleted".
 */
async function forget(req, env, ctx) {
  const { body, error } = await authorised(req, env);
  if (error) return error;
  const t = now();
  await env.DB.batch([
    env.DB.prepare(
      `INSERT INTO forget_audio (key, id, bytes, received, store)
       SELECT id || '/' || clip || '.flac', id, bytes, ?2, ?3 FROM donations WHERE id = ?1
       ON CONFLICT(key, store) DO NOTHING`,
    ).bind(body.id, t, env.FILES ? "r2" : "kv"),
    // A last look in KV for files D1 doesn't know (an upload cut off halfway).
    env.DB.prepare(
      `INSERT INTO forget_sweep (id, received, touched) VALUES (?1, ?2, ?2)
       ON CONFLICT(id) DO UPDATE SET touched = ?2`,
    ).bind(body.id, t),
    ...["installs", "usage", "interest", "donations", "tasks"].map((tb) =>
      env.DB.prepare(`DELETE FROM ${tb} WHERE id = ?1`).bind(body.id),
    ),
    env.DB.prepare(
      "INSERT INTO posthog_forget (id, received) VALUES (?1, ?2) ON CONFLICT(id) DO NOTHING",
    ).bind(body.id, t),
  ]);
  // Files from before R2 may still be in KV (until moved): one listing.
  if (env.FILES) await queueStrays(env, body.id, "kv");
  await drainAudio(env, { id: body.id, max: 5000, kvMax: FORGET_NOW_MAX });
  const left = await env.DB.prepare("SELECT COUNT(*) AS n FROM forget_audio WHERE id = ?1")
    .bind(body.id)
    .first();
  ctx.waitUntil(drainForget(env).catch((e) => console.log("posthog forget", String(e))));
  return json({ ok: true, pending: left ? left.n : 0 });
}

// ----- The free plan's daily KV limits -----
//
// Workers KV allows 1,000 writes, 1,000 deletes and 1,000 lists a day on the
// free plan (reset 00:00 UTC), then answers with errors. Each is counted in
// `kv_ops` per UTC day and kept under a budget, so uploads say "try later"
// and deletions wait for the next day instead of failing halfway. With
// Workers Paid, raise the budgets (vars KV_*_BUDGET in wrangler.toml).

const KV_BUDGET_DEFAULT = { write: 900, delete: 900, list: 100, r2_write: 30000 };
// KV files deleted within the /forget request itself; the rest goes hourly.
const FORGET_NOW_MAX = 200;
// Audio files deleted per hourly run (KV allows 1,000 operations per call).
const DRAIN_MAX = 500;
// Files moved from KV to R2 per hourly run (each costs one KV delete).
const MOVE_MAX = 100;

const utcDay = (t = now()) => new Date(t * 1000).toISOString().slice(0, 10);

/** The daily budget of `kind`: KV_WRITE_BUDGET, …, and R2_WRITE_BUDGET (R2's
 * free 1M writes a month is ~33,000 a day). */
export function kvBudget(env, kind) {
  const name = kind === "r2_write" ? "R2_WRITE_BUDGET" : `KV_${kind.toUpperCase()}_BUDGET`;
  const v = Number(env[name]);
  return Number.isFinite(v) && v >= 0 ? v : KV_BUDGET_DEFAULT[kind];
}

/**
 * Takes up to `want` KV operations of `kind` from today's budget; returns
 * how many were granted. One query: the count goes up by `want` either way,
 * which only overstates a day already at its budget.
 */
export async function takeKv(env, kind, want) {
  if (want <= 0) return 0;
  const row = await env.DB.prepare(
    `INSERT INTO kv_ops (day, kind, n) VALUES (?1, ?2, ?3)
     ON CONFLICT(day, kind) DO UPDATE SET n = n + ?3 RETURNING n`,
  )
    .bind(utcDay(), kind, want)
    .first();
  const before = (row ? row.n : want) - want;
  return Math.max(0, Math.min(want, kvBudget(env, kind) - before));
}

/** KV said no although the budget had room: nothing more of `kind` today. */
async function kvExhausted(env, kind) {
  await env.DB.prepare(
    `INSERT INTO kv_ops (day, kind, n) VALUES (?1, ?2, ?3)
     ON CONFLICT(day, kind) DO UPDATE SET n = MAX(n, ?3)`,
  )
    .bind(utcDay(), kind, kvBudget(env, kind))
    .run();
}

/** Seconds until the next UTC day, when KV's limits reset. */
export function secondsToNextDay(t = now()) {
  return 86400 - (t % 86400);
}

/**
 * Deletes queued audio files (`forget_audio`), oldest first: those of `id`
 * only, or everyone's. R2 files go at once; KV files within today's budget.
 * Then a last look in both stores for each finished id (`forget_sweep`),
 * five minutes after its last delete, since a KV listing can lag behind
 * deletes. Returns how many files it deleted.
 */
export async function drainAudio(env, { id = null, max = DRAIN_MAX, kvMax = DRAIN_MAX } = {}) {
  const queued = await env.DB.prepare(
    `SELECT rowid AS r, key, id, store FROM forget_audio
     WHERE ?1 IS NULL OR id = ?1 ORDER BY rowid LIMIT ?2`,
  )
    .bind(id, max)
    .all();
  const rows = queued.results || [];
  const gone = [];
  const inR2 = env.FILES ? rows.filter((r) => r.store === "r2") : [];
  for (let i = 0; i < inR2.length; i += 1000) {
    const chunk = inR2.slice(i, i + 1000);
    try {
      await env.FILES.delete(chunk.map((r) => r.key));
      gone.push(...chunk);
    } catch (e) {
      console.log("r2 delete", String(e));
      break;
    }
  }
  const inKv = rows.filter((r) => r.store !== "r2").slice(0, kvMax);
  if (inKv.length) {
    const granted = await takeKv(env, "delete", inKv.length);
    const todo = inKv.slice(0, granted);
    for (let i = 0; i < todo.length; i += 50) {
      const chunk = todo.slice(i, i + 50);
      const results = await Promise.allSettled(chunk.map((row) => env.AUDIO.delete(row.key)));
      chunk.forEach((row, k) => results[k].status === "fulfilled" && gone.push(row));
      const failed = results.find((r) => r.status === "rejected");
      if (failed) {
        console.log("kv delete", String(failed.reason));
        await kvExhausted(env, "delete");
        break;
      }
    }
  }
  if (gone.length) {
    const t = now();
    await env.DB.batch([
      env.DB.prepare(
        "DELETE FROM forget_audio WHERE rowid IN (SELECT value FROM json_each(?1))",
      ).bind(JSON.stringify(gone.map((g) => g.r))),
      env.DB.prepare(
        `UPDATE forget_sweep SET touched = ?1
         WHERE id IN (SELECT value FROM json_each(?2))`,
      ).bind(t, JSON.stringify([...new Set(gone.map((g) => g.id))])),
      // Recounted on the next upload (storedBytes).
      env.DB.prepare("DELETE FROM cache WHERE key = 'audio_bytes'"),
    ]);
  }
  if (!id) await sweepAudio(env);
  return gone.length;
}

/**
 * Lists `id`'s files in one store and queues them for deletion. Returns
 * how many it found, or null when the store can't be listed today.
 */
async function queueStrays(env, id, store) {
  let keys;
  try {
    if (store === "r2") {
      const page = await env.FILES.list({ prefix: `${id}/`, limit: 1000 });
      keys = page.objects.map((o) => o.key);
    } else {
      if ((await takeKv(env, "list", 1)) < 1) return null;
      const page = await env.AUDIO.list({ prefix: `${id}/`, limit: 1000 });
      keys = page.keys.map((k) => k.name);
    }
  } catch (e) {
    console.log(`${store} list`, String(e));
    if (store === "kv") await kvExhausted(env, "list");
    return null;
  }
  if (keys.length) {
    // One query (the free plan allows 50 per run), however many files.
    await env.DB.prepare(
      `INSERT INTO forget_audio (key, id, bytes, received, store)
       SELECT value, ?1, 0, ?2, ?3 FROM json_each(?4) WHERE true
       ON CONFLICT(key, store) DO NOTHING`,
    )
      .bind(id, now(), store, JSON.stringify(keys))
      .run();
  }
  return keys.length;
}

async function sweepAudio(env) {
  const { results = [] } = await env.DB.prepare(
    `SELECT s.id FROM forget_sweep s
     WHERE s.touched < ?1 AND NOT EXISTS (SELECT 1 FROM forget_audio a WHERE a.id = s.id)
     LIMIT 5`,
  )
    .bind(now() - 300)
    .all();
  for (const { id } of results) {
    let found = 0;
    for (const store of env.FILES ? ["kv", "r2"] : ["kv"]) {
      const n = await queueStrays(env, id, store);
      if (n === null) return;
      found += n;
    }
    if (!found) await env.DB.prepare("DELETE FROM forget_sweep WHERE id = ?1").bind(id).run();
  }
}

/**
 * Moves the audio uploaded before R2 from Workers KV into R2, a batch per
 * hourly run, within the day's KV delete budget (deletions of "Delete what
 * was shared" go first). A file whose recording was deleted meanwhile is
 * only deleted. Stops listing once KV is empty.
 */
export async function moveToR2(env) {
  if (!env.FILES) return 0;
  const done = await env.DB.prepare("SELECT 1 FROM cache WHERE key = 'kv_moved'").first();
  if (done) return 0;
  if ((await takeKv(env, "list", 1)) < 1) return 0;
  const page = await env.AUDIO.list({ limit: MOVE_MAX });
  if (!page.keys.length) {
    await env.DB.prepare(
      "INSERT INTO cache (key, value, updated) VALUES ('kv_moved', 'all', ?1) ON CONFLICT(key) DO NOTHING",
    )
      .bind(now())
      .run();
    return 0;
  }
  const names = page.keys.map((k) => k.name);
  // Files queued for deletion are the queue's; the rest move if D1 still has them.
  const { results: kept = [] } = await env.DB.prepare(
    `SELECT value AS key FROM json_each(?1)
     WHERE EXISTS (SELECT 1 FROM donations d WHERE d.id || '/' || d.clip || '.flac' = value)
       AND NOT EXISTS (SELECT 1 FROM forget_audio f WHERE f.key = value)`,
  )
    .bind(JSON.stringify(names))
    .all();
  const queued = await env.DB.prepare(
    "SELECT value AS key FROM json_each(?1) WHERE EXISTS (SELECT 1 FROM forget_audio f WHERE f.key = value)",
  )
    .bind(JSON.stringify(names))
    .all();
  const skip = new Set((queued.results || []).map((r) => r.key));
  const keep = new Set(kept.map((r) => r.key));
  const todo = names.filter((n) => !skip.has(n));
  const granted = await takeKv(env, "delete", todo.length);
  let moved = 0;
  for (const key of todo.slice(0, granted)) {
    try {
      if (keep.has(key)) {
        const audio = await env.AUDIO.get(key, "arrayBuffer");
        if (audio) await env.FILES.put(key, audio);
      }
      await env.AUDIO.delete(key);
      moved += 1;
    } catch (e) {
      console.log("move to r2", String(e));
      await kvExhausted(env, "delete");
      break;
    }
  }
  return moved;
}

// A daily report is small; anything bigger is a bug or abuse. The first
// one after turning "Help improve" on also carries setup and older events.
const MAX_USAGE_BYTES = 64 * 1024;

async function usage(req, env, ctx) {
  if (Number(req.headers.get("Content-Length") || 0) > MAX_USAGE_BYTES) {
    return json({ error: "too big" }, 413);
  }
  const { body, error } = await authorised(req, env);
  if (error) return error;
  const { id, pow, ...report } = body;
  const text = JSON.stringify(report);
  if (text.length > MAX_USAGE_BYTES) return json({ error: "too big" }, 413);
  const t = now();
  const day = new Date(t * 1000).toISOString().slice(0, 10);
  await env.DB.prepare(
    `INSERT INTO usage (id, day, report, received) VALUES (?1, ?2, ?3, ?4)
     ON CONFLICT(id, day) DO UPDATE SET report = ?3, received = ?4`,
  )
    .bind(id, day, text, t)
    .run();
  if (env.POSTHOG_KEY && shouldRelay(report)) ctx.waitUntil(relay(env, id, report, t));
  return json({ ok: true });
}

/**
 * Only reports from apps whose "What's shared" names PostHog go there:
 * those carry the `usage` part (the app's usage.rs, same release as the
 * new text). Older apps' users agreed to a text without any third party,
 * so their reports stay in D1 only.
 */
export function shouldRelay(report) {
  const u = report && report.usage;
  return !!u && typeof u === "object" && !Array.isArray(u);
}

// ----- PostHog (EU), for charts -----
//
// The daily report goes on to PostHog as events, sent from here, so PostHog
// never sees the user's IP address (only Cloudflare's) and the app talks to
// this counter alone. Same project as sokki.org's counter; `$lib` tells them
// apart. Only what the report already holds: counts, times, fixed names.

const NAME = /^[a-z0-9_]{1,40}$/;
const EVENTS = new Set([
  "first_start",
  "setup_step",
  "first_dictation",
  "first_meeting",
  "app_updated",
  "crash",
]);

const num = (v) => (typeof v === "number" && Number.isFinite(v) ? v : undefined);
const word = (v) =>
  typeof v === "string" && /^[A-Za-z0-9_.:\/ ~+-]{1,120}$/.test(v) ? v : undefined;

/** Keeps numbers, booleans and short plain words of a flat object. */
function flat(obj, prefix, out = {}) {
  if (!obj || typeof obj !== "object") return out;
  for (const [k, v] of Object.entries(obj).slice(0, 60)) {
    if (!NAME.test(k)) continue;
    if (typeof v === "boolean") out[prefix + k] = v;
    else if (num(v) !== undefined) out[prefix + k] = v;
    else if (word(v) !== undefined) out[prefix + k] = v;
  }
  return out;
}

/** The PostHog events for one daily report. */
export function posthogEvents(id, r, t) {
  const base = { $lib: "sokki-app", $geoip_disable: true };
  const app = r.app || {};
  const device = r.device || {};
  const u = r.usage || {};
  const act = r.activity || {};
  const meetings = Array.isArray(act.meetings) ? act.meetings : [];
  const person = {
    app_version: word(app.version),
    os: word(device.os),
    os_version: word(device.os_version),
    chip: word(device.chip),
    ram_gb: num(device.ram_gb),
    region: word(device.region),
    model: word(app.model),
    language: word(app.language),
    ui_language: word(app.ui_language),
  };
  const daily = {
    ...base,
    ...person,
    dictations: num(act.dictations),
    dictation_seconds: num(act.dictation_seconds),
    meetings: meetings.length,
    meeting_minutes: meetings.reduce((a, m) => a + (num(m?.minutes) || 0), 0),
    problems: Array.isArray(r.problems) ? r.problems.length : 0,
    accessibility: u.permissions?.accessibility === true,
    ...flat(u.counts, "count_"),
    ...flat(u.settings, "setting_"),
    $set: person,
  };
  if (num(u.first_start)) daily.$set_once = { first_start: new Date(u.first_start * 1000).toISOString() };
  for (const [k, v] of Object.entries(u.timings || {}).slice(0, 20)) {
    if (!NAME.test(k) || !v || typeof v !== "object") continue;
    daily[`${k}_median`] = num(v.median);
    daily[`${k}_p90`] = num(v.p90);
    daily[`${k}_n`] = num(v.n);
  }
  const iso = (s) => new Date(s * 1000).toISOString();
  const out = [{ event: "app_daily", distinct_id: id, timestamp: iso(t), properties: daily }];
  for (const e of (Array.isArray(u.events) ? u.events : []).slice(0, 200)) {
    if (!e || !EVENTS.has(e.event) || !num(e.at)) continue;
    // Not before Sokki existed, not in the future.
    const at = Math.min(Math.max(e.at, 1_760_000_000), t);
    out.push({
      event: e.event,
      distinct_id: id,
      timestamp: iso(at),
      properties: { ...base, app_version: person.app_version, ...flat(e.props, "") },
    });
  }
  for (const line of (Array.isArray(r.problems) ? r.problems : []).slice(0, 30)) {
    if (typeof line !== "string") continue;
    const level = line.includes("[ERROR]") ? "error" : "warning";
    out.push({
      event: "app_problem",
      distinct_id: id,
      timestamp: iso(t),
      properties: { ...base, app_version: person.app_version, level, message: line.slice(0, 240) },
    });
  }
  return out;
}

async function relay(env, id, report, t) {
  try {
    const res = await fetch(`${env.POSTHOG_HOST}/batch/`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ api_key: env.POSTHOG_KEY, batch: posthogEvents(id, report, t) }),
    });
    if (!res.ok) console.log("posthog", res.status, await res.text());
  } catch (e) {
    console.log("posthog", String(e));
  }
}

/**
 * "Delete what was shared" also deletes the install at PostHog, with its
 * events. That needs a personal API key (secret POSTHOG_PERSONAL_KEY); until
 * it is set, ids wait in `posthog_forget` and the hourly run retries.
 */
async function forgetAtPostHog(env, id) {
  if (!env.POSTHOG_PERSONAL_KEY || !env.POSTHOG_PROJECT) return false;
  const api = `${env.POSTHOG_API}/api/projects/${env.POSTHOG_PROJECT}/persons`;
  const auth = { Authorization: `Bearer ${env.POSTHOG_PERSONAL_KEY}` };
  const found = await fetch(`${api}/?distinct_id=${id}`, { headers: auth });
  if (!found.ok) return false;
  const { results = [] } = await found.json();
  for (const p of results) {
    const del = await fetch(`${api}/${p.id}/?delete_events=true`, { method: "DELETE", headers: auth });
    if (!del.ok && del.status !== 404) return false;
  }
  return true;
}

async function drainForget(env) {
  const { results } = await env.DB.prepare("SELECT id FROM posthog_forget LIMIT 50").all();
  for (const { id } of results) {
    if (await forgetAtPostHog(env, id).catch(() => false)) {
      await env.DB.prepare("DELETE FROM posthog_forget WHERE id = ?1").bind(id).run();
    }
  }
}

async function interest(req, env) {
  const { body, error } = await authorised(req, env);
  if (error) return error;
  const notice = Number.isInteger(body.notice) ? body.notice : null;
  const hours = Number.isInteger(body.hours) && body.hours >= 0 && body.hours < 5000 ? body.hours : null;
  if (notice === null || hours === null || !LANG.test(body.language)) {
    return json({ error: "bad answer" }, 400);
  }
  await env.DB.prepare(
    `INSERT INTO interest (id, notice, language, hours, received) VALUES (?1, ?2, ?3, ?4, ?5)
     ON CONFLICT(id, notice) DO UPDATE SET hours = ?4, received = ?5`,
  )
    .bind(body.id, notice, body.language, hours, now())
    .run();
  return json({ ok: true });
}

// "Tell me about paid recording tasks": a tick and the languages a task could
// be in; unticking removes the row.
async function tasks(req, env) {
  const { body, error } = await authorised(req, env);
  if (error) return error;
  if (body.on !== true) {
    await env.DB.prepare("DELETE FROM tasks WHERE id = ?1").bind(body.id).run();
    return json({ ok: true });
  }
  const languages = Array.isArray(body.languages) ? body.languages.slice(0, 5) : [];
  if (!languages.every((l) => typeof l === "string" && LANG.test(l))) {
    return json({ error: "bad languages" }, 400);
  }
  await env.DB.prepare(
    `INSERT INTO tasks (id, languages, updated) VALUES (?1, ?2, ?3)
     ON CONFLICT(id) DO UPDATE SET languages = ?2, updated = ?3`,
  )
    .bind(body.id, JSON.stringify(languages), now())
    .run();
  return json({ ok: true });
}

function base64Bytes(s) {
  try {
    const bin = atob(s);
    const out = new Uint8Array(bin.length);
    for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
    return out;
  } catch {
    return null;
  }
}

async function donate(req, env) {
  if (Number(req.headers.get("Content-Length") || 0) > MAX_CLIP_BYTES * 1.4 + 64 * 1024) {
    return json({ error: "too big" }, 413);
  }
  const { body, error } = await authorised(req, env);
  if (error) return error;
  const { id, clip, language, source, seconds, text } = body;
  if (
    !CLIP.test(clip || "") ||
    !LANG.test(language || "") ||
    !["dictation", "meeting"].includes(source) ||
    typeof seconds !== "number" ||
    !(seconds >= 1 && seconds <= 900) ||
    typeof text !== "string" ||
    text.length < 1 ||
    text.length > 10000
  ) {
    return json({ error: "bad clip" }, 400);
  }
  const audio = typeof body.audio === "string" ? base64Bytes(body.audio) : null;
  // FLAC files start with "fLaC".
  if (
    !audio ||
    audio.length < 64 ||
    audio.length > MAX_CLIP_BYTES ||
    String.fromCharCode(...audio.slice(0, 4)) !== "fLaC"
  ) {
    return json({ error: "bad audio" }, 400);
  }
  // Only installs that share all recordings (their count says donate = 1).
  const donor = await env.DB.prepare("SELECT donate FROM installs WHERE id = ?1").bind(id).first();
  if (!donor || donor.donate !== 1) return json({ error: "not a supporter" }, 403);
  // A clip sent again replaces the earlier upload of it.
  const prev = await env.DB.prepare("SELECT bytes FROM donations WHERE id = ?1 AND clip = ?2")
    .bind(id, clip)
    .first();
  const delta = audio.length - (prev ? prev.bytes : 0);
  if ((await storedBytes(env)) + delta > audioCap(env)) return json({ error: "store full" }, 507);
  // Today's writes are used up: the app keeps the clip and tries again the
  // next day. R2 once it is bound (FILES), Workers KV before.
  const kind = env.FILES ? "r2_write" : "write";
  if ((await takeKv(env, kind, 1)) < 1) return tryLater();
  try {
    await (env.FILES || env.AUDIO).put(`${id}/${clip}.flac`, audio);
  } catch (e) {
    console.log(`${kind} put`, String(e));
    await kvExhausted(env, kind);
    return tryLater();
  }
  const t = now();
  await env.DB.batch([
    env.DB.prepare(
      `INSERT INTO donations (id, clip, language, source, seconds, bytes, text, received)
       VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
       ON CONFLICT(id, clip) DO UPDATE SET bytes = ?6, text = ?7, received = ?8`,
    ).bind(id, clip, language, source, Math.round(seconds * 100) / 100, audio.length, text, t),
    env.DB.prepare(
      "UPDATE cache SET value = CAST(value AS INTEGER) + ?1, updated = ?2 WHERE key = 'audio_bytes'",
    ).bind(delta, t),
  ]);
  return json({ ok: true });
}

/** "Try again tomorrow": 503 with the seconds until KV's limits reset. */
function tryLater() {
  const wait = secondsToNextDay();
  return new Response(JSON.stringify({ error: "try later", retry_after: wait }), {
    status: 503,
    headers: { "Content-Type": "application/json", "Retry-After": String(wait), ...cors },
  });
}

/**
 * Bytes of audio stored, kept as a running total in `cache` ('audio_bytes').
 * Summing every donation on each upload read ~700,000 rows a day for one
 * sharing install (KNOWN_ISSUES U5); this reads one row. The sum runs only
 * when the total is missing: the first upload after this change and after
 * queued audio was deleted.
 */
export async function storedBytes(env) {
  const row = await env.DB.prepare("SELECT value FROM cache WHERE key = 'audio_bytes'").first();
  if (row) return Number(row.value);
  // Audio still waiting to be deleted (forget_audio) is still in the store.
  const { b } = await env.DB.prepare(
    `SELECT (SELECT COALESCE(SUM(bytes), 0) FROM donations)
          + (SELECT COALESCE(SUM(bytes), 0) FROM forget_audio) AS b`,
  ).first();
  await env.DB.prepare(
    `INSERT INTO cache (key, value, updated) VALUES ('audio_bytes', ?1, ?2)
     ON CONFLICT(key) DO UPDATE SET value = ?1, updated = ?2`,
  )
    .bind(String(b), now())
    .run();
  return b;
}

// /total is served from a stored copy, recomputed at most this often, so a
// popular page costs one row read per view instead of one per install.
const TOTAL_TTL_S = 600;

async function total(env) {
  const t = now();
  const cached = await env.DB.prepare(
    "SELECT value, updated FROM cache WHERE key = 'total'",
  ).first();
  // An optional message to every install, set by hand: plain text, or a
  // buyer as JSON {"language": "de", "text": "..."}. See README.
  const notice = await env.DB.prepare(
    "SELECT value, updated FROM cache WHERE key = 'notice'",
  ).first();
  // notice_id changes whenever the message is set, so each one notifies once.
  const withNotice = (v) => ({
    ...v,
    notice: notice ? notice.value : null,
    notice_id: notice ? notice.updated : 0,
  });
  if (cached && t - cached.updated < TOTAL_TTL_S) {
    return json(withNotice(JSON.parse(cached.value)));
  }
  const fresh = await computeTotal(env);
  await env.DB.prepare(
    `INSERT INTO cache (key, value, updated) VALUES ('total', ?1, ?2)
     ON CONFLICT(key) DO UPDATE SET value = ?1, updated = ?2`,
  )
    .bind(JSON.stringify(fresh), t)
    .run();
  return json(withNotice(fresh));
}

async function computeTotal(env) {
  const { results } = await env.DB.prepare(
    "SELECT seconds, donate FROM installs",
  ).all();
  const byLang = {};
  let all = 0;
  let donors = 0;
  for (const row of results) {
    if (row.donate) donors += 1;
    for (const [lang, secs] of Object.entries(JSON.parse(row.seconds))) {
      byLang[lang] = (byLang[lang] || 0) + secs;
      all += secs;
    }
  }
  // Only the order of magnitude is public: 1000, 10000, …; below 1000, 0
  // (the app then shows no number).
  return {
    installs: atLeast(results.length),
    donors: atLeast(donors),
    total_seconds: Math.round(all),
    seconds_by_language: byLang,
  };
}

function atLeast(n) {
  if (n < 1000) return 0;
  let step = 1000;
  while (step * 10 <= n) step *= 10;
  return step;
}

// sokki.org pages that count their views, and the link tags they accept.
const PAGES = new Set(["/", "/get/", "/p/", "/privacy/", "/impressum/", "/changelog/", "/sponsor/"]);
const SOURCE = /^[a-z0-9-]{1,24}$/;

/** One page view per call: day, page, link tag. Always 204, even if not counted. */
async function visit(url, env) {
  let page = (url.searchParams.get("p") || "").replace(/index\.html$/, "");
  if (!page.endsWith("/")) page += "/";
  const s = url.searchParams.get("s") || "";
  if (PAGES.has(page)) {
    await env.DB.prepare(
      `INSERT INTO visits (day, page, source, views) VALUES (?1, ?2, ?3, 1)
       ON CONFLICT(day, page, source) DO UPDATE SET views = views + 1`,
    )
      .bind(new Date().toISOString().slice(0, 10), page, SOURCE.test(s) ? s : "")
      .run();
  }
  return new Response(null, { status: 204, headers: cors });
}

const ROUTES = {
  "/report": report,
  "/usage": usage,
  "/donate": donate,
  "/interest": interest,
  "/tasks": tasks,
  "/leave": leave,
  "/forget": forget,
};

export default {
  async scheduled(_event, env, ctx) {
    ctx.waitUntil(drainForget(env));
    ctx.waitUntil(
      drainAudio(env)
        .then(() => moveToR2(env))
        .catch((e) => console.log("audio forget", String(e))),
    );
  },
  async fetch(req, env, ctx) {
    const { pathname } = new URL(req.url);
    if (req.method === "OPTIONS") return new Response(null, { headers: cors });
    if (req.method === "GET" && pathname === "/total") return total(env);
    // Per IP address, counted in memory by Cloudflare and never stored.
    const ip = req.headers.get("CF-Connecting-IP") || "unknown";
    if (req.method === "GET" && pathname === "/visit") {
      const { success } = env.WRITE_LIMIT
        ? await env.WRITE_LIMIT.limit({ key: ip })
        : { success: true };
      if (!success) return new Response(null, { status: 204, headers: cors });
      return visit(new URL(req.url), env);
    }
    const route = ROUTES[pathname];
    if (req.method !== "POST" || !route) return json({ error: "not found" }, 404);
    const limiter = pathname === "/donate" ? env.DONATE_LIMIT : env.WRITE_LIMIT;
    if (limiter) {
      const { success } = await limiter.limit({ key: ip });
      if (!success) return json({ error: "slow down" }, 429);
    }
    return route(req, env, ctx);
  },
};
