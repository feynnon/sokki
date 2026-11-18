#!/usr/bin/env python3
"""Generates long_meeting_60min.txt: a synthetic ~60 minute German quarterly
planning meeting, long enough to need map-reduce summarisation.

Deterministic (fixed seed). Each agenda topic has a few scripted "key" lines
that carry the facts the live test checks, plus discussion lines assembled
from topic-specific sentence pools so the transcript has realistic bulk.

Key facts (checked by tests/live_llm.rs):
  - Marketing budget 2027: 480.000 Euro, 60 % performance marketing (early)
  - CRM go-live: 15. März; Speaker 4 owns the data migration (middle)
  - Churn rose from 3,1 to 4,2 Prozent (middle)
  - Trade fair in Köln on 22. April, booth 18.000 Euro, decided to attend (late)
  - Speaker 5 writes the two job ads by Thursday (late-middle)

Run: python3 gen_long_meeting.py > long_meeting_60min.txt
"""

import random

random.seed(20261117)

SPEAKERS = ["Ich", "Speaker 2", "Speaker 3", "Speaker 4", "Speaker 5"]

TOPICS = [
    {
        "minutes": 3,
        "key": [
            ("Ich", "Guten Morgen zusammen, schön dass alle da sind. Heute ist unser Quartalsplanung für das nächste Jahr, wir haben ungefähr eine Stunde. Auf der Agenda stehen das Budget für 2027, die CRM Umstellung, Recruiting, die Kundenzahlen und die Messe im Frühjahr."),
            ("Speaker 3", "Klingt gut. Sollen wir mit dem Budget anfangen? Da hängt ja vieles dran."),
        ],
        "pool": [
            "Ich habe die Unterlagen gestern Abend noch mal verschickt, falls jemand sie nicht gesehen hat.",
            "Kurze Frage vorab, nehmen wir das Meeting auf?",
            "Ja, ich nehme es auf, damit wir hinterher eine Zusammenfassung haben.",
            "Ich muss um kurz vor zwölf leider los, aber bis dahin bin ich voll dabei.",
            "Der Kaffee in der Küche ist übrigens wieder leer, nur so als Hinweis.",
        ],
    },
    {
        "minutes": 9,
        "key": [
            ("Ich", "Also, zum Budget. Die Geschäftsführung hat das Marketing Budget für 2027 jetzt final freigegeben. Es sind vierhundertachtzigtausend Euro für das ganze Jahr."),
            ("Speaker 2", "Das ist mehr als dieses Jahr, oder? Wir hatten doch knapp vierhunderttausend."),
            ("Ich", "Genau, es sind ungefähr zwanzig Prozent mehr. Mein Vorschlag ist, dass sechzig Prozent davon in Performance Marketing gehen und der Rest in Content, Events und Tools."),
            ("Speaker 3", "Sechzig Prozent Performance finde ich richtig, da sehen wir die Ergebnisse am direktesten."),
            ("Ich", "Dann halten wir das so fest: Von den vierhundertachtzigtausend Euro gehen sechzig Prozent in Performance Marketing."),
        ],
        "pool": [
            "Wir sollten aber genau schauen, welche Kanäle wirklich funktionieren und welche nur Geld kosten.",
            "Bei Google Ads sind die Klickpreise im letzten Quartal noch mal deutlich gestiegen.",
            "Ich würde gern einen Teil für Experimente reservieren, zum Beispiel für TikTok oder Podcasts.",
            "Die Frage ist, wie wir das Budget über das Jahr verteilen, im Sommer läuft ja traditionell weniger.",
            "Wir könnten das Budget quartalsweise freigeben und nach jedem Quartal neu bewerten.",
            "Ich hätte gern eine Übersicht, wie viel wir dieses Jahr pro Kanal tatsächlich ausgegeben haben.",
            "Die Agenturkosten müssen wir da auch noch mit reinrechnen, die laufen bisher über ein anderes Budget.",
            "Wenn wir mehr in Content gehen, brauchen wir auch mehr Leute, die schreiben können.",
            "Meta hat zuletzt gut funktioniert, vor allem mit den kurzen Videos.",
            "Bei LinkedIn bin ich skeptisch, die Leads waren teuer und nicht besser.",
            "Wir sollten uns ein klares Ziel für die Kosten pro Lead setzen, sonst diskutieren wir jedes Quartal neu.",
            "Für Tools geben wir aktuell zu viel aus, da gibt es einige Abos, die keiner mehr nutzt.",
        ],
    },
    {
        "minutes": 10,
        "key": [
            ("Ich", "Nächstes Thema ist die CRM Umstellung. Magst du kurz den Stand erzählen?"),
            ("Speaker 4", "Klar. Wir haben den Vertrag mit dem neuen CRM Anbieter unterschrieben. Der Go-live ist für den fünfzehnten März geplant."),
            ("Speaker 2", "Fünfzehnter März ist sportlich, wenn man an die ganzen alten Daten denkt."),
            ("Speaker 4", "Deshalb übernehme ich die Datenmigration selbst. Ich habe schon angefangen, die alten Kontakte zu bereinigen, das sind ungefähr fünfundzwanzigtausend Datensätze."),
            ("Ich", "Gut, dann ist das klar: Go-live am fünfzehnten März und du verantwortest die Datenmigration."),
        ],
        "pool": [
            "Wir müssen die Vertriebsleute früh schulen, sonst arbeiten die weiter mit ihren Excel Listen.",
            "Viele Kontakte im alten System sind doppelt angelegt, das müssen wir vor der Migration aufräumen.",
            "Die Schnittstelle zur Buchhaltung ist noch offen, da warten wir auf eine Antwort vom Anbieter.",
            "Ich würde vorschlagen, dass wir eine Testphase mit zwei oder drei Leuten aus dem Vertrieb machen.",
            "Die Felder für die Lead Quelle sollten wir einheitlich definieren, das war bisher ein großes Chaos.",
            "Was passiert eigentlich mit den alten E-Mail Verläufen, nehmen wir die mit?",
            "Die E-Mail Verläufe der letzten zwei Jahre nehmen wir mit, alles ältere wird archiviert.",
            "Für das Reporting brauchen wir im neuen System andere Dashboards, die alten passen nicht mehr.",
            "Der Datenschutz muss sich das auch noch anschauen, vor allem die Löschfristen.",
            "Wir sollten einen Plan B haben, falls der Termin im März doch nicht klappt.",
            "Die Lizenzen sind pro Nutzer, also überlegen wir genau, wer wirklich Zugang braucht.",
            "Ich habe mit dem Support des Anbieters telefoniert, die wirken ziemlich kompetent.",
        ],
    },
    {
        "minutes": 9,
        "key": [
            ("Ich", "Dann zum Recruiting. Wir haben zwei offene Stellen, eine Senior Entwicklerin oder ein Senior Entwickler und eine Werkstudentenstelle im Marketing."),
            ("Speaker 5", "Für beide Stellen fehlen noch die Stellenanzeigen. Ich kann die schreiben, ich schicke euch die Entwürfe bis Donnerstag."),
            ("Ich", "Super, danke. Dann kommen die beiden Stellenanzeigen bis Donnerstag von dir."),
        ],
        "pool": [
            "Wir sollten in der Anzeige ehrlich sagen, dass wir hybrid arbeiten, zwei Tage im Büro.",
            "Die Gehaltsspanne würde ich direkt in die Anzeige schreiben, das bringt mehr passende Bewerbungen.",
            "Bei der Werkstudentenstelle sollten wir die Hochschulen direkt anschreiben.",
            "Letztes Mal hat der Prozess fast drei Monate gedauert, das war viel zu lang.",
            "Ich wäre dafür, dass wir maximal zwei Interviewrunden machen.",
            "Eine kleine Probeaufgabe finde ich sinnvoll, aber nicht mehr als zwei Stunden Aufwand.",
            "Wir könnten auch über Empfehlungen aus dem Team gehen, das hat bei den letzten Stellen gut funktioniert.",
            "Für die Entwicklerstelle sollten wir auch auf Englisch ausschreiben.",
            "Wer aus dem Team hat Zeit, bei den Interviews dabei zu sein?",
            "Das Onboarding für neue Leute müssen wir auch verbessern, da ging zuletzt einiges unter.",
        ],
    },
    {
        "minutes": 10,
        "key": [
            ("Speaker 3", "Ich habe mir die Kundenzahlen angeschaut. Leider ist die Churn Rate im letzten Quartal von drei Komma eins auf vier Komma zwei Prozent gestiegen."),
            ("Ich", "Das ist ein deutlicher Anstieg. Wissen wir, woran das liegt?"),
            ("Speaker 3", "Zum Teil ja. Die meisten Kündigungen kamen von kleinen Kunden, die das Produkt nach dem Onboarding kaum genutzt haben."),
            ("Speaker 2", "Das deckt sich mit dem, was wir im Support hören. Viele kommen mit der Einrichtung nicht klar."),
        ],
        "pool": [
            "Wir könnten nach zwei Wochen automatisch nachfragen, ob alles funktioniert.",
            "Ein kurzes Video zur Einrichtung würde vermutlich schon viel helfen.",
            "Die großen Kunden sind sehr stabil, da gab es fast keine Kündigungen.",
            "Vielleicht sollten wir die Kündigungsgründe systematischer abfragen, bisher ist das ein Freitextfeld.",
            "Ich würde gern mit fünf oder sechs Kunden sprechen, die gekündigt haben.",
            "Der Preis wird als Grund eher selten genannt, das überrascht mich ehrlich gesagt.",
            "Einige Kunden wünschen sich eine bessere Integration mit ihren bestehenden Tools.",
            "Wir sollten die Kundenzufriedenheit regelmäßiger messen, nicht nur einmal im Jahr.",
            "Die Supportzeiten sind auch ein Thema, viele Kunden schreiben abends.",
            "Ein Health Score für Kunden wäre hilfreich, damit wir früher reagieren können.",
            "Im Onboarding verlieren wir die meisten Leute beim Import der eigenen Daten.",
        ],
    },
    {
        "minutes": 8,
        "key": [
            ("Speaker 2", "Zur Messe: Die Fachmesse in Köln ist am zweiundzwanzigsten April. Ein Stand kostet uns insgesamt achtzehntausend Euro, inklusive Standbau."),
            ("Speaker 3", "Achtzehntausend ist viel Geld. Was haben wir letztes Mal von der Messe mitgenommen?"),
            ("Speaker 2", "Letztes Mal hatten wir gut sechzig qualifizierte Gespräche und daraus sind acht Kunden geworden."),
            ("Ich", "Das rechnet sich. Ich bin dafür, dass wir teilnehmen. Gibt es Einwände?"),
            ("Speaker 4", "Keine Einwände, ich wäre gern am Stand dabei."),
            ("Ich", "Dann ist entschieden: Wir nehmen an der Messe in Köln am zweiundzwanzigsten April teil."),
        ],
        "pool": [
            "Wir brauchen neue Roll-ups, die alten haben noch das alte Logo.",
            "Hotels in Köln sind zur Messezeit schnell ausgebucht, wir sollten früh buchen.",
            "Eine Live Demo am Stand fand ich letztes Mal am wirkungsvollsten.",
            "Wir könnten vorher gezielt Kunden einladen, uns am Stand zu besuchen.",
            "Für die Lead Erfassung am Stand sollten wir direkt das neue CRM nutzen.",
            "Giveaways würde ich diesmal weglassen, die bringen nichts.",
            "Wer betreut den Stand an welchem Tag? Das sollten wir früh planen.",
            "Ein kleiner Vortrag im Rahmenprogramm wäre super, die Bewerbungsfrist läuft noch.",
        ],
    },
    {
        "minutes": 8,
        "key": [
            ("Speaker 5", "Noch ein Punkt zum Büro: Der Umzug in die neuen Räume ist jetzt für Juli bestätigt."),
            ("Ich", "Gut zu wissen. Das betrifft uns vor allem bei der Planung der Teamtage."),
        ],
        "pool": [
            "Die neuen Räume haben endlich mehr Besprechungsräume, das hilft enorm.",
            "Wir sollten klären, ob jeder einen festen Platz bekommt oder ob wir Desk Sharing machen.",
            "Die Anbindung mit der Bahn ist am neuen Standort deutlich besser.",
            "Für den Umzug brauchen wir einen Tag, an dem niemand Termine bei Kunden hat.",
            "Die alten Möbel können wir teilweise mitnehmen, der Rest wird gespendet.",
            "Ich fände eine kleine Einweihungsfeier schön, vielleicht mit Kunden zusammen.",
            "Das Internet im neuen Büro muss vor dem Umzug laufen, nicht danach.",
            "Parkplätze gibt es leider weniger, da müssen wir eine Regel finden.",
            "Der Teamtag im Frühjahr könnte schon im neuen Büro stattfinden, wenn alles klappt.",
            "Wir sollten eine Liste machen, was beim Umzug alles erledigt werden muss.",
        ],
    },
    {
        "minutes": 3,
        "key": [
            ("Ich", "Okay, wir sind fast durch. Ich schicke euch die Zusammenfassung heute Nachmittag. Gibt es noch etwas Dringendes?"),
            ("Speaker 2", "Von mir nicht, danke."),
            ("Speaker 4", "Nein, alles gut."),
            ("Ich", "Dann danke euch allen, bis nächste Woche."),
        ],
        "pool": [
            "Danke für die gute Vorbereitung, das ging heute echt zügig.",
            "Wann ist das nächste Planungsmeeting, wieder in drei Monaten?",
            "Ja, Ende des nächsten Quartals machen wir das wieder.",
        ],
    },
]

FILLERS = ["Ja, ", "Okay, ", "Also, ", "Genau, ", "Hm, ", "Gute Frage. ", "", "", "", ""]


def words(s):
    return len(s.split())


def fmt(t):
    t = int(round(t))
    h, m, s = t // 3600, (t % 3600) // 60, t % 60
    return f"{h}:{m:02}:{s:02}" if h else f"{m:02}:{s:02}"


def main():
    t = 0.0
    out = []
    for topic in TOPICS:
        end = t + topic["minutes"] * 60
        key = list(topic["key"])
        # Spread key lines over the topic: first one at the start, the others
        # after some discussion.
        lines = [key.pop(0)]
        n_disc = 0
        while True:
            if key and (n_disc >= 2 and random.random() < 0.4):
                lines.append(key.pop(0))
                n_disc = 0
                continue
            k = random.choice([1, 2, 2, 3])
            text = random.choice(FILLERS) + " ".join(random.sample(topic["pool"], k=min(k, len(topic["pool"]))))
            sp = random.choice(SPEAKERS)
            if lines and lines[-1][0] == sp:
                sp = random.choice([s for s in SPEAKERS if s != sp])
            lines.append((sp, text))
            n_disc += 1
            est = t + sum(words(x[1]) / 2.4 + 0.8 for x in lines)
            if est >= end and not key:
                break
        for sp, text in lines:
            if t >= end and topic is not TOPICS[-1]:
                pass
            out.append(f"[{fmt(t)}] {sp}: {text}")
            t += words(text) / 2.4 + 0.8
        t = max(t, end)
    print("# Synthetic ~60 min German quarterly planning meeting (gen_long_meeting.py).")
    for line in out:
        print(line)


if __name__ == "__main__":
    main()
