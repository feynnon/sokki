// Sokki's counter.
//
// Installs where "Help improve Sokki" or "Support Sokki & Open Source" is on
// report a random id and how many seconds of their own data they keep per
// language; anyone can read the total. With "Help improve" on, an install
// also sends one daily report (usage counts, problems, the Mac's model and
// macOS version; never names, audio or anyone's words). Supporters upload
// their own recordings (FLAC + transcript), only ones without names or
// numbers. A user who answers a buyer message with "I'm interested" sends
// the id, language and hours. Nothing here stores IP addresses.
//
//   POST /report    {"id", "pow", "clips": 12, "seconds": {"de": 3600}, "donate": false}
//   POST /usage     {"id", "pow", ...daily report...}
//   POST /donate    {"id", "pow", "clip", "language", "source", "seconds", "text", "audio": "<base64 FLAC>"}
//   POST /interest  {"id", "pow", "notice": 1700000000, "language": "de", "hours": 12}
//   POST /tasks     {"id", "pow", "on": true, "languages": ["de"]}  "tell me about paid tasks"
//   POST /leave     {"id", "pow"}   out of the count (both choices off)
//   POST /forget    {"id", "pow"}   everything stored under the id, audio included
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
// - uploads only for supporters, FLAC only, and the audio store has a cap.

const ID = /^[0-9a-f]{32}$/;
const POW = /^[0-9a-f]{1,16}$/;
const LANG = /^([a-z]{2}|other)$/;
const CLIP = /^(dictation|meeting)-\d{8}-\d{9}-[0-9a-f]{4}$/;
// One person can't plausibly have kept more than this; stops silly numbers.
const MAX_SECONDS = 2000 * 3600;
// Leading zero bits of the proof of work. The app and this must agree.
const POW_BITS = 24;
// The audio store stops taking uploads here (Workers KV holds 1 GB free).
const AUDIO_CAP_BYTES = 900 * 1024 * 1024;
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

async function forget(req, env) {
  const { body, error } = await authorised(req, env);
  if (error) return error;
  // The audio first: if this stops halfway, the rows still say what's left.
  let cursor;
  do {
    const page = await env.AUDIO.list({ prefix: `${body.id}/`, cursor });
    await Promise.all(page.keys.map((k) => env.AUDIO.delete(k.name)));
    cursor = page.list_complete ? undefined : page.cursor;
  } while (cursor);
  await env.DB.batch(
    ["installs", "usage", "interest", "donations", "tasks"].map((t) =>
      env.DB.prepare(`DELETE FROM ${t} WHERE id = ?1`).bind(body.id),
    ),
  );
  return json({ ok: true });
}

// A daily report is small; anything bigger is a bug or abuse.
const MAX_USAGE_BYTES = 32 * 1024;

async function usage(req, env) {
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
  return json({ ok: true });
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
  // Only installs that said "Support" (their count says donate = 1).
  const donor = await env.DB.prepare("SELECT donate FROM installs WHERE id = ?1").bind(id).first();
  if (!donor || donor.donate !== 1) return json({ error: "not a supporter" }, 403);
  const stored = await env.DB.prepare("SELECT COALESCE(SUM(bytes), 0) AS b FROM donations").first();
  if (stored.b + audio.length > AUDIO_CAP_BYTES) return json({ error: "store full" }, 507);
  await env.AUDIO.put(`${id}/${clip}.flac`, audio);
  await env.DB.prepare(
    `INSERT INTO donations (id, clip, language, source, seconds, bytes, text, received)
     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
     ON CONFLICT(id, clip) DO UPDATE SET bytes = ?6, text = ?7, received = ?8`,
  )
    .bind(id, clip, language, source, Math.round(seconds * 100) / 100, audio.length, text, now())
    .run();
  return json({ ok: true });
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
  async fetch(req, env) {
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
    return route(req, env);
  },
};
