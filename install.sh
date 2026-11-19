#!/bin/bash
# Installs (or updates) Sokki from the latest GitHub release:
#
#   curl -fsSL https://raw.githubusercontent.com/feynnon/sokki/main/install.sh | bash
#
# Downloads the app, puts it in /Applications and opens it. Your settings and
# meetings are kept on update. Also adds the `sokki` command to ~/.local/bin
# (transcribe a file, ask the local model). Nothing else on your Mac is changed.
set -euo pipefail

REPO="feynnon/sokki"
APP="/Applications/Sokki.app"

say() { printf '\033[1m%s\033[0m\n' "$*"; }
fail() { printf '\033[31m%s\033[0m\n' "$*" >&2; exit 1; }

[[ "$(uname -s)" == "Darwin" ]] || fail "Sokki runs on macOS only."
[[ "$(uname -m)" == "arm64" ]] || fail "Sokki needs a Mac with Apple Silicon (M1 or later)."
major="$(sw_vers -productVersion | cut -d. -f1)"
if (( major < 14 )); then
  say "Note: recording calls (system audio) needs macOS 14.2 or later. Dictation works on older versions."
fi

say "Looking up the latest release…"
url="$(curl -fsSL "https://api.github.com/repos/$REPO/releases/latest" \
  | grep -o '"browser_download_url": *"[^"]*arm64\.zip"' | head -1 | cut -d'"' -f4)"
[[ -n "$url" ]] || fail "No release found at https://github.com/$REPO/releases"

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
say "Downloading $(basename "$url")…"
curl -fL --progress-bar "$url" -o "$tmp/Sokki.zip"

OLD_APP="/Applications/Verbatim.app"  # the app's earlier name
if pgrep -f "$APP/Contents/MacOS" >/dev/null || pgrep -f "$OLD_APP/Contents/MacOS" >/dev/null; then
  INDEX="$HOME/Library/Application Support/dev.handymeetings.app/meetings/index.json"
  if [[ -f "$INDEX" ]] && grep -Eq '"kind" *: *"(recording|processing)"' "$INDEX"; then
    fail "A meeting is being recorded or processed. Run this again when it's done."
  fi
  say "Closing the running Sokki…"
  osascript -e 'quit app "Sokki"' >/dev/null 2>&1 || true
  osascript -e 'quit app "Verbatim"' >/dev/null 2>&1 || true
  for _ in $(seq 1 20); do
    pgrep -f "$APP/Contents/MacOS" >/dev/null || pgrep -f "$OLD_APP/Contents/MacOS" >/dev/null || break
    sleep 0.5
  done
fi

say "Installing to /Applications…"
ditto -x -k "$tmp/Sokki.zip" "$tmp/unzipped"
bundle="$(find "$tmp/unzipped" -maxdepth 1 -name '*.app' | head -1)"
[[ -n "$bundle" ]] || fail "The download contains no app."
rm -rf "$APP" "$OLD_APP"
mv "$bundle" "$APP"
# Downloaded with curl, so macOS has not quarantined it; clear it just in case.
xattr -dr com.apple.quarantine "$APP" 2>/dev/null || true

open "$APP"
say "Sokki is installed and open."

# The `sokki` command: transcribe a file, ask the local model. A `sokki` that is
# not ours (or a link to a source checkout) is left alone.
BIN="$HOME/.local/bin"
CLI="$BIN/sokki"
if [[ -L "$CLI" ]] || { [[ -e "$CLI" ]] && ! grep -q "Sokki from the command line" "$CLI" 2>/dev/null; }; then
  :
elif mkdir -p "$BIN" && curl -fsSL "https://raw.githubusercontent.com/$REPO/main/scripts/sokki" -o "$tmp/sokki" \
     && grep -q "Sokki from the command line" "$tmp/sokki"; then
  install -m 755 "$tmp/sokki" "$CLI"
  echo "In the terminal: sokki transcribe FILE, sokki ask \"…\""
  case ":$PATH:" in *":$BIN:"*) ;; *) echo "Add $BIN to your PATH to use it." ;; esac
fi
if ! command -v ollama >/dev/null 2>&1 && [[ ! -d /Applications/Ollama.app ]]; then
  echo "For summaries on your Mac, Sokki uses Ollama (free): https://ollama.com/download"
  echo "The app offers this too, under Models → Language model."
fi
