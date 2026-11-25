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
#   Sokki-arm64.zip              the same zip under a fixed name, for the
#                                README's Download button
#                                (releases/latest/download/Sokki-arm64.zip)
#   Sokki-<v>-arm64.app.tar.gz   the app, for the updater
#   latest.json                  the update feed: version, notes, the tar.gz
#                                URL and its signature
#
# The updater only installs packages signed with the key whose public half is
# in brands/sokki/tauri.conf.json. Its private half is ~/.config/sokki/updater.key
# (or $SOKKI_UPDATER_KEY). Without it no release can update the installed apps,
# so keep a backup.
#
# The app is signed with Robin's "Developer ID Application" certificate and
# notarised by Apple, so a zip downloaded in a browser opens with the normal
# "downloaded from the internet" prompt. Notarisation logs in with the App Store
# Connect API key named in ~/.config/sokki/notary.env (NOTARY_KEY_ID, the .p8
# file NOTARY_KEY, and NOTARY_ISSUER for a team key; leave it out for an
# individual key), or else with the keychain profile "sokki-notary"
# ($SOKKI_NOTARY_PROFILE) made by `xcrun notarytool store-credentials`.
# Every Mach-O in the bundle needs that signature, the hardened runtime and a
# secure timestamp; the Swift code is a static lib inside the main binary, and
# the one helper binary is resources/llm/llama-server, signed below before
# Tauri seals the bundle.
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
export SOKKI_COUNTER_URL="${SOKKI_COUNTER_URL:-$(tr -d '[:space:]' < "$REPO/counter/url.txt")}"
export TAURI_SIGNING_PRIVATE_KEY="$(cat "$KEY")"
export TAURI_SIGNING_PRIVATE_KEY_PASSWORD="${TAURI_SIGNING_PRIVATE_KEY_PASSWORD:-}"
# macOS keeps granted permissions across updates only while the certificate
# stays the same. Builds up to 0.2.x used the self-made "Sokki Dev"; the first
# Developer ID build asks every user once more for its permissions.
IDENTITY="${SOKKI_SIGNING_IDENTITY:-$(security find-identity -v -p codesigning 2>/dev/null \
  | sed -n 's/.*"\(Developer ID Application: [^"]*\)".*/\1/p' | head -1)}"
[[ -n "$IDENTITY" ]] || { echo "no \"Developer ID Application\" certificate in the keychain" >&2; exit 1; }
NOTARY_ENV="${SOKKI_NOTARY_ENV:-$HOME/.config/sokki/notary.env}"
if [[ -f "$NOTARY_ENV" ]]; then
  source "$NOTARY_ENV"
  NOTARY_AUTH=(--key "$NOTARY_KEY" --key-id "$NOTARY_KEY_ID")
  [[ -n "${NOTARY_ISSUER:-}" ]] && NOTARY_AUTH+=(--issuer "$NOTARY_ISSUER")
else
  NOTARY_AUTH=(--keychain-profile "${SOKKI_NOTARY_PROFILE:-sokki-notary}")
fi
xcrun notarytool history "${NOTARY_AUTH[@]}" >/dev/null \
  || { echo "notarytool cannot log in (${NOTARY_AUTH[*]})" >&2; exit 1; }
export APPLE_SIGNING_IDENTITY="$IDENTITY"
echo "Signing with \"$IDENTITY\"."
sign() { codesign --force --options runtime --timestamp --sign "$IDENTITY" "$@"; }

source "$REPO/scripts/build-lock.sh"
BUILD_SLOT_PRIORITY=release build_lock_acquire  # a release goes first in the queue
"$REPO/scripts/build-llama-server.sh"  # the built-in language model
LLAMA="$REPO/src-tauri/resources/llm/llama-server"
[[ -f "$LLAMA" ]] && sign "$LLAMA"  # Tauri signs only the main binary
[[ -d "$REPO/node_modules" ]] || (cd "$REPO" && bun install --frozen-lockfile)
(cd "$REPO" && bun run tauri build --bundles app --config "src-tauri/brands/sokki/tauri.conf.json")
build_lock_release

APP="$BUNDLE_DIR/Sokki.app"
rm -rf "$OUT" && mkdir -p "$OUT"
ZIP="Sokki-$VERSION-arm64.zip"
TGZ="Sokki-$VERSION-arm64.app.tar.gz"

# Check every signature before Apple does: notarisation takes minutes.
codesign --verify --deep --strict "$APP"
while IFS= read -r -d '' f; do
  file -b "$f" | grep -q Mach-O || continue
  info="$(codesign -dvv "$f" 2>&1)"
  grep -qF "Authority=$IDENTITY" <<<"$info" && grep -q "flags=.*runtime" <<<"$info" \
    && grep -q "^Timestamp=" <<<"$info" \
    || { echo "not Developer ID + hardened runtime + timestamp: ${f#$APP/}" >&2; exit 1; }
done < <(find "$APP/Contents" -type f -print0)

echo "Notarising (usually a few minutes)…"
ditto -c -k --keepParent "$APP" "$OUT/notarize.zip"
xcrun notarytool submit "$OUT/notarize.zip" "${NOTARY_AUTH[@]}" \
  --wait --output-format json > "$OUT/notary.json"
rm "$OUT/notarize.zip"
if [[ "$(jq -r .status "$OUT/notary.json")" != Accepted ]]; then
  xcrun notarytool log "$(jq -r .id "$OUT/notary.json")" "${NOTARY_AUTH[@]}" "$OUT/notary-log.json" || true
  echo "Apple did not accept the build: $OUT/notary.json, $OUT/notary-log.json" >&2; exit 1
fi
xcrun stapler staple "$APP"
xcrun stapler validate "$APP"
spctl --assess --type execute -vv "$APP" 2>&1 | grep -q "source=Notarized Developer ID" \
  || { echo "Gatekeeper does not see a notarised app" >&2; spctl --assess --type execute -vv "$APP"; exit 1; }

# Tauri packed the updater archive before stapling; pack and sign it again.
ditto -c -k --keepParent "$APP" "$OUT/$ZIP"
cp "$OUT/$ZIP" "$OUT/Sokki-arm64.zip"  # fixed name for the README's Download button
COPYFILE_DISABLE=1 tar -czf "$OUT/$TGZ" -C "$BUNDLE_DIR" Sokki.app
(cd "$REPO" && bun run tauri signer sign "$OUT/$TGZ" >/dev/null)
jq -n \
  --arg version "$VERSION" \
  --arg notes "$(cat "$NOTES")" \
  --arg date "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
  --arg sig "$(cat "$OUT/$TGZ.sig")" \
  --arg url "https://github.com/$GH_REPO/releases/download/$TAG/$TGZ" \
  '{version: $version, notes: $notes, pub_date: $date,
    platforms: {"darwin-aarch64": {signature: $sig, url: $url}}}' > "$OUT/latest.json"
rm "$OUT/$TGZ.sig" "$OUT/notary.json"
echo "Release files in $OUT:"
ls -lh "$OUT"

if [[ "$PUBLISH" == "--publish" ]]; then
  gh release create "$TAG" -R "$GH_REPO" --title "Sokki $VERSION" --notes-file "$NOTES" \
    "$OUT/$ZIP" "$OUT/Sokki-arm64.zip" "$OUT/$TGZ" "$OUT/latest.json"
  echo "Published https://github.com/$GH_REPO/releases/tag/$TAG"

  # The Homebrew tap (`brew install feynnon/sokki/sokki`): point its cask at
  # this release. Skipped without the tap's clone ($SOKKI_TAP) or a gh login.
  TAP="${SOKKI_TAP:-$HOME/PycharmProjects/homebrew-sokki}"
  CASK="$TAP/Casks/sokki.rb"
  if [[ -f "$CASK" ]] && gh auth status >/dev/null 2>&1; then
    SHA="$(shasum -a 256 "$OUT/$ZIP" | cut -d' ' -f1)"
    git -C "$TAP" pull -q --ff-only \
      && sed -i '' -e "s/^  version \".*\"/  version \"$VERSION\"/" \
                   -e "s/^  sha256 \".*\"/  sha256 \"$SHA\"/" "$CASK" \
      && git -C "$TAP" -c user.name="Robin Dehde" \
           -c user.email=18052826+feynnon@users.noreply.github.com \
           commit -q -m "Sokki $VERSION" Casks/sokki.rb \
      && git -C "$TAP" push -q \
      && echo "Homebrew cask now at $VERSION." \
      || echo "WARNING: the Homebrew cask is not updated; bump $CASK by hand."
  fi
fi
