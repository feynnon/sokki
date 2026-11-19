Read @AGENTS.md

## This fork: Sokki (internally "Handy Meetings")

- `ARCHITECTURE.md`: the map. One engine (`crates/sokki-engine`), thin hosts
  per platform (macOS: `src-tauri`; Linux/Omarchy: `crates/sokki`), and the
  invariants every change keeps.
- `MEETINGS.md`: what the fork adds, layout, tests, build.
- `CONFIG.md`: every setting, and how to change it. The app's settings live in
  `~/Library/Application Support/dev.handymeetings.app/settings_store.json`;
  the app applies external edits within a second (`src-tauri/src/config_watch.rs`).
  Regenerate with `scripts/gen_config_docs.py` after changing `settings.rs`.
- `KNOWN_ISSUES.md`: every known problem. Add new ones there; don't wave
  quality problems away.
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

## As few rules as possible

Add a rule, heuristic or threshold only when a real recording shows it is
needed. Code for a rule that has not yet fixed a real case stays in but
switched off, with a note saying so; turn it on once a real case shows up
(example: `DiarizeConfig::merge_similarity`).
