#!/bin/bash
# Assembles the Sokki keyboard as a project of its own, the tree the public
# repo feynnon/sokki-keyboard gets (a mirror: development stays in the Sokki
# monorepo). The release manager runs it with each release:
#
#   mobile/keyboard/standalone/assemble.sh <out-dir>
#   cd <out-dir> && ./gradlew assembleDebug      # needs JDK 17, Android SDK+NDK, cargo-ndk
#
# Copies: the keyboard module (mobile/keyboard, minus this folder), the
# speech engine (mobile/src-tauri/src/keyboard_voice.rs + mobile/dictation-core),
# the Gradle wrapper, the licenses. Like the main mirror it's a tree copy, so
# no history (old history has real names).
set -euo pipefail
# Not ready to publish yet (checked 24.11.2026: the tree builds with
# -Psokki.voice=false, a 45 MB keyboard APK):
# - on its own the keyboard has no way to download the speech model (inside
#   Sokki the app's Dictate tab does it);
# - the speech engine doesn't build alone: keyboard_voice.rs uses
#   dictation::engine_at from the app crate (Tauri). Move the engine part into
#   its own crate first (mobile/dictation-core or a keyboard-voice crate).
# --test assembles anyway, for build checks.
if [[ "${2:-}" != "--test" && ! -f "$(dirname "$0")/../src/main/java/dev/verbatim/sokki/ModelDownload.kt" ]]; then
  echo "Not ready to publish: the standalone keyboard can't download its speech model yet." >&2
  exit 1
fi
here="$(cd "$(dirname "$0")" && pwd)"
mobile="$(cd "$here/../.." && pwd)"
out="${1:?usage: assemble.sh <out-dir> [--test]}"
mkdir -p "$out"
out="$(cd "$out" && pwd)"
case "$out" in "$mobile"*) echo "out-dir must be outside the repo" >&2; exit 1;; esac

rsync -a --delete --exclude standalone --exclude build --exclude .cxx --exclude .gradle \
  "$mobile/keyboard/" "$out/keyboard/"
rsync -a "$here/" "$out/" --exclude assemble.sh --exclude README.md
cp "$here/README.md" "$out/README.md"
mkdir -p "$out/voice/src" "$out/gradle"
cp "$mobile/src-tauri/src/keyboard_voice.rs" "$out/voice/src/lib.rs"
rsync -a --delete --exclude target "$mobile/dictation-core/" "$out/voice/dictation-core/"
rsync -a "$mobile/src-tauri/gen/android/gradle/wrapper" "$out/gradle/"
cp "$mobile/src-tauri/gen/android/gradlew" "$mobile/src-tauri/gen/android/gradlew.bat" "$out/"
cp "$mobile/LICENSE" "$out/LICENSE"                       # GPL-3.0, like HeliBoard
cp "$mobile/keyboard/LICENSE-Apache-2.0" "$mobile/keyboard/LICENSE-CC-BY-SA-4.0" "$mobile/keyboard/UPSTREAM" "$out/"
printf '%s\n' build .gradle .cxx local.properties '*.iml' voice/target app/src/main/jniLibs > "$out/.gitignore"
echo "Assembled in $out"
