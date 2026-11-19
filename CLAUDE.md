Read @AGENTS.md

## This fork: Sokki (internally "Handy Meetings")

- `MEETINGS.md`: what the fork adds, layout, tests, build.
- `CONFIG.md`: every setting, and how to change it. The app's settings live in
  `~/Library/Application Support/dev.handymeetings.app/settings_store.json`;
  the app applies external edits within a second (`src-tauri/src/config_watch.rs`).
  Regenerate with `scripts/gen_config_docs.py` after changing `settings.rs`.
- `KNOWN_ISSUES.md`: every known problem. Add new ones there; don't wave
  quality problems away.
- Releases: `scripts/release.sh`. It uploads the update feed (`latest.json`)
  that installed apps update from; a release made by hand has none.

## As few rules as possible

Add a rule, heuristic or threshold only when a real recording shows it is
needed. Code for a rule that has not yet fixed a real case stays in but
switched off, with a note saying so; turn it on once a real case shows up
(example: `DiarizeConfig::merge_similarity`).
