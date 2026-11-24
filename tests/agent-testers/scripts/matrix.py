"""Builds the job matrix (one row per persona, with its phone) and enforces the
run's dollar cap before any emulator starts.

Env: PERSONAS (comma-separated ids, or "all"), BUDGET_USD (cap per phone),
MAX_RUN_USD (cap for the whole run), MAX_PERSONAS (at most this many phones).
Prints `android=<json>` and `cap=<dollars>` for $GITHUB_OUTPUT; exits non-zero
when the personas are unknown or the run could cost more than MAX_RUN_USD.

Based on scripts/matrix.py of pierridotite/mobile-agent-testers at d610b95a
(MIT): Android only, plus the cost cap."""
import json
import os
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
personas = {p["id"]: p for p in json.loads((ROOT / "personas.json").read_text(encoding="utf-8"))}
wanted = [p.strip() for p in os.environ.get("PERSONAS", "").split(",") if p.strip()]
if wanted == ["all"]:
    wanted = list(personas)
missing = [p for p in wanted if p not in personas]
if missing:
    raise SystemExit(f"Unknown personas: {missing}. Known: {', '.join(personas)}")
if not wanted:
    raise SystemExit("No personas given")
wanted = list(dict.fromkeys(wanted))

budget = float(os.environ.get("BUDGET_USD") or "0.80")
max_run = float(os.environ.get("MAX_RUN_USD") or "7")
max_personas = int(os.environ.get("MAX_PERSONAS") or "8")
if not 0 < budget <= 2:
    raise SystemExit(f"budget per phone must be between 0 and 2 dollars, got {budget}")
if len(wanted) > max_personas:
    raise SystemExit(f"{len(wanted)} phones asked, at most {max_personas} per run")
cap = round(budget * len(wanted), 2)
if cap > max_run:
    raise SystemExit(f"{len(wanted)} phones x ${budget:.2f} = ${cap:.2f}, more than the run cap of ${max_run:.2f}")

rows = []
for pid in wanted:
    p = personas[pid]
    d = p.get("device", {})
    geo = p.get("geo") or [52.52, 13.405]
    rows.append({"persona": pid, "theme": d.get("theme", "light"), "locale": p.get("locale", "en-US"),
                 "speech": p.get("speech", "en"), "geo": f"{geo[0]},{geo[1]}",
                 "profile": d.get("profile", "pixel_6"), "api": d.get("android_api", 34),
                 "font_scale": str(d.get("font_scale", 1.0)), "device": d.get("name", "Pixel 6, Android 14")})
print("android=" + json.dumps(rows, ensure_ascii=False))
print(f"cap={cap:.2f}")
print("speech=" + json.dumps(sorted({r["speech"] for r in rows})))
