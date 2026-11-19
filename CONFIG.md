# Configuration

All settings of Handy Meetings live in **one JSON file**:

```
~/Library/Application Support/dev.handymeetings.app/settings_store.json
```

Shape: `{"settings": { …keys below… }}`. Edit it with any tool — the app
watches the file and applies changes within about a second, even while it is
running (shortcut changes, theme and meeting detection are re-applied; every
other setting is read when it is next used). Invalid JSON is ignored until
the file is valid again; a single invalid value falls back to its default
without touching the other settings.

## For Claude

- Read: `jq .settings "~/Library/Application Support/dev.handymeetings.app/settings_store.json"`
- Change one value, keeping everything else, e.g. the notes folder:

  ```sh
  F="$HOME/Library/Application Support/dev.handymeetings.app/settings_store.json"
  jq --arg d "$HOME/Notes/Meetings" '.settings.meetings.notes_dir = $d' "$F" > "$F.tmp" && mv "$F.tmp" "$F"
  ```

- Write the whole file atomically (temp file + `mv`) so the app never reads
  a half-written file.
- Check the log after a change: `~/Library/Logs/dev.handymeetings.app/handy.log`
  shows "Applied settings edited outside the app".
- Keys and values must match the tables below; enum values are listed
  exactly as stored.

Most useful keys:

| What | Key |
|---|---|
| Notes folder | `meetings.notes_dir` (string path, `null` = `~/Documents/Sokki`; existing installs keep `~/Documents/Handy Meetings`) |
| Your name in transcripts | `meetings.user_name` |
| Language model | `meetings.llm_provider` (`local`, `anthropic`, `openai`, `openrouter`), `meetings.llm_model` (local), `meetings.cloud_model` |
| Keep audio | `meetings.keep_audio_days` |
| Meeting shortcut | `bindings.meeting.current_binding` (e.g. `"option+shift+m"`) |
| Dictation shortcut | `bindings.transcribe.current_binding` |
| Speech model | `selected_model` |
| Custom words for dictation | `custom_words` |


## Meeting settings (`meetings.*`)

| Key | Type | Default | Meaning |
|---|---|---|---|
| `meetings.notes_dir` | `Option<String>` | `null` | Folder the Markdown notes are written to. `None`: ~/Documents/Sokki (~/Documents/Handy Meetings in the Handy build). |
| `meetings.user_name` | `String` | `"Ich"` | How the user is named in transcripts (the main voice on the microphone). |
| `meetings.diarization_enabled` | `bool` | `true` | Separate speakers within each track. |
| `meetings.correction_enabled` | `bool` | `true` | Fix misheard dictionary words (`glossary`) in the transcript with the language model before the summary. Only words that sound like a dictionary entry are changed; every change is listed at the end of the note. Without dictionary entries nothing is corrected. |
| `meetings.glossary` | `Vec<String>` | `[]` | Your dictionary: correctly spelled company names, customers, colleagues, products and jargon, e.g. ["Kalasar", "AVDO", "Claude Code", "Petard", "Noxi"]. Misheard forms are fixed ("Cloud Code" → "Claude Code"), and entries are never changed themselves, so add names that must stay as they are. Keep it short: very short or everyday-sounding words (e.g. "Lead") match more often. The dictation `custom_words` are added automatically. |
| `meetings.summary_enabled` | `bool` | `true` | Write a summary, decisions and action items with a language model. |
| `meetings.footer_enabled` | `bool` | `true` | End every note, and the summary shown in the app and on the phone, with "Transcribed on my Mac with sokki.org". |
| `meetings.llm_provider` | `String` | `"local"` | Which language model corrects and summarises: "local" (the server at `llm_base_url`, normally Ollama; nothing leaves the computer), or a cloud provider: "anthropic", "openai" or "openrouter". A cloud provider gets the transcript; its API key is the one stored for that provider under `post_process_api_keys`. |
| `meetings.cloud_model` | `String` | `""` | Model for the cloud provider. Empty: the provider's default (see `meeting::process::default_cloud_model`). |
| `meetings.llm_base_url` | `String` | `"http://localhost:11434"` | Local server: Ollama (`http://localhost:11434`) or any OpenAI-compatible server (a URL ending in `/v1`). |
| `meetings.llm_model` | `String` | `"qwen3.5:9b"` | Model on the local server. |
| `meetings.keep_audio_days` | `u32` | `0` | Keep the recorded audio for this many days after the note is written. 0 deletes it as soon as processing succeeds. |
| `meetings.detection_enabled` | `bool` | `true` | Offer to record when a meeting app starts using the microphone. |

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
| `update_checks_enabled` | `bool` | `true` | Ignored in this fork: the upstream updater is always off. |
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
| `custom_words` | `Vec<String>` | `["Sokki"]` | Words to prefer in dictation (names, jargon), e.g. `["Kalasar"]`. Starts with `Sokki`. |
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
| `keyboard_implementation` | `KeyboardImplementation` | `"handy_keys"` | Shortcut backend: `tauri`, `handy_keys` or `none` (restart after editing). |
| `show_tray_icon` | `bool` | `true` | Show the menu bar icon (needed for the meeting status). |
| `paste_delay_ms` | `u64` | `60` | Wait before pasting, ms. |
| `paste_delay_after_ms` | `u64` | `60` | Wait after pasting before restoring the clipboard, ms. |
| `reliable_paste` | `bool` | `false` | Debug-gated ("beta") receipt-sequenced paste: restore the clipboard only after the target app actually reads the transcript, instead of after a fixed delay. See `paste_tx`. macOS and Windows only. |
| `typing_tool` | `TypingTool` | `"auto"` | Linux only. |
| `external_script_path` | `Option<String>` | `null` | Script for `paste_method = external_script`. |
| `filler_word_removal_enabled` | `bool` | `true` | Remove ähm/um from dictation. |
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

## Shortcut bindings (`bindings.<id>.current_binding`)

| id | Default | What |
|---|---|---|
| `cancel` | `escape` | Cancels the current recording. |
| `meeting` | `option+shift+m` | Starts or stops recording a meeting (microphone and system audio as two tracks). |
| `transcribe` | `option+space` | Converts your speech into text. |
| `transcribe_with_post_process` | `option+shift+space` | Converts your speech into text and applies AI post-processing. |

Generated by `scripts/gen_config_docs.py`; regenerate after changing settings.rs.
