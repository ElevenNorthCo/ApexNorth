#!/usr/bin/env bash
# Run on macOS after `tauri ios build --target aarch64-sim` (CI:
# .github/workflows/mobile.yml): boot a simulator, install the app, launch it
# with APEXNORTH_AUTODEMO=1 (starts demo mode and prints "apexnorth-smoke:"
# lines), and check it goes live and records laps.
#
#   scripts/mobile/ios-smoke.sh <out dir>
set -uo pipefail
out=$1
bundle=com.elevennorth.apexnorth
mkdir -p "$out"

app="$(find src-tauri/gen/apple/build -type d -name '*.app' -path '*sim*' | head -1)"
[[ -z "$app" ]] && app="$(find src-tauri/gen/apple/build -type d -name '*.app' | head -1)"
[[ -z "$app" ]] && { echo "::error::no .app built"; find src-tauri/gen/apple/build -maxdepth 4 | head -50; exit 1; }
echo "App: $app"
(cd "$(dirname "$app")" && zip -qry "$OLDPWD/$out/ApexNorth-iOS-Simulator.zip" "$(basename "$app")")

device="$(xcrun simctl list devices available -j | python3 -c '
import json, sys
d = json.load(sys.stdin)["devices"]
phones = [x for rt, xs in d.items() if "iOS" in rt for x in xs if x["name"].startswith("iPhone")]
print(phones[-1]["udid"] if phones else "")')"
[[ -z "$device" ]] && { echo "::error::no iPhone simulator"; xcrun simctl list devices; exit 1; }
xcrun simctl boot "$device" || true
xcrun simctl bootstatus "$device" -b
xcrun simctl install "$device" "$app"

shot() {
  xcrun simctl io "$device" screenshot "$out/$1.png" >/dev/null 2>&1
  sips -Z 480 -s format jpeg -s formatOptions 60 "$out/$1.png" --out "$out/$1.jpg" >/dev/null 2>&1
}

SIMCTL_CHILD_APEXNORTH_AUTODEMO=1 xcrun simctl launch --console-pty --terminate-running-process "$device" "$bundle" > "$out/console.txt" 2>&1 &
launcher=$!

ok=0
for i in $(seq 1 300); do
  if grep -q "apexnorth-smoke: lap .* recorded" "$out/console.txt"; then ok=1; break; fi
  if grep -q "apexnorth-smoke: demo failed" "$out/console.txt"; then break; fi
  [[ $i == 30 ]] && shot dashboard
  sleep 1
done
shot racing
kill "$launcher" 2>/dev/null || true
echo "--- console:"
grep -E "apexnorth-smoke|panicked|error" "$out/console.txt" | head -60
grep -q "apexnorth-smoke: connection Live" "$out/console.txt" || { echo "::error::the demo never went live"; exit 1; }
[[ $ok == 1 ]] || { echo "::error::no lap recorded within 5 minutes"; exit 1; }
echo "iOS demo race OK."
