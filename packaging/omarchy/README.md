# Sokki on Omarchy

How Sokki fits into Omarchy 4: a shell plugin for the bar, a systemd user service, two keys, a few menu rows, and Omarchy's dictation contract. The daemon and its API are in `crates/sokki` (`src/api.rs` is the contract).

## Files

| Path | What it is |
|---|---|
| `plugin/` | The shell plugin `sokki.meetings` (kinds `service` + `bar-widget`). `Service.qml` runs one `sokki events` stream and holds the state; `Widget.qml` is the bar icon and its panel (the record prompt, Stop, progress, recent notes); `Model.js` parses and formats, and loads in Node too; `i18n/` holds the strings (English, German). |
| `systemd/sokki.service` | User unit: `sokki daemon`, part of `graphical-session.target`, restarts on failure. Refreshes the user's plugin copy before each start. |
| `bin/sokki-omarchy-setup` | Per-user setup, run by `install.sh`: plugin into `~/.config/omarchy/plugins/sokki.meetings`, widget on the bar, service enabled, keys in `~/.config/hypr/bindings.lua`, rows in `~/.config/omarchy/extensions/omarchy-menu.jsonc`. `--dictation` also makes Sokki the dictation engine, `--remove` undoes everything, `--refresh` only updates the plugin copy. Safe to run again. |
| `bin/omarchy-dictation-sokki` | Omarchy's dictation adapter (`start`, `stop`, `toggle` → `sokki dictation …`), written to be sent upstream next to the Hex one. |
| `bin/omarchy-install-dictation-sokki`, `bin/omarchy-remove-dictation-sokki` | Select or unselect Sokki as the dictation engine, the way Omarchy does for Hex and Voxtype. Removing leaves the app installed, since it also takes meeting notes. |
| `sokki-bin/` | `PKGBUILD` for the release tarballs, its install message, and `.omarchy/package.json` for omarchy-pkgs' release tracking. |
| `sokki-git/` | `PKGBUILD` that builds `crates/sokki` from the main branch. |
| `install.sh` | Target of `curl -fsSL https://sokki.org/omarchy \| bash` (pass `-s -- --dictation` to also use it for dictation). |

Keys: `Super + Ctrl + M` opens the meetings panel (or brings a waiting record prompt forward so Enter and Esc answer it), `Super + Ctrl + Alt + M` records or stops right away. Both were free in Omarchy's defaults; the setup leaves a key commented out if Hyprland already uses it.

## Release contract

Each GitHub release that carries Linux builds attaches:

- `sokki-<version>-linux-x86_64.tar.gz` and `sokki-<version>-linux-aarch64.tar.gz`, each with one top folder `sokki-<version>-linux-<arch>/` holding `sokki` (the binary), `LICENSE`, and `omarchy/{bin,plugin,systemd}` copied from this folder;
- `PKGBUILD` and `sokki-bin.install` from `sokki-bin/`, with `pkgver` and the checksums filled in (`updpkgsums`). `install.sh` builds from these when the package is in neither the configured repositories nor the AUR.

## Testing in an Omarchy VM

1. Build in the Arch container (`packaging/docker`), or use `sokki-git`: `cd sokki-git && makepkg -si`.
2. Run `sokki-omarchy-setup` as your user in the desktop. Check: `omarchy plugin list` shows `sokki.meetings` enabled, `systemctl --user status sokki` is running, `omarchy menu keybindings --print` lists both keys, `hyprctl configerrors` is clean.
3. Without a call nothing shows in the bar. `Super + Ctrl + M` opens the panel with "Record now" and recent notes.
4. Open Google Meet in Chromium and join a call. The microphone icon pulses beside the clock and the prompt opens without taking the keyboard or the next click. On Omarchy 4.0, whose panels can't stay out of the way like that, a notification asks instead (Record / Not now), so the click on the browser's "Allow microphone" still lands. `Super + Ctrl + M` opens the panel; Enter records, Esc says not now. Unanswered, it closes when the countdown ends.
5. While recording: a red dot and the time in the bar; Stop in the panel. After the call: a progress pie in the bar, then a "Meeting note ready" notification with Open.
6. `omarchy dictation start|stop` with `sokki-omarchy-setup --dictation` set: Alt + Space and holding Right Alt dictate into the focused window.
7. `sokki-omarchy-setup --remove` leaves `bindings.lua` and the menu file as they were before setup.

Check the plugin before a release: `omarchy plugin validate plugin/` and `omarchy agent look plugin ~/.config/omarchy/plugins/sokki.meetings` (renders the widget with the current theme). Switch a few themes (`omarchy theme set …`) and turn animations off (`omarchy toggle animations`) to see that it follows both.
