#!/usr/bin/env bash
# ApexNorth installer for macOS and Linux. Paste this into Terminal:
#
#   curl -fsSL https://raw.githubusercontent.com/ElevenNorthCo/ApexNorth/main/install.sh | bash
#
# It downloads the latest release from GitHub, installs it, and opens it.
# Run it again any time to update. A specific version:
#   curl -fsSL .../install.sh | APEXNORTH_VERSION=v0.2.0 bash
set -euo pipefail

REPO="ElevenNorthCo/ApexNorth"
if [ -n "${APEXNORTH_VERSION:-}" ]; then
  BASE="https://github.com/$REPO/releases/download/$APEXNORTH_VERSION"
else
  BASE="https://github.com/$REPO/releases/latest/download"
fi

say() { printf '\033[1;32m==>\033[0m %s\n' "$*"; }
fail() { printf '\033[1;31mError:\033[0m %s\n' "$*" >&2; exit 1; }

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

download() {
  say "Downloading $1"
  curl -fL --progress-bar -o "$2" "$BASE/$1" || fail "Could not download $BASE/$1"
}

install_mac() {
  if [ "$(uname -m)" = "arm64" ]; then file="ApexNorth-macOS-AppleSilicon.dmg"; else file="ApexNorth-macOS-Intel.dmg"; fi
  download "$file" "$TMP/ApexNorth.dmg"

  say "Installing ApexNorth"
  mnt="$TMP/mnt"
  mkdir -p "$mnt"
  hdiutil attach -nobrowse -quiet -mountpoint "$mnt" "$TMP/ApexNorth.dmg" || fail "Could not open the disk image"
  app="$(find "$mnt" -maxdepth 1 -name '*.app' | head -1)"
  [ -n "$app" ] || { hdiutil detach -quiet "$mnt"; fail "No app found in the disk image"; }

  dest="/Applications"
  [ -w "$dest" ] || dest="$HOME/Applications"
  mkdir -p "$dest"
  # Quit a running copy so it can be replaced.
  osascript -e 'quit app "ApexNorth"' >/dev/null 2>&1 || true
  sleep 1
  rm -rf "$dest/ApexNorth.app"
  cp -R "$app" "$dest/ApexNorth.app"
  hdiutil detach -quiet "$mnt" || true
  # Downloaded with curl, so macOS doesn't block it; clear the flag just in case.
  xattr -dr com.apple.quarantine "$dest/ApexNorth.app" 2>/dev/null || true

  say "Installed to $dest/ApexNorth.app. Opening it now."
  open "$dest/ApexNorth.app"
}

install_linux() {
  [ "$(uname -m)" = "x86_64" ] || fail "ApexNorth for Linux is built for x86_64 only."
  bin="$HOME/.local/bin"
  mkdir -p "$bin" "$HOME/.local/share/applications"
  download "ApexNorth-Linux.AppImage" "$TMP/ApexNorth.AppImage"
  install -m 755 "$TMP/ApexNorth.AppImage" "$bin/ApexNorth.AppImage"
  cat > "$HOME/.local/share/applications/apexnorth.desktop" <<DESKTOP
[Desktop Entry]
Name=ApexNorth
Comment=GT7 live telemetry and race engineer
Exec=$bin/ApexNorth.AppImage
Terminal=false
Type=Application
Categories=Utility;Game;
DESKTOP
  say "Installed to $bin/ApexNorth.AppImage (also in your app menu)."
  if ! ldconfig -p 2>/dev/null | grep -q libfuse.so.2; then
    echo "AppImages need FUSE 2. On Ubuntu/Debian: sudo apt install libfuse2"
  fi
  echo "Start it from your app menu, or run: $bin/ApexNorth.AppImage"
}

case "$(uname -s)" in
  Darwin) install_mac ;;
  Linux) install_linux ;;
  *) fail "This installer is for macOS and Linux. On Windows, use install.ps1 (see the README)." ;;
esac
