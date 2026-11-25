# Store art (Google Play, F-Droid)

Rendered with headless Chromium at 1x; output goes to `../metadata/android/<locale>/images/`.

    CH=~/Library/Caches/ms-playwright/chromium_headless_shell-*/chrome-headless-shell-mac-arm64/chrome-headless-shell
    $CH --headless --hide-scrollbars --force-device-scale-factor=1 --window-size=1024,500 --screenshot=$PWD/../metadata/android/en-US/images/featureGraphic.png "file://$PWD/feature.html#en"
    $CH ... --screenshot=$PWD/../metadata/android/de-DE/images/featureGraphic.png "file://$PWD/feature.html#de"
    $CH --headless --hide-scrollbars --force-device-scale-factor=1 --window-size=512,512 --screenshot=$PWD/icon.png "file://$PWD/icon.html"

- `feature.html`: feature graphic, 1024×500, Sokki design system colours (ground, glow, ink, ink-2, accent).
- `icon.html`: the 512×512 Play icon, the launcher's adaptive foreground on its background (#17181B), full bleed: Play rounds the corners itself.
- `shot.html`: frames an emulator screenshot (1080×2400) with a caption into 1080×1920, Play's 9:16 limit (`#img=…&h=…&s=…&crop=…`). `raw-*.png` are the emulator shots (2026-11-25, made-up speech from macOS `say`, made-up number 555-0142). Dictation shots come from a debug build fed through `window.__dictate`, cropped above its developer link; same UI as the store build.
