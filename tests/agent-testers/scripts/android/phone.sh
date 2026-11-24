#!/usr/bin/env bash
# The phone's own buttons and gestures, for the tester agent. Only these:
#   back | home | recents        the navigation buttons
#   notifications                pull down the notification shade
#   quick-settings               pull down Quick Settings (fully expanded)
#   collapse                     close the shade
#   landscape | portrait         turn the phone
#   enter                        the keyboard's Enter key
#   crash-log                    the last crash Android logged (empty: none)
set -euo pipefail
case "${1:-}" in
  back) adb shell input keyevent KEYCODE_BACK ;;
  home) adb shell input keyevent KEYCODE_HOME ;;
  recents) adb shell input keyevent KEYCODE_APP_SWITCH ;;
  notifications) adb shell cmd statusbar expand-notifications ;;
  quick-settings) adb shell cmd statusbar expand-settings ;;
  collapse) adb shell cmd statusbar collapse ;;
  landscape) adb shell settings put system accelerometer_rotation 0; adb shell settings put system user_rotation 1 ;;
  portrait) adb shell settings put system accelerometer_rotation 0; adb shell settings put system user_rotation 0 ;;
  enter) adb shell input keyevent KEYCODE_ENTER ;;
  crash-log) adb logcat -d -b crash | tail -40 ;;
  *) sed -n '2,10p' "$0"; exit 2 ;;
esac
echo "done: take a screenshot to see the result"
