# Sokki from the command line

Two command lines: `sokki` (a script, for people and local agents) and the
app's own flags (mostly for scripts and window managers). Everything runs on
this Mac. Settings: [CONFIG.md](CONFIG.md).

## `sokki` (macOS)

The script `scripts/sokki`. Install once:

```bash
ln -s "$PWD/scripts/sokki" ~/.local/bin/sokki
```

| Command | What it does |
|---|---|
| `sokki transcribe FILE [--json]` | Any audio or video file to text with Sokki's speech model; `--json` for JSON. Needs `ffmpeg`. |
| `sokki ask [--think] [PROMPT]` | Asks the local language model Sokki uses for summaries. Text piped in is appended: `cat notes.md \| sokki ask "Summarise"`. |
| `sokki config` | Lists every setting with its value. |
| `sokki config KEY` | Shows one setting. |
| `sokki config KEY VALUE` | Changes one setting. Sokki applies it within a second, no restart. |
| `sokki hooks list\|allow\|revoke\|run\|test\|log` | Your own commands, run by Sokki on events: [HOOKS.md](HOOKS.md). Runs the Rust `sokki` from `crates/sokki` (`$SOKKI_BIN`, else the repository's build: `cargo build --release --manifest-path crates/sokki/Cargo.toml`). |
| `sokki help` | The list above. |

`transcribe` and `ask` wait while Sokki records or processes a meeting, so
the meeting keeps the Mac (a transcription during a call once cost a
meeting its call audio).

### Settings: `sokki config`

Every setting in [CONFIG.md](CONFIG.md) is a `KEY`: its dotted name, as in
the table there.

```bash
sokki config                                       # all settings
sokki config meetings.auto_start_enabled           # → true
sokki config meetings.auto_start_show_after_s 90   # listen in 90 s before showing a meeting
sokki config meetings.user_name "Alex"             # text with a space: quote it
sokki config meetings.detection_enabled false
```

- `VALUE` is JSON: `true`, `60`, `"text"`, `["a","b"]`. A bare word counts
  as text.
- Only settings that exist, and only with their type: a typo or `maybe` for
  a switch is refused and changes nothing. Typos get suggestions.
- API keys, tokens and passwords are never printed (`"(hidden)"`).
- It edits `~/Library/Application Support/org.sokki.app/settings_store.json`;
  the running app picks the change up by itself
  (`src-tauri/src/config_watch.rs`). With Sokki closed, it applies at the
  next start.

### Environment

| Variable | Default | For |
|---|---|---|
| `SOKKI_SETTINGS` | the installed app's settings file | `config`, and the model `ask` uses |
| `SOKKI_MODEL` | `meetings.llm_model`, else `qwen3.5:9b` | `ask` |
| `SOKKI_APP_BIN` | `/Applications/Sokki.app/Contents/MacOS/…` | `transcribe` |
| `SOKKI_MEETINGS_INDEX` | the app's `meetings/index.json` | waiting during meetings (tests) |
| `SOKKI_WAIT_EVERY_S` | `10` | how often the wait checks |

## The app's flags (macOS, Windows, Linux)

Given to the app binary. Flags marked "running app" go to the Sokki that is
already running and exit; they never change settings.

| Flag | What it does |
|---|---|
| `--toggle-meeting` | Start or stop a meeting recording (running app) |
| `--start-meeting` | Start a meeting; nothing if one runs (running app) |
| `--stop-meeting` | Stop the meeting; nothing if none runs (running app) |
| `--toggle-transcription` | Start or stop a dictation (running app) |
| `--toggle-post-process` | Dictation with cleanup, on or off (running app) |
| `--cancel` | Cancel what's running (running app) |
| `--start-hidden` | Start without the window (tray icon only) |
| `--no-tray` | Start without the tray icon |
| `--debug` | Verbose log |
| `-f, --transcribe-file WAV` | Transcribe a 16 kHz mono WAV and exit, headless (what `sokki transcribe` uses) |
| `--model ID` | Model for `--transcribe-file` |
| `--device-index N` | Compute device for `--transcribe-file` (see `--list-devices`) |
| `--list-devices`, `--list-models` | List them and exit |
| `--repeat N` | Run `--transcribe-file` N times (timing) |
| `--json` | JSON output for the above |
| `--print-default-settings` | All default settings as JSON (CONFIG.md is made from it) |

On macOS, start Sokki only with `open -a Sokki` (`open -a Sokki --args
--start-meeting`): a Sokki started from a shell gets the shell's macOS
permissions.

## Linux (`crates/sokki`)

One binary for the service and its command line: `sokki help` lists
`meeting`, `dictation`, `status`, `notes`, `open`, `speakers`, `name`,
`transcribe`, `ask`, `models`, `reload`, `events`, `hooks`, `doctor`.
`sokki hooks` works the same on both platforms ([HOOKS.md](HOOKS.md)), and
`sokki events` also streams every hook event. Its settings
live in `~/.config/sokki/sokki.toml`; edit the file, then `sokki reload`.
