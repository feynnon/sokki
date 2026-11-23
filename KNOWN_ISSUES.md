# Known issues

Every problem seen so far, with an example and an idea how to fix it.
Status: open unless marked.

## Speech recognition (Parakeet)

| # | Problem | Example (truth → transcript) | Fix idea |
|---|---|---|---|
| A1 | Domain words misheard, worst on phone-speaker audio | "Kosten pro Lead" → "Kosten pro Lied"; "Klickrate" → "Klipprate", "Glückgabe" | LLM correction pass with context and a glossary (D1) |
| A2 | Meaning changes, numbers wrong | "über unserem Ziel von fünfzehn Euro" → "über unseren 10 von 15 Euro" | LLM correction; flag low-confidence spans |
| A3 | Words merged or split | "bis Freitag darum" → "bis drei Tadladung"; "Einverstanden" → "Ein verstanden" | LLM correction |
| A4 | English loanwords in German | "Release" → "Liliase"; "live gegangen" → "lieber/liefer gegangen"; "Status Update" → "Status Optata"; "Planungsmeeting" → "Planungsmeting" | glossary of the user's terms + LLM correction |
| A5 | Dropped consonants on the remote voice | "die haben wir aber schon behoben" → "die abend wir aber schon beoben" | LLM correction |
| A6 | Parakeet has no vocabulary biasing (checked: neither v3 nor Ultra supports `Feature::Vocabulary`) | – | correction after ASR is the only lever, or a model that supports vocabulary |
| A7 | Test metric counts digit formatting as errors ("18" vs "achtzehn") | inflates WER by a few points | normalise numbers in the metric |

## Summary (Qwen 3.5 9B)

| # | Problem | Example | Fix idea |
|---|---|---|---|
| S1 | ASR errors flow into the summary and title | title "Statusupdate Optata und Liliase"; "Kosten pro Lied" | correct the transcript first (D1) |
| S2 | Wrong facts in action items | "Übersicht bis drei Tage senden" (truth: by Friday) | same, plus a fact check against the transcript |
| S3 | Wrong words | "bewirten" instead of "bewerben" | bigger model or a second pass |
| S4 | Missed facts | mixed call: "49 € per seat" missing | prompt tuning, evaluation set |
| S5 | Misattributed action items (seen once, fixed by a prompt rule) | agency confirmation credited to the wrong speaker | keep the live test |
| S6 | Ollama's MLX runner rejects structured output (HTTP 501) | JSON comes from the prompt only | retry logic exists; watch failure rate |
| S7 | Built-in model: the GGUF carries no sampling settings, so llama-server would use its own (temperature 0.8, top-k 40, min-p 0.05) | Ollama's model uses Qwen's: 1.0, top-k 20, top-p 0.95, presence penalty 1.5 | fixed 2026-11-20: llama-server starts with Qwen's values (`BuiltinModel::sampling`, Robin: "Nimm Qwens Werte"); requests still send none |

## Speakers

| # | Problem | Fix idea |
|---|---|---|
| P1 | "Who is the user": on an online call the mic voice (most speech on the mic track); on a single mic nobody is named. Two people in the room on one mic are not told apart from the user | enroll the user's own voice once; voice profile wins |
| P2 | Similar voices merge (synthetic `three_remote`: 3 → 2) | real-recording tests; `num_speakers` hint |
| P3 | Sortformer handles at most 4 speakers per track | fallback to sherpa clustering for larger meetings |
| P4 | Voice profiles tested only on synthetic voices; phone vs. clean voice similarity only 0.78 | test with real recordings |
| P5 | Renaming a real name ("Ich") only changes structured spots in the note, not running text | fine for now |

## Echo and dedup

| # | Problem | Fix idea |
|---|---|---|
| E1 | Acoustic echo filter tuned on one synthetic scenario; double-talk (both sides talking at once) untested | real laptop-call test without headphones |
| E2 | Text dedup tested only on synthetic text | real tests |

## Transcript during the call (2026-11-20)

| # | Problem | Example | Fix idea |
|---|---|---|---|
| L1 | Speakers found during the call are joined from windows, not the model's own streaming state; a voice silent in an overlap is matched by its embedding (cosine ≥ 0.6, unproven threshold) | three real calls: 99.2–100 % same speaker as today; per track the stitcher makes more short-lived speakers than today (call1402 mic: 4 vs 2), which ghost absorption folds back | the model's own streaming (a stream API in transcribe.cpp, or FluidAudio's streaming Sortformer on the Mac) |
| L2 | Fixed 2026-11-23: echo was cancelled twice (during the call and again after the stop) | 12 s per 41 min after the stop | the run takes the call's echo-free mic when it was lined up and filtered alike (`live::take_mic`) |
| L3 | Short back-channel turns inside a piece are sometimes left out | 41-min call: 9 of 229 lines ("Yeah.", "Mm-hmm.", "Um") missing | none yet; turn by turn they cost a transcriber call each |
| L4 | A piece across a change of speaker is not used: its turns are transcribed again after the stop. Calls with quick exchanges gain little | 41-min call: 51 of 231 turns; 13-min call: 46 of 73, so the simulated call was ready 39.1 s after the stop instead of 40.8 s. Shorter pieces reuse more but recognise worse (call1402's passages: 8–25 s 7.2 %, 4–12 s 10.3 %, 2–8 s 9.6 %; turn by turn 7.4 %) | fixed 2026-11-23 for all but the last minutes: those turns are transcribed during the call once their speakers are final (`pipeline::pending_slices`, `live::TextCache`). Shorter speaker windows (2 min) to shrink the rest were measured and rejected: 7.7 % word errors on call1402, one speaker lost on a 13-min call |
| L5 | Fixed 2026-11-23: system audio starting late (the tap waits for sound) was left unaligned when more than 5 s late, the echo filter ran against a reference 12 s off, and the live transcript was thrown away | in-person meeting 2026-11-23: system 12.1 s after the mic, 43 min transcribed again after the stop | now padded however late (`track_pads`), echo removed from the system's first sample, live tracks checked one by one. Still open: a system track that starts but stays under the silence threshold drops the live mic transcript |

## App

| # | Problem | Fix idea |
|---|---|---|
| X1 | The real recording flow in the app (mic + system tap → processing with the dictation engine → note) has not run yet | first real meeting |
| X2 | Diarization models download during the first meeting's processing, no progress shown; offline → no speakers (warning) | download on first start with progress |
| X3 | Quitting the app while a meeting is processing may crash at exit (ggml-metal assert) | wait for processing on quit |
| X4 | Same icon as the upstream app; both use ⌥Space for dictation | own icon; quit the upstream app |
| X5 | Meeting detection: several bundle IDs unverified; voice messages / browser voice search can trigger a prompt; uses a private API | real tests, cooldown |
| X6 | Apple Intelligence is a stub (Command Line Tools only) | install Xcode if wanted |
| X7 | Processing starts only after the meeting ends (21-min call: ready ~3 min after stopping) | live transcription measured and rejected, see F3 |
| X8 | **Fixed 2026-11-19:** keyboard and mouse froze Mac-wide (13:3x, forced restart; 15:02, 5 min). Onboarding ran `tccutil reset` before asking for a permission. A second copy (started from a shell, or a test copy with a fresh data folder) reads "not granted" and resets while the running Sokki holds an active keyboard event tap; revoking that grant hangs all input (Apple FB24619068, developer.apple.com/forums/thread/844416) | reset removed (onboarding only asks; a stale grant after a signature change is fixed by the user in System Settings). Still open: a listen-only tap when no shortcut needs blocking |
| X9 | **Partly fixed 2026-11-19:** an in-person meeting recorded in front of the laptop was cut off when the Mac went to idle sleep. Every recording now keeps the device awake: macOS a `PreventUserIdleSystemSleep` assertion (`src-tauri/src/meeting/keep_awake.rs`; the display may still turn off), Linux/Omarchy a logind sleep inhibitor (`crates/sokki/src/keep_awake.rs`; screen may still lock and turn off), Android keeps the screen on while recording (`MainActivity.kt`, `SokkiScreen`; with the screen off a background app records only silence). Still open: closing the lid sleeps a laptop and ends the capture (Mac: with no external display; only the admin-level `pmset disablesleep` prevents it; Linux: logind's `LidSwitchIgnoreInhibited=yes`); on Android, pressing the power button cuts the recording; Windows holds nothing | warn when the lid closes during a recording; Android: record in a foreground service; Windows: `PowerSetRequest` |

## Found in the first real call (2026-11-17)

| # | Problem | Status |
|---|---|---|
| R1 | System track 3x too short and unintelligible: with a Bluetooth headset in call mode the tap delivers 16 kHz but reports 48 kHz | **fixed**: the writer measures the real rate and resamples to 16 kHz (`meeting/rate.rs`) |
| R2 | Reinstalling/quitting the app during a recording lost it (`.partial` files, record "Interrupted") | **fixed**: recordings are finalized at quit and processed at the next launch. The 14:02 recording still needs manual recovery from its `.partial` files |
| R3 | `--toggle-meeting` stopped a recording that meeting detection had started (the CLI can't tell start from stop) | open: add `--start-meeting`/`--stop-meeting`; never toggle blindly in tests |
| R4 | Test audio played through the speakers during the user's call | process: never play audio without asking first |
| R6 | Dictation failed ("Failed to create SileroVad … init_backends_with: status 8") after a meeting was processed: Sortformer registered GPU backends in the app process | **fixed**: CPU-only registration before loading the diarizer (2.5 s per audio minute) |
| R7 | Ghost speakers: 2–7 words as "Speaker 4/5/6" in a 3-person call | **fixed**: tiny clusters are merged into the nearest speaker |
| R8 | ASR garbage on muffled call audio ("anoň Yeah", "Stavbe se Besser"); jargon misheard ("Kelasur" → "Colazur", "Claude Code" → "Cloud Code") | partly: LLM correction with glossary; garbage in other languages still open |
| R9 | Every install asked for all permissions again (ad-hoc signature changes per build) | **fixed**: builds are signed with the self-signed certificate "Sokki Dev" (`scripts/build-and-install.sh`); verified that permissions survive a reinstall |
| R10 | LLM correction on a real call made wrong, meaning-changing edits: "Nuxa" → "Nardum" (glossary name), "Whatever Tool" → "Claude Code", "schweiß" → "schreiben", syllables dropped; fixed only 1 of 6 "Cloud Code" | **fixed** by stricter rules (names used ≥3× are protected, glossary terms must sound alike, no shortening, confirmed glossary fixes applied everywhere); synthetic eval with glossary now 2 right / 0 wrong / 0 harmful. Watch on the next real calls |
| R11 | Garbage in other languages from muffled call audio ("anoň", "Stavbe se") is not removed | open. Findings: the garbage sits inside otherwise good lines, so dropping whole lines by language doesn't help; per-word ASR confidence is too weak a signal (good words 0.70–0.85, garbage 0.80); the same audio transcribed with more context comes out clean ("Ja." instead of "anoň Yeah"). Tried: transcribing 25 s windows across turns with word times (`PipelineConfig::window_s`): phone_speaker WER 17.6% → 15.3%, but de_en_mixed 3.8% → 31.2% (a window holding both languages comes out in one). Off by default. Next: windows of one speaker's nearby turns only |
| R13 | **Memory runaway, 51 GB**: propagating a confirmed glossary fix searched from the line start again after each replacement; when the fix contains the misheard words ("Account" → "OVDA-Account") it looped forever, growing the line and the corrections list | **fixed** (search continues after the replacement, regression test). Safety net: memory guard (`meeting/memguard.rs`) stops the app above 6 GB while a meeting processes and marks the meeting failed without retrying it |
| R14 | A meeting waiting for another one to finish showed "diarizing" for half an hour | **fixed**: shows "waiting" |
| R15 | Ghost "Speaker 4" with seven back-channel lines ("Yeah", "Yep"), over the 8 s ghost limit | **fixed**: clusters with ≤ 20 words and < 4 % share are merged too |
| R16 | Summaries mix up topics and owners on 20-min calls (qwen3.5:9b): a time or availability moved to another customer, a task already done listed as open, the wrong customer in a task, small talk from garbled audio stated as fact | partly: prompt rules (keep topics apart, done ≠ task, no small talk, decisions include "wait"). Still seen after the change. Measured and rejected: thinking mode (> 5 min per summary), summarising in 6–8-minute parts (more invented links). Next lever: a larger model or letting the user check tasks |
| R17 | The correction pass cost a whole model round before the summary; in a 41-min meeting on a loaded Mac (2026-11-20) it made 2 corrections, then the summary timed out | **removed** 2026-11-20: no correction pass; the glossary goes into the summary prompt, so names in the summary are spelled right. The transcript stays as recognised |
| R12 | Without a glossary the correction rarely helps (correction removed, see R17) | the glossary is the lever; keep it filled (`meetings.glossary`) |
| R5 | A meeting with only silence on the mic (user listening) — check that diarization/echo do not invent a "user" | to verify with the current recording |

## Measurements (2026-11-17, two real calls, 34 hand-labelled misheard words)

| # | What | Result |
|---|---|---|
| F1 | LLM correction: free "fix any error" vs dictionary-only with checks | free: 16 fixed, 6 correct words broken, 63 stray changes; dictionary-only: 17–18 fixed, 0 broken |
| F2 | Correction models (dictionary-only) | qwen3.5:9b 18 (134 s for both calls); gemma4:12b 16; gemma4:e4b 14 (91 s, 10 GB); qwen3:14b 12; gemma3:12b slow (69 min); qwen3.5:4b 8 + 2 wrong; ministral-3:3b 8; qwen3:4b-instruct 5; qwen3:4b (thinking-only) unusable |
| F3 | Live transcription (chunks cut at pauses, before speaker detection) | mixed DE/EN fixture WER 3.8 % → 10 % (10 s chunks), 16 % (15 s); would save only ~30 s; not built |
| F4 | Other speech models (pipeline unchanged) | Fixtures: Whisper Large v3 Turbo phone-on-speaker WER 12.9 % vs Parakeet 17.6–18.8 %, others equal. Real 22-min call (2026-11-18, six passages, 768 words, hand reference, `testdata/real/wer_passages.py`): Primeline 6.4 %, **Ultra 6.9 % (67 s)**, Whisper Turbo + dictionary prompt 8.5 % (269 s), Whisper Turbo auto 9.1 % (266 s), German forced 9.6 % (253 s, translates English phrases into German), Parakeet v3 10.5 %, Whisper Large v3 12.2 % (335 s). Whisper hears some words better (Fuß fassen, Show-Up, WingTsun) but adds hallucinations („Bis zum nächsten Mal“, „Vielen Dank“, „Codric City“). Keep Ultra. |
| F6 | Summary at the model's default temperature (2026-11-18, 14:02 call, qwen3.5:9b, Ultra and Primeline transcripts, two runs each) | Worse than at 0.2: fewer topics, more invented links (a 3 pm call moved to the wrong customer, a person attached to the wrong company, a prototype attached to the wrong topic, invented names). Qwen's own recommendation for non-thinking mode is 0.7, Ollama's model default 1.0. Kept at the default as decided; revisit if summaries disappoint. |
| F5 | Correction speed | sound pre-filter (45 % of lines sent, 22/23 dictionary errors kept): ~3× faster; batches of 16 + compact answers: faster but 13–14 instead of 18; temperature 0 ~14, 0.2 18, 0.7 9, model default (qwen3.5: 1.0) 15 and 13 (plus „Nixon“ → „Nuxa“ on the Ultra transcript, correct per Robin, not in the labels) and one unparsable answer (raw control character; now tolerated); one runaway answer at 0 (3072 tokens, 133 s) → output cap 400. Since 2026-11-18 no temperature is sent (Robin: always the model's default) |
| F6 | Speaker detection GPU vs CPU | identical turns (same speaker on 100 % of speech), 19 s vs 41 s per 21-min track; both tracks in parallel: no gain (36 vs 37 s) |
| F7 | Ollama memory (MLX runner) | grows to 13–16 GB during correction + summary; freed 1 min after (keep_alive) |
| F8 | Built-in llama.cpp vs Ollama (2026-11-20, M1 Pro 32 GB, the 41-min 14:20 call, ~8,800 prompt tokens, one request, both cold, two runs each, GPU otherwise idle) | built-in 92.6 s and 70.6 s (load 1.3 s; 7.4 s with a cold disk cache), peak 6.2 GB RSS, 0 GB right after; Ollama 86.6 s and 81.3 s, peak 5.8–6.5 GB RSS (`ollama ps`: 8.1 GB), 3.2–4.6 GB still held 2 s after. Built-in: prompt 138 tok/s, output 12 tok/s at that context; the JSON schema costs nothing. Another app on the GPU at the same time doubled the time |

## Videos (2026-11-20, first version)

- Real screen and camera capture, the camera bubble and the recording bar
  have not run on a Mac yet: they need Screen Recording and Camera
  permission, which no session may trigger. Rendering is tested on sample
  files.
- The recording bar has no microphone picker and no settings cog (system
  sound is always recorded, as in meetings); the look menu is a plain menu
  without the pictures from the design; the screen picker lists displays, not
  windows.
- The call-detected dialog is still macOS's own dialog: "Mit Bildschirm" is a
  third button, not a switch in a Sokki window as designed. Its text is German
  only, also in an English app.
- After Screen Recording is allowed, macOS asks to quit and reopen Sokki; the
  bar then says so, but there is no "welcome back" line yet.
- A recording cut by a quit is rendered at the next launch without its first
  frame time, so screen and camera may be slightly out of step with the sound.

## Planned

- **D1** LLM correction pass over the transcript (context window + glossary), measured against truth. Built, then removed on 2026-11-20 (R17).
- **D2** Live pipeline during the recording: utterance-wise ASR and correction, so the note is ready right after the call.
