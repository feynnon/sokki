#!/bin/bash
# Builds llama.cpp's llama-server (Metal, one static binary) for the built-in
# language model and puts it where the app bundle picks it up:
# src-tauri/resources/llm/llama-server (ignored by git).
#
# The source is pinned to one llama.cpp release and its commit is checked.
# The result is cached per release in ~/.cache/sokki-llama, so only the first
# build after a version change compiles (a few minutes, 4 jobs).
#
# Called by build-and-install.sh and release.sh while they hold the build
# lock; run it alone only with the lock free (scripts/build-lock.sh).
set -euo pipefail

TAG="b12572"
COMMIT="652d559edf278347160baeb91ab654ac57ac429e"

REPO="$(cd "$(dirname "$0")/.." && pwd)"
CACHE="$HOME/.cache/sokki-llama"
SRC="$CACHE/src-$TAG"
OUT="$CACHE/llama-server-$TAG"
DEST="$REPO/src-tauri/resources/llm/llama-server"

if [[ "$(uname -s)-$(uname -m)" != "Darwin-arm64" ]]; then
  echo "The built-in language model needs a Mac with Apple silicon; skipping llama-server."
  exit 0
fi

if [[ ! -x "$OUT" ]]; then
  mkdir -p "$CACHE"
  if [[ ! -d "$SRC/.git" ]]; then
    rm -rf "$SRC"
    git clone -q --depth 1 --branch "$TAG" https://github.com/ggml-org/llama.cpp "$SRC"
  fi
  got="$(git -C "$SRC" rev-parse HEAD)"
  if [[ "$got" != "$COMMIT" ]]; then
    echo "llama.cpp $TAG is at $got, expected $COMMIT" >&2
    exit 1
  fi
  cmake -S "$SRC" -B "$SRC/build" -DCMAKE_BUILD_TYPE=Release \
    -DBUILD_SHARED_LIBS=OFF -DGGML_METAL=ON -DGGML_METAL_EMBED_LIBRARY=ON \
    -DGGML_NATIVE=OFF -DCMAKE_OSX_DEPLOYMENT_TARGET=13.3 \
    -DLLAMA_OPENSSL=OFF -DLLAMA_BUILD_TESTS=OFF -DLLAMA_BUILD_EXAMPLES=OFF \
    -DLLAMA_BUILD_UI=OFF -DLLAMA_USE_PREBUILT_UI=OFF -DLLAMA_BUILD_APP=OFF >/dev/null
  cmake --build "$SRC/build" --target llama-server -j 4
  cp "$SRC/build/bin/llama-server" "$OUT"
fi

mkdir -p "$(dirname "$DEST")"
cp "$OUT" "$DEST"
echo "llama-server $TAG ready at $DEST"
