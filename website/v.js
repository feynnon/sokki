// One count per page view: this page and the tag of the link that led here
// (?s=note, ?s=mac-card). Nothing about you; the counter stores no IP addresses.
fetch(
  "https://verbatim-counter.feynnon.workers.dev/visit?p=" +
    encodeURIComponent(location.pathname) +
    "&s=" +
    encodeURIComponent(new URLSearchParams(location.search).get("s") || ""),
  { mode: "no-cors", keepalive: true },
).catch(() => {});
