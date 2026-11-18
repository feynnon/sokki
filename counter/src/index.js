// Sokki's counter.
//
// Installs where "Help improve Sokki" or "Support Sokki & Open Source" is on
// report a random id and how many seconds of their own data they keep per
// language; anyone can read the total. With "Help improve" on, an install
// also sends one daily report (usage counts, problems, the Mac's model and
// macOS version; never names, audio or anyone's words). A user who answers
// a buyer message with "I'm interested" sends the id, language and hours.
// Nothing here stores IP addresses.
//
//   POST /report    {"id": "<32 hex>", "clips": 12, "seconds": {"de": 3600}, "donate": false}
//   POST /usage     {"id": "<32 hex>", ...daily report...}
//   POST /interest  {"id": "<32 hex>", "notice": 1700000000, "language": "de", "hours": 12}
//   POST /leave     {"id": "<32 hex>"}   out of the count (both choices off)
//   POST /forget    {"id": "<32 hex>"}   everything stored under the id
//   GET  /total     -> {"installs": 3, "donors": 1, "total_seconds": 5400,
//                       "seconds_by_language": {...}, "notice": ..., "notice_id": ...}
//                      (refreshed at most every 10 minutes)

const ID = /^[0-9a-f]{32}$/;
const LANG = /^([a-z]{2}|other)$/;
// One person can't plausibly have kept more than this; stops silly numbers.
const MAX_SECONDS = 2000 * 3600;

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

function cleanSeconds(raw) {
  if (!raw || typeof raw !== "object") return null;
  const out = {};
  let total = 0;
  for (const [lang, secs] of Object.entries(raw).slice(0, 20)) {
    if (!LANG.test(lang) || typeof secs !== "number" || !(secs >= 0)) return null;
    out[lang] = Math.round(secs * 10) / 10;
    total += secs;
  }
  return total <= MAX_SECONDS ? out : null;
}

async function report(req, env) {
  const body = await req.json().catch(() => null);
  if (!body || !ID.test(body.id)) return json({ error: "bad id" }, 400);
  const seconds = cleanSeconds(body.seconds);
  const clips = Number.isInteger(body.clips) && body.clips >= 0 ? body.clips : null;
  if (!seconds || clips === null) return json({ error: "bad numbers" }, 400);
  const donate = body.donate === true ? 1 : 0;
  await env.DB.prepare(
    `INSERT INTO installs (id, clips, seconds, updated, donate) VALUES (?1, ?2, ?3, ?4, ?5)
     ON CONFLICT(id) DO UPDATE SET clips = ?2, seconds = ?3, updated = ?4, donate = ?5`,
  )
    .bind(body.id, clips, JSON.stringify(seconds), Math.floor(Date.now() / 1000), donate)
    .run();
  return json({ ok: true });
}

async function leave(req, env) {
  const body = await req.json().catch(() => null);
  if (!body || !ID.test(body.id)) return json({ error: "bad id" }, 400);
  await env.DB.prepare("DELETE FROM installs WHERE id = ?1").bind(body.id).run();
  return json({ ok: true });
}

async function forget(req, env) {
  const body = await req.json().catch(() => null);
  if (!body || !ID.test(body.id)) return json({ error: "bad id" }, 400);
  await env.DB.batch(
    ["installs", "usage", "interest"].map((t) =>
      env.DB.prepare(`DELETE FROM ${t} WHERE id = ?1`).bind(body.id),
    ),
  );
  return json({ ok: true });
}

// A daily report is small; anything bigger is a bug or abuse.
const MAX_USAGE_BYTES = 32 * 1024;

async function usage(req, env) {
  const text = await req.text();
  if (text.length > MAX_USAGE_BYTES) return json({ error: "too big" }, 413);
  let body;
  try {
    body = JSON.parse(text);
  } catch {
    return json({ error: "bad json" }, 400);
  }
  if (!body || !ID.test(body.id)) return json({ error: "bad id" }, 400);
  const now = Math.floor(Date.now() / 1000);
  const day = new Date(now * 1000).toISOString().slice(0, 10);
  const { id, ...report } = body;
  await env.DB.prepare(
    `INSERT INTO usage (id, day, report, received) VALUES (?1, ?2, ?3, ?4)
     ON CONFLICT(id, day) DO UPDATE SET report = ?3, received = ?4`,
  )
    .bind(id, day, JSON.stringify(report), now)
    .run();
  return json({ ok: true });
}

async function interest(req, env) {
  const body = await req.json().catch(() => null);
  if (!body || !ID.test(body.id)) return json({ error: "bad id" }, 400);
  const notice = Number.isInteger(body.notice) ? body.notice : null;
  const hours = Number.isInteger(body.hours) && body.hours >= 0 && body.hours < 5000 ? body.hours : null;
  if (notice === null || hours === null || !LANG.test(body.language)) {
    return json({ error: "bad answer" }, 400);
  }
  await env.DB.prepare(
    `INSERT INTO interest (id, notice, language, hours, received) VALUES (?1, ?2, ?3, ?4, ?5)
     ON CONFLICT(id, notice) DO UPDATE SET hours = ?4, received = ?5`,
  )
    .bind(body.id, notice, body.language, hours, Math.floor(Date.now() / 1000))
    .run();
  return json({ ok: true });
}

// /total is served from a stored copy, recomputed at most this often, so a
// popular page costs one row read per view instead of one per install.
const TOTAL_TTL_S = 600;

async function total(env) {
  const now = Math.floor(Date.now() / 1000);
  const cached = await env.DB.prepare(
    "SELECT value, updated FROM cache WHERE key = 'total'",
  ).first();
  // An optional message to every install, set by hand: plain text, or a
  // buyer as JSON {"language": "de", "text": "..."}. See README.
  const notice = await env.DB.prepare(
    "SELECT value, updated FROM cache WHERE key = 'notice'",
  ).first();
  // notice_id changes whenever the message is set, so each one notifies once.
  const withNotice = (t) => ({
    ...t,
    notice: notice ? notice.value : null,
    notice_id: notice ? notice.updated : 0,
  });
  if (cached && now - cached.updated < TOTAL_TTL_S) {
    return json(withNotice(JSON.parse(cached.value)));
  }
  const fresh = await computeTotal(env);
  await env.DB.prepare(
    `INSERT INTO cache (key, value, updated) VALUES ('total', ?1, ?2)
     ON CONFLICT(key) DO UPDATE SET value = ?1, updated = ?2`,
  )
    .bind(JSON.stringify(fresh), now)
    .run();
  return json(withNotice(fresh));
}

async function computeTotal(env) {
  const { results } = await env.DB.prepare(
    "SELECT seconds, donate FROM installs",
  ).all();
  const byLang = {};
  let sum = 0;
  let donors = 0;
  for (const row of results) {
    if (row.donate) donors += 1;
    for (const [lang, secs] of Object.entries(JSON.parse(row.seconds))) {
      byLang[lang] = (byLang[lang] || 0) + secs;
      sum += secs;
    }
  }
  return {
    installs: results.length,
    donors,
    total_seconds: Math.round(sum),
    seconds_by_language: byLang,
  };
}

export default {
  async fetch(req, env) {
    const { pathname } = new URL(req.url);
    if (req.method === "OPTIONS") return new Response(null, { headers: cors });
    if (req.method === "POST" && pathname === "/report") return report(req, env);
    if (req.method === "POST" && pathname === "/usage") return usage(req, env);
    if (req.method === "POST" && pathname === "/interest") return interest(req, env);
    if (req.method === "POST" && pathname === "/leave") return leave(req, env);
    if (req.method === "POST" && pathname === "/forget") return forget(req, env);
    if (req.method === "GET" && pathname === "/total") return total(env);
    return json({ error: "not found" }, 404);
  },
};
