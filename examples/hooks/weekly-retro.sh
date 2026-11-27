#!/bin/sh
# Sokki hook: the week's decisions and tasks into ~/.gstack/retro-context.md,
# which gstack's /retro reads (HOOKS.md). on = "schedule",
# cron = "0 17 * * fri".
#
# - Reads the meeting notes in $SOKKI_NOTES_DIR whose `date:` is on or after
#   the previous run (the payload's schedule.previous), else the last 7 days.
# - Writes only its own block, between the two sokki markers. Whatever else
#   is in the file (gstack calls it user-authored) stays.
# - RETRO_CONTEXT: another file. SOKKI_HOOK_TEST=1: every note, printed to
#   the log, no file written.
#
# Later: once Sokki's index has `sokki decisions --since 7d --json`, that
# replaces the Markdown parsing below.
set -eu

payload=$(cat)
notes=${SOKKI_NOTES_DIR:?SOKKI_NOTES_DIR is not set}
out=${RETRO_CONTEXT:-$HOME/.gstack/retro-context.md}
begin="<!-- sokki:weekly-retro begin -->"
end="<!-- sokki:weekly-retro end -->"

previous=$(printf '%s' "$payload" | sed -n 's/.*"previous":"\([0-9]\{4\}-[0-9]\{2\}-[0-9]\{2\}\).*/\1/p')
if [ -n "${SOKKI_HOOK_TEST:-}" ]; then
  since=0000-00-00
elif [ -n "$previous" ]; then
  since=$previous
else
  since=$(date -v-7d +%F 2>/dev/null || date -d '7 days ago' +%F)
fi
today=$(date +%F)

meetings=$(find "$notes" -name '*.md' -type f 2>/dev/null | LC_ALL=C sort | while IFS= read -r f; do
  awk -v since="$since" '
    NR == 1 && $0 == "---" { front = 1; next }
    front && $0 == "---"   { front = 0; next }
    front && /^date: /     { date = $2 }
    front && /^type: /     { type = $2 }
    front                  { next }
    /^# / && title == ""   { title = substr($0, 3); next }
    /^## / {
      h = substr($0, 4)
      section = (h == "Entscheidungen" || h == "Decisions") ? "d" : (h == "Aufgaben" || h == "Action items") ? "t" : ""
      next
    }
    section == "d" && /^- / { d[++nd] = $0 }
    section == "t" && /^- / { t[++nt] = $0 }
    END {
      if (type != "meeting" || date < since || (nd == 0 && nt == 0)) exit
      printf "### %s %s\n\n", date, title
      if (nd) { print "Decisions:"; for (i = 1; i <= nd; i++) print d[i]; print "" }
      if (nt) { print "Tasks:"; for (i = 1; i <= nt; i++) print t[i]; print "" }
    }' "$f"
done)

[ -n "$meetings" ] || meetings="No meetings with decisions or tasks since $since."
heading="Meetings since $since"
[ -z "${SOKKI_HOOK_TEST:-}" ] || heading="Meetings (test run: every note)"
block=$(printf '%s\n## %s (from Sokki, %s)\n\n%s\n\n%s\n' "$begin" "$heading" "$today" "$meetings" "$end")

if [ -n "${SOKKI_HOOK_TEST:-}" ]; then
  printf '%s\n' "$block"
  exit 0
fi

mkdir -p "$(dirname "$out")"
tmp="$out.sokki-tmp"
if [ -f "$out" ]; then
  # Everything outside the old block, then the new block.
  awk -v b="$begin" -v e="$end" '$0 == b { skip = 1 } !skip { print } $0 == e { skip = 0 }' "$out" > "$tmp"
else
  : > "$tmp"
fi
[ -s "$tmp" ] && [ -n "$(tail -c 1 "$tmp")" ] && echo >> "$tmp"
printf '%s\n' "$block" >> "$tmp"
mv "$tmp" "$out"
echo "wrote $(grep -c '^### ' "$out" || true) meeting(s) since $since to $out"
