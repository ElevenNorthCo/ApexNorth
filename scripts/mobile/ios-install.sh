#!/usr/bin/env bash
# Build ApexNorth for your iPhone and install it over Wi-Fi (or USB).
#
#   apexnorth-ios              pull the latest code, build, install, open
#   apexnorth-ios --no-pull    build what's checked out now
#   apexnorth-ios --branch beta   switch to a branch first (main, beta, ...)
#
# Install the command once:  ln -sf "$PWD/scripts/mobile/ios-install.sh" ~/.local/bin/apexnorth-ios
#
# Needs (once): Xcode signed in to your Apple ID, the iPhone paired with this
# Mac (plug it in once and trust it; after that Wi-Fi works while both are on
# the same network), and Developer Mode on the phone. Your Apple team ID is
# read from APEXNORTH_IOS_TEAM or ~/.config/apexnorth/ios.env, never from the
# repo. A free Apple ID's builds expire after 7 days: run this again.
set -euo pipefail

say() { printf '\033[1;32m▸\033[0m %s\n' "$*"; }
die() { printf '\033[1;31m✗\033[0m %s\n' "$*" >&2; exit 1; }

repo="$(cd "$(dirname "$(readlink -f "${BASH_SOURCE[0]}")")/../.." && pwd)"
cd "$repo"

pull=true
branch=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --no-pull) pull=false ;;
    --branch) branch="${2:?--branch needs a name}"; shift ;;
    -h|--help) sed -n '2,15p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) die "unknown option $1 (try --help)" ;;
  esac
  shift
done

# Your Apple team (personal teams are fine).
[[ -f "$HOME/.config/apexnorth/ios.env" ]] && source "$HOME/.config/apexnorth/ios.env"
team="${APEXNORTH_IOS_TEAM:-}"
[[ -n "$team" ]] || die "No Apple team ID. Put APEXNORTH_IOS_TEAM=<id> in ~/.config/apexnorth/ios.env (Xcode → Settings → Accounts)."

# Full Xcode, even when xcode-select points at the Command Line Tools: the
# Tauri CLI runs xcodebuild/xcrun from PATH, so put shims first.
export DEVELOPER_DIR="${DEVELOPER_DIR:-/Applications/Xcode.app/Contents/Developer}"
shims="$(mktemp -d)"
trap 'rm -rf "$shims"' EXIT
for tool in xcodebuild xcrun; do
  printf '#!/bin/sh\nexport DEVELOPER_DIR="%s"\nexec /usr/bin/%s "$@"\n' "$DEVELOPER_DIR" "$tool" > "$shims/$tool"
  chmod +x "$shims/$tool"
done
# CocoaPods: a Homebrew/gem install, or the private one from the first setup.
if ! command -v pod >/dev/null && [[ -x "$HOME/.cocoapods-gems/bin/pod" ]]; then
  ruby="$(ls -d /usr/local/Homebrew/Library/Homebrew/vendor/portable-ruby/*/bin/ruby /opt/homebrew/Library/Homebrew/vendor/portable-ruby/*/bin/ruby 2>/dev/null | sort -V | tail -1 || true)"
  printf '#!/bin/sh\nexport GEM_HOME="%s" GEM_PATH="%s" LANG=en_US.UTF-8\nexec "%s" "%s" "$@"\n' \
    "$HOME/.cocoapods-gems" "$HOME/.cocoapods-gems" "$ruby" "$HOME/.cocoapods-gems/bin/pod" > "$shims/pod"
  chmod +x "$shims/pod"
fi
export PATH="$shims:$PATH"

# The phone: APEXNORTH_IOS_DEVICE, else the first available iPhone.
device="${APEXNORTH_IOS_DEVICE:-}"
if [[ -z "$device" ]]; then
  list="$(mktemp)"
  xcrun devicectl list devices --json-output "$list" >/dev/null 2>&1 || true
  device="$(python3 - "$list" <<'EOF'
import json, sys
try:
    devices = json.load(open(sys.argv[1]))["result"]["devices"]
except Exception:
    devices = []
for d in devices:
    hw = d.get("hardwareProperties", {})
    if hw.get("platform") == "iOS" and d.get("connectionProperties", {}).get("tunnelState") != "unavailable":
        print(hw.get("udid", "")); break
EOF
)"
  rm -f "$list"
fi
[[ -n "$device" ]] || die "No iPhone found. Unlock it, make sure it's on the same Wi-Fi as this Mac (or plug it in), and try again."

if $pull; then
  if [[ -n "$(git status --porcelain --untracked-files=no)" ]]; then
    say "Local changes here: building them as they are (not pulling)."
  else
    [[ -n "$branch" ]] && { say "Switching to $branch"; git checkout -q "$branch"; }
    say "Pulling the latest $(git rev-parse --abbrev-ref HEAD)"
    git pull --ff-only -q
  fi
fi
say "Building $(git rev-parse --abbrev-ref HEAD) @ $(git rev-parse --short HEAD) ($(node -p "require('./src-tauri/tauri.conf.json').version"))"

[[ -d node_modules ]] || npm install
if [[ ! -d src-tauri/gen/apple ]]; then
  say "Generating the Xcode project"
  npx tauri ios init --ci
fi
# ios init leaves Tauri's placeholder icon; keep ApexNorth's in place.
cp src-tauri/icons/ios/*.png src-tauri/gen/apple/Assets.xcassets/AppIcon.appiconset/

log="$(mktemp -t apexnorth-ios-build)"
say "Compiling (a few minutes; log: $log)"
if ! APPLE_DEVELOPMENT_TEAM="$team" npx tauri ios build --ci --target aarch64 --export-method debugging >"$log" 2>&1; then
  grep -E 'error:|Error ' "$log" | tail -10 >&2
  die "Build failed. Full log: $log"
fi

ipa="$(ls -t src-tauri/gen/apple/build/arm64/*.ipa | head -1)"
app_dir="$(mktemp -d)"
unzip -q "$ipa" -d "$app_dir"
say "Installing on your iPhone"
xcrun devicectl device install app --device "$device" "$app_dir/Payload/ApexNorth.app" >/dev/null \
  || die "Install failed. Is the phone unlocked and on the same network? (Plugging it in always works.)"
rm -rf "$app_dir"
xcrun devicectl device process launch --device "$device" com.elevennorth.apexnorth >/dev/null 2>&1 || true
say "Done: ApexNorth $(node -p "require('./src-tauri/tauri.conf.json').version") is on your iPhone. 🏁"
