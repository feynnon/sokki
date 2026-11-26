# Live dictation on the phone: speed and accuracy

24–26 Nov 2026. Task (Robin, 24 Nov): live dictation on the Pixel is slow; get the first words sooner and make it feel live, like the Mac, without losing quality. Measured with `mobile/asr-bench` (`--stream`, `--via-mac`, details in its README) on 13 made-up clips (macOS voices: English, German, German with English words, one language switch; 383 words), on Robin's Pixel 7 (Tensor G2, CPU only) and his Mac (M1 Pro, Metal).

## Short version

| Setup | First words | Text behind speech | Done after stop | Word errors (383 words) | Commas, periods, words |
|---|---|---|---|---|---|
| Phone, Parakeet Ultra, two passes (until 25 Nov) | 2.9 s | freezes ~3 s at pauses | 3.1–3.6 s | (final = whole read, ≈ 22) | |
| Phone, Nemotron Streaming 3.5, lookahead 6 (now) | 1.8 s | longest wait 1.2 s | 0.2 s | 29 | 59 |
| Mac reads it: Parakeet Ultra, phone's audio over the link, direct | 0.66 s | 0.12 s median, 0.30 s worst | 0.16 s median, 0.52 s worst | 19 | 41 |
| same, through n0's relay (away from home) | 0.72 s | 0.16 s / 0.29 s | 0.20 s / 0.58 s | 19 | 41 |

- **Via the Mac is fastest and most accurate.** It is Parakeet Ultra on the GPU: live text re-read every 0.5 s, the whole dictation read once at the stop.
- **Its limit:** it needs the Mac awake with Sokki running. Otherwise the phone has to fall back to Nemotron.
- **Not yet measured:** the "via the Mac" rows ran with the Mac as the phone (loopback for direct; the relay row went through the internet for real). The Pixel was off on 26 Nov.
  - Phone to Mac over home Wi-Fi added 65 ms median and 372 ms worst per request (18 Nov), so expect about +0.1 s on Wi-Fi.
- **Parakeet Ultra can't stream** (`stream begin` refuses it). On the phone, re-reading the growing audio costs 0.8 s for 3 s of speech, 2.3 s for 10 s and 7.3 s for 30 s, so live text with Ultra on the phone stays choppy.
- **German is equal across models** (3 vs 3 errors). Nemotron loses on English (7 vs 5) and on German with English words (19 vs 14).

## Newer phones (CPU, Geekbench 6 multi-core, rough; numbers vary by source)

| Chip (phone) | Multi-core | vs Pixel 7 |
|---|---|---|
| Tensor G2 (Pixel 7) | 2 900–4 000 | 1× |
| Tensor G5 (Pixel 10) | ~6 100 | ~1.7× |
| Tensor G6 (Pixel 11) | ~6 800 | ~1.9× |
| A19 Pro (iPhone 17 Pro) | ~9 700 | ~2.8× |
| Snapdragon 8 Elite Gen 5 | ~11 400–12 400 | ~3.3× |

Speech models scale roughly with this. On the fastest Android phone, a Parakeet re-read of 10 s would still take about 0.7 s, against 0.18 s on the Mac. Nemotron would get the first words sooner there.

Sources:
- [nanoreview: Tensor G2](https://nanoreview.net/en/benchmark-ranking/google-tensor-g2)
- [Geekbench Browser: Pixel 7 Pro](https://browser.geekbench.com/android_devices/google-pixel-7-pro)
- [Beebom: Tensor G5](https://gadgets.beebom.com/guides/google-tensor-g5-benchmark-specs)
- [Beebom: Tensor G6 vs G5](https://gadgets.beebom.com/guides/google-tensor-g6-vs-tensor-g5-benchmark-specs)
- [Gizmochina: Snapdragon 8 Elite Gen 5 vs A19 Pro](https://www.gizmochina.com/2025/09/26/snapdragon-8-elite-gen-5-tops-apple-a19-pro-and-dimensity-9500-in-early-benchmarks/)
- [Wccftech: Snapdragon 8 Elite Gen 5 vs A19 Pro, multi-core](https://wccftech.com/snapdragon-8-elite-gen-5-beats-a19-pro-to-become-fastest-mobile-cpu-in-multi-core-workloads/)

## Why the phone was slow (fixed 25 Nov)

- Android dropped the idle keyboard's model from memory, so the first words waited for flash.
  - Fix: reload it (MADV_WILLNEED) plus a 0.5 s warm-up when the keyboard opens after 30 s idle. Ready after a kill: 1.0 s, was 17–22 s.
- Two passes used 8 threads each on 8 cores; the busy-waiting threads stalled each other.
  - Fix: one streaming pass, with as many threads as cores − 2, at most 4.
- One big feed is much slower than 200 ms feeds (2 s at once took 3.9 s), so the audio goes in block by block.
