#!/usr/bin/env python3
"""Generates synthetic meeting audio with ground truth, for the meeting tests.

Uses macOS `say` voices and ffmpeg. Output (gitignored) goes to testdata/audio/:

  <scenario>/mic.wav       16 kHz mono, the "me" track
  <scenario>/system.wav    16 kHz mono, the "them" track (only some scenarios)
  <scenario>/truth.json    turns with track, speaker, start_s, end_s, text, lang

Run:  python3 -I testdata/generate.py            (all scenarios)
      python3 -I testdata/generate.py de_two     (one scenario)

Scenarios:
  de_two         two German speakers alternating on the mic track
  de_en_mixed    German and English speakers on the mic track (Denglisch call)
  phone_speaker  one close speaker, one through a phone speaker (band-limited,
                 quieter, room echo, noise) on the same mic track
  call_tracks    a laptop call: me on mic, two remote speakers on system audio,
                 and the mic also picks up the remote voices from the laptop
                 speakers (echo), which cross-track dedup must remove
  three_remote   three speakers on the system track
  four_remote    four speakers on the system track, none on the mic (notes
                 to named speakers: two with an address, one without, one unnamed)
  silence        ten seconds of faint noise on the mic track
  demo_en        an English laptop call for demos: me on mic, two remote
                 speakers on system audio, speaker echo on the mic
"""
import json
import os
import subprocess
import sys
import tempfile
import wave

import numpy as np

SR = 16_000
ROOT = os.path.dirname(os.path.abspath(__file__))
OUT = os.path.join(ROOT, "audio")
RNG = np.random.default_rng(42)

ANNA = "Anna"
EDDY = "Eddy (German (Germany))"
FLO = "Flo (German (Germany))"
REED = "Reed (German (Germany))"
DANIEL = "Daniel"


def tts(voice: str, text: str) -> np.ndarray:
    with tempfile.TemporaryDirectory() as d:
        aiff = os.path.join(d, "t.aiff")
        raw = os.path.join(d, "t.raw")
        subprocess.run(["say", "-v", voice, "-o", aiff, text], check=True)
        subprocess.run(
            ["ffmpeg", "-loglevel", "error", "-y", "-i", aiff, "-ac", "1", "-ar", str(SR),
             "-f", "f32le", raw],
            check=True,
        )
        return np.fromfile(raw, dtype=np.float32)


def ffmpeg_filter(x: np.ndarray, af: str) -> np.ndarray:
    with tempfile.TemporaryDirectory() as d:
        src = os.path.join(d, "in.raw")
        dst = os.path.join(d, "out.raw")
        x.astype(np.float32).tofile(src)
        subprocess.run(
            ["ffmpeg", "-loglevel", "error", "-y", "-f", "f32le", "-ar", str(SR), "-ac", "1",
             "-i", src, "-af", af, "-f", "f32le", "-ar", str(SR), "-ac", "1", dst],
            check=True,
        )
        y = np.fromfile(dst, dtype=np.float32)
    return y[: len(x)] if len(y) >= len(x) else np.pad(y, (0, len(x) - len(y)))


def phone(x: np.ndarray) -> np.ndarray:
    """Phone line + small loudspeaker + room: band-limit, compress, echo, quieter."""
    y = ffmpeg_filter(x, "highpass=f=300,lowpass=f=3400,acompressor=threshold=-20dB:ratio=4,"
                         "aecho=0.6:0.4:35|70:0.25|0.12")
    return 0.45 * y


def write_wav(path: str, x: np.ndarray) -> None:
    x = np.clip(x, -1.0, 1.0)
    pcm = (x * 32767.0).astype("<i2")
    with wave.open(path, "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(SR)
        w.writeframes(pcm.tobytes())


def place(turns, gap_s=0.7, lead_s=0.5):
    """turns: list of (track, speaker, voice, lang, text, fx). Lays them out
    one after another with pauses; returns per-track arrays and truth."""
    clips, t = [], lead_s
    for track, speaker, voice, lang, text, fx in turns:
        audio = tts(voice, text)
        clean = audio.copy()
        if fx is not None:
            audio = fx(audio)
        clips.append((track, speaker, lang, text, t, audio, clean))
        t += len(audio) / SR + gap_s
    total = int((t + lead_s) * SR)
    tracks = {}
    truth = []
    for track, speaker, lang, text, start, audio, clean in clips:
        buf = tracks.setdefault(track, np.zeros(total, dtype=np.float32))
        i = int(start * SR)
        buf[i:i + len(audio)] += audio
        truth.append({"track": track, "speaker": speaker, "lang": lang, "text": text,
                      "start_s": round(start, 3), "end_s": round(start + len(audio) / SR, 3)})
    return tracks, truth, total


def add_noise(x: np.ndarray, level=0.002) -> np.ndarray:
    return x + RNG.normal(0, level, len(x)).astype(np.float32)


def save(name, tracks, truth, total, extra=None):
    d = os.path.join(OUT, name)
    os.makedirs(d, exist_ok=True)
    for track in ("mic", "system"):
        p = os.path.join(d, f"{track}.wav")
        if os.path.exists(p):
            os.remove(p)
    for track, buf in tracks.items():
        write_wav(os.path.join(d, f"{track}.wav"), add_noise(buf))
    meta = {"scenario": name, "duration_s": round(total / SR, 3),
            "speakers": sorted({t["speaker"] for t in truth}), "turns": truth}
    if extra:
        meta.update(extra)
    with open(os.path.join(d, "truth.json"), "w") as f:
        json.dump(meta, f, ensure_ascii=False, indent=2)
    print(f"{name}: {meta['duration_s']}s, speakers={meta['speakers']}, "
          f"tracks={sorted(tracks)}")


DE_DIALOG = [
    (ANNA, "Hallo zusammen, danke dass ihr Zeit habt. Wir wollen heute den Launch der neuen Kampagne besprechen."),
    (EDDY, "Gerne. Ich habe die Zahlen von letzter Woche mitgebracht. Die Klickrate lag bei zwei Komma drei Prozent."),
    (ANNA, "Das ist besser als erwartet. Was ist mit den Kosten pro Lead?"),
    (EDDY, "Die liegen bei achtzehn Euro, also etwas über unserem Ziel von fünfzehn Euro."),
    (ANNA, "Okay, dann schlage ich vor, dass wir das Budget für die schwachen Anzeigen kürzen."),
    (EDDY, "Einverstanden. Ich kümmere mich bis Freitag darum und schicke dir dann eine Übersicht."),
]


def scenario_de_two():
    names = {ANNA: "A", EDDY: "B"}
    turns = [("mic", names[v], v, "de", t, None) for v, t in DE_DIALOG]
    save("de_two", *place(turns))


def scenario_de_en_mixed():
    turns = [
        ("mic", "A", ANNA, "de", "Hi Daniel, schön dass du dabei bist. Können wir kurz über das Onboarding sprechen?", None),
        ("mic", "B", DANIEL, "en", "Sure, happy to. We finished the new onboarding flow last week and the first results look promising.", None),
        ("mic", "A", ANNA, "de", "Super. Wie viele Kunden haben den Flow schon komplett durchlaufen?", None),
        ("mic", "B", DANIEL, "en", "About forty customers so far. Most of them completed it in under ten minutes.", None),
        ("mic", "A", ANNA, "de", "Sehr gut. Dann lass uns nächste Woche ein Review mit dem Sales Team machen.", None),
        ("mic", "B", DANIEL, "en", "Sounds good. I will send out an invite for Tuesday afternoon.", None),
    ]
    save("de_en_mixed", *place(turns))


def scenario_phone_speaker():
    turns = []
    for v, t in DE_DIALOG:
        if v == ANNA:
            turns.append(("mic", "A", ANNA, "de", t, None))
        else:
            turns.append(("mic", "B", REED, "de", t, phone))
    save("phone_speaker", *place(turns), extra={"note": "speaker B is on a phone speaker next to the laptop"})


def scenario_call_tracks():
    turns = [
        ("mic", "Me", ANNA, "de", "Guten Morgen, könnt ihr mich gut hören?", None),
        ("system", "R1", EDDY, "de", "Ja, wir hören dich gut. Lass uns direkt mit dem Status Update starten.", None),
        ("system", "R2", FLO, "de", "Von meiner Seite gibt es gute Nachrichten. Das Release ist gestern live gegangen.", None),
        ("mic", "Me", ANNA, "de", "Sehr schön. Gab es Probleme beim Deployment?", None),
        ("system", "R1", EDDY, "de", "Nur kleinere Dinge, die haben wir aber schon behoben.", None),
        ("mic", "Me", ANNA, "de", "Perfekt. Dann machen wir nächste Woche mit dem Monitoring weiter.", None),
    ]
    tracks, truth, total = place(turns)
    # The mic hears the laptop speakers: a quieter, delayed, filtered copy.
    delay = int(0.04 * SR)
    leak = np.zeros_like(tracks["system"])
    leak[delay:] = tracks["system"][:-delay]
    leak = 0.12 * ffmpeg_filter(leak, "lowpass=f=6000,aecho=0.5:0.3:20:0.2")
    tracks["mic"] = tracks["mic"] + leak
    save("call_tracks", tracks, truth, total,
         extra={"note": "mic contains an echo of the system track (laptop speakers)"})


def scenario_demo_en():
    me, sam, reed = "Samantha", "Daniel", "Reed (English (US))"
    turns = [
        ("mic", "Me", me, "en", "Morning, everyone. Quick one today: are we ready to ship the Linux version on Friday?", None),
        ("system", "R1", sam, "en", "Almost. Recording and call detection work on Omarchy. The installer still needs testing on a real laptop.", None),
        ("system", "R2", reed, "en", "I can do that tomorrow on my Framework. If it passes, Friday works for me.", None),
        ("mic", "Me", me, "en", "Great. Then let's decide: we ship on Friday if the laptop test passes.", None),
        ("system", "R1", sam, "en", "Agreed. I'll write the release notes and record a short demo video.", None),
        ("mic", "Me", me, "en", "Perfect. I'll send the demo to D H H once it's out. Thanks, both of you.", None),
    ]
    tracks, truth, total = place(turns)
    delay = int(0.04 * SR)
    leak = np.zeros_like(tracks["system"])
    leak[delay:] = tracks["system"][:-delay]
    leak = 0.12 * ffmpeg_filter(leak, "lowpass=f=6000,aecho=0.5:0.3:20:0.2")
    # The voice alone too, for live tests that add their own speaker echo.
    tracks["mic_dry"] = tracks["mic"].copy()
    tracks["mic"] = tracks["mic"] + leak
    save("demo_en", tracks, truth, total,
         extra={"note": "demo call; mic contains an echo of the system track"})


def scenario_three_remote():
    turns = [
        ("system", "R1", EDDY, "de", "Willkommen zum wöchentlichen Planungsmeeting.", None),
        ("system", "R2", FLO, "de", "Ich würde gern mit dem Thema Recruiting beginnen.", None),
        ("system", "R3", REED, "de", "Wir haben diese Woche drei Bewerbungsgespräche geführt.", None),
        ("system", "R1", EDDY, "de", "Wie war euer Eindruck von den Kandidaten?", None),
        ("system", "R3", REED, "de", "Zwei davon waren sehr stark, wir sollten beide einladen.", None),
        ("system", "R2", FLO, "de", "Einverstanden, ich organisiere die zweiten Gespräche.", None),
    ]
    save("three_remote", *place(turns))


def scenario_four_remote():
    anna, sandy = ANNA, "Sandy (German (Germany))"
    turns = [
        ("system", "R1", EDDY, "de", "Willkommen zum Planungsmeeting, wir haben heute vier Themen.", None),
        ("system", "R2", anna, "de", "Ich fange mit dem Budget an, wir liegen gut im Plan.", None),
        ("system", "R3", REED, "de", "Beim Recruiting haben wir zwei starke Kandidaten gefunden.", None),
        ("system", "R4", sandy, "de", "Ich kümmere mich bis Freitag um die neue Website.", None),
        ("system", "R1", EDDY, "de", "Dann laden wir beide Kandidaten zum zweiten Gespräch ein.", None),
        ("system", "R2", anna, "de", "Einverstanden, ich schicke die Einladungen morgen raus.", None),
        ("system", "R3", REED, "de", "Ich bereite die Fragen für die Gespräche vor.", None),
        ("system", "R4", sandy, "de", "Und ich zeige euch nächste Woche den ersten Entwurf.", None),
    ]
    tracks, truth, total = place(turns)
    tracks["mic"] = np.zeros(total, dtype=np.float32)
    save("four_remote", tracks, truth, total)


def scenario_silence():
    total = 10 * SR
    save("silence", {"mic": np.zeros(total, dtype=np.float32)}, [], total)


SCENARIOS = {
    "de_two": scenario_de_two,
    "de_en_mixed": scenario_de_en_mixed,
    "phone_speaker": scenario_phone_speaker,
    "call_tracks": scenario_call_tracks,
    "demo_en": scenario_demo_en,
    "three_remote": scenario_three_remote,
    "four_remote": scenario_four_remote,
    "silence": scenario_silence,
}

if __name__ == "__main__":
    wanted = sys.argv[1:] or list(SCENARIOS)
    for name in wanted:
        SCENARIOS[name]()
