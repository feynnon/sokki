"""Builds Sokki's sample meeting from script.json.

An invented call between three invented people, spoken by Kokoro-82M
(Apache-2.0, https://huggingface.co/hexgrad/Kokoro-82M). macOS's own voices
can't be used: Apple's licence allows their output only for personal,
non-commercial use, and this file ships inside the app.

Output:
  src-tauri/resources/sample-meeting/call.flac  16 kHz mono, the call side
  testdata/sample_meeting/truth.json            who says what, when

Run (needs ffmpeg and Homebrew's espeak-ng; Python 3.12 with kokoro,
soundfile and spaCy's en_core_web_sm in a virtual environment):
  .venv/bin/python -I testdata/sample_meeting/build.py
"""

import json
import pathlib
import random
import subprocess
import sys
import tempfile

import espeakng_loader
import numpy as np
import soundfile as sf

# Words Kokoro's lexicon lacks (e.g. "Maya") go to espeak-ng; the wheel's
# own data path points at its build machine, so use Homebrew's.
espeakng_loader.get_data_path = lambda: "/opt/homebrew/share/espeak-ng-data"
espeakng_loader.get_library_path = lambda: "/opt/homebrew/lib/libespeak-ng.dylib"

from kokoro import KPipeline  # noqa: E402  (after the espeak paths)

HERE = pathlib.Path(__file__).resolve().parent
ROOT = HERE.parent.parent
OUT = ROOT / "src-tauri" / "resources" / "sample-meeting" / "call.flac"
RATE = 24_000  # Kokoro's rate

script = json.loads((HERE / "script.json").read_text())
# The same pauses every build.
rng = random.Random(7)
pipe = KPipeline(lang_code="a")  # American English; bf_emma is British but reads fine

pieces: list[np.ndarray] = []
truth = []
t = 0.6
pieces.append(np.zeros(int(0.6 * RATE), dtype=np.float32))
for speaker, text in script["lines"]:
    voice = script["voices"][speaker]
    audio = np.concatenate(
        [np.asarray(a, dtype=np.float32) for _, _, a in pipe(text, voice=voice)]
    )
    truth.append(
        {"speaker": speaker, "start_s": round(t, 2), "end_s": round(t + len(audio) / RATE, 2), "text": text}
    )
    pieces.append(audio)
    t += len(audio) / RATE
    gap = rng.uniform(0.35, 0.8)
    pieces.append(np.zeros(int(gap * RATE), dtype=np.float32))
    t += gap

call = np.concatenate(pieces)
call = 0.6 * call / max(1e-6, float(np.abs(call).max()))
OUT.parent.mkdir(parents=True, exist_ok=True)
with tempfile.TemporaryDirectory() as tmp:
    wav = pathlib.Path(tmp) / "call.wav"
    sf.write(wav, call, RATE)
    subprocess.run(
        ["ffmpeg", "-y", "-loglevel", "error", "-i", str(wav), "-ar", "16000", "-ac", "1",
         "-sample_fmt", "s16", str(OUT)],
        check=True,
    )
(HERE / "truth.json").write_text(json.dumps({"lines": truth}, indent=1, ensure_ascii=False) + "\n")
print(f"{OUT.relative_to(ROOT)}: {len(call) / RATE:.1f} s, {OUT.stat().st_size / 1e6:.2f} MB", file=sys.stderr)
