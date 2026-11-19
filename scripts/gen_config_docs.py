#!/usr/bin/env python3
"""Generates CONFIG.md: every setting with its doc comment, type and default.

Reads the field docs from src-tauri/src/settings.rs (AppSettings and
MeetingSettings) and the defaults from a defaults JSON file (written by
`handy --print-default-settings`). Run from the repo root:

    "src-tauri/target/debug/handy" --print-default-settings > /tmp/defaults.json
    python3 -I scripts/gen_config_docs.py /tmp/defaults.json > CONFIG.md
"""
import json
import re
import sys

SRC = "src-tauri/src/settings.rs"


# Docs for fields without a doc comment in settings.rs, with allowed values.
EXTRA = {
    "audio_feedback": "Play a sound when dictation starts and stops.",
    "audio_feedback_volume": "Volume of those sounds, 0.0–1.0.",
    "sound_theme": "`marimba`, `pop` or `custom`.",
    "start_hidden": "Start without showing the settings window.",
    "autostart_enabled": "Start at login (applied when changed in the app; after editing the file, restart once).",
    "update_checks_enabled": "Sokki downloads a new release in the background, offers “Update now” in the footer, and installs it by itself after the first login of the day; never while recording or processing. Off: update by hand with install.sh.",
    "show_whats_new_on_update": "Show release notes after an update.",
    "selected_model": "Speech model for dictation and meetings, by id (e.g. `parakeet-tdt-0.6b-v3`) or Hugging Face path; see the Models page.",
    "onboarding_completed": "First-run setup finished.",
    "always_on_microphone": "Keep the microphone open between dictations (faster start, mic indicator stays on).",
    "selected_microphone": "Dictation microphone by name; `null` = system default. Meetings use the system default.",
    "clamshell_microphone": "Microphone to use when the MacBook lid is closed; `null` = automatic.",
    "selected_output_device": "Output device for feedback sounds; `null` = system default.",
    "translate_to_english": "Translate dictation to English (models that support it).",
    "selected_language": "Dictation language code (`de`, `en`, …) or `auto`.",
    "overlay_position": "Dictation overlay position: `top` or `bottom`.",
    "debug_mode": "Show the debug page and extra logging.",
    "log_level": "`trace`, `debug`, `info`, `warn` or `error`.",
    "custom_words": "Words to prefer in dictation (names, jargon), e.g. `[\"Kalasar\"]`. Starts with `Sokki`.",
    "model_unload_timeout": "Unload the model after inactivity: `never`, `immediately`, `min2`, `min5`, `min10`, `min15`, `hour1`.",
    "word_correction_threshold": "How close a transcribed word must be to a custom word to be replaced (0–1; lower = stricter).",
    "history_limit": "Dictations kept in History.",
    "recording_retention_period": "Dictation audio kept: `never`, `preserve_limit`, `days3`, `weeks2`, `months3`.",
    "paste_method": "How dictation is inserted: `ctrl_v` (Cmd+V on macOS), `direct`, `none`, `shift_insert`, `ctrl_shift_v`, `external_script`.",
    "clipboard_handling": "`dont_modify` restores the clipboard after pasting; `copy_to_clipboard` leaves the text there.",
    "auto_submit": "Press a key after pasting (e.g. send a chat message).",
    "auto_submit_key": "`enter`, `ctrl_enter` or `cmd_enter`.",
    "post_process_enabled": "AI cleanup of dictation (second shortcut).",
    "post_process_provider_id": "Cleanup provider id: `openai`, `anthropic`, `groq`, `openrouter`, `cerebras`, `zai`, `apple_intelligence`, `bedrock_mantle`, `custom`.",
    "post_process_providers": "Provider definitions (base URLs); `custom` can point at Ollama (`http://localhost:11434/v1`).",
    "post_process_api_keys": "API key per provider id. Secret: don't paste keys into chats.",
    "post_process_models": "Model per provider id.",
    "post_process_prompts": "Cleanup prompts (`id`, `name`, `prompt` with `${output}`).",
    "post_process_selected_prompt_id": "Id of the prompt in use.",
    "mute_while_recording": "Mute system output while dictating (never during meeting recording).",
    "append_trailing_space": "Add a space after pasted dictation.",
    "app_language": "Interface language, e.g. `de`, `en-GB`.",
    "theme": "`system`, `light` or `dark`.",
    "experimental_enabled": "Show experimental features.",
    "lazy_stream_close": "Keep the live stream open briefly after dictation (faster consecutive dictations).",
    "keyboard_implementation": "Shortcut backend: `tauri`, `handy_keys` or `none` (restart after editing).",
    "show_tray_icon": "Show the menu bar icon (needed for the meeting status).",
    "paste_delay_ms": "Wait before pasting, ms.",
    "paste_delay_after_ms": "Wait after pasting before restoring the clipboard, ms.",
    "typing_tool": "Linux only.",
    "external_script_path": "Script for `paste_method = external_script`.",
    "filler_word_removal_enabled": "Remove ähm/um from dictation.",
    "custom_filler_words": "Your own filler words; `null` = built-in list.",
    "transcribe_accelerator": "`auto`, `cpu` or `gpu`.",
    "transcribe_gpu_device": "GPU by key; `null` = automatic.",
    "extra_recording_buffer_ms": "Keep recording this long after releasing the dictation key.",
    "vad_enabled": "Cut silence from dictation before transcribing.",
    "language_detection": "`when_needed` or `always`.",
}


def struct_fields(src: str, name: str):
    m = re.search(r"pub struct " + name + r" \{(.*?)\n\}", src, re.S)
    if not m:
        return []
    out, doc = [], []
    for line in m.group(1).splitlines():
        s = line.strip()
        if s.startswith("///"):
            doc.append(s[3:].strip())
        elif s.startswith("#["):
            continue
        else:
            f = re.match(r"pub (\w+): (.+),$", s)
            if f:
                out.append((f.group(1), f.group(2), " ".join(doc)))
            doc = []
    return out


def fmt(v):
    t = json.dumps(v, ensure_ascii=False)
    return t if len(t) <= 60 else t[:57] + "…"


HEADER = """# Configuration

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
- "Your data" choices and remembered voices are separate
  files; see "Other files" at the end.

Most useful keys:

| What | Key |
|---|---|
| Notes folder | `meetings.notes_dir` (string path, `null` = `~/Documents/Handy Meetings`) |
| Your name in transcripts | `meetings.user_name` |
| Language model | `meetings.llm_provider` (`local`, `anthropic`, `openai`, `openrouter`), `meetings.llm_model` (local), `meetings.cloud_model` |
| Keep audio | `meetings.keep_audio_days` |
| Meeting shortcut | `bindings.meeting.current_binding` (e.g. `"option+shift+m"`) |
| Dictation shortcut | `bindings.transcribe.current_binding` |
| Speech model | `selected_model` |
| Custom words for dictation | `custom_words` |

"""


OTHER_FILES = """## Other files

These sit in the same folder but are not part of `settings_store.json`.

| File | What | Editing |
|---|---|---|
| `voice-data/config.json` | "Your data": `improve` (Help improve Sokki) and `support` (Support Sokki & Open Source), both `true`/`false`. Read each time it is used, so an edit applies on the next use. | These two are the user's consent to share data: change them only when the user asks for exactly that. Leave every other field (`install_id`, counters, dates) alone. |
| `voice-profiles.json` | Remembered voices: `profiles[].name` with `embeddings`. Read when a meeting is processed. | Removing a profile forgets that voice. Embeddings come only from the app. |
| `history.db` | Dictation history (SQLite). | Read only. |
| `meetings/` | `index.json` (every meeting) and each meeting's audio. | Read only; delete meetings in the app. |
"""


def main():
    defaults = json.load(open(sys.argv[1]))
    src = open(SRC).read()
    print(HEADER)
    print("## Meeting settings (`meetings.*`)\n")
    print("| Key | Type | Default | Meaning |\n|---|---|---|---|")
    for name, ty, doc in struct_fields(src, "MeetingSettings"):
        d = defaults.get("meetings", {}).get(name)
        doc = doc or EXTRA.get(f"meetings.{name}", "–")
        print(f"| `meetings.{name}` | `{ty}` | `{fmt(d)}` | {doc} |")
    print("\n## All other settings\n")
    print("| Key | Type | Default | Meaning |\n|---|---|---|---|")
    for name, ty, doc in struct_fields(src, "AppSettings"):
        if name == "meetings":
            continue
        d = defaults.get(name)
        if name in ("bindings", "post_process_prompts", "post_process_providers"):
            d = "(see file)"
        doc = (doc + " " if doc else "") + EXTRA.get(name, "")
        print(f"| `{name}` | `{ty}` | `{fmt(d)}` | {doc.strip() or '–'} |")
    print("\n## Shortcut bindings (`bindings.<id>.current_binding`)\n")
    print("| id | Default | What |\n|---|---|---|")
    for bid, b in sorted(defaults.get("bindings", {}).items()):
        print(f"| `{bid}` | `{b.get('default_binding')}` | {b.get('description', '')} |")
    print("\n" + OTHER_FILES)
    print("\nGenerated by `scripts/gen_config_docs.py`; regenerate after changing settings.rs.")


main()
