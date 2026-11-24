# AI testers for Sokki on Android

Each tester is a Claude Code agent with its own Android emulator. It plays a
persona (phone size, language, text size, dark mode), uses Sokki like a new
user and writes a bug report with numbered screenshots. The workflow is
`.github/workflows/agent-testers.yml`; it runs on github.com/feynnon/sokki.

Adapted from [pierridotite/mobile-agent-testers](https://github.com/pierridotite/mobile-agent-testers)
at commit `d610b95a` (MIT, `LICENSE-mobile-agent-testers`). Copied, not
depended on: nothing of that repo is fetched at run time.

## Run it

Actions → **Agent testers** → Run workflow. Or:

```sh
gh workflow run agent-testers.yml -R feynnon/sokki -f personas=lena,tom,ada -f brief=explore
```

Then fetch and decrypt the reports (needs the private key, see below):

```sh
bash tests/agent-testers/scripts/collect.sh <run id>    # → runs/<run id>/SUMMARY.md
```

| Input              | Meaning                                                                                                                                                                                                                                                   |
| ------------------ | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `personas`         | ids from `personas.json`, comma-separated, or `all` (8)                                                                                                                                                                                                   |
| `brief`            | `explore`: new users chase their own goal. `onboarding`, `keyboard`, `meeting`, `settings`: one flow end to end. `reproduce`: one reported bug on every phone (`bug_report` = issue or steps). `improve`: a walkthrough with scores, for comparing builds |
| `ref`              | which Sokki to build (branch, tag, SHA)                                                                                                                                                                                                                   |
| `model`            | `sonnet` (default), `haiku` (cheaper, weaker eyes), `opus`                                                                                                                                                                                                |
| `budget_per_phone` | dollar cap per phone, at most 2                                                                                                                                                                                                                           |

**Compare two builds:** run `improve` twice with the same personas and two
`ref`s, collect both, then
`python3 scripts/compare.py runs/<A> runs/<B> v0.2.10 v0.2.11 > COMPARE.md`.

**After each release:** set the repo variable `AGENT_TESTERS_ON_RELEASE` to
`true` and every published release runs the three default personas. Off by
default.

## Personas

| id      | phone                     | language              | text  | goal                                 |
| ------- | ------------------------- | --------------------- | ----- | ------------------------------------ |
| lena    | small, Android 11         | de-DE                 | 130 % | keyboard dictation                   |
| tom     | Pixel 7, Android 14, dark | en-GB                 | 100 % | meeting with the Quick Settings tile |
| ada     | Pixel 6, Android 14       | en-US                 | 200 % | setup and first dictation            |
| jonas   | Pixel Tablet, Android 14  | de-DE                 | 100 % | meeting, then keyboard note          |
| camille | small, Android 13, dark   | fr-FR                 | 115 % | fastest way to dictation             |
| lucia   | Pixel 7, Android 15       | es-ES                 | 130 % | privacy, then keyboard               |
| marek   | Pixel 6, Android 14, dark | pl-PL                 | 100 % | try to break recordings              |
| noor    | Pixel 7, Android 14       | ar-EG (right to left) | 115 % | keyboard, English dictation          |

## Costs and limits

- GitHub runners: free (public repo). One run: ~25–40 min build, then
  ~15–30 min per phone, all phones at once.
- Model: each phone stops at `budget_per_phone` (Claude Code's
  `--max-budget-usd`, default $0.80). The run refuses to start when phones ×
  budget is above the repo variable `AGENT_TESTERS_MAX_RUN_USD` (default $7),
  or more than 8 phones. One run at a time (`concurrency`). The run page shows
  the cap before anything starts and each phone's real cost at the end.
- The Anthropic key lives in its own Console workspace with a monthly spend
  limit, so a mistake can't cost more than that limit.

## Test audio (all made up)

Emulators have no microphone. `scripts/make_audio.sh` speaks made-up
sentences with espeak-ng (a synthetic voice, nobody's recording):

- the **Sokki keyboard** hears `keyboard.wav` through its debug-build test hook
  (`Android/data/org.sokki.app/files/sokki_test.wav`, `SokkiVoice.kt`);
- the **phone's microphone** (Dictate tab, meetings) hears `mic.wav` on a loop:
  `scripts/mic_feed.sh` makes a PulseAudio virtual microphone on the runner and
  the emulator records from it (`-allow-host-audio`).

Sokki's model reads these voices well (checked on the Mac with `sokki
transcribe`, 2026-11-24): German, English, Spanish, Polish almost word for word;
the French microphone sentence comes out garbled, the French keyboard sentence
is exact.

## What this can't test

- **The Mac side.** No Mac on the runner: pairing, sending recordings, the
  notes that come back. Agents skip the Mac step.
- **Real voices, rooms and phone microphones.** Only a clean synthetic voice.
- **Calls.** Android silences other apps during calls; the emulator has no calls.
- **Speed on real phones.** The emulator runs on a server CPU (x86_64), not a
  phone chip: timings are no guide.
- **The release build.** Testers use a debug build (needed for the keyboard's
  test audio and for preloading the model).
- **Screens in other languages.** Sokki's phone screens are English only for
  now; the keyboard and the tile are translated. Testers report this once.

## Privacy and security

- Results (screenshots, reports, logs of the agent) are encrypted with
  `results_public.pem` before upload; only the private key opens them. The
  private key is on Robin's Mac at `~/.config/sokki-agent-testers/results.pem`,
  never in the repo. Public logs show only status lines and costs.
- Only test data goes in: made-up names, made-up sentences, a synthetic voice.
- One secret: `ANTHROPIC_API_KEY`, passed only to the emulator step. Runs start
  only by hand or on a release; forks and pull requests get nothing.
- Every action is pinned to a commit; Claude Code (2.1.330) and Pillow are
  pinned versions. The agent may only take screenshots, tap, type, press the
  phone's buttons, sleep, and read and write files; no web access.

What changed against upstream after reading it: actions pinned to SHAs
(upstream used tags), Claude Code pinned (upstream: latest), a dollar cap per
phone and per run (upstream: none), no `adb install -g` (upstream grants every
permission, which hides the dialogs a new user sees), no general `adb`, `cat`
or `ls` for the agent, no encrypted APK or build log (Sokki is open source).
Nothing in upstream sent data anywhere except GitHub artifacts and the model
API; `collect.sh` only printed a link to their feedback form (removed).

## Later: the Mac app

Same idea, two ways, not built:

1. **Cheap, now:** agents drive Sokki's main window in headless Chromium with
   test data, like `tests/main-window.spec.ts` already does, on a Linux runner.
   Finds layout, text and flow bugs of the window; no permissions needed.
2. **The whole app** on a `macos-15` runner (free for public repos): the agent
   sees the screen with `screencapture` and clicks with a small helper. The
   menu bar, the global shortcut, the recording pill and system audio need
   Accessibility, Microphone and Screen Recording grants, which a hosted runner
   can't give without a configured image. Needs research before building.
