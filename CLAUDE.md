Read @AGENTS.md

## This fork: Verbatim (internally "Handy Meetings")

- `MEETINGS.md`: what the fork adds, layout, tests, build.
- `CONFIG.md`: every setting, and how to change it. The app's settings live in
  `~/Library/Application Support/dev.handymeetings.app/settings_store.json`;
  the app applies external edits within a second (`src-tauri/src/config_watch.rs`).
  Regenerate with `scripts/gen_config_docs.py` after changing `settings.rs`.
- `KNOWN_ISSUES.md`: every known problem. Add new ones there; don't wave
  quality problems away.
