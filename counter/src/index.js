// Verbatim's voice-data counter. Opted-in installs report a random id and
// how many seconds of their own voice they keep per language; anyone can
// read the total. No names, no audio, no text ever reach this service.
//
//   POST /report  {"id": "<32 hex>", "clips": 12, "seconds": {"de": 3600}}
//   POST /forget  {"id": "<32 hex>"}
//   GET  /total   -> {"installs": 3, "total_seconds": 5400, "seconds_by_language": {...}}
//                    (refreshed at most every 10 minutes)

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
  await env.DB.prepare(
    `INSERT INTO installs (id, clips, seconds, updated) VALUES (?1, ?2, ?3, ?4)
     ON CONFLICT(id) DO UPDATE SET clips = ?2, seconds = ?3, updated = ?4`,
  )
    .bind(body.id, clips, JSON.stringify(seconds), Math.floor(Date.now() / 1000))
    .run();
  return json({ ok: true });
}

async function forget(req, env) {
  const body = await req.json().catch(() => null);
  if (!body || !ID.test(body.id)) return json({ error: "bad id" }, 400);
  await env.DB.prepare("DELETE FROM installs WHERE id = ?1").bind(body.id).run();
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
  if (cached && now - cached.updated < TOTAL_TTL_S) {
    return json(JSON.parse(cached.value));
  }
  const fresh = await computeTotal(env);
  await env.DB.prepare(
    `INSERT INTO cache (key, value, updated) VALUES ('total', ?1, ?2)
     ON CONFLICT(key) DO UPDATE SET value = ?1, updated = ?2`,
  )
    .bind(JSON.stringify(fresh), now)
    .run();
  return json(fresh);
}

async function computeTotal(env) {
  const { results } = await env.DB.prepare("SELECT seconds FROM installs").all();
  const byLang = {};
  let sum = 0;
  for (const row of results) {
    for (const [lang, secs] of Object.entries(JSON.parse(row.seconds))) {
      byLang[lang] = (byLang[lang] || 0) + secs;
      sum += secs;
    }
  }
  return {
    installs: results.length,
    total_seconds: Math.round(sum),
    seconds_by_language: byLang,
  };
}

export default {
  async fetch(req, env) {
    const { pathname } = new URL(req.url);
    if (req.method === "OPTIONS") return new Response(null, { headers: cors });
    if (req.method === "POST" && pathname === "/report") return report(req, env);
    if (req.method === "POST" && pathname === "/forget") return forget(req, env);
    if (req.method === "GET" && pathname === "/total") return total(env);
    return json({ error: "not found" }, 404);
  },
};
