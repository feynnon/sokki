#!/bin/zsh
# Builds "Sokki Autocorrect Lab", a tiny app that runs autocorrect against a real app's focused
# text field, so autocorrect can be tested on a Mac without Sokki's keyboard hook.
# It must start as its own app (open), so macOS checks its own Accessibility grant.
# Signed with the Developer ID, so the grant survives rebuilds.
#   scripts/autocorrect-lab.sh build
#   scripts/autocorrect-lab.sh run <pid> <bundle id> <seconds> <out file>
#   scripts/autocorrect-lab.sh paste-edit <pid> <bundle id> <wait ms> <out file>
set -e
root=${0:A:h:h}
lab="${LAB_DIR:-$HOME/Library/Application Support/org.sokki.autocorrect-lab}/Sokki Autocorrect Lab.app"
case $1 in
build)
  (cd $root/crates/sokki-autocorrect && cargo build --example live --example explore_ax --example paste_edit -q)
  mkdir -p "$lab/Contents/MacOS"
  cp $root/crates/sokki-autocorrect/target/debug/examples/live "$lab/Contents/MacOS/lab"
  cp $root/crates/sokki-autocorrect/target/debug/examples/explore_ax "$lab/Contents/MacOS/explore"
  cp $root/crates/sokki-autocorrect/target/debug/examples/paste_edit "$lab/Contents/MacOS/paste_edit"
  cat > "$lab/Contents/Info.plist" <<PL
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>org.sokki.autocorrect-lab</string>
<key>CFBundleName</key><string>Sokki Autocorrect Lab</string>
<key>CFBundleExecutable</key><string>lab</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>LSUIElement</key><true/>
</dict></plist>
PL
  codesign --force -s "Developer ID Application" -i org.sokki.autocorrect-lab "$lab"
  echo "$lab" ;;
run)
  shift
  open -g -n -W "$lab" --args "$@" ;;
paste-edit)
  # paste-edit <pid> <bundle id> <wait ms> <out file>: the instant paste's edit in a real
  # field (examples/paste_edit.rs). Needs the app in front: night runs only.
  shift
  open -g -n -W "$lab" --args paste-edit "$@" ;;
explore)
  # explore <pid> [manual,enhanced,wait] <out file>: same bundle, so the same grant
  shift
  open -g -n -W "$lab" --args explore "$@" ;;
esac
