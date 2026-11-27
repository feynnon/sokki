#!/bin/sh
# Sokki hook: the notes folder as a git history (HOOKS.md).
# on = "meeting.note_ready", no scope (it reads the files itself).
#
# - After each note, commits every change in $SOKKI_NOTES_DIR, with the
#   meeting's title as the message. The first run makes the folder a git
#   repository.
# - Never pushes. A `git push` here would send your notes off the Mac; add
#   it only to a remote you trust.
set -eu

payload=$(cat)
dir=${SOKKI_NOTES_DIR:?SOKKI_NOTES_DIR is not set}
cd "$dir"
[ -d .git ] || git init -q

title=$(printf '%s' "$payload" | sed -n 's/.*"title":"\([^"]*\)".*/\1/p' | head -n 1)
git add -A
if git diff --cached --quiet; then
  echo "nothing changed"
  exit 0
fi
# Your own name when git knows it, else "Sokki".
set --
[ -n "$(git config user.name || true)" ] || set -- -c user.name=Sokki
[ -n "$(git config user.email || true)" ] || set -- "$@" -c user.email=sokki@localhost
git "$@" commit -q -m "${title:-Meeting note} ($SOKKI_ID)"
git log -1 --format='committed %h: %s'
