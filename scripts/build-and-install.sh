#!/bin/bash
# Builds Sokki and installs it to /Applications.
#
# Signing: when the code-signing identity "Sokki Dev" exists in the
# login keychain, every build is signed with it. macOS ties the granted
# permissions (Microphone, Accessibility, System Audio Recording) to the
# signing certificate, so they survive reinstalls. Without it the build is
# signed ad hoc and macOS asks for every permission again after each install.
#
# Safety: never replaces the running app while a meeting is recording or
# being processed.
#
#   scripts/build-and-install.sh            build, then install when idle
#   scripts/build-and-install.sh --no-build install the last build when idle
#   … --reprocess <meeting-id>…               also process these meetings again
#                                              at launch (audio must still exist)
set -euo pipefail

BUILD=1
REPROCESS=()
while [[ $# -gt 0 ]]; do
  case "$1" in
    --no-build) BUILD=0 ;;
    --reprocess) shift; while [[ $# -gt 0 && "$1" != --* ]]; do REPROCESS+=("$1"); shift; done; continue ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
  shift
done

REPO="$(cd "$(dirname "$0")/.." && pwd)"
APP_NAME="Sokki"
# A dev build never updates itself: the next release would replace it.
export SOKKI_DEV_BUILD=1
# The voice-data counter (counter/), once deployed; empty: count locally only.
if [[ -z "${SOKKI_COUNTER_URL:-}" && -f "$REPO/counter/url.txt" ]]; then
  export SOKKI_COUNTER_URL="$(tr -d '[:space:]' < "$REPO/counter/url.txt")"
fi
IDENTITY="Sokki Dev"
# The installed app's bundle id and binary, so this works before and after an
# update that renamed them.
INSTALLED_PLIST="/Applications/Sokki.app/Contents/Info.plist"
APP_ID="$(defaults read "$INSTALLED_PLIST" CFBundleIdentifier 2>/dev/null || echo org.sokki.app)"
INDEX="$HOME/Library/Application Support/$APP_ID/meetings/index.json"
BUNDLE="$REPO/src-tauri/target/release/bundle/macos/$APP_NAME.app"

export PATH="/opt/homebrew/opt/rustup/bin:$HOME/.cargo/bin:$PATH"
export CMAKE_POLICY_VERSION_MINIMUM=3.5

if security find-identity -p codesigning 2>/dev/null | grep -q "\"$IDENTITY\""; then
  export APPLE_SIGNING_IDENTITY="$IDENTITY"
  echo "Signing with \"$IDENTITY\" (permissions are kept across installs)."
else
  export APPLE_SIGNING_IDENTITY="-"
  echo "WARNING: no \"$IDENTITY\" certificate; signing ad hoc. macOS will ask for permissions again."
fi

if [[ "$BUILD" == 1 ]]; then
  source "$REPO/scripts/build-lock.sh"
  build_lock_acquire
  "$REPO/scripts/build-llama-server.sh"  # the built-in language model
  (cd "$REPO" && bun run tauri build --bundles app --config '{"bundle":{"createUpdaterArtifacts":false}}' --config src-tauri/brands/sokki/tauri.conf.json)
  build_lock_release
fi

busy() {
  # Only a running app can be recording or processing; a record left at
  # "processing" by a crash or kill is resumed at the next launch.
  pgrep -f "/Applications/($APP_NAME|Verbatim).app/Contents/MacOS" >/dev/null || return 1
  [[ -f "$INDEX" ]] && jq -e '[.[] | select(.status.kind=="recording" or .status.kind=="processing")] | length > 0' "$INDEX" >/dev/null
}
if busy; then
  echo "A meeting is recording or being processed; waiting before installing…"
  while busy; do sleep 10; done
fi

for name in "$APP_NAME" Verbatim; do
  osascript -e "quit app \"$name\"" 2>/dev/null || true
done
for _ in $(seq 1 20); do pgrep -f "/Applications/($APP_NAME|Verbatim).app/Contents/MacOS" >/dev/null || break; sleep 0.5; done
if [[ ${#REPROCESS[@]} -gt 0 ]]; then
  ids=$(printf '%s\n' "${REPROCESS[@]}" | jq -R . | jq -s .)
  jq --argjson ids "$ids" '(.[] | select(.id as $i | $ids | index($i))) |= (.status={"kind":"failed","error":"Interrupted: reprocess"} | .warnings=[])' "$INDEX" > "$INDEX.tmp" && mv "$INDEX.tmp" "$INDEX"
  echo "Queued for processing at launch: ${REPROCESS[*]}"
fi
rm -rf "/Applications/$APP_NAME.app" /Applications/Verbatim.app
cp -R "$BUNDLE" /Applications/
codesign -dv "/Applications/$APP_NAME.app" 2>&1 | grep -E "Authority|Signature=" || true
open "/Applications/$APP_NAME.app"
echo "Installed and started $APP_NAME."
