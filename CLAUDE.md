Read @AGENTS.md

## This fork: Sokki

- `ARCHITECTURE.md`: the map. One engine (`crates/sokki-engine`), thin hosts
  per platform (macOS: `src-tauri`; Linux/Omarchy: `crates/sokki`), and the
  invariants every change keeps.
- `MEETINGS.md`: what the fork adds, layout, tests, build.
- `CONFIG.md`: every setting, and how to change it. The app's settings live in
  `~/Library/Application Support/org.sokki.app/settings_store.json`;
  the app applies external edits within a second (`src-tauri/src/config_watch.rs`).
  Regenerate with `scripts/gen_config_docs.py` after changing `settings.rs`.
- `KNOWN_ISSUES.md`: every known problem. Add new ones there; don't wave
  quality problems away.
- `docs/research/`: the measurements behind the choice of language model,
  prompt, live mode and runtime (dictation cleanup, summaries, Müsli). Read
  it before changing any of these, and add every new measurement there with
  its numbers (no transcript text). Robin (2026-11-25): "This information
  needs to be kept."
- Releases: `scripts/release.sh`. It uploads the update feed (`latest.json`)
  that installed apps update from; a release made by hand has none.

## Test it yourself

Test every change yourself, end to end in the installed app, before asking
Robin to try it. Robin: "We really need you to be able to test everything
yourself … This makes everything soooo much faster." When a change can't be
tested without a person yet, first build what's missing (a test mode, a CLI
command, a log line, a fixture), then test.

Never start Sokki's binary from a shell, only `open -a Sokki`: a shell-started
Sokki gets the shell's macOS permissions, and on 2026-11-19 its onboarding
reset the running Sokki's Accessibility grant, which froze all keyboard and
mouse input. Never run `tccutil`, never click onboarding's permission
buttons, no relaunch loops.

## Building

- Test apps: `scripts/test-dictation.sh --build` builds with the
  `fastrelease` profile (release without LTO, `src-tauri/Cargo.toml`) into
  `src-tauri/target/fastrelease/`. `PROFILE=release` builds exactly what
  `scripts/release.sh` ships. By hand: `bun run tauri build … -- --profile
  fastrelease`.
- Every heavy build waits in the Mac-wide queue (`~/.local/bin/build-slot`,
  see `~/PycharmProjects/CLAUDE.md`). Start it detached, e.g.
  `~/.local/bin/build-slot submit --name "<task>: test app" --
  scripts/test-dictation.sh --build`, then repeat `build-slot wait <job>`
  until it exits with anything but 124. A foreground tool call ends after
  10 min and loses its place in the queue. The build scripts queue through
  `scripts/build-lock.sh` and run on the job's slot.

## Every action feels instant

A general design rule for every click, shortcut and menu item. Robin: "Any
action the user takes, should feel as instant as possible."

- Show the result at once (button state, pill, list), then do the slow work in
  the background. The UI never waits for a model, processing, a lock or the
  main thread.
- Recording outranks everything: starting and stopping a meeting take effect
  immediately, also while another meeting is being processed; background work
  yields (measured 31–35 ms from click to recording, 7c1a1311).
- Log how long an action takes from click to visible result; a slow one is a
  bug. Past cases: meeting start/stop on the main thread (ea7f823a), a pill
  stuck at "starting" after a stop (7c1a1311).

## Sokki does the step itself

A general design rule for every feature that ends in an action: sending a
mail, posting in a chat, sharing a note, inviting someone. Robin (2026-11-24):
"weniger von diesem der Mensch muss abschicken: Das macht NIEMAND. … Wir
automatisieren, was wir automatisieren können."

- Sokki does the action itself. The user's part is one opt-in (a switch, a
  permission, a sign-in), not a step they repeat every time. A draft, a copy
  button or "paste it yourself" is not a design; nobody does it.
- When an obstacle comes up (a permission, an app review, a law, a missing
  API), work out exactly what it takes before you rule anything out. Then
  build the automatic path, and handle the obstacle in the design: a consent
  switch, the user's own account as sender, a text the user approved once.
- A manual step stays only where the law or a platform requires a person, and
  then it is one click inside Sokki. Name the source that requires it.

## As few rules as possible

Add a rule, heuristic or threshold only when a real recording shows it is
needed. Code for a rule that has not yet fixed a real case stays in but
switched off, with a note saying so; turn it on once a real case shows up
(example: `DiarizeConfig::merge_similarity`).
