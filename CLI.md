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

Everything Sokki indexed: meetings, summaries, decisions, tasks, own
notes, transcripts and dictations (the typing journal is never indexed).
No setting gates it (Robin, 27 Nov 2026: "as direct data access as
possible"). Whatever a cloud agent reads leaves this Mac. Every read through
`sokki` is logged locally in `agent-access.log` in the data folder (time,
calling program, command, ids returned; no text).

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

### SQL: the index directly

`index.sqlite` is a plain SQLite file. Any SQLite 3.45 or newer reads it,
FTS5 and trigram included (macOS's `/usr/bin/sqlite3` is 3.51). Read it,
don't write it: it is rebuilt from the notes, so writes go through
`sokki note append` and `sokki item add`.

```bash
sokki db path      # the file, brought up to date first
sokki db schema    # every table and view with its columns
sokki sql "SELECT id, title, started_at FROM meetings ORDER BY started_ms DESC LIMIT 5"
sqlite3 -readonly "$(sokki db path)" "SELECT cite, speaker, text FROM turns WHERE person_id = 'me' LIMIT 20"
```

`sokki sql` brings the index up to date, runs one read-only statement and
prints tab-separated rows (`--json`: `{columns, rows, row_count,
truncated}`, at most `--max-rows`, default 500). A direct `sqlite3` reads
the index as of its last sync: the app syncs after every note, rename,
delete and dictation, and `sokki db path` syncs too. Reading works while
the app writes (WAL; the index keeps its `-wal` and `-shm` files so
read-only readers can always open it).

The views are stable: within a schema version (`SELECT value FROM meta
WHERE key = 'schema'`, now 2) tables and views keep their columns; a new
version may add some.

| View or table | One row per | Columns |
|---|---|---|
| `meetings` | meeting | `id`, `cite`, `title`, `started_at`, `started_ms`, `duration_s`, `app`, `note_path`, `has_audio`, `online` |
| `turns` | speaker turn | `segment_id`, `meeting_id`, `cite` (`m:<id>@mm:ss`), `start_ms`, `end_ms`, `at_ms` (unix ms), `speaker`, `speaker_id`, `person_id` (`me`, the name as a slug, or NULL for "Speaker 2"), `name_confirmed`, `channel` (`mic` = this Mac's person, `system` = the call), `text`, `meeting_title`, `meeting_started_at`, `app` |
| `tasks` | task | `item_id`, `meeting_id`, `meeting_title`, `meeting_started_at`, `text`, `owner`, `owner_person_id`, `status` (`open`, `done`), `evidence_ms`, `evidence_quote`, `quote_verified`, `evidence_origin` |
| `decisions` | decision | as `tasks`, without owner and status |
| `hook_items` | item a hook added | `item_id`, `meeting_id`, `kind`, `text`, `owner`, `evidence_ms`, `evidence_quote`, `quote_verified`, `added_by` |
| `own_notes` | line of own notes | `segment_id`, `meeting_id`, `line`, `start_ms`, `typed` (1: the person's line, 0: Sokki's addition), `text` |
| `dictations` (table) | dictation | `source_id` (`d:<n>`), `history_id`, `at`, `at_ms`, `target_app`, `raw`, `cleaned`, `language` |
| `people` (table) | person | `person_id`, `name`, `is_me`, `meetings`, `talk_ms`, `first_ms`, `last_ms` |
| `sources`, `segments`, `items`, `source_people` (tables) | the rows behind the views | `sokki db schema` |

Full text: `segments_fts` (whole words, accents folded; `vertrag*` for a
prefix) and `segments_tri` (three-letter pieces: words inside compounds).
Both are keyed by `segments.id`, which is `turns.segment_id`:

```sql
-- "Vertrag" inside "Rahmenvertrag", best first
SELECT t.cite, t.speaker, t.text FROM segments_tri
  JOIN turns t ON t.segment_id = segments_tri.rowid
 WHERE segments_tri MATCH 'vertrag' ORDER BY bm25(segments_tri) LIMIT 10;

-- my open tasks, newest meeting first
SELECT meeting_started_at, text, evidence_quote FROM tasks
 WHERE owner_person_id = 'me' AND status = 'open' ORDER BY meeting_started_at DESC;

-- who talked how much in the last 30 days
SELECT speaker, sum(end_ms - start_ms) / 60000.0 AS minutes FROM turns
 WHERE at_ms > (strftime('%s', 'now') - 30 * 86400) * 1000
 GROUP BY speaker ORDER BY minutes DESC;

-- dictations that went into Mail
SELECT at, coalesce(cleaned, raw) AS text FROM dictations
 WHERE target_app = 'Mail' ORDER BY at_ms DESC;
```

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
  the item in the hook's own words; the quote is its evidence. Items are
  kept in `items.jsonl` in the meeting's folder, so a rebuild keeps them.
  `SOKKI_HOOK` names the hook in `added_by`.

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
| `SOKKI_DATA_DIR` | `$SOKKI_APP_DATA` (set by the hooks runner for each hook), else `~/Library/Application Support/<the installed app's bundle id>` | everything on the Mac, `sokki hooks` included (tests: a made-up folder or a test app's) |
| `SOKKI_SETTINGS` | `settings_store.json` in the data folder | `config`, the model `ask` uses |
| `SOKKI_SOCKET` | `<data folder>/sokki.sock` (Linux: `$XDG_RUNTIME_DIR/sokki/sokki.sock`) | the local API |
| `SOKKI_INDEX` | `index.sqlite` in the data folder | the search index |
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
