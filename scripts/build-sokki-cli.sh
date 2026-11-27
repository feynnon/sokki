#!/usr/bin/env bash
# Builds the `sokki` command line (crates/sokki, release) and puts it where
# the app bundles it: src-tauri/resources/bin/sokki (ignored by git), so
# /Applications/Sokki.app/Contents/Resources/resources/bin/sokki is what
# install.sh links to ~/.local/bin/sokki. scripts/release.sh calls this and
# signs the binary like llama-server.
#
# A release build is a heavy job: it waits in the Mac-wide build queue
# (scripts/build-lock.sh) unless the caller already holds the slot.
set -euo pipefail
REPO="$(cd "$(dirname "$0")/.." && pwd)"
DEST="$REPO/src-tauri/resources/bin/sokki"
export PATH="$HOME/.cargo/bin:/opt/homebrew/opt/rustup/bin:$PATH"
source "$REPO/scripts/build-lock.sh"
build_lock_acquire
CARGO_TARGET_DIR="$REPO/target/cli" cargo build --release --manifest-path "$REPO/crates/sokki/Cargo.toml"
build_lock_release
mkdir -p "$(dirname "$DEST")"
install -m 755 "$REPO/target/cli/release/sokki" "$DEST"
echo "sokki $("$DEST" version | awk '{print $2}') ready at $DEST"
