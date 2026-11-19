#!/bin/bash
# Installs (or updates) Sokki on Omarchy:
#
#   curl -fsSL https://sokki.org/omarchy | bash
#   curl -fsSL https://sokki.org/omarchy | bash -s -- --dictation   # also for Alt + Space
#   curl -fsSL https://sokki.org/omarchy | bash -s -- --no-llm      # no Ollama, no summaries
#
# Installs the sokki-bin package (from your configured repositories, else
# the AUR, else built from the latest GitHub release), sets Sokki up for you
# with sokki-omarchy-setup, and downloads the speech models.
set -euo pipefail

REPO="feynnon/sokki"
PACKAGE="sokki-bin"

say() { printf '\033[1m%s\033[0m\n' "$*"; }
fail() { printf '\033[31m%s\033[0m\n' "$*" >&2; exit 1; }

[[ $(uname -s) == "Linux" && -f /etc/arch-release ]] || fail "This installer is for Omarchy. For the Mac app, see https://sokki.org"
command -v omarchy-shell >/dev/null || fail "Sokki needs Omarchy 4 (omarchy-shell was not found)."
(( EUID != 0 )) || fail "Run this as yourself, not as root."
case "$(uname -m)" in
  x86_64 | aarch64) ;;
  *) fail "Sokki is built for x86_64 and aarch64, not $(uname -m)." ;;
esac

in_aur() {
  curl -fsS --connect-timeout 10 "https://aur.archlinux.org/rpc/v5/info?arg%5B%5D=$PACKAGE" 2>/dev/null |
    jq -e '.resultcount > 0' >/dev/null 2>&1
}

# makepkg with the PKGBUILD attached to the latest release; it downloads the
# release tarball and checks it against the checksums in that PKGBUILD.
build_from_release() {
  tmp=$(mktemp -d)
  trap 'rm -rf "$tmp"' EXIT
  say "Downloading the latest release…"
  curl -fsSL "https://github.com/$REPO/releases/latest/download/PKGBUILD" -o "$tmp/PKGBUILD" ||
    fail "No Linux release found at https://github.com/$REPO/releases"
  curl -fsSL "https://github.com/$REPO/releases/latest/download/$PACKAGE.install" -o "$tmp/$PACKAGE.install"
  (cd "$tmp" && makepkg -si --needed --noconfirm)
}

if pacman -Qq sokki-git &>/dev/null; then
  say "sokki-git is installed; leaving it as it is."
elif pacman -Si "$PACKAGE" &>/dev/null; then
  say "Installing $PACKAGE…"
  if pacman -Qq "$PACKAGE" &>/dev/null; then
    sudo pacman -S --needed --noconfirm "$PACKAGE"
  else
    omarchy-pkg-add "$PACKAGE"
  fi
elif in_aur; then
  say "Installing $PACKAGE from the AUR…"
  if pacman -Qq "$PACKAGE" &>/dev/null; then
    yay -S --needed --noconfirm "$PACKAGE"
  else
    omarchy-pkg-aur-add "$PACKAGE"
  fi
else
  build_from_release
fi

say "Setting Sokki up…"
setup_args=()
for a in "$@"; do [[ $a == --no-llm ]] || setup_args+=("$a"); done
sokki-omarchy-setup "${setup_args[@]}"

# Summaries come from a local model through Ollama. Omarchy's own Ollama
# install picks the CPU-only package unless CUDA or ROCm is already there, so
# on AMD and Intel graphics we install the Vulkan build, and switch on
# integrated AMD GPUs, which Ollama otherwise skips. An Ollama that is
# already installed is left as it is (`sokki doctor` says what to change).
install_ollama() {
  local vendors pkg
  vendors=$(cat /sys/class/drm/card*/device/vendor 2>/dev/null | sort -u)
  if grep -q 0x10de <<<"$vendors"; then pkg=ollama-cuda; else pkg=ollama-vulkan; fi
  say "Installing Ollama ($pkg) for summaries…"
  omarchy-pkg-add "$pkg"
  if grep -q 0x1002 <<<"$vendors"; then
    sudo mkdir -p /etc/systemd/system/ollama.service.d
    printf '[Service]\nEnvironment=OLLAMA_IGPU_ENABLE=1\n' |
      sudo tee /etc/systemd/system/ollama.service.d/sokki-igpu.conf >/dev/null
    sudo systemctl daemon-reload
  fi
  sudo systemctl enable --now ollama
}
if [[ " $* " != *" --no-llm "* ]] && ! command -v ollama >/dev/null; then
  install_ollama
fi

say "Downloading the speech models and the summary model…"
sokki models install

sokki doctor || true

say "Done. Sokki asks when a call starts; Super + Ctrl + M shows your meetings."
