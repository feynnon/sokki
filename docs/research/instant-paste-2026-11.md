# Instant paste: the dictation in the field at once, its end cleaned in place

26 Nov 2026, on Robin's Mac (M1 Pro, 32 GB). Numbers only; the dictations are private and not in this repo. Per-dictation numbers: `data/instant_paste.json`.

**Task.** Robin, 26 Nov 2026: "Text erscheint SOFORT am Ende des Transkripts in der Box drin. Und wird dann nochmal nach 2,4 s edited (man könnte schon abschicken – für Orte wo die letzte Formatierung egal ist)." He also chose the dimmed look for raw words in the live panel and, for the wait before the edit, the small pill (version 2 of four on the design canvas, page "Offen").

## Short version

- **The text is in the field a median 0.40 s after the stop instead of 2.41 s.** The edit follows a median 2.28 s after the stop, about when the single paste used to come.
- The first paste is the paragraphs cleaned while speaking, then the rest as transcribed. In half of the dictations (median 53 %) the first paste already starts with more than half of the final text's words unchanged. In 6 of 34 there was nothing to edit.
- The edit replaces only the end that changed, and only while the field still ends with exactly the first paste at the cursor, no key was pressed since and the same app is in front. Otherwise the field is left alone; the history has the cleaned text.
- Where Sokki can't read and select the field through Accessibility, it pastes once, cleaned, as before.

## How it works

- `sokki_engine::cleanup::Draft`: at the stop, after the final transcript, the cleanup hands over `fixed` (paragraphs cleaned live, which the last steps never change) and `rest` (the open paragraph and the words not cleaned yet) before it runs the last steps.
- `src-tauri/src/instant_paste.rs`: decides at the stop whether the field is editable (Accessibility trusted, a readable text field with a settable selection, Cmd+V paste, no auto-submit, no transcription hook), pastes the draft on its own thread, then shrinks the panel to its pill.
- `sokki_autocorrect::tail::replace_tail`: finds the first paste at the end of the text before the cursor (each run of whitespace counts as one, because rich editors show a blank line as one or two line breaks and a trailing space as a no-break space), selects from the first word that changed to the cursor, and pastes the cleaned end over it. Then it checks the field holds what was meant.
- Undo in the app: the first Cmd+Z brings the first paste back, the second removes it.
- Chrome shows page fields to Accessibility only in its screen-reader mode, about 2 s after it is asked; Sokki asks when a dictation with the cleanup starts (KNOWN_ISSUES M1).

## Replay of Robin's 34 Mac dictations

Same method as `live-pieces-and-summary-preload-2026-11.md`: each dictation played in real time as the microphone of a test app with Robin's settings, cleanup on, 30-word live pieces. Test mode pastes nothing, so these times leave out the paste itself (Sokki's clipboard paste takes about 0.1–0.3 s).

| | Instant paste | One paste (before, same build otherwise) |
|---|---|---|
| Text in the field after the stop, median / p90 / max | 0.40 / 0.83 / 3.30 s | 2.41 s median, 5.49 s max |
| Edit after the stop, median / p90 / max | 2.28 / 3.46 / 5.23 s | – |
| Nothing to edit (first paste already final) | 6 of 34 | – |
| Failed | 0 | 0 |

By length (median, first paste / edit): up to 30 words 0.21 / 1.82 s; 30–60 words 0.32 / 2.44 s; 60–140 words 0.68 / 2.39 s; the two longest (177 and 480 words) 2.78 / 4.74 s. The first paste waits for the second transcription of the whole recording, which takes up to 3.1 s for 175 s of audio (`live-vs-final-2026-11.md`).

## Not measured yet

- **Real text fields.** Pasting needs the target window in front, so the lab test (`scripts/autocorrect-lab.sh paste-edit`, `crates/sokki-autocorrect/examples/paste_edit.rs`) runs at night when nobody uses the Mac: TextEdit, a Chrome textarea and a Chrome rich editor, three runs each, with Cmd+Z twice after each edit. Results: to be added (night of 26–27 Nov).
- **Slack, Claude, Notes.** Not in the lab (real accounts and notes). Each instant paste logs its outcome (`Instant paste: …`), so a few days of Robin's use will show how often the edit is skipped there.
- **Unit tests** (`tail.rs`): the edit replaces only the changed words, leaves the field when the person typed on, moved the cursor, selected text or sent the message, and copes with one line break shown for a blank line, emoji and umlauts.

## Sources

- Replay logs: private, `reports/Sokki eval data/live-vs-final/logs8` (instant paste) and `logs5` (one paste, 30-word pieces).
- Robin's choices: design canvas, page "Offen" (Live-Panel-2-Gedimmt, Sofort-2-Pille), 26 Nov 2026.
