# Live dictation on the phone: speed and accuracy

24–26 Nov 2026. Task (Robin, 24 Nov): live dictation on the Pixel is slow; get the first words sooner and make it feel live, like the Mac, without losing quality. Measured with `mobile/asr-bench` (`--stream`, `--via-mac`, details in its README) on 13 made-up clips (macOS voices: English, German, German with English words, one language switch; 383 words), on Robin's Pixel 7 (Tensor G2, CPU only) and his Mac (M1 Pro, Metal).

## Short version

| Setup | First words | Text behind speech | Done after stop | Word errors (383 words) | Commas, periods, words |
|---|---|---|---|---|---|
| Phone, Parakeet Ultra, two passes (until 25 Nov) | 2.9 s | freezes ~3 s at pauses | 3.1–3.6 s | (final = whole read, ≈ 22) | |
| Phone, Nemotron Streaming 3.5, lookahead 6 (now) | 1.8 s | longest wait 1.2 s | 0.2 s | 29 | 59 |
| Mac reads it: Parakeet Ultra, phone's audio over the link, direct | 0.66 s | 0.12 s median, 0.30 s worst | 0.16 s median, 0.52 s worst | 19 | 41 |
| same, through n0's relay (away from home) | 0.72 s | 0.16 s / 0.29 s | 0.20 s / 0.58 s | 19 | 41 |

- **Via the Mac is fastest and most accurate**, and on Robin's own voice the gap is much bigger than on the test clips: 10.3 % against 17.0 % of words (next section). It is Parakeet Ultra on the GPU: live text re-read every 0.5 s, the whole dictation read once at the stop.
- **Its limit:** it needs the Mac awake with Sokki running. Otherwise the phone has to fall back to Nemotron.
- **Not yet measured:** the "via the Mac" rows ran with the Mac as the phone (loopback for direct; the relay row went through the internet for real). The Pixel was off on 26 Nov.
  - Phone to Mac over home Wi-Fi added 65 ms median and 372 ms worst per request (18 Nov), so expect about +0.1 s on Wi-Fi.
- **Parakeet Ultra can't stream** (`stream begin` refuses it). On the phone, re-reading the growing audio costs 0.8 s for 3 s of speech, 2.3 s for 10 s and 7.3 s for 30 s, so live text with Ultra on the phone stays choppy.
- **German is equal across models** (3 vs 3 errors). Nemotron loses on English (7 vs 5) and on German with English words (19 vs 14).

## On Robin's own voice (26 Nov, 34 Mac dictations)

- **Data:** 34 Mac dictations, 16 min, 2,570 words (the set from `live-vs-final-2026-11.md`).
  - All German with English terms; none English-only.
  - Recordings and texts stay in the private archive (`Sokki eval data/phone-nemotron-vs-parakeet`).
- **References:** Whisper large-v3 and Canary 1B v2, each on the whole recording. They differ from each other in 11.6 % of words.
- **Counting:** fillers removed. Formatting is evened out: spelled-out letters are joined, and number words and digits are left out.

| Words different from | Parakeet Ultra (via the Mac) | Nemotron, lookahead 6 (phone) | Nemotron, lookahead 13 |
|---|---|---|---|
| Whisper large-v3 | 10.3 % | 17.0 % | 16.8 % |
| Canary 1B v2 | 10.9 % | 17.9 % | 18.3 % |
| Whisper, formatting not evened out | 10.2 % | 17.9 % | 17.3 % |

- **Per dictation:** Parakeet is closer to Whisper in 30 of 34 dictations; Nemotron in 1; 3 are the same.
- **Where the two models differ (297 places):** both references side with Parakeet in 183 and with Nemotron in 14.
- **What Nemotron misses most:** English terms in German sentences, and short German words (ein/eine, das/dass).
- **Speed through the Mac on these dictations** (Mac as the phone, loopback):
  - first words 0.64 s (median);
  - text 0.12 s behind the speech;
  - done 0.36 s after the stop (median), 4.2 s at most. That maximum is one 64 s dictation read in one go; the app reads it in 30 s pieces.
- **Decision (Robin, 26 Nov):** dictate through the Mac when it is reachable, Nemotron on the phone otherwise.
- **Report:** https://claude.ai/artifact/SCNqgpXwnNGHXZJQZ7DQWD

## In the apps (26 Nov, Sokki Test Phone on the Mac)

The Mac app serves the dictation (`meeting/phone_link.rs`), and `crates/sokki-link/examples/dictate` plays the phone on the same Mac, so no emulator competes for the CPU and GPU.

| Clip | First words | Behind speech (median / worst) | Done after stop |
|---|---|---|---|
| English 15 s | 1.27 s | 0.19 / 0.32 s | 0.30–1.53 s |
| German 28 s | 0.69 s | 0.17 / 0.34 s | 0.79 s |
| Mixed 9 s / switch 12 s | 0.66 s | 0.16 / 0.28–0.44 s | 0.35–0.45 s |
| Robin, 65 s / 30 s | 1.28 / 0.66 s | 0.15–0.16 / 0.27 s | 1.54 / 0.53 s |

- **The Mac's reads:** live text every 0.5 s (the growing piece read again); the final read takes 0.16–1.5 s.
- **First words** come when the first ½ s of speech has been read; clips that start with a pause take longer.
- **On the emulator (4 cores):**
  - Through the Mac: first words 0.5–1.7 s.
  - The Mac frozen mid-dictation: the phone took over 2.0 s later and finished the text from the audio so far.
  - The Mac quit: the link was gone; the phone dictated by itself.
  - Relay only: first words 0.7–1.3 s.
  - First dictation on a brand-new link: see `KNOWN_ISSUES.md` K12.

## Parakeet on the phone, faster (26 Nov, the fallback without a Mac)

Robin (26 Nov): without the Mac, "using the local parakeet, and looking to making that as fast as possible", especially the first words.

**Speed by model and threads** (Pixel 7, `asr-bench --dictation`, reading the first N s, median of 3):

| Model | Threads | 1 s | 3 s | 10 s | 30 s |
|---|---|---|---|---|---|
| Q8_0 | 4 | 0.31 | 0.64 | 2.09 | 6.71 |
| Q8_0 | 6 | 0.28 | 0.82 | 2.82 | 9.09 |
| Q5_K_M | 4 | 0.40 | 0.90 | 3.00 | 9.45 |
| Q4_K_M | 4 | 0.34 | 0.75 | 2.45 | 7.73 |
| Q4_K_M | 6 / 8 | 0.39 | 1.05 / 1.00 | 3.19 / 3.16 | 9.80 / 9.58 |

- Q8 with 4 threads is fastest: the phone's CPU has fast paths for 8-bit, and more threads only wait for each other.
- Accuracy of the smaller models on Robin's 34 dictations (20 s pieces) is the same:

| | Q8_0 | Q5_K_M | Q4_K_M |
|---|---|---|---|
| Whisper | 11.2 % | 11.1 % | 11.1 % |
| Canary | 11.7 % | 12.0 % | 11.9 % |

**Live in pieces** (`asr-bench --pieces`, `mobile/src-tauri/src/pieces.rs`): the growing piece is read every 0.5 s, and a piece ends at a pause after min s (at most max s) and is kept.

| Pieces (min–max) | Words different, Robin's 34 (Whisper / Canary) | First words | Text behind speech (median, worst at the median clip) | Done after stop (median / worst) |
|---|---|---|---|---|
| 4–10 s | 12.0 / 12.2 % | | | |
| 6–15 s (shipped) | 12.0 / 12.2 % | 1.38 s | 0.97 s, 2.4 s | 2.4 / 4.5 s |
| 8–20 s | 10.5 / 11.6 % | 1.43 s | 1.19 s, 3.5 s | 3.5 / 7.3 s |
| 10–25 s | 11.0 / 12.2 % | | | |
| whole dictation (Mac) | 10.3 / 10.9 % | | | |
| Nemotron, lookahead 6 | 17.0 / 17.9 % | 1.45 s | 0.24 s, 0.5 s | 0.29 / 0.44 s |

- Speed: 13 test clips on the Pixel 7, Q8, 4 threads.
- Test clips against their scripts: Parakeet 6.7 % of words, Nemotron 9.0 %.
- **Decision (Robin's rule, quality before a bit of speed):** Parakeet Ultra Q8 in 6–15 s pieces replaces Nemotron on the phone.
- **Emulator check (4 cores, 2 threads; much slower than the Pixel):** first words 1.5–1.9 s, done 5.5–7 s after the stop. The text came out complete, and no word typed for good changed afterwards.

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
