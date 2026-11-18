#!/bin/bash
# Installs (or updates) Verbatim from the latest GitHub release:
#
#   curl -fsSL https://raw.githubusercontent.com/feynnon/verbatim/main/install.sh | bash
#
# Downloads the app, puts it in /Applications and opens it. Your settings and
# meetings are kept on update. Nothing else on your Mac is changed.
set -euo pipefail

REPO="feynnon/verbatim"
APP="/Applications/Verbatim.app"

say() { printf '\033[1m%s\033[0m\n' "$*"; }
fail() { printf '\033[31m%s\033[0m\n' "$*" >&2; exit 1; }

[[ "$(uname -s)" == "Darwin" ]] || fail "Verbatim runs on macOS only."
[[ "$(uname -m)" == "arm64" ]] || fail "Verbatim needs a Mac with Apple Silicon (M1 or later)."
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
curl -fL --progress-bar "$url" -o "$tmp/Verbatim.zip"

if pgrep -f "$APP/Contents/MacOS" >/dev/null; then
  say "Closing the running Verbatim…"
  osascript -e 'quit app "Verbatim"' >/dev/null 2>&1 || true
  for _ in $(seq 1 20); do pgrep -f "$APP/Contents/MacOS" >/dev/null || break; sleep 0.5; done
fi

say "Installing to /Applications…"
rm -rf "$APP"
ditto -x -k "$tmp/Verbatim.zip" /Applications
# Downloaded with curl, so macOS has not quarantined it; clear it just in case.
xattr -dr com.apple.quarantine "$APP" 2>/dev/null || true

open "$APP"
say "Verbatim is installed and open."
if ! command -v ollama >/dev/null 2>&1 && [[ ! -d /Applications/Ollama.app ]]; then
  echo "For summaries on your Mac, Verbatim uses Ollama (free): https://ollama.com/download"
  echo "The app offers this too, under Models → Language model."
fi
