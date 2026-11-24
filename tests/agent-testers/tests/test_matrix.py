"""scripts/matrix.py: one job per persona, and the run's dollar cap."""
import json
import os
import subprocess
import sys
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
PERSONAS = json.loads((ROOT / "personas.json").read_text(encoding="utf-8"))


def run(**env):
    e = {k: v for k, v in os.environ.items() if k not in ("PERSONAS", "BUDGET_USD", "MAX_RUN_USD", "MAX_PERSONAS")}
    e.update(env)
    return subprocess.run([sys.executable, "scripts/matrix.py"], cwd=ROOT, env=e, capture_output=True, text=True)


def outputs(res):
    assert res.returncode == 0, res.stderr
    return dict(line.split("=", 1) for line in res.stdout.strip().splitlines())


class Matrix(unittest.TestCase):
    def test_default_three_phones_cost_at_most_their_caps(self):
        out = outputs(run(PERSONAS="lena,tom,ada", BUDGET_USD="0.80"))
        rows = json.loads(out["android"])
        self.assertEqual([r["persona"] for r in rows], ["lena", "tom", "ada"])
        self.assertEqual(out["cap"], "2.40")
        self.assertEqual(json.loads(out["speech"]), ["de", "en"])
        self.assertEqual(rows[0]["locale"], "de-DE")
        self.assertEqual(rows[2]["font_scale"], "2.0")

    def test_all_is_every_persona(self):
        out = outputs(run(PERSONAS="all", BUDGET_USD="0.80"))
        self.assertEqual(len(json.loads(out["android"])), len(PERSONAS))

    def test_refuses_a_run_above_the_cap(self):
        res = run(PERSONAS="all", BUDGET_USD="1.00", MAX_RUN_USD="5")
        self.assertNotEqual(res.returncode, 0)
        self.assertIn("more than the run cap", res.stderr)

    def test_refuses_a_big_budget_per_phone_and_unknown_personas(self):
        self.assertIn("between 0 and 2", run(PERSONAS="lena", BUDGET_USD="5").stderr)
        self.assertIn("Unknown personas", run(PERSONAS="lena,nobody").stderr)

    def test_every_persona_has_test_sentences_and_a_known_brief_language(self):
        audio = (ROOT / "scripts/make_audio.sh").read_text(encoding="utf-8")
        for p in PERSONAS:
            self.assertIn(f"  {p['speech']}) KB=", audio, p["id"])
            self.assertEqual(p["platform"], "android")


class Workflow(unittest.TestCase):
    WF = (ROOT.parent.parent / ".github/workflows/agent-testers.yml").read_text(encoding="utf-8")

    def test_every_action_is_pinned_to_a_commit(self):
        uses = [line.split("uses:", 1)[1].strip() for line in self.WF.splitlines() if "uses:" in line]
        self.assertTrue(uses)
        for u in uses:
            ref = u.split("@", 1)[1].split()[0]
            self.assertRegex(ref, r"^[0-9a-f]{40}$", u)

    def test_only_manual_or_release_triggers(self):
        on = self.WF.split("\non:\n", 1)[1].split("\npermissions:", 1)[0]
        self.assertIn("workflow_dispatch:", on)
        self.assertIn("release:", on)
        for t in ("push:", "pull_request", "schedule:"):
            self.assertNotIn(t, on)

    def test_briefs_offered_exist(self):
        for name in ("onboarding", "keyboard", "meeting", "settings", "reproduce", "improve"):
            self.assertIn(name, self.WF)
            self.assertTrue((ROOT / "briefs" / f"{name}.md").exists(), name)


if __name__ == "__main__":
    unittest.main()
