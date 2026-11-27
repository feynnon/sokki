#!/usr/bin/env python3
"""Made-up keyboard sound: a WAV of someone typing, nobody speaking.

Each key press is a short noise burst with a resonance (key down, then a
softer key up 80 ms later), at a random gap around `rate` keys per second,
over faint room noise. Seeded, so the same arguments give the same file.
Used by scripts/test-typing-no-meeting.sh (no meeting may start from it) and
mirrored by `typing_sound` in src-tauri/src/meeting/auto.rs.

    scripts/make-typing-wav.py OUT.wav [seconds=90] [rate=8] [level=0.3] [seed=7]
"""
import math
import struct
import sys
import wave

RATE = 16000


class Lcg:
    def __init__(self, seed):
        self.s = seed

    def next(self):
        """Uniform in [-1, 1)."""
        self.s = (self.s * 6364136223846793005 + 1442695040888963407) % (1 << 64)
        return ((self.s >> 40) / (1 << 24)) * 2.0 - 1.0


def typing(secs, rate, level, seed):
    n = int(secs * RATE)
    r = Lcg(seed)
    out = [r.next() * 0.002 for _ in range(n)]
    t = 0.0
    while t < secs:
        t += 1.0 / rate * (0.5 + (r.next() + 1.0) * 0.5)
        start = int(t * RATE)
        for off, amp in ((0, 1.0), (1280, 0.5)):
            f = 1800.0 + 1200.0 * (r.next() + 1.0)
            for i in range(800):
                k = start + off + i
                if k >= n:
                    break
                env = math.exp(-i / 120.0)
                s = 0.6 * r.next() + 0.4 * math.sin(2 * math.pi * f * i / RATE)
                out[k] += level * amp * env * s
    return [max(-1.0, min(1.0, x)) for x in out]


def main():
    if len(sys.argv) < 2:
        sys.exit(__doc__)
    path = sys.argv[1]
    secs, rate, level, seed = (float(sys.argv[2]) if len(sys.argv) > 2 else 90.0,
                               float(sys.argv[3]) if len(sys.argv) > 3 else 8.0,
                               float(sys.argv[4]) if len(sys.argv) > 4 else 0.3,
                               int(sys.argv[5]) if len(sys.argv) > 5 else 7)
    samples = typing(secs, rate, level, seed)
    with wave.open(path, "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(RATE)
        w.writeframes(b"".join(struct.pack("<h", int(x * 32767)) for x in samples))


if __name__ == "__main__":
    main()
