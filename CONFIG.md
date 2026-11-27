# Configuration

All settings of Sokki live in **one JSON file**:

```
~/Library/Application Support/org.sokki.app/settings_store.json
```

Shape: `{"settings": { …keys below… }}`. Edit it with any tool — the app
watches the file and applies changes within about a second, even while it is
running (shortcut changes, theme and meeting detection are re-applied; every
other setting is read when it is next used). Invalid JSON is ignored until
the file is valid again; a single invalid value falls back to its default
without touching the other settings.

## From the command line

`sokki config KEY VALUE` changes one setting, checked against its type
(`sokki config meetings.auto_start_show_after_s 90`); the app applies it
within a second. `sokki config` lists every setting, `sokki config KEY`
shows one. All commands: [CLI.md](CLI.md).

## For Claude

- Read: `jq .settings "~/Library/Application Support/org.sokki.app/settings_store.json"`
- Change one value, keeping everything else, e.g. the notes folder:

  ```sh
  F="$HOME/Library/Application Support/org.sokki.app/settings_store.json"
  jq --arg d "$HOME/Notes/Meetings" '.settings.meetings.notes_dir = $d' "$F" > "$F.tmp" && mv "$F.tmp" "$F"
  ```

- Write the whole file atomically (temp file + `mv`) so the app never reads
  a half-written file.
- Check the log after a change: `~/Library/Logs/org.sokki.app/sokki.log`
  shows "Applied settings edited outside the app".
- Keys and values must match the tables below; enum values are listed
  exactly as stored.
- "Your data" choices, remembered voices and phone pairing are separate
  files; see "Other files" at the end.

Most useful keys:

| What | Key |
|---|---|
| Notes folder | `meetings.notes_dir` (string path, `null` = `~/Documents/Sokki`; existing installs from before schema 5 keep `~/Documents/Handy Meetings`) |
| Your name in transcripts | `meetings.user_name` |
| Language model | `meetings.llm_provider` (`local`, `anthropic`, `openai`, `openrouter`), `meetings.llm_model` (local), `meetings.cloud_model` |
| Keep audio | `meetings.keep_audio_days` |
| Meeting shortcut | `bindings.meeting.current_binding` (e.g. `"option+m"`) |
| Dictation shortcut | `bindings.transcribe.current_binding` |
| Speech model | `selected_model` |
| Custom words for dictation | `custom_words` |
| Developer mode: Keep what I write | `keep_what_i_write`, `learn_words` |
| Clean up dictations | `dictation_cleanup_enabled` |
| Fix typos as you type (macOS) | `autocorrect_enabled` |


## Meeting settings (`meetings.*`)

| Key | Type | Default | Meaning |
|---|---|---|---|
| `meetings.notes_dir` | `Option<String>` | `null` | Folder the Markdown notes are written to. `None`: ~/Documents/Sokki. |
| `meetings.user_name` | `String` | `"Ich"` | How the user is named in transcripts (the main voice on the microphone). |
| `meetings.diarization_enabled` | `bool` | `true` | Separate speakers within each track. |
| `meetings.glossary` | `Vec<String>` | `[]` | Your dictionary: correctly spelled company names, customers, colleagues, products and jargon, e.g. ["Kelasur", "OVDA", "Claude Code"]. The summary prompt lists them, so the summary spells them right where the transcript has a misheard form ("Cloud Code"). The transcript itself stays as recognised. The dictation `custom_words` are added automatically. Dictation uses these words too: exact "heard as" fixes, and the sound-and-spelling swap for words of 6+ letters. |
| `meetings.summary_enabled` | `bool` | `true` | Write a summary, decisions and action items with a language model. |
| `meetings.footer_enabled` | `bool` | `true` | End every summary (note, app, phone, copy) with the Sokki line ("Summarized by AI. Local, free, open source. Dictation app and meeting notetaker: sokki.org"). Only a supporter (`supporter_code`) can turn it off; without a valid code the line stays even when this is false. |
| `meetings.supporter_code` | `String` | `""` | Supporter code: proof of at least €20 paid for Sokki, checked offline (`sokki_engine::supporter`). Empty: none. |
| `meetings.llm_provider` | `String` | `"builtin"` | Which language model summarises: "builtin" (Gemma 4 E4B run by llama.cpp inside Sokki, on Macs with Apple silicon), "local" (the server at `llm_base_url`, normally Ollama; nothing leaves the computer either way), or a cloud provider: "anthropic", "openai" or "openrouter". A cloud provider gets the transcript; its API key is the one stored for that provider under `post_process_api_keys`. |
| `meetings.cloud_model` | `String` | `""` | Model for the cloud provider. Empty: the provider's default (see `meeting::process::default_cloud_model`). |
| `meetings.llm_base_url` | `String` | `"http://localhost:11434"` | Local server: Ollama (`http://localhost:11434`) or any OpenAI-compatible server (a URL ending in `/v1`). |
| `meetings.llm_model` | `String` | `"gemma4:e4b"` | Model on the local server. |
| `meetings.keep_audio_days` | `u32` | `0` | Keep the recorded audio for this many days after the note is written. 0 deletes it as soon as processing succeeds. |
| `meetings.detection_enabled` | `bool` | `true` | Offer to record when a meeting app starts using the microphone. |
| `meetings.chat_notice_enabled` | `bool` | `false` | When Sokki records a detected call (Zoom, Teams, Google Meet in Chrome, Arc, Edge or Brave), post one line into the meeting's chat, once per call, from the user's account: that Sokki records, locally, and that "stop" in the chat stops and deletes the recording. Uses Accessibility. |
| `meetings.call_names_enabled` | `bool` | `true` | Name the speakers of a recorded call after who the call app shows as speaking: Zoom's active-speaker tile, Google Meet's captions (Sokki turns Meet's captions on for this, visible to the user only). Uses Accessibility (`meeting/call_speakers.rs`). |
| `meetings.mail_notes_auto` | `bool` | `true` | After a meeting, send its notes (summary, decisions, action items; no transcript) to everyone invited in the calendar, from the user's own Gmail or Apple Mail and in their name. On by default since 2026-11-25 (older installs: turned on once, unless `mail_notes_auto_set`); the note's "Send to everyone" button works either way. |
| `meetings.mail_notes_auto_set` | `bool` | `false` | The person set the switch above themselves; a later default never changes it. |
| `meetings.gmail_address` | `Option<String>` | `null` | The Gmail address notes are sent from, once Gmail is connected (Settings › Meetings › Notes by mail). Shown only; the token is in the Keychain. None: Apple Mail sends. |
| `meetings.gmail_testing` | `bool` | `false` | Offer Gmail before Google has reviewed Sokki (test mode: only the Google project's test users can connect; everyone else would see Google's "Access blocked"). For Robin's test Mac; set by hand in the settings file. Off: Apple Mail only, until the review passes. |
| `meetings.slack_post_huddles` | `bool` | `false` | After a Slack huddle, post the summary (with the Sokki line) into the huddle's thread as the user, with the transcript as a file. Needs "Connect Slack"; connecting turns it on. |
| `meetings.phone_link_enabled` | `bool` | `false` | Accept recordings from the Sokki phone app: direct and end-to-end encrypted from any network, paired by QR code. Pairing state (this Mac's key, code, paired phones) is in `phone_link.json` in the app data folder. |
| `meetings.silence_hint_enabled` | `bool` | `true` | While a meeting records: say so when the microphone brings no speech (muted, wrong device) or a detected call brings no sound, once per recording (`meeting/silence.rs`). |
| `meetings.auto_start_enabled` | `bool` | `true` | Start a meeting by itself when the user's own voice talks near the Mac and no call app runs; stop it after two minutes without speech. On by default (Robin, 2026-11-26), but it only listens once the user's own voice is known (a voice profile named `user_name`). A one-line card has an Auto-Start switch that turns this off (`meeting/auto.rs`). While it listens, the microphone stays open. |
| `meetings.auto_start_set` | `bool` | `false` | The person set auto start themselves (the card's switch); a later default never changes it. |
| `meetings.auto_start_show_after_s` | `u32` | `60` | Seconds Sokki listens in on a conversation, in memory only, before it shows it as a meeting. A conversation that ends sooner is dropped and nothing is kept. At most 240. |
| `meetings.auto_start_min_talk_s` | `u32` | `120` | A meeting that started by itself is deleted when its conversation lasted less than this many seconds, counted from when Sokki began listening in. |

## All other settings

| Key | Type | Default | Meaning |
|---|---|---|---|
| `settings_schema_version` | `u32` | `3` | Internal settings schema marker for one-time migrations. Fresh installs start at the current version; existing stores missing this key are treated as version 0 and migrated forward. |
| `bindings` | `HashMap<String, ShortcutBinding>` | `"(see file)"` | Defaults to empty on partial stores; the load path merges in the default bindings for any missing keys before the settings are used. |
| `shortcut_activation` | `ShortcutActivation` | `"hold_or_toggle"` | Replaces the pre-0.10 `push_to_talk` bool; stores missing this key are migrated from it in `apply_settings_migrations`. |
| `hold_threshold_ms` | `u64` | `300` | Hold-or-toggle only: a press held at least this long is push-to-talk, anything shorter is a tap that locks recording on. |
| `audio_feedback` | `bool` | `false` | Play a sound when dictation starts and stops. |
| `audio_feedback_volume` | `f32` | `1.0` | Volume of those sounds, 0.0–1.0. |
| `sound_theme` | `SoundTheme` | `"marimba"` | `marimba`, `pop` or `custom`. |
| `start_hidden` | `bool` | `false` | Start without showing the settings window. |
| `autostart_enabled` | `bool` | `false` | Start at login (applied when changed in the app; after editing the file, restart once). |
| `update_checks_enabled` | `bool` | `true` | Sokki downloads a new release in the background, offers “Update now” in the footer, and installs it by itself after the first login of the day; never while recording or processing. Off: update by hand with install.sh. |
| `show_whats_new_on_update` | `bool` | `true` | Show release notes after an update. |
| `whats_new_last_seen_version` | `String` | `"0.10.1"` | The app version whose What's New the user has already seen. Fresh installs default to the current version (nothing is "new" to them). Existing users upgrading from before this key existed are blanked by the migration so they see the current release's notes — see `apply_settings_migrations`. |
| `selected_model` | `String` | `""` | Speech model for dictation and meetings, by id (e.g. `parakeet-tdt-0.6b-v3`) or Hugging Face path; see the Models page. |
| `onboarding_completed` | `bool` | `false` | First-run setup finished. |
| `always_on_microphone` | `bool` | `false` | Keep the microphone open between dictations (faster start, mic indicator stays on). |
| `selected_microphone` | `Option<String>` | `null` | Dictation microphone by name; `null` = system default. Meetings use the system default. |
| `selected_channel` | `Option<u16>` | `null` | Which input channel to use on the selected microphone device. None means "average all channels" (original behavior). |
| `clamshell_microphone` | `Option<String>` | `null` | Microphone to use when the MacBook lid is closed; `null` = automatic. |
| `selected_output_device` | `Option<String>` | `null` | Output device for feedback sounds; `null` = system default. |
| `translate_to_english` | `bool` | `false` | Translate dictation to English (models that support it). |
| `selected_language` | `String` | `"auto"` | Dictation language code (`de`, `en`, …) or `auto`. |
| `spoken_languages` | `Vec<String>` | `[]` | Every language the person dictates in (codes, e.g. ["en", "zh"]), picked during onboarding. Empty until then. |
| `overlay_position` | `OverlayPosition` | `"bottom"` | Dictation overlay position: `top` or `bottom`. |
| `debug_mode` | `bool` | `false` | Show the debug page and extra logging. |
| `log_level` | `LogLevel` | `"debug"` | `trace`, `debug`, `info`, `warn` or `error`. |
| `custom_words` | `Vec<String>` | `["Sokki"]` | Words to prefer in dictation (names, jargon), e.g. `["Kelasur"]`. Starts with `Sokki`. |
| `keep_what_i_write` | `bool` | `true` | Developer mode, "Keep what I write": saves the text the user sends or leaves in any app, with the app and the time, in `<app data>/typing_log/` on this Mac only (`sokki-journal`). Never password fields, secure input, terminals or password managers. On by default since schema 9 (Robin, 2026-11-26: the data stays on the Mac, and word learning and autocorrect learn from it). |
| `journal_excluded_apps` | `Vec<String>` | `[]` | Apps "Keep what I write" never reads, as bundle ids, on top of the built-in list (password managers, terminals). E.g. ["com.apple.mail"]. |
| `learn_words` | `bool` | `true` | Learn names and terms from what "Keep what I write" keeps: once a day while the Mac is idle, a local language model checks each new word and Sokki speaks it to itself to hear how the speech model writes it (`learn_words.rs`). Does nothing while the journal is off. On by default (schema 11, Robin, 2026-11-26). |
| `vocabulary_learned` | `Vec<String>` | `[]` | Words learned from the user's writing (Vocabulary › Learned from your writing). They reach dictation and meeting summaries like the Vocabulary list. |
| `vocabulary_heard_as` | `Vec<HeardAs>` | `[]` | How the speech model writes a word, e.g. `{"heard": "Fassel", "word": "Vercel", "source": "hear_test"}` (`source`: `hear_test` or `correction`). Dictations get these fixed exactly. |
| `vocabulary_rejected` | `Vec<String>` | `[]` | Learned words and "heard as" forms the user removed, lowercase: never learned again. |
| `learn_words_last_run` | `u64` | `0` | When word learning last ran, ms since 1970. |
| `model_unload_timeout` | `ModelUnloadTimeout` | `"min5"` | Unload the model after inactivity: `never`, `immediately`, `min2`, `min5`, `min10`, `min15`, `hour1`. |
| `word_correction_threshold` | `f64` | `0.18` | How close a transcribed word must be to a custom word to be replaced (0–1; lower = stricter). |
| `history_limit` | `usize` | `5` | Dictations kept in History. |
| `recording_retention_period` | `RecordingRetentionPeriod` | `"preserve_limit"` | Dictation audio kept: `never`, `preserve_limit`, `days3`, `weeks2`, `months3`. |
| `paste_method` | `PasteMethod` | `"ctrl_v"` | How dictation is inserted: `ctrl_v` (Cmd+V on macOS), `direct`, `none`, `shift_insert`, `ctrl_shift_v`, `external_script`. |
| `clipboard_handling` | `ClipboardHandling` | `"dont_modify"` | `dont_modify` restores the clipboard after pasting; `copy_to_clipboard` leaves the text there. |
| `auto_submit` | `bool` | `false` | Press a key after pasting (e.g. send a chat message). |
| `auto_submit_key` | `AutoSubmitKey` | `"enter"` | `enter`, `ctrl_enter` or `cmd_enter`. |
| `post_process_enabled` | `bool` | `false` | AI cleanup of dictation (second shortcut). |
| `post_process_provider_id` | `String` | `"openai"` | Cleanup provider id: `openai`, `anthropic`, `groq`, `openrouter`, `cerebras`, `zai`, `apple_intelligence`, `bedrock_mantle`, `custom`. |
| `post_process_providers` | `Vec<PostProcessProvider>` | `"(see file)"` | Provider definitions (base URLs); `custom` can point at Ollama (`http://localhost:11434/v1`). |
| `post_process_api_keys` | `SecretMap` | `{"apple_intelligence": "", "groq": "", "cerebras": "", "o…` | API key per provider id. Secret: don't paste keys into chats. |
| `post_process_models` | `HashMap<String, String>` | `{"openrouter": "", "groq": "", "apple_intelligence": "App…` | Model per provider id. |
| `post_process_prompts` | `Vec<LLMPrompt>` | `"(see file)"` | Cleanup prompts (`id`, `name`, `prompt` with `${output}`). |
| `post_process_selected_prompt_id` | `Option<String>` | `null` | Id of the prompt in use. |
| `mute_while_recording` | `bool` | `false` | Mute system output while dictating (never during meeting recording). |
| `append_trailing_space` | `bool` | `false` | Add a space after pasted dictation. |
| `app_language` | `String` | `"en-GB"` | Interface language, e.g. `de`, `en-GB`. |
| `theme` | `Theme` | `"system"` | `system`, `light` or `dark`. |
| `experimental_enabled` | `bool` | `false` | Show experimental features. |
| `lazy_stream_close` | `bool` | `false` | Keep the live stream open briefly after dictation (faster consecutive dictations). |
| `airpods_mode` | `bool` | `false` | Experimental, macOS only: move a Bluetooth headset (AirPods) to this Mac while recording, so its microphone does not return silence while another Apple device is playing through it. |
| `keyboard_implementation` | `KeyboardImplementation` | `"native_keys"` | Shortcut backend: `tauri`, `native_keys` or `none` (restart after editing). |
| `show_tray_icon` | `bool` | `true` | Show the menu bar icon (needed for the meeting status). |
| `paste_delay_ms` | `u64` | `60` | Wait before pasting, ms. |
| `paste_delay_after_ms` | `u64` | `60` | Wait after pasting before restoring the clipboard, ms. |
| `reliable_paste` | `bool` | `false` | Debug-gated ("beta") receipt-sequenced paste: restore the clipboard only after the target app actually reads the transcript, instead of after a fixed delay. See `paste_tx`. macOS and Windows only. |
| `typing_tool` | `TypingTool` | `"auto"` | Linux only. |
| `external_script_path` | `Option<String>` | `null` | Script for `paste_method = external_script`. |
| `filler_word_removal_enabled` | `bool` | `true` | Remove ähm/um from dictation. |
| `dictation_cleanup_enabled` | `bool` | `false` | Clean up each dictation with the built-in cleanup model before it is pasted (`dictation_cleanup`): fillers, repetitions and self-corrections out, misheard words and grammar fixed, paragraphs and lists in. Only while the model is downloaded. Off until Robin has tried it (release manager, 25.11.2026). |
| `autocorrect_enabled` | `bool` | `false` | Fix typos as you type, in the apps where macOS doesn't (Chrome, Slack, Claude …), with the phone keyboard's model (`autocorrect`). Backspace right after a fix undoes it. Never in Apple's apps, terminals, code editors, password managers or password fields. macOS only. Off until the user turns it on (Robin, 25.11.2026: for everyone, off by default). Each fix, undo and skipped fix is listed in `autocorrect_fixes.jsonl` in the app data folder (typed word and fix only, stays on this Mac). |
| `custom_filler_words` | `Option<Vec<String>>` | `null` | Your own filler words; `null` = built-in list. |
| `chinese_script` | `ChineseScript` | `"simplified"` | Fresh installs default from the OS locale; existing stores are migrated in `apply_settings_migrations`. |
| `transcribe_accelerator` | `TranscribeAcceleratorSetting` | `"auto"` | `auto`, `cpu` or `gpu`. |
| `transcribe_gpu_device` | `Option<String>` | `null` | GPU by key; `null` = automatic. |
| `extra_recording_buffer_ms` | `u64` | `0` | Keep recording this long after releasing the dictation key. |
| `vad_enabled` | `bool` | `true` | Cut silence from dictation before transcribing. |
| `vad_backend` | `VadBackend` | `"silero"` | Experimental detector implementation. Silero remains the stable default. |
| `long_form_chunking` | `bool` | `true` | Decode long audio in chunks cut at pauses, for models whose accuracy degrades on long audio. Independent of `vad_enabled`, which only decides whether silence is dropped from recordings. |
| `language_detection` | `LanguageDetectionMode` | `"when_needed"` | `when_needed` or `always`. |
| `language_detection_window_secs` | `u32` | `5` | Seconds of speech the language detector hears (debug). Read through [`AppSettings::language_detection_window_samples`], which clamps it. |
| `overlay_style` | `OverlayStyle` | `"live"` | Which recording overlay to show: None / Minimal / Live. Streaming mode is not gated on this — that follows model capability. Migrated from the old `overlay_position` (position `none` → style `None`). |
| `agent_access` | `String` | `"off"` | What scripts, hooks and AI agents may read through the `sokki` command (`sokki search`, `get`, `meetings`, `tasks`, …; CLI.md): "off" (nothing), "summaries" (the meeting list, summaries, decisions, tasks, the person's own notes), "transcripts" (plus the meeting transcripts) or "dictations" (plus the dictations). Off by default: what an agent running in the cloud reads leaves this Mac. The typing journal is never shared. No Settings screen yet; `sokki config agent_access summaries` sets it. |

## Shortcut bindings (`bindings.<id>.current_binding`)

| id | Default | What |
|---|---|---|
| `cancel` | `escape` | Cancels the current recording. |
| `meeting` | `option+m` | Starts or stops recording a meeting (microphone and system audio as two tracks). |
| `transcribe` | `option+space` | Converts your speech into text. |
| `transcribe_with_post_process` | `option+shift+space` | Converts your speech into text and applies AI post-processing. |

## Other files

These sit in the same folder but are not part of `settings_store.json`.

| File | What | Editing |
|---|---|---|
| `voice-data/config.json` | "Your data": `improve` (Help improve Sokki), `buyers` (Ask me when there's a buyer) and `support` (Share all my recordings), each `true`/`false`. Read each time it is used, so an edit applies on the next use. | These three are the user's consent to share data: change them only when the user asks for exactly that. Leave every other field (`install_id`, counters, dates) alone. |
| `voice-profiles.json` | People: `profiles[].name`, their `emails` (calendar invitees get named by address), and only with the person's consent (`consent_at`) their voice `embeddings`, dropped a year after `last_heard`. Read when a meeting is processed and by the Sprecher page. | Adding or fixing `emails` by hand is fine. Removing a profile forgets that person. Embeddings come only from the app. |
| `phone_link.json` | This Mac's key, the pairing code and paired phones. | Don't edit; pair and remove phones in the app. |
| `history.db` | Dictation history (SQLite). | Read only. |
| `meetings/` | `index.json` (every meeting) and each meeting's audio. | Read only; delete meetings in the app. |
| `~/Movies/Sokki/<date time>/` | A meeting's screen video: `screen.mov`, copies of the audio tracks, the finished `<date time>.mp4` and `video.json`. Outside the app data folder. | Read only; deleting the meeting in the app moves it to the Trash. |

Generated by `scripts/gen_config_docs.py`; regenerate after changing settings.rs.
