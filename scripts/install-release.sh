#!/bin/bash
# Installs a published Sokki release into /Applications, the way users get it.
#
#   scripts/install-release.sh [version]   # default: the newest release
#
# A release build updates itself; the dev builds from build-and-install.sh don't.
# Never replaces Sokki while a meeting is recording or being processed: it waits
# until Sokki has been idle for 30 s in a row and checks once more right before
# quitting the app.
set -euo pipefail

GH_REPO="feynnon/sokki"
# The installed app's bundle id and binary, so this works before and after an
# update that renamed them.
INSTALLED_PLIST="/Applications/Sokki.app/Contents/Info.plist"
APP_ID="$(defaults read "$INSTALLED_PLIST" CFBundleIdentifier 2>/dev/null || echo org.sokki.app)"
INDEX="$HOME/Library/Application Support/$APP_ID/meetings/index.json"
VERSION="${1:-$(gh release view -R "$GH_REPO" --json tagName -q .tagName | sed 's/^v//')}"
ZIP="Sokki-$VERSION-arm64.zip"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

gh release download "v$VERSION" -R "$GH_REPO" -p "$ZIP" -D "$WORK"
ditto -x -k "$WORK/$ZIP" "$WORK/app"
codesign --verify --deep --strict "$WORK/app/Sokki.app"

busy() {
  [[ -f "$INDEX" ]] && jq -e 'any(.[]; .status.kind == "recording" or .status.kind == "processing")' "$INDEX" >/dev/null
}
quiet=0
while (( quiet < 3 )); do
  if busy; then
    (( quiet == 0 )) || echo "A meeting started; waiting…"
    quiet=0
  else
    quiet=$((quiet + 1))
  fi
  sleep 10
done
busy && { echo "A meeting started just now; try again later." >&2; exit 1; }

osascript -e "tell application id \"$APP_ID\" to quit" 2>/dev/null || true
for _ in $(seq 1 40); do pgrep -f "/Applications/Sokki.app/Contents/MacOS/" >/dev/null || break; sleep 0.5; done
if pgrep -f "/Applications/Sokki.app/Contents/MacOS/" >/dev/null; then
  echo "Sokki didn't quit; nothing replaced." >&2
  exit 1
fi
rm -rf /Applications/Sokki.app
ditto "$WORK/app/Sokki.app" /Applications/Sokki.app
open -a Sokki
echo "Installed Sokki $VERSION."
