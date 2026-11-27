# Sokki from the command line

Two command lines: `sokki` (one Rust binary, `crates/sokki`, for people,
scripts, hooks and agents, on macOS and Linux) and the app's own flags
(mostly for scripts and window managers). Everything runs on this computer.
Settings: [CONFIG.md](CONFIG.md).

## `sokki`

The same binary on both platforms. On the Mac it is the app's command line:
it talks to the running app over the local socket (`crates/sokki-api`), asks
the installed app for transcriptions, and reads the search index
(`crates/sokki-index`) directly. On Linux it is also the background service
(`sokki daemon`).

Install on the Mac: `install.sh` links `~/.local/bin/sokki` to the binary
inside `/Applications/Sokki.app` (`Contents/Resources/resources/bin/sokki`).
From a source checkout, `scripts/sokki` runs the same binary (the installed
app's, else a build in `target/`), so a link to it keeps working:

```bash
ln -s "$PWD/scripts/sokki" ~/.local/bin/sokki
```

| Command | What it does |
|---|---|
| `sokki transcribe FILE [--json]` | Any audio or video file to text with Sokki's speech model; `--json` for JSON. Needs `ffmpeg`. |
| `sokki ask [--think] [PROMPT]` | Asks the local language model Sokki uses for summaries. Text piped in is appended: `cat notes.md \| sokki ask "Summarise"`. |
| `sokki config [KEY [VALUE]]` | Lists every setting, shows one, or changes one (macOS; Linux edits `sokki.toml`). |
| `sokki status [--json]`, `sokki events` | What Sokki is doing; `events` streams it, and every hook event, as JSON lines. Needs Sokki running. |
| `sokki meeting record\|stop\|toggle\|dismiss` | Record a meeting, or answer the "record this call?" question. |
| `sokki dictation start\|stop\|toggle\|cancel` | Start or stop a dictation. |
| `sokki notes [-n N]`, `sokki open [ID\|latest]` | Recent meetings from the app; open a note. |
| `sokki speakers ID`, `sokki name ID SPEAKER NAME [--forget]` | A meeting's speakers; name one (the note is rewritten, the voice remembered). |
| `sokki search`, `get`, `meetings`, `tasks`, `decisions`, `people`, `index` | Search what Sokki recorded (below). Work while Sokki is closed. |
| `sokki note append`, `sokki item add` | Write back, for hooks (below). |
| `sokki hooks list\|allow\|revoke\|run\|test\|log` | Your own commands, run by Sokki on events: [HOOKS.md](HOOKS.md). |
| `sokki help` | The list. |

`transcribe` and `ask` wait while Sokki records or processes a meeting, so
the meeting keeps the Mac (a transcription during a call once cost a
meeting its call audio).

### Searching what Sokki recorded

Off until you allow it, because an agent in the cloud sends what it reads
off this computer:

```bash
sokki config agent_access summaries     # meetings, summaries, decisions, tasks, own notes
sokki config agent_access transcripts   # … plus the transcripts
sokki config agent_access dictations    # … plus the dictations
sokki config agent_access off           # nothing (the default)
```

`sokki config agent_access …` works only typed in a terminal: the setting
is the person's consent, and an agent's shell has no terminal. On Linux:
`access = "summaries"` under `[agents]` in
`~/.config/sokki/sokki.toml`. The typing journal is never shared. Every read
is logged locally in `agent-access.log` in the data folder (time, calling
program, command, ids returned; no text).

```bash
sokki search "Rahmenvertrag Kündigung" --since 30d --json
sokki search Vertrag --person "Jana" --section transcript
sokki get m:meeting-20261103-100000-001@01:05 --window 2m
sokki meetings --person jana --since 2026-11-01
sokki tasks --owner me --status open
sokki decisions --since 14d --query Budget
sokki people
sokki index status        # what is indexed; `rebuild` builds it again from the files
```

| Flag | For | Meaning |
|---|---|---|
| `--person P` | search, meetings, tasks, decisions | `me`, a name, or a person id. In search: turns that person said (in a summary: meetings they were in). |
| `--since`, `--until` | all lists | `3h`, `14d`, `2w`, `6mo`, `1y`, `today`, `yesterday`, a date (`2026-11-20`) or an RFC 3339 time |
| `--kind` | search | `meeting`, `notes` (own notes), `dictation` |
| `--app APP` | search, meetings | the call app (`Zoom`) or, for dictations, the app the text went into |
| `--section S` | search | `transcript`, `summary`, `decisions`, `tasks`, `notes`, or a section a hook added (`crm`) |
| `--mode` | search | `both` (default), `words` (whole words, FTS5 unicode61) or `trigram` (parts of German compounds); see `docs/research/memory-index-recall-2026-11.md` |
| `-n N`, `--max-tokens T`, `--cursor C` | all | at most N results and about T tokens (4,000) per call; `next_cursor` continues |
| `--owner`, `--status` | tasks | `me` or a name; `open` (default), `done`, `all` |
| `--query TEXT` | decisions | decisions that contain the text |
| `--window` | get | the transcript around `@mm:ss`, e.g. `90s`, `2m` (default) |
| `--json` | all | JSON (below) instead of lines |

Unknown flags fail with the list of known ones; nothing is ignored.

**Ids.** A meeting is its folder name (`meeting-20261103-100000-001`). A
cite is `m:<id>@mm:ss` (a transcript turn), `m:<id>#summary` (a part
without a time), `n:<id>@mm:ss` (the person's own notes), `d:<n>` (a
dictation). `sokki get` takes any of them.

**JSON.** Every list answers `{query, filters_applied, results, next_cursor,
truncated, est_tokens}`; `truncated` says the token budget cut it. A result
has `cite`, `speaker`, `speaker_id`, `person_id` (`me`, or the name as a
slug; null for "Speaker 2"), `name_confirmed`, `channel` (`mic` = this
computer's person, `system` = the call), `start_ms`, `end_ms`, `section` and
`text`. No result: `hint` says which filter to drop, with how many results
each would give back. Tasks and decisions carry `evidence` (`cite`,
`quote`, `verified`, `origin`): where in the transcript it was said, and a
quote code found there word for word. For the summary's items it is code's
best match (`origin: "match"`), a pointer, not proof (`KNOWN_ISSUES.md`).

### Writing back (hooks)

```bash
echo "Kunde: Druckerei Muster" | sokki note append m:ID --section CRM [--replace]
sokki item add m:ID --kind custom:crm --text "Vertrag läuft aus" --quote "läuft im März aus" --at 00:10 [--owner …]
```

- `note append` adds a section to the meeting's note, before the transcript
  (`--replace` swaps its text, so a hook that runs again doesn't repeat
  itself). Sokki's own sections, and `#`/`##` headings inside the text, are
  refused.
- `item add` checks the quote against the transcript within a minute of
  `--at`, word for word, and fails if it isn't there (saying where it is, if
  elsewhere); the item is stored at the turn where the quote is. `--text` is
  the item in the hook's own words: it is shown at `summaries` access, the
  quote only from `transcripts` on. The check itself reveals what was said,
  so `item add` needs `transcripts`. Items are kept in `items.jsonl` in the
  meeting's folder, so a rebuild keeps them. `SOKKI_HOOK` names the hook in
  `added_by`.

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
| `SOKKI_DATA_DIR` | `~/Library/Application Support/<the installed app's bundle id>` | everything on the Mac (tests: a made-up folder) |
| `SOKKI_SETTINGS` | `settings_store.json` in the data folder | `config`, `agent_access`, the model `ask` uses |
| `SOKKI_SOCKET` | `<data folder>/sokki.sock` (Linux: `$XDG_RUNTIME_DIR/sokki/sokki.sock`) | the local API |
| `SOKKI_INDEX` | `index.sqlite` in the data folder | the search index |
| `SOKKI_SCOPE` | — | narrows `agent_access` for one call (a hook's scope); never widens it |
| `SOKKI_HOOK` | `cli` | who added an item or a section |
| `SOKKI_MODEL` | `meetings.llm_model`, else `qwen3.5:9b` | `ask` |
| `SOKKI_APP_BIN` | the installed app's binary | `transcribe` |
| `SOKKI_MEETINGS_INDEX` | the app's `meetings/index.json` | waiting during meetings (tests) |
| `SOKKI_WAIT_EVERY_S` | `10` | how often the wait checks |

The Claude Code skill for agents: [docs/skills/sokki/SKILL.md](docs/skills/sokki/SKILL.md)
(copy it to `~/.claude/skills/sokki/`).

Made-up data to try it without real meetings:
`cargo run --example make_fixture --features fixtures -- /tmp/sokki-demo` in
`crates/sokki-index`, then `SOKKI_DATA_DIR=/tmp/sokki-demo sokki search Vertrag`.

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
permissions. The `sokki` command never starts the app; `sokki transcribe`
runs its binary only headless (`--transcribe-file`, which exits at once
when done and opens no window).

## Linux (`crates/sokki`)

The same binary is the service (`sokki daemon`, a systemd user unit) and its
command line: `sokki help` lists `meeting`, `dictation`, `status`, `notes`,
`open`, `speakers`, `name`, `transcribe`, `ask`, the index commands above,
`models`, `reload`, `events`, `hooks`, `doctor`. `sokki hooks` works the same
on both platforms ([HOOKS.md](HOOKS.md)), and `sokki events` also streams
every hook event. Its settings live in `~/.config/sokki/sokki.toml`; edit
the file, then `sokki reload`.
