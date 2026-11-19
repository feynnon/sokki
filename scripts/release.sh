#!/bin/bash
# Builds a Sokki release with everything the in-app updater needs.
#
#   scripts/release.sh <notes.md>            build into src-tauri/target/release-files/<version>/
#   scripts/release.sh <notes.md> --publish  … and create the GitHub release
#
# The version is the one in src-tauri/brands/sokki/tauri.conf.json; bump it
# first. Build from the commit you want to ship (public main).
#
# Files, all uploaded to the release:
#   Sokki-<v>-arm64.zip          install.sh downloads this
#   Sokki-<v>-arm64.app.tar.gz   the app, for the updater
#   latest.json                  the update feed: version, notes, the tar.gz
#                                URL and its signature
#
# The updater only installs packages signed with the key whose public half is
# in brands/sokki/tauri.conf.json. Its private half is ~/.config/sokki/updater.key
# (or $SOKKI_UPDATER_KEY). Without it no release can update the installed apps,
# so keep a backup.
set -euo pipefail

NOTES="${1:?usage: scripts/release.sh <notes.md> [--publish]}"
PUBLISH="${2:-}"
REPO="$(cd "$(dirname "$0")/.." && pwd)"
CONF="$REPO/src-tauri/brands/sokki/tauri.conf.json"
VERSION="$(jq -r .version "$CONF")"
TAG="v$VERSION"
GH_REPO="feynnon/sokki"
KEY="${SOKKI_UPDATER_KEY:-$HOME/.config/sokki/updater.key}"
OUT="$REPO/src-tauri/target/release-files/$VERSION"
BUNDLE_DIR="$REPO/src-tauri/target/release/bundle/macos"

[[ -f "$NOTES" ]] || { echo "no notes file: $NOTES" >&2; exit 1; }
[[ -f "$KEY" ]] || { echo "no updater key at $KEY" >&2; exit 1; }
if gh release view "$TAG" -R "$GH_REPO" >/dev/null 2>&1; then
  echo "$TAG is already released; bump the version in $CONF" >&2; exit 1
fi
[[ -z "$(git -C "$REPO" status --porcelain --untracked-files=no)" ]] \
  || echo "WARNING: uncommitted changes go into this build."
echo "Building Sokki $VERSION from $(git -C "$REPO" rev-parse --short HEAD)."

export PATH="/opt/homebrew/opt/rustup/bin:$HOME/.cargo/bin:$PATH"
export CMAKE_POLICY_VERSION_MINIMUM=3.5
export HANDY_BRAND=sokki VITE_HANDY_BRAND=sokki
export SOKKI_COUNTER_URL="${SOKKI_COUNTER_URL:-$(tr -d '[:space:]' < "$REPO/counter/url.txt")}"
export TAURI_SIGNING_PRIVATE_KEY="$(cat "$KEY")"
export TAURI_SIGNING_PRIVATE_KEY_PASSWORD="${TAURI_SIGNING_PRIVATE_KEY_PASSWORD:-}"
# The same certificate as every earlier build, so macOS keeps the granted
# permissions across updates.
IDENTITY="Handy Meetings Dev"
security find-identity -p codesigning 2>/dev/null | grep -q "\"$IDENTITY\"" \
  || { echo "no \"$IDENTITY\" signing certificate" >&2; exit 1; }
export APPLE_SIGNING_IDENTITY="$IDENTITY"

source "$REPO/scripts/build-lock.sh"
build_lock_acquire
[[ -d "$REPO/node_modules" ]] || (cd "$REPO" && bun install --frozen-lockfile)
(cd "$REPO" && bun run tauri build --bundles app --config "src-tauri/brands/sokki/tauri.conf.json")
build_lock_release

rm -rf "$OUT" && mkdir -p "$OUT"
ZIP="Sokki-$VERSION-arm64.zip"
TGZ="Sokki-$VERSION-arm64.app.tar.gz"
ditto -c -k --keepParent "$BUNDLE_DIR/Sokki.app" "$OUT/$ZIP"
cp "$BUNDLE_DIR/Sokki.app.tar.gz" "$OUT/$TGZ"
jq -n \
  --arg version "$VERSION" \
  --arg notes "$(cat "$NOTES")" \
  --arg date "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
  --arg sig "$(cat "$BUNDLE_DIR/Sokki.app.tar.gz.sig")" \
  --arg url "https://github.com/$GH_REPO/releases/download/$TAG/$TGZ" \
  '{version: $version, notes: $notes, pub_date: $date,
    platforms: {"darwin-aarch64": {signature: $sig, url: $url}}}' > "$OUT/latest.json"
echo "Release files in $OUT:"
ls -lh "$OUT"

if [[ "$PUBLISH" == "--publish" ]]; then
  gh release create "$TAG" -R "$GH_REPO" --title "Sokki $VERSION" --notes-file "$NOTES" \
    "$OUT/$ZIP" "$OUT/$TGZ" "$OUT/latest.json"
  echo "Published https://github.com/$GH_REPO/releases/tag/$TAG"
fi
