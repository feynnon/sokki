// sokki.org's visitor count. This Worker sits in front of the website
// (GitHub Pages) and counts each page as it is delivered, so visitors' browsers
// run no tracking script and get no cookie. Each count goes to PostHog (EU).
//
// Sent per page view: the page, the site that linked here, the link's tag
// (?s=note, utm_*), country, browser, OS, device type, and a visitor number
// that changes every day. The IP address only goes into that number (with a
// random salt that is thrown away after two days); it is never stored or sent.
//
//   sokki.org/download  counts a click, then redirects to the latest release
//   sokki.org/github    counts a click, then redirects to the repository
//   every hour          counts new zip downloads from GitHub's release numbers
//                       (the Terminal install downloads the zip too)

const RELEASES = "https://github.com/feynnon/sokki/releases/latest";
const REPO = "https://github.com/feynnon/sokki";
const RELEASES_API = "https://api.github.com/repos/feynnon/sokki/releases?per_page=100";
const LINKS = { "/download": [RELEASES, "download_click"], "/github": [REPO, "github_click"] };

// Not people: crawlers, link previews, scripts, monitors.
const BOT =
  /bot|crawl|spider|slurp|preview|facebookexternalhit|embedly|whatsapp|telegram|discord|slack|skype|vkshare|pinterest|quora|curl|wget|python|go-http|java\/|okhttp|axios|node-fetch|undici|headless|lighthouse|pagespeed|monitor|uptime|scan|httpclient|libwww|feedfetcher|validator|^$/i;
const TAGS = ["s", "utm_source", "utm_medium", "utm_campaign", "utm_content", "utm_term", "ref"];

export default {
  async fetch(req, env, ctx) {
    const url = new URL(req.url);
    const link = LINKS[url.pathname.replace(/\/$/, "")];
    if (link) {
      const [to, event] = link;
      if (isPerson(req)) ctx.waitUntil(count(req, env, url, event));
      return Response.redirect(to, 302);
    }
    // In production the request goes on to GitHub Pages; ORIGIN is for `wrangler dev`.
    const upstream = env.ORIGIN ? new Request(env.ORIGIN + url.pathname + url.search, req) : req;
    const res = await fetch(upstream);
    if (
      res.status === 200 &&
      (res.headers.get("content-type") || "").startsWith("text/html") &&
      isPerson(req)
    ) {
      ctx.waitUntil(count(req, env, url, "$pageview"));
    }
    return res;
  },

  async scheduled(_event, env, ctx) {
    ctx.waitUntil(countDownloads(env));
  },
};

/** A real page load by a browser: GET, not a prefetch, not a bot. */
function isPerson(req) {
  if (req.method !== "GET") return false;
  const purpose = req.headers.get("sec-purpose") || req.headers.get("purpose") || "";
  if (/prefetch/i.test(purpose)) return false;
  return !BOT.test(req.headers.get("user-agent") || "");
}

async function count(req, env, url, event) {
  try {
    const ua = req.headers.get("user-agent") || "";
    const ip = req.headers.get("cf-connecting-ip") || "";
    const day = new Date().toISOString().slice(0, 10);
    const salt = await dailySalt(env, day);
    const hash = new Uint8Array(
      await crypto.subtle.digest("SHA-256", new TextEncoder().encode(`${salt}|${ip}|${ua}|${url.host}`)),
    );
    const visitor = hex(hash.slice(0, 16));
    const tags = {};
    const clean = new URLSearchParams();
    for (const key of TAGS) {
      const value = url.searchParams.get(key);
      if (value) {
        tags[key === "s" ? "link_tag" : key] = value.slice(0, 100);
        clean.set(key, value.slice(0, 100));
      }
    }
    const query = clean.toString();
    const referrer = req.headers.get("referer") || "";
    let referringDomain = "$direct";
    try {
      if (referrer) referringDomain = new URL(referrer).hostname;
    } catch {}
    const properties = {
      $current_url: `https://${url.host}${url.pathname}${query ? "?" + query : ""}`,
      $host: url.host,
      $pathname: url.pathname,
      $referrer: referrer || "$direct",
      $referring_domain: referringDomain,
      $geoip_disable: true,
      $geoip_country_code: req.cf?.country || null,
      $session_id: sessionId(day, hash),
      $process_person_profile: false,
      $lib: "sokki-website-counter",
      ...device(ua),
      ...tags,
    };
    await send(env, [{ event, distinct_id: visitor, properties, timestamp: new Date().toISOString() }]);
  } catch (e) {
    console.log("count failed", String(e));
  }
}

let saltCache = { day: "", salt: "" };

/** A random salt per day, shared by all Worker copies via KV, gone after two days. */
async function dailySalt(env, day) {
  if (saltCache.day === day) return saltCache.salt;
  let salt = await env.SALTS.get(day);
  if (!salt) {
    salt = hex(crypto.getRandomValues(new Uint8Array(32)));
    await env.SALTS.put(day, salt, { expirationTtl: 2 * 24 * 3600 });
  }
  saltCache = { day, salt };
  return salt;
}

/** One session per visitor and day: a UUIDv7 stamped with the day's start. */
function sessionId(day, hash) {
  const b = new Uint8Array(16);
  let ms = Date.parse(day + "T00:00:00Z");
  for (let i = 5; i >= 0; i--) {
    b[i] = ms % 256;
    ms = Math.floor(ms / 256);
  }
  b.set(hash.slice(16, 26), 6);
  b[6] = 0x70 | (b[6] & 0x0f);
  b[8] = 0x80 | (b[8] & 0x3f);
  const h = hex(b);
  return `${h.slice(0, 8)}-${h.slice(8, 12)}-${h.slice(12, 16)}-${h.slice(16, 20)}-${h.slice(20)}`;
}

/** Browser, OS and device type from the user agent; the user agent itself is not sent. */
function device(ua) {
  const browser = /Edg\//.test(ua)
    ? "Microsoft Edge"
    : /OPR\/|Opera/.test(ua)
      ? "Opera"
      : /Firefox\//.test(ua)
        ? "Firefox"
        : /Chrome\/|CriOS/.test(ua)
          ? "Chrome"
          : /Safari\//.test(ua)
            ? "Safari"
            : "Other";
  const os = /iPhone|iPad|iPod/.test(ua)
    ? "iOS"
    : /Android/.test(ua)
      ? "Android"
      : /Mac OS X|Macintosh/.test(ua)
        ? "Mac OS X"
        : /Windows/.test(ua)
          ? "Windows"
          : /Linux|CrOS/.test(ua)
            ? "Linux"
            : "Other";
  const type = /iPad|Tablet/.test(ua) ? "Tablet" : /Mobi|iPhone|Android/.test(ua) ? "Mobile" : "Desktop";
  return { $browser: browser, $os: os, $device_type: type };
}

/** New zip downloads since the last hour, from GitHub's release numbers. */
async function countDownloads(env) {
  try {
    const res = await fetch(RELEASES_API, {
      headers: { "User-Agent": "sokki-website-counter", Accept: "application/vnd.github+json" },
    });
    if (!res.ok) return console.log("github", res.status);
    let total = 0;
    for (const release of await res.json()) {
      for (const asset of release.assets || []) {
        if (asset.name.endsWith(".zip")) total += asset.download_count;
      }
    }
    const before = Number((await env.SALTS.get("zip-downloads")) || NaN);
    await env.SALTS.put("zip-downloads", String(total));
    if (!Number.isFinite(before) || total <= before) return;
    await send(env, [
      {
        event: "zip_downloads",
        distinct_id: "github-releases",
        properties: { count: total - before, total, $process_person_profile: false, $lib: "sokki-website-counter" },
        timestamp: new Date().toISOString(),
      },
    ]);
  } catch (e) {
    console.log("downloads failed", String(e));
  }
}

async function send(env, batch) {
  const res = await fetch(`${env.POSTHOG_HOST}/batch/`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ api_key: env.POSTHOG_KEY, batch }),
  });
  if (!res.ok) console.log("posthog", res.status, await res.text());
}

function hex(bytes) {
  return [...bytes].map((x) => x.toString(16).padStart(2, "0")).join("");
}
