#!/usr/bin/env python3
"""Quick German competence check for local models (Ollama).

14 spoken-style sentences: 5 contain one real grammar/spelling error, 9 are
correct (including colloquial speech and English loanwords, which must NOT
be flagged). Scores: errors found and fixed right, correct sentences left alone.

  python3 -I scripts/german_probe.py gemma4:12b qwen3:14b
"""

import json
import re
import sys
import urllib.request

SENTENCES = [
    ("Kannst du mir die Unterlagen bis morgen schicken, damit ich sie vor dem Termin durchlesen kann?", None),
    ("Der Kunde hat gestern angerufen und gesagt, dass er mit dem Ergebnis sehr Zufriedenheit ist.", "zufrieden ist"),
    ("Wir haben die Kampagne letzte Woche gelauncht und die ersten Zahlen sehen gut aus.", None),
    ("Ich glaube, wir sollten das Meeting auf nächste Woche verschieben, weil Thomas noch im Urlaub ist.", None),
    ("Die Kosten sind deutlich höher als erwartet, deswegen müssen wir das Budget nochmal überdenken.", None),
    ("Das Angebot liegt bei zwölftausend Euro pro Jahr, was für den Kunde zu teuer war.", "für den Kunden"),
    ("Sie hat mir erzählt, dass sie den Job gekündigt hat, weil der Chef sie ständig gemobbt hat.", None),
    ("Wir müssen das noch mit der Rechtsabteilung abstimmen, bevor wir den Vertrag unterschreiben.", None),
    ("Er ist gestern nach Hamburg gefahrt, um dort einen neuen Kunden zu treffen.", "gefahren"),
    ("Könnten wir die Präsentation etwas kürzer machen? Die Leute verlieren sonst das Interesse.", None),
    ("Ich hab das heute Morgen schon gecheckt, passt alles.", None),
    ("Die neue Mitarbeiterin fängt am ersten Dezember an und wird sich im Vertrieb um die Großkunden kümmern, die sie betreuen sollen.", "betreuen soll"),
    ("Wenn wir das Feature nicht bis zum Release fertig bekommen, müssen wir es halt rausnehmen.", None),
    ("Das Wetter war so schlecht, das wir das Event absagen mussten.", "dass wir"),
]

SYSTEM = (
    "Du prüfst Sätze aus gesprochenem Deutsch in einem Business-Kontext. Umgangssprache und englische "
    "Lehnwörter (z. B. 'gelauncht', 'gecheckt', 'Feature') sind korrekt. Für jeden Satz: Ist er grammatisch "
    "und orthografisch korrekt? Wenn nicht, gib den korrigierten Satz an (nur den Fehler korrigieren). "
    "Antworte nur mit JSON: {\"results\": [{\"id\": <nummer>, \"correct\": true|false, \"fixed\": \"<korrigierter Satz oder leer>\"}]}"
)


def chat(model, user):
    body = {
        "model": model,
        "messages": [{"role": "system", "content": SYSTEM}, {"role": "user", "content": user}],
        "stream": False,
        "think": False,
        "options": {"temperature": 0, "seed": 1, "num_ctx": 4096},
        "keep_alive": "1m",
    }
    req = urllib.request.Request("http://127.0.0.1:11434/api/chat", json.dumps(body).encode(),
                                 {"Content-Type": "application/json"})
    with urllib.request.urlopen(req, timeout=600) as r:
        return json.load(r)["message"]["content"]


def main():
    user = "\n".join(f"{i}. {s}" for i, (s, _) in enumerate(SENTENCES))
    for model in sys.argv[1:]:
        reply = chat(model, user)
        text = re.sub(r"<think>.*?</think>", "", reply, flags=re.S)
        data = json.loads(text[text.find("{"): text.rfind("}") + 1])
        by_id = {int(r["id"]): r for r in data.get("results", [])}
        found = fixed = false_alarm = 0
        notes = []
        for i, (s, fix) in enumerate(SENTENCES):
            r = by_id.get(i, {})
            flagged = r.get("correct") is False
            if fix:
                if flagged:
                    found += 1
                    if fix.lower() in (r.get("fixed") or "").lower():
                        fixed += 1
                    else:
                        notes.append(f"  wrong fix  [{i}] {r.get('fixed')!r}")
                else:
                    notes.append(f"  missed     [{i}] {s}")
            elif flagged:
                false_alarm += 1
                notes.append(f"  false alarm [{i}] {s} -> {r.get('fixed')!r}")
        print(f"{model:14} errors found {found}/5, fixed right {fixed}/5, correct sentences flagged {false_alarm}/9")
        print("\n".join(notes))


if __name__ == "__main__":
    main()
