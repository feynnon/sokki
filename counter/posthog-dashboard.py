#!/usr/bin/env python3
"""Creates (or recreates) the "Sokki app" dashboard in PostHog EU from the
events the counter relays (see README, "Help improve Sokki").

Reads the personal API key from ~/.config/posthog/personal.key (never
printed). Run: python3 counter/posthog-dashboard.py
"""
import json
import pathlib
import urllib.request

API = "https://eu.posthog.com"
PROJECT = 367071
NAME = "Sokki app"
KEY = (pathlib.Path.home() / ".config/posthog/personal.key").read_text().strip()

# Only the app's events; the self-test install is left out.
APP = [
    {"key": "$lib", "value": ["sokki-app"], "operator": "exact", "type": "event"},
    {"key": "app_version", "value": ["0.0.0-selftest"], "operator": "is_not", "type": "event"},
]


def call(method, path, body=None):
    req = urllib.request.Request(
        f"{API}/api/projects/{PROJECT}/{path}",
        method=method,
        data=json.dumps(body).encode() if body is not None else None,
        headers={"Authorization": f"Bearer {KEY}", "Content-Type": "application/json"},
    )
    with urllib.request.urlopen(req, timeout=30) as r:
        text = r.read()
        return json.loads(text) if text else None


def ev(event, **kw):
    return {"kind": "EventsNode", "event": event, "name": event, **kw}


def step(name):
    return ev("setup_step", custom_name=name,
              properties=[{"key": "step", "value": [name], "operator": "exact", "type": "event"}])


def trends(series, **kw):
    return {"kind": "TrendsQuery", "series": series, "properties": APP, "dateRange": {"date_from": "-30d"}, **kw}


INSIGHTS = [
    ("Installs sharing, per day", "Installs that sent a daily report.",
     trends([ev("app_daily", math="dau", custom_name="Installs")], interval="day")),
    ("Setup, from Your data on", "Of the installs that reached setup step 'Your data' (where the switch is).",
     {"kind": "FunnelsQuery", "properties": APP, "dateRange": {"date_from": "-30d"},
      "series": [step("voice"), step("try"), step("done"), step("finished"),
                 ev("first_dictation", custom_name="First dictation")],
      "funnelsFilter": {"funnelWindowInterval": 14, "funnelWindowIntervalUnit": "day"}}),
    ("Seconds to the first dictation", "Median time from first start to the first dictation, in seconds.",
     trends([ev("first_dictation", math="median", math_property="seconds_since_start",
                custom_name="Seconds to first dictation")],
            trendsFilter={"display": "BoldNumber"})),
    ("Still using Sokki, by first week", "Installs that sent a daily report again, per week after first start.",
     {"kind": "RetentionQuery", "properties": APP, "dateRange": {"date_from": "-56d"},
      "retentionFilter": {"period": "Week", "totalIntervals": 8, "retentionType": "retention_first_time",
                          "targetEntity": {"id": "first_start", "type": "events", "name": "first_start"},
                          "returningEntity": {"id": "app_daily", "type": "events", "name": "app_daily"}}}),
    ("Features used, per week", "Totals over all sharing installs.",
     trends([ev("app_daily", math="sum", math_property=p, custom_name=n) for p, n in [
         ("count_dictation_typed", "Dictations"),
         ("count_meeting_done", "Meetings"),
         ("count_summary_on_mac", "Summaries on the Mac"),
         ("count_summary_cloud", "Summaries in the cloud"),
         ("count_file_transcribed", "Files transcribed"),
         ("count_sample_meeting_started", "Sample meetings"),
     ]], interval="week")),
    ("Crashes, by version", "Panics (place in code) and macOS crash reports.",
     trends([ev("crash", custom_name="Crashes")], interval="day",
            breakdownFilter={"breakdown": "app_version", "breakdown_type": "event"})),
    ("Top problems", "Warnings and errors from the log, last 30 days.",
     trends([ev("app_problem", math="dau", custom_name="Installs")],
            breakdownFilter={"breakdown": "message", "breakdown_type": "event", "breakdown_limit": 20},
            trendsFilter={"display": "ActionsTable"})),
    ("Speed, median per day", "Milliseconds. Let go → text typed, key → mic ready, meeting start, call end → note.",
     trends([ev("app_daily", math="median", math_property=p, custom_name=n) for p, n in [
         ("dictation_typed_ms_median", "Let go → text typed"),
         ("dictation_ready_ms_median", "Key → microphone ready"),
         ("meeting_start_ms_median", "Meeting start"),
         ("meeting_ready_ms_median", "Call end → note ready"),
     ]], interval="day")),
]


def main():
    existing = call("GET", "dashboards/?limit=200")["results"]
    for d in existing:
        if d["name"] == NAME and not d.get("deleted"):
            call("PATCH", f"dashboards/{d['id']}/", {"deleted": True})
    dash = call("POST", "dashboards/", {"name": NAME, "description":
                "Help improve Sokki: opt-in daily reports from the Mac app, relayed by the counter without IP addresses."})
    for name, desc, query in INSIGHTS:
        call("POST", "insights/", {"name": name, "description": desc, "dashboards": [dash["id"]],
                                   "query": {"kind": "InsightVizNode", "source": query}})
    print(f"{API}/project/{PROJECT}/dashboard/{dash['id']}")


if __name__ == "__main__":
    main()
