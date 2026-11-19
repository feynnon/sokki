# One Sokki build at a time on this Mac, across all checkouts and worktrees.
# Sourced by build-and-install.sh and release.sh around their build step.
#
# Two release builds at once (each uses every core and several GB) froze the
# Mac on 2026-11-19. The lock is ~/.cache/sokki-build.lock; a lock left by a
# process that no longer runs is taken over. Before building it also waits
# while another Rust build runs that doesn't use these scripts
# (~/.local/bin/build-busy, when present).

BUILD_LOCK="$HOME/.cache/sokki-build.lock"

build_lock_acquire() {
  mkdir -p "$HOME/.cache"
  local said=""
  while true; do
    if mkdir "$BUILD_LOCK" 2>/dev/null; then
      echo "$$ $REPO" > "$BUILD_LOCK/owner"
      trap build_lock_release EXIT
      if [[ -x "$HOME/.local/bin/build-busy" ]] && "$HOME/.local/bin/build-busy" >/dev/null; then
        # Someone builds without the lock; give it the Mac first.
        rmdir_lock
        [[ -n "$said" ]] || { echo "Another Rust build is running; waiting…"; said=1; }
        sleep 15
        continue
      fi
      return 0
    fi
    local pid; pid=$(awk '{print $1}' "$BUILD_LOCK/owner" 2>/dev/null)
    local age=$(( $(date +%s) - $(stat -f %m "$BUILD_LOCK" 2>/dev/null || date +%s) ))
    if { [[ -n "$pid" ]] && ! kill -0 "$pid" 2>/dev/null; } || { [[ -z "$pid" ]] && (( age > 60 )); }; then
      echo "Taking over a stale build lock (process $pid is gone)."
      rmdir_lock
      continue
    fi
    [[ -n "$said" ]] || { echo "Another Sokki build holds the lock ($(cat "$BUILD_LOCK/owner" 2>/dev/null)); waiting…"; said=1; }
    sleep 15
  done
}

rmdir_lock() { rm -f "$BUILD_LOCK/owner"; rmdir "$BUILD_LOCK" 2>/dev/null; }

build_lock_release() {
  [[ "$(awk '{print $1}' "$BUILD_LOCK/owner" 2>/dev/null)" == "$$" ]] && rmdir_lock
  trap - EXIT
}
