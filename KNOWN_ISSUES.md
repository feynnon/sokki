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

## Dictionary and "Keep what I write" (2026-11-24)

| # | Problem | Example | Fix idea |
|---|---|---|---|
| V1 | The sound-and-spelling swap overwrites normal words with short dictionary words | a 4-letter word turned "mit" into "Meta" 28 times in 36 real dictations; a short first name replaced "Mac" | fixed: Vocabulary and learned words swap only from 6 letters (`swap_words`) |
| V2 | The swap let a correct word swallow its neighbour | "to Kubernetes" → "Kubernetes" | fixed in `apply_custom_words` |
| V3 | Far-off mishearings are out of the swap's reach | "Vercel" → "Fassel", "OVDA" → "Off-Dar", "Quandtmüller" (ü: the swap is ASCII only) | "heard as" pairs from the hear test and from the user's own fixes (`learn_words.rs`) |
| V4 | The hear test uses the Mac's voices, not the user's | a pair can be missing or not match the user's accent | the user's own fixes after a dictation confirm or add pairs |
| V5 | Chrome and Electron fields not tested yet with "Keep what I write" | Electron shows its fields after `AXManualAccessibility`; Chrome's switch (`AXEnhancedUserInterface`) upsets window managers and is never set | test in Slack, VS Code, Chrome on a real Mac session |
| V6 | Parakeet takes no word list (A6), but a CTC word spotter could add one | FluidAudio: Parakeet CTC 110M next to TDT finds 85 % of listed words, 99 % precision (Earnings22) | check whether transcribe-cpp can run it |
| V8 | "Typed" is judged by key count vs. text growth (at most one character per key press between reads) | macOS autocorrect replacing a typed word, or a word finished from a suggestion, counts as not typed; so does text typed in a burst faster than the reads (40–80 ms) only if it outruns the key count | conservative on purpose: such words are never learned. The phone keyboard can mark the source of each character exactly |
| V7 | Measurements are small | 14 TTS sentences, 30 terms, 36 real dictations | re-measure on real dictations once the journal has pairs |

## Summary (built-in model: Gemma 4 E4B since 25.11.2026; S1–S8 seen with Qwen 3.5 9B)

| # | Problem | Example | Fix idea |
|---|---|---|---|
| S1 | ASR errors flow into the summary and title | title "Statusupdate Optata und Liliase"; "Kosten pro Lied" | correct the transcript first (D1) |
| S2 | Wrong facts in action items | "Übersicht bis drei Tage senden" (truth: by Friday) | same, plus a fact check against the transcript |
| S3 | Wrong words | "bewirten" instead of "bewerben" | bigger model or a second pass |
| S4 | Missed facts | mixed call: "49 € per seat" missing | prompt tuning, evaluation set |
| S5 | Misattributed action items (seen once, fixed by a prompt rule) | agency confirmation credited to the wrong speaker | keep the live test |
| S6 | Ollama's MLX runner rejects structured output (HTTP 501) | JSON comes from the prompt only | retry logic exists; watch failure rate |
| S7 | Built-in model: the GGUF carries no sampling settings, so llama-server would use its own (temperature 0.8, top-k 40, min-p 0.05) | Ollama's model uses Qwen's: 1.0, top-k 20, top-p 0.95, presence penalty 1.5 | fixed 2026-11-20: llama-server starts with Qwen's values (`BuiltinModel::sampling`, Robin: "Nimm Qwens Werte"); requests still send none; since 25.11.2026 Gemma 4's card values (1.0, top-p 0.95, top-k 64) and min-p 0 |
| S8 | Speaker names in the summary vary from run to run: the same sample meeting (`resources/sample-meeting/`), where two of three people are named in the call, gave "Daniel" and "Priya" in one run and only "Speaker 1/2/3" in the next (2026-11-24) | the summary model at its own default temperature | renaming speakers in the app fixes one meeting; none yet in the prompt |
| S9 | Gemma 4 E4B pads the decisions list with points that were only discussed (25.11. test, 4 real meetings) | "was discussed", "is promising" listed as decisions | a prompt rule once a real note shows it in Sokki |
| S10 | Gemma 4 E4B once wrote a placeholder instead of a name | "[the person being referred to]" | a prompt rule, or drop such items |
| S11 | Gemma 4 E4B still gives details to the wrong side (twice in one meeting) | who handles leads; whose call funnel it is | correct the transcript first (D1) |
| S12 | Not yet tested with Gemma: a meeting long enough to be summarised in parts, a second meeting set, and summaries with the MTP draft model (tested in dictation only, where quality was the same) | – | the next long real meeting |
| S13 | The summary model loads during a meeting only with memory to spare: at least a quarter of the Mac's memory must stay free after its 5.6 GB, so an 8 GB Mac never, a 16 GB Mac only with 9.6 GB free; otherwise it loads for the summary as before (summary step 11.9–14.9 s instead of 8.4–9.5 s in the sample meeting) | 26.11.: loading this model raises macOS's memory warning whether during a recording (4 of 7 test meetings, free memory at least 34 %) or for the summary (2 of 2 measured); during a recording the warning can stay while the model waits, with half of memory free; so it is stopped only at critical pressure, below 15 % free, or at a warning below 25 % free | – |
| S14 | Reading the transcript, not loading the model, is most of a long summary's wait: 3.6 s for 1,000 words, 11.8 s for 3,000, 30.8 s for 6,000, 52 s for 9,000 (M1 Pro, 26.11.). Doing it during the meeting would need the prompt to start with the transcript as it was heard (speaker names and the speaker list come only after the stop) and GPU work during the call | – | summarise parts of a long meeting while it records, after a test meeting under that load shows no gaps in the call audio |

## Dictation cleanup (Gemma 4 E4B, 2026-11-24)

Test set: 15 of Robin's dictations with his own ideal corrections (texts private, not in this repo); numbers in `reports/Dictation formatting with Qwen.md` (local).

| # | Problem | Example | Fix idea |
|---|---|---|---|
| C1 | Meaning errors remain: 13–14 per 15 dictations by two blind raters (changed, dropped or added content) | a dropped "not", a dropped priority clause, dropped closing requests | a check that every content word of the transcript survives; a bigger model once it fits |
| C2 | A list heading was written twice where two live pieces meet (1 of 15) | heading final, its list still open, the next step began with the heading again | fixed: a block equal to the previous block's first line is dropped (`cleanup::Live::step`) |
| C3 | When the final transcript does not start with what was cleaned live, everything was cleaned again at the stop | a 480-word dictation (26.11.): one early change, all of it again at the stop, the 20 s limit hit, raw pasted 23 s after the stop | fixed 26.11.: the cleanup goes back to the step before the first changed word and goes on from there, live and at the stop (`Live::roll_back`) |
| C4 | Live cleanup needs live text: with the overlay off, the decoded chunks are not handed on, so the whole dictation is cleaned at the stop | – | hand decoded chunks to the cleanup without the preview |
| C5 | Memory next to meetings: the cleanup model (4.5–5 GB) next to the summary model (then the 9B) would be ~11 GB for Sokki | – | fixed 25.11.: it stops 2 min after the last dictation, stops when a meeting starts or is processed or a local summary model opens, and does not start then (those dictations are pasted uncleaned) |
| C9 | On by default since 26.11.2026 (Robin), also turned on once for existing installs: the cleanup changes every dictated text and adds about 2 s before the paste, and its meaning errors (C1) are real. Without the model (8 GB Macs, or not downloaded yet) nothing changes: the transcript is pasted as before | – | watch for complaints about changed meaning; the switch is in Settings › Dictation |
| C6 | A failed or slow cleanup shows Post Process's notice ("Pasted Without Post-Processing") | – | its own notice text |
| C7 | Not measured on the phone; Android does not clean dictations yet | – | out of scope for the Mac build |
| C8 | Transcribing a History entry again gives the transcript uncleaned (the retry runs no cleanup) | – | run the cleanup in the retry job too |
| C10 | Short dictations get no live cleanup: a live piece needs 30 words of settled sentences (60 until 26.11.), so in 34 of Robin's dictations 16 showed cleaned text while he spoke (8 with 60 words); the others are cleaned once at the stop (median 1.7 s up to 30 words, 2.1 s up to 60) | – | smaller pieces clean more live but go back more often when the live text changes (31 times instead of 5 in the replay; 20-word pieces: 21 of 34 dictations, 37 times) |
| C11 | The live panel showed about three lines, so cleaned paragraphs scrolled out of view while the newest raw words showed | – | fixed 26.11.: with the cleanup on, the panel is six lines tall (400 × 184) and the raw words follow the cleaned text on the same line. Still, the raw words after it are a median 40 words (p90 77) because the sentence being spoken is never cleaned, so the end of the cleaned text is in view in about 58 % of panel updates (`docs/research/live-pieces-and-summary-preload-2026-11.md`) |
| C12 | The open last paragraph shown live is cleaned again at the stop and may change: it stayed the same in 1 of 8 dictations, while finished paragraphs shown live stayed the same in 19 of 20 (26.11., 60-word pieces, `docs/research/live-vs-final-2026-11.md`) | – | by design: the open paragraph is cleaned again with the next piece, now in place in the panel |
| C13 | Instant paste (26.11.): the draft's raw end is edited only where Sokki can read and select the focused field through Accessibility; elsewhere (no access, a terminal, a field Accessibility doesn't show) the dictation is pasted once, cleaned, after the wait as before. The edit is skipped when a key was pressed after the paste (sending it, typing on), the cursor moved or another app is in front: then the raw end stays in the field, and the cleaned text is only in the history. Undo: the first Cmd+Z brings the draft back. Between paste and edit the panel shrinks to its pill (Robin's pick of four, 26.11.). In Chrome the screen-reader mode (M1) is switched on at the first dictation with the cleanup | replay of 34 dictations (test mode): first paste a median 0.40 s after the stop (one paste before: 2.41 s), edit 2.28 s, none failed (`docs/research/instant-paste-2026-11.md`); real fields: lab night run in TextEdit and Chrome (textarea, rich editor), Slack, Claude and Notes only from real use (`Instant paste:` lines in the log) | measure skips in Slack, Claude and Notes from the log after a few days of use |

## Speakers

| # | Problem | Fix idea |
|---|---|---|
| P1 | "Who is the user": on an online call the mic voice (most speech on the mic track); on a single mic nobody is named. Two people in the room on one mic are not told apart from the user | enroll the user's own voice once; voice profile wins |
| P2 | Similar voices merge (synthetic `three_remote`: 3 → 2) | real-recording tests; `num_speakers` hint |
| P3 | Sortformer handles at most 4 speakers per track | fallback to sherpa clustering for larger meetings |
| P4 | Voice profiles tested only on synthetic voices; phone vs. clean voice similarity only 0.78 | test with real recordings |
| P5 | Renaming a real name ("Ich") only changes structured spots in the note, not running text | fine for now |
| P6 | Names from the call app: Meet (captions) and Zoom (active-speaker tile) are read, but not yet confirmed in a real call with two people: Meet and Zoom refuse automated test guests, and Sokki Test has no Accessibility. Teams isn't read. Check the log after a real call: "Call speakers: N span(s) of M name(s)" | first real two-person Meet and Zoom call; then Teams (Live Captions panel) |
| P7 | Names said in the meeting ("Danke, Lena") aren't used yet | let the summary model propose, as a suggestion only |
| P8 | Slack huddle members aren't proposed yet: branch `slack-huddle` isn't merged | after the merge, write the huddle's members to `invitees.json` |
| P9 | A name given from a guess (the last invitee left) is applied without asking; if the guest list is wrong (someone uninvited spoke), the name is wrong until the user renames it | wait for a real case before adding a check |
| P11 | Fixed 2026-11-26: one voice split across speaker windows (one in-person meeting of two: 5 speakers, the colleague twice at cosine 0.93; 9 such pairs in 8 of 38 meetings) | speakers of one track at cosine ≥ 0.75 are joined after the windows (`stitch::merge_same_voice`); different people scored at most 0.59. Pairs at 0.67–0.72 stay apart, unverified |
| P12 | Not a problem (Robin, 2026-11-26): sound the Mac plays in a meeting in the room (a video ad, 8 s) shows as speakers on the system track | kept as it is: "In General, these should be speakers." |
| P13 | Fixed 2026-11-26: with any speech on the system track the mic speaker with the most speech was "the user", even when the voice profile recognised another one as the user (would give two speakers named after the user) | the voice recognised as the user is the user |
| P10 | Grouping voices across meetings compares every speaker with every other: fine for hundreds of speakers, slow for many thousands | cache groups, or compare only against group centroids |

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
| X10 | **Fixed 2026-11-23:** Android: a recording made during a phone call or internet call is silent, but the app showed a normal recording with a running clock (emulator, Android 15: `dumpsys audio` shows Sokki's capture `silenced` from call accept to hang-up). Android gives no app but the call app the microphone during a call | while Android silences the recording, a line under the clock says so (`SokkiRecorder.silenced()` in `MainActivity.kt`, `isClientSilenced`). The part itself stays silent: Android offers no way around it. So calls can't be recorded on the phone (Robin, 2026-11-24); on the Mac Sokki asks to record an iPhone call taken there (Robin, 2026-11-25; MEETINGS.md "Other people's voices") |
| X11 | Android: the first recording sent after Sokki starts takes 15–45 s to reach the Mac, once it hung until "Check again" (emulator, 2026-11-23, with the tile and with the button); a ping right after answers in 150 ms | connect to the Mac when a recording starts, so the link is up when it stops |
| X12 | **Fixed 2026-11-24:** Android: Sokki couldn't be installed next to HeliBoard, nor a debug `org.sokki.app` next to the old `dev.verbatim.mobile` (emulator, 2026-11-24: `INSTALL_FAILED_CONFLICTING_PROVIDER`). The keyboard's two file providers had HeliBoard's fixed names (`helium314.keyboard.clipprovider` and `helium314.keyboard.provider`, debug `helium314.keyboard.debug.*`), which HeliBoard itself uses | both names follow the app ID: `org.sokki.app.clipprovider` and `org.sokki.app.gesturedata` (`mobile/keyboard/src/main/AndroidManifest.xml`, read back from `context.packageName` in `ClipboardHistoryEntry.kt` and `gesturedata/Share.kt`) |
| X13 | Call audio went silent mid-call: in the 2026-11-24 09:30 call the system track turned to exact zeros at 10:02:20, during a coreaudiod IO overload (other jobs were running sessions' model tests), and stayed silent for the last 13 min although the other side was talking. The tap kept delivering callbacks, just zeros. The afternoon meetings 14:19–15:20 (all system tracks silent) were mostly a session's own test calls with no far side, so they prove nothing either way. Sokki ran without restart from 2026-11-23 16:12 until the 0.2.12 install on 2026-11-24 15:42 | detect stuck zeros while an output runs and rebuild the tap, warn if that fails (board card "Anrufton darf nicht still wegbleiben", `src-tauri/swift/system_audio_tap.swift`); check the next real call after the restart |
| X14 | Not a bug, for the record: 7 desktop meetings ended with an empty note and the title "Meeting <date> <time>" (17.11. 15:32/15:42, 19.11. 21:02/21:08, 24.11. 14:42/14:43/14:51). None had speech: 0 words from Parakeet on each track, also 25 dB louder; mic never above -40 dB for a second, system track zeros. The note said "Keine Sprache erkannt" | **fixed** (2026-11-25): a meeting without a word on any track gets no note; its audio goes to the Trash and the record bar says "No speech, nothing saved." (`sokki_engine::NoSpeech`). At start, older meetings whose note is only "Keine Sprache erkannt" go to the Trash with their audio (`meeting/store.rs` `trash_no_speech`) |
| X15 | A timing test fails under load: `live::tests::turns_transcribed_during_the_call_give_the_same_note_with_fewer_calls_after` (sokki-engine) failed once on 2026-11-27 while release tests ran (66 s run), and passed twice alone (8.6 s). It needs its worker to keep up in real time | give it more slack, or run it only without other heavy jobs |
| X16 | **Fixed 2026-11-27:** meetings started while typing. The meeting shortcut Option+M fired on typing "mö"/"mü" on the US layout (Option+U is the umlaut dead key, Option goes down before M). 4 starts on 26–27 Nov. The swallowed M is still missing from the typed word (Option was down, so it would have been "µ" anyway) | the meeting shortcut fires on release, only with no other key in the chord (`register_on_clean_release`, `docs/research/meeting-shortcut-typing-2026-11.md`) |
| X17 | **Fixed 2026-11-27:** the "record this call?" window came only for the first call after Sokki started; every later call (Slack huddle, Google Meet, Zoom) got nothing. Log: `Meeting prompt could not be shown: a webview with label meeting_prompt already exists`, from 2026-11-24 on, 12 calls missed. The app's close handler (`lib.rs`, meant for the main window) turned the prompt's close into a hide, so the hidden window kept its label | the prompt window is destroyed, not closed, and a leftover one is gone before the next is built (`meeting/prompt.rs`). Test: `scripts/test-meeting-prompt.sh` (4 prompts in a row, hidden window): 1/4 before, 4/4 after. Still open: the close handler still hides every other window too (`meeting_indicator` relies on it) |
| X18 | **Fixed 2026-11-27:** a recording started by hand during a detected call ran on after the call. Only one started from the prompt was offered a stop. Real cases on 2026-11-27, both started by hand because of X17: a Slack huddle ended 10:24:59 and was stopped by hand 49 s later, a Google Meet ended 11:38:32 and was stopped by hand 83 s later | a recording started while call detection sees a call belongs to that call (`meeting/detect.rs`, `recording_started`). When the call ends, the existing "call ended" window (Keep recording / Stop) comes, and without an answer the recording stops 20 s later; the watcher already waited 20 s (`end_after`) for the app to take the mic back. Test: `scripts/test-call-end.sh`. Still open: a recording started before the call keeps running (it may be an in-person meeting); the 20 s + 20 s are not yet tuned on real reconnects |

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
| R11 | Garbage in other languages from muffled call audio ("anoň", "Stavbe se") is not removed | open. Findings: the garbage sits inside otherwise good lines, so dropping whole lines by language doesn't help; per-word ASR confidence is too weak a signal (good words 0.70–0.85, garbage 0.80); the same audio transcribed with more context comes out clean ("Ja." instead of "anoň Yeah"). Tried: transcribing 25 s windows across turns with word times (`PipelineConfig::window_s`): phone_speaker WER 17.6% → 15.3%, but de_en_mixed 3.8% → 31.2% (a window holding both languages comes out in one). Off by default. Next: windows of one speaker's nearby turns only. 2026-11-26, in-person meeting: 3 short lines in Polish/Czech/Cyrillic, as loud as normal speech; a de/en language hint changed 0 of 158 lines with Parakeet Ultra (`docs/research/speaker-count-2026-11.md` §5) |
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

## Meeting with screen (2026-11-23)

- Real screen capture and the call window have not run on a Mac yet: they
  need Screen Recording permission, which no session may trigger. Rendering
  is tested on sample files.
- Always records the main display; there is no screen or window picker.
- After Screen Recording is allowed, macOS asks to quit and reopen Sokki;
  nothing says so in Sokki after the reopen.
- Setup asks for System sound by opening the tap for a moment; macOS has
  no way to read the answer, so the row says "Asked", not "Allowed".
- Setup without Meetings turns off "Notice meetings"; picking Meetings
  leaves it as it was.

## Notice in the meeting chat (2026-11-24)

- Teams has not been tried: it isn't installed on the test Mac. Its
  meeting window and chat field are found by the same generic rules
  (a "leave" button, a field named like "Type a message").
- Meet only when its tab is the active tab of a window: Chromium shows
  background tabs to Accessibility not at all. Sokki retries for 2 min.
- Zoom posts to whoever the chat's "To:" is set to; usually Everyone.
- "stop" is seen only while the chat panel stays open (Sokki leaves it
  open after posting). Closed, Sokki stops watching and says so in the log.
- Someone without the chat (phone dial-in, mobile with the chat hidden)
  sees neither the line nor a way to type "stop"; they can still tell
  the user.
- A screen video moved to the Trash on "stop" may still be rendering.
- Accessibility sees only windows on the current desktop (Space). Zoom or
  Teams on another desktop: Sokki brings the app here, at most once a
  minute, and gives the front back after posting. A browser is never
  brought here (its window in front need not be the call's), so Meet on
  another desktop fails after 2 min with the banner.
- Chrome rejects both accessibility switches (AXManualAccessibility
  -25205, AXEnhancedUserInterface -25208) yet shows Meet's page anyway
  once another assistive app has woken it; on a Mac with none, untested.

- Sokki brings the call app to the front for about a second to post.
  It first waits for a 1.5 s pause in typing (at most 15 s); keys typed
  in that second still land in the meeting chat.

Tested 2026-11-24 in own test calls, Sokki in English: Zoom (posted in
0.8 s; "stop" → stopped, deleted, "Stopped and deleted.") and Meet in
Chrome (posted in 2.1 s; "stop" → same).

## Notes to everyone (2026-11-24)

- Sending needs Gmail connected or Apple Mail with an account; Outlook and
  other mail apps don't work yet. Until Google's review (card
  `privacy-google`) is through, the Gmail option is hidden
  (`GOOGLE_VERIFIED = false`) except with `meetings.gmail_testing` on, for
  the project's test users; they see "Google hasn't verified this app",
  and their consent runs out after 7 days.
- With Gmail, "stop" replies land in the person's inbox, but Sokki can't
  read them (`gmail.send` only; reading needs a restricted scope with a
  paid security audit): the person adds the sender under "No mail to".
- Attendees come from the Mac calendar. A calendar that lives only in a
  browser (Google Calendar not added to the Mac's Calendar app) has no
  attendees for Sokki, and the button says "No calendar event".
- Everyone invited gets the mail, also people who never joined; only a
  "declined" answer keeps someone out.
- Two events at the same time with links for the same app (two Meet
  invitations, a browser call): the one that overlaps the recording most
  wins, which can be the wrong one.
- "Stop" replies are read from Mail's inbox only when Sokki sends next;
  an objection arriving in a mailbox Mail doesn't show is missed.
- A blocked Mail (a dialog open, offline account) makes sending wait up to
  90 s before it fails.
- On by default (2026-11-25): installs from before turn it on once, also
  for someone who had turned it on and off again in 0.2.13 (that wasn't
  recorded).
- Gmail connected before 2026-11-25 also granted `gmail.metadata` (for the
  dropped mail before a meeting); Sokki no longer uses it. Disconnect and
  connect again to grant only sending.

## Notes into the Slack huddle's thread (2026-11-24)

- Tried against a real huddle on 2026-11-24 (alone in a private test
  channel): the profile gives `in_a_huddle` and the call id, but its
  `huddle_state_channel_id` stays empty, and `calls.info` answers
  `internal_error` for a huddle. The channel therefore comes from the
  Slack Mac app's log (`Created huddle_<channel> child window`). If Slack
  changes that line, Sokki finds no channel and says so; Slack in a
  browser has no such log. Fix idea then: the huddle window's title
  ("Huddle in #name") through Accessibility, resolved with
  `conversations.list`.
- Unverified: that `room.participant_history` lists everyone (one person
  so far).
- Slack deletes the message of a huddle once it ends if nobody wrote in
  its thread (seen alone in a test channel; with two or more people not
  yet tried). A post into that thread is accepted but lands in the
  channel as a normal message, so Sokki checks first that the message
  still exists and otherwise posts nothing ("no thread to post into").
  Fix idea: post the chat notice ("Ich schreibe lokal mit Sokki mit …")
  into the thread while the huddle runs; that keeps the thread and tells
  everyone.
- **Fixed 2026-11-27:** no huddle was ever posted from a release. Releases
  were built without the Slack app's client id, so Settings › Meetings
  showed no "Connect Slack" row, nobody could connect, and
  `slack::recording_started` returned at once (no `Slack:` line in the
  log; the huddle at 10:08 on 27.11. was detected and recorded). Now the id
  is built in. Robin still has to connect once.
- The Slack app isn't publicly distributed: the "Connect Slack" row shows
  in every release, but only members of the app's own workspace can
  connect; anyone else presumably gets an error from Slack at sign-in (not
  tried) and Sokki waits out its 5 minutes. Fix: public distribution (api.slack.com, needs
  Robin's login), see `crates/sokki-slack/SLACK_APP.md`.
- Only a huddle Sokki detected as a Slack call is posted; a recording
  started by hand during a huddle has no channel. Fix idea: ask the
  profile on a manual start too.
- The huddle's message must be among the newest 15 messages before Sokki
  started recording, else up to three more pages, a minute apart (the
  limit for apps outside the Marketplace).
- The sign-in comes back to `http://localhost:47821`. Slack asks for HTTPS
  redirects before an app may be installed in other workspaces; whether
  localhost counts with PKCE is not documented. Fix idea if not: a
  `sokki://` link (custom schemes are always "desktop" redirects).
- Workspaces where admins approve apps: the first sign-in waits for an
  admin.
- The summary's Markdown becomes Slack mrkdwn (headings bold, `- [ ]` as
  ☐); tables and images aren't converted.
- Other languages than German and English: the post's file name uses the
  English "Transcript".
- Notes mail after a huddle: Sokki reads who is in the huddle once a
  minute while it records, so someone who joined in the last minute can be
  missing. Everyone who was in the huddle gets the mail, also someone who
  joined for ten seconds. Slack shows an address only if the workspace
  lets apps see it (Slack's default) and only for people in the same
  workspace; guests from other companies in a Slack Connect channel may
  have none and are left out ("no address").

## Phone app (Android, 2026-11-26)

| # | Problem | Fix idea |
|---|---|---|
| P1 | **Gone 2026-11-26: the phone no longer writes meeting transcripts** (Robin: too battery intensive; without a Mac a recording waits for one). It was: a transcript written on the phone (no Mac) only runs while Sokki is on screen. Leave the app or lock the phone and Android freezes it (`isFrozen=true`), and the transcript waits until Sokki is opened again. Pixel 7: a 3-minute clip stood at 0 words for 6 minutes in the background, then finished 59 s after Sokki came back. A 30-minute meeting takes 12.6 minutes with the screen on (`docs/research/android-phone-transcript-2026-11.md`) | run the transcript in a foreground service with a "Writing the transcript" notification, in Rust or Kotlin, not in the WebView |
| P2 | "Note ready" can come up to 15 minutes after the Mac finishes when Sokki is closed: Android runs a background job at most every 15 minutes (`NoteCheck.kt`). With Sokki open the note arrives within 4 s | the Mac pushes to the phone (Firebase Cloud Messaging, or a relay the phone keeps open), if 15 minutes turns out too slow in real use |
| P3 | After the phone restarts, "Note ready" for notes still in progress waits until Sokki is opened once: the job isn't kept across a restart (that needs the boot permission) | `setPersisted(true)` plus `RECEIVE_BOOT_COMPLETED`, once a real case shows it's needed |
| P4 | A meeting sent live from the phone has its summary model loaded only at the stop (the Mac preloads it during its own recordings, `meeting/preload.rs`, tied to the recorder): 3-minute test meeting, summary step 23 s instead of ~17 s | start `preload` for phone live meetings too (`phone_live.rs`) |
| P5 | A freshly paired phone's first link requests sometimes get no answer for a minute or more although the Mac answered (emulator, 26 Nov 2026; root cause in the link layer not found). Live pieces then start ~20 s late and catch up | the 12 s limit per live request and the retries cover it (`mobile/src-tauri/src/link.rs`); look into iroh if it shows on real phones |
| P6 | When the Mac restarts during a phone live meeting, the part it had received stays behind in its meeting folder (no meeting in the list; the phone sends the whole recording after the stop) | delete orphaned `*-phone` folders without a record at start, once it happens for real |

## Keyboard (Android)

Measured on Robin's writing (47k words, `mobile/keyboard/tools/robin_eval.py`, 2026-11-23).

| # | Problem | Fix idea |
|---|---|---|
| K1 | Autocorrect only runs in fields that ask for it or are multi-line (HeliBoard/AOSP rule): a single-line field without the autocorrect flag changes no word on space | decide whether to autocorrect plain single-line text fields too |
| K2 | After "das"/"die" used as a pronoun, an English verb gets a capital: "lass uns das Push", "kann ich das Skip", "war das Fine" | a small German part-of-speech tagger |
| K3 | A third of lower-case nouns stay lower case: words German writes both ways (die kosten, die suche), English nouns with no article before (sales call) | same tagger; capitals after articles measured and rejected (half wrong in chat) |
| K4 | One-slip typos with two plausible fixes are left alone (fot: for/got, wiel: weil/viel): 18% of his typos | correct the previous word once the next is typed (77.5% instead of 74.6%) |
| K5 | His own names and terms get changed, about one per 4,000 words (tool, product and people's names, e.g. a name like Jovan → Johan) | learn a word after an undone autocorrection |
| K6 | **Fixed 2026-11-24:** a word typed right after "." or "," stuck to it (like "done.we", "klar,dass"): 15 times in Robin's journal 19.–24.11. (2,700 words), plus "z.B." which must stay. HeliBoard adds no space after typed punctuation | the space goes in when the next word ends (`Spacing.kt`); replayed on the journal: 15 fixed, nothing else changed. After a dictation the next typed word gets a space (`InputLogic.sokkiAfterDictation`) |
| K7 | **Fixed 2026-11-25, check on real dictations:** Android dictation: a pause inside a sentence ended a piece, and Parakeet ended it with a period, so the next piece started lower case (like "…the meeting is. ready by noon", 14 times in 6 days of Robin's journal) | the keyboard now streams the whole dictation into Nemotron Streaming (`keyboard_voice.rs`): no pieces, one punctuation pass. Test clips with pauses between sentences come out right; a real pause inside a sentence is not tested yet |
| K8 | Android dictation keeps English fillers ("um", a few times a day); the Mac removes them | the Mac's filler filter on the phone too |
| K9 | Robin's Pixel runs the old debug app `dev.verbatim.mobile`; its journal (since 19.11.) and keyboard settings stay with that app when `org.sokki.app` is installed | copy `files/typing_log` over by adb before switching, or "Send to Mac" from the old app's developer page once it has one |
| K10 | Android: keys typed while a dictation is still finishing (strip shows the dots) can land at the start of the field: the keyboard ignores cursor updates while Sokki types, so it works from an old cursor position (emulator, 2026-11-24: "Bus." landed before the dictated sentence). Since 2026-11-25 finishing takes 0.2 s instead of ~3 s (Robin's Pixel), so the window is small, but still there | let the keyboard catch up before the first key after a dictation, or end the dictation on that key |
| K11 | **Gone 2026-11-26 with Nemotron (the phone reads with Parakeet again):** Android dictation: Nemotron Streaming left out the period at the very end of a dictation ("… bis später und liebe Grüße" for "… liebe Grüße.", 1 of 1 German test clips on the Pixel, 2026-11-25); the keyboard then underlines the last word, as it does for any word the cursor touches | wait for a real dictation that shows it; then end the text with a period when the model gives none |
| K12 | **Worked around 2026-11-26 (keyboard link over the relay only; direct only in debug builds with `debug_direct`):** Android dictation through the Mac: on the emulator and on Robin's Pixel, the first dictation on a link younger than a few seconds stalled ~16 s (the link had just moved from n0's relay to a direct path) and the phone took over after 2.5 s. The same on the Mac's loopback and relay-only on the emulator worked. Not seen on a real phone yet | check on Robin's Pixel over Wi-Fi (logcat `SokkiVoice: Mac: dictating (direct|relay, …)`); if it shows there, hold dictation on the relay until the direct path has settled |
| K13 | Android dictation through the Mac (with the cleanup on since 26.11.: cleaned paragraphs live, done 1.1–3 s after the stop): the text stays underlined until the stop (the Mac's live text may still change), then the whole text is typed at once. Done 0.3–1.5 s after the stop (Mac reads it all again; 15–65 s clips, 2026-11-26) | type the words the Mac's live text has kept unchanged for a while, as the Mac's cleanup does with settled sentences |
| K14 | Android dictation on the phone (no Mac, Parakeet in pieces, 2026-11-26): done 2.4 s after the stop at the median, up to 4.5 s (Pixel 7, test clips; Nemotron was 0.3 s), and the live text ~1 s behind the speech. The cost of 12 % instead of 17 % different words on Robin's voice | read the last piece while the person still speaks (only its last second is new at the stop); a read of the growing piece that runs at the stop could be cut short |

## Typing on the Mac: autocorrect (2026-11-25)

`crates/sokki-autocorrect`, the phone keyboard's model (`crates/sokki-typos`). Off by default.

| # | Problem | Fix idea |
|---|---|---|
| M1 | Chrome and other Chromium browsers show page text fields only in their screen-reader mode (`AXEnhancedUserInterface`), so Sokki switches it on for the browser in front while autocorrect is on (`mac.rs`, `CHROMIUM`). That mode makes the browser animate window moves done by window managers (Rectangle, Magnet …) and costs it some CPU; not measured yet. Electron apps (Slack, Claude) get `AXManualAccessibility` instead. Measured 25.11. with the lab (`scripts/autocorrect-lab.sh`): fields appear about 2 s after the switch, so Sokki sets it about once a second for the app in front | measure the browser's CPU with the mode on; if a user with a window manager complains, offer a per-browser opt-out |
| M5 | Chrome takes the typed fix only in its active window. Tested 26.11. with a test window in front: input, textarea and rich editor (contenteditable) each fixed every word exactly, typing with 150 and 400 ms pauses (18 of 18 fixes). If the window isn't active, the fix doesn't arrive: Sokki leaves the word as typed and selects nothing (`the fix didn't arrive` in the log). A word typed within ~2 s of Chrome's start is missed while its screen-reader mode wakes (M1) | Robin tries it in Chrome, Slack and Claude after the release |
| M2 | Cmd-Z after a fix: unknown whether apps undo a word replaced through Accessibility (a 2008 report says TextEdit doesn't). Backspace right after a fix works | try it in Chrome, Slack, Claude |
| M3 | Third-party apps built on Apple's text fields (not Chromium/Electron) get macOS's autocorrect and Sokki's: two correctors on one word | exclude them once a real case shows up |
| M4 | The same open cases as the phone (K2–K5): two plausible fixes are left alone, the user's own names can get changed until undone once | as K4, K5 |
| M6 | A German noun + s that is a real genitive stays as typed, also where the plural was meant ("die Transkripts" for Transkripte); the English "transcripts" is one slip away too (`docs/research/mac-autocorrect-2026-11.md`, "The user's own words as fixes") | an -s form after "die" competes with the -e plural, once more real cases show up; needs the sentence's language to outweigh the English word |

## Usage data, "Help improve Sokki" (2026-11-24)

| # | Problem | Example | Fix idea |
|---|---|---|---|
| U1 | Setup is counted only from people who turn the switch on (setup step 5); quitters and people who say no are never counted | – | compare with sokki.org's `zip_downloads` in PostHog; no tracking without consent |
| U2 | The frontend's call per setup screen is checked by the type check only; the Rust side by `scripts/test-usage.sh` (`SOKKI_TEST_SETUP` replays setup, as the test app never shows the setup window) | – | test it once in a fresh test app when a window on screen is fine |
| U3 | Notes sent by mail or Slack, Ask and History search are not counted yet (mail and Slack aren't in `meetings` yet) | – | `usage::count` with a new `Count` where they land |
| U4 | Deleting at PostHog needs a personal API key (`POSTHOG_PERSONAL_KEY`); until Robin sets it, ids wait in `posthog_forget` | – | Robin creates the key; the hourly cron then deletes |
| U5 | The counter read ~700,000 rows a day for one sharing install: every upload summed all earlier ones (`SUM(bytes) FROM donations`); 5M a day is the free limit | 874 rows × 227 uploads | fixed 2026-11-25: running total in `cache` (`storedBytes`), one row per upload |
| U6 | Workers KV's free plan allows 1,000 writes, deletes and lists a day, and 1 GB. On 24.11. one `/forget` used up the deletes and stopped halfway | 874 clips of one install | fixed 2026-11-25: deletes in stages within a daily budget, uploads "try later" above one (`kv_ops`). fixed 2026-11-25: audio in R2 (EU, 10 GB free, deletes free); older files move over from KV hourly |

## Planned

- **D1** LLM correction pass over the transcript (context window + glossary), measured against truth. Built, then removed on 2026-11-20 (R17).
- **D2** Live pipeline during the recording: utterance-wise ASR and correction, so the note is ready right after the call.

## Meetings that start by themselves (2026-11-26, `meeting/auto.rs`, off by default)

| # | Problem | Example | Fix idea |
|---|---|---|---|
| AM1 | The microphone stays open while auto start is on | macOS shows the orange dot all the time | none planned; it's the price of listening, said in the setting's doc |
| AM2 | The user's own voice in a video (their own recording, a talk they gave) could start a meeting | not seen yet; the test only had strangers' voices | compare system output with the mic, if a real case shows up |
| AM3 | The user's own voice may be known from one stored sample only | Robin's Mac: one sample; untested how well 3 s pieces from the built-in mic match it | read "best similarity" in sokki.log; add the user's voice from each meeting if it misses |
| AM4 | Music counts as speech for the voice detector | synthesised music: 28–30 s of 30 s "speech" | harmless while the known-voice check holds; noted in the research doc |
| AM5 | First real start was a recording the user didn't want | 2026-11-26 12:01 UTC: own voice + one unknown person in the room, similarity 0.64; the user turned auto start off after 11.8 s | open: reason not known yet; auto start is off on that Mac |
| AM6 | Own voice scores low against one stored sample | real voice on the built-in mic 0.64–0.70 (test voices 0.91), unknown speech up to 0.47, bar 0.5 | open: more samples of the user's voice would widen the gap |
| AM7 | The pill's timer starts at 0:00 when an auto meeting shows, though the recording already holds the minute before | by design of listening in (2026-11-26) | open: the pill could start at the kept length |

## Search index and the `sokki` command line (2026-11-26, `crates/sokki-index`, CLI.md)

| # | Problem | Example | Fix idea |
|---|---|---|---|
| IX1 | The summary's tasks and decisions have no time or quote from the model: their evidence is the turn code finds with the most words in common (`evidence_origin: "match"`), a pointer, not proof | "Angebot der Druckerei prüfen" → the turn "Dann prüfe ich bis Freitag das neue Angebot der Druckerei" (made-up set); an item without two shared words of four letters or more gets none | ask the summary model for `evidence_ts` and a quote and keep only quotes code finds (research: Gemma's extraction is incomplete, never invented); needs a summary eval first |
| IX2 | Keyword search misses another ending and any English question about a German meeting | `memory-search-de`: "reklamieren" doesn't find "reklamiert"; "termination notice" finds nothing (0 of 3) | stemming, or the vector eval (`memory-layer-2026-11.md`) |
| IX3 | Trigram false matches are not measured: the made-up set has 46 turns and few distractors | "rat" also matches "Beratung", "Rathaus" | measure on a real archive in the private eval (50–100 questions) |
| IX4 | Notes written before 26.11.2026 have no `segments.jsonl`: their turns have the note's mm:ss times, the next turn's start as end, and no channel (mic/system) | `sokki index status` counts `turns_with_exact_times` | reprocessing a meeting writes the file (only while its audio is kept) |
| IX5 | A person is a name: "Anna" in one meeting and "Anna Beispiel" in another are two people, and a speaker named only in the transcript (not in a voice profile) counts like one named by hand | `sokki people` lists both | person ids from voice profiles and calendar e-mails |
| IX6 | The dictation's app is the app in front when the history entry is saved, not where the text was pasted; older dictations have none | the test app's dictation (nothing pasted) got "Problem Reporter", a crash dialog that was in front (26.11.) | take the app at the paste, from `paste_tx` |
| IX7 | No Settings screen for `agent_access` yet; only `sokki config agent_access …` | – | four designs on the canvas first (Robin's rule), then the screen |
| IX8 | On the Mac the API's `models` state always says ready, and `install_models` is refused: the app's Models page manages downloads | `sokki status --json` | report the selected speech model's state |
| IX9 | `sokki ask` on the Mac talks to `meetings.llm_base_url` (Ollama) only, as the old script did: with the built-in model selected it fails unless Ollama runs | `llm_provider: builtin` | start the built-in model for one question through `sokki_engine::llm::open` |
| IX10 | The Linux build of `crates/sokki` after the split into macOS and Linux parts | – | fixed 27.11.: `cargo check --all-targets` and `cargo test` in the Arch box (`packaging/docker`, aarch64) without a warning; not yet run in the Omarchy VM |
| IX11 | No item changes over time (`valid_from`/`invalid_at`): a task closed in a later meeting stays open in the index until the note's checkbox is ticked | – | as in the research, once a real case needs it |
| IX12 | Hooks need `agent_access` on: the runner passes each hook's scope as `SOKKI_SCOPE`, which only narrows the setting | a CRM hook with access off gets "agent access is off" | Robin's call: a per-run grant from the runner for approved hooks (e.g. `SOKKI_HOOK_GRANT`) |
| IX13 | Quitting by signal (SIGTERM, as the test scripts do) skips `RunEvent::Exit`, so `sokki.sock` stays as a file; clients then say "not running", and the next start replaces it (tested 26.11.) | – | none needed; a quit from the menu removes it |
| IX14 | Own notes were in the index twice: the typed lines from `notes.md` and again from the note's "Meine Notizen" section | – | fixed 27.11.: with a `notes.md`, the note's section adds only Sokki's additions |

## Hooks (2026-11-26, HOOKS.md)

| # | Problem | Example | Fix idea |
|---|---|---|---|
| H1 | Fixed: `own_notes.saved` fires on the Mac, 30 s after the last save (`meeting/own_notes.rs::save`) | – | – |
| H2 | Linux has no `meeting.deleted` and no `own_notes.saved` | the Linux service can't delete a meeting and has no own notes | fire both once it has them |
| H3 | Linux dictation ids are times, not history ids | `d:1764194412000` | once Linux keeps a dictation history |
| H4 | `sokki hooks` on the Mac needed the repository's build of `crates/sokki` | – | fixed 27.11.: the app bundles the Rust CLI (`resources/bin/sokki`, `scripts/build-sokki-cli.sh`), `install.sh` links it; `scripts/sokki` runs it |
| H5 | A hook paused for a meeting (SIGSTOP) can lose a network call | a hook halfway through an HTTP request when a recording starts may get a timeout from the server after the meeting | the retry runs it again; if a real case shows up, let a hook opt out of pausing |
| H6 | Approval pins only the files the `run` line names | a script that sources `lib.sh` can change `lib.sh` without a new allow | documented; pin the whole hooks folder if a real case shows up |
| H7 | `meeting.note_ready` can fire more than once per meeting | after a reprocess; once more when a summary that waited for the model lands | by design (the note changed); hooks dedupe, as the Reminders example does |
| H8 | Not tried for real: whether Reminders shows its window when the example hook starts it | tested only in test mode, which leaves Reminders alone | try it once on a Mac nobody is using; it quits Reminders again if it started it |
| H9 | Hooks have no Settings page yet | allowing and the log are CLI-only; the notice names the command | four designs on the canvas first (Robin's rule) |
| H10 | A schedule runs only while Sokki runs | Friday 17:00 with the app closed runs at the next start, once | by design (anacron-like), documented |
| H11 | A hook running when Sokki quits keeps running, and runs again at the next start | the run counts as cut off (`recover`), so a slow hook can do its step twice | stop the hook's process group at quit, if a real case shows up |
