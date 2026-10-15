#!/usr/bin/env bash
# Run on a booted Android emulator (CI: .github/workflows/mobile.yml):
# install the APK, tap "Try demo mode", check the dashboard goes live, a lap
# is recorded and shows up in Activity, and the app never crashes.
#
#   scripts/mobile/android-smoke.sh <apk> <out dir>
set -uo pipefail
apk=$1
out=$2
pkg=com.elevennorth.apexnorth
here="$(cd "$(dirname "$0")" && pwd)"
mkdir -p "$out"

shot() {
  adb exec-out screencap -p > "$out/$1.png"
  if command -v convert >/dev/null; then convert "$out/$1.png" -resize 360x -quality 70 "$out/$1.jpg"; fi
}
dump() {
  adb shell uiautomator dump /sdcard/ui.xml >/dev/null 2>&1
  adb pull /sdcard/ui.xml "$out/$1.xml" >/dev/null 2>&1
}
alive() { adb shell pidof "$pkg" >/dev/null; }
fail() {
  echo "::error::$1"
  shot failure
  adb logcat -d > "$out/logcat.txt"
  grep -E "AndroidRuntime|panicked|FATAL|RustStdoutStderr|$pkg" "$out/logcat.txt" | tail -60
  exit 1
}
# Wait up to $2 seconds for text $1 on screen, dumping the UI to $3.xml.
wait_for() {
  for _ in $(seq 1 "$2"); do
    alive || fail "ApexNorth stopped running"
    dump "$3"
    python3 "$here/ui.py" "$out/$3.xml" "$1" >/dev/null && return 0
    sleep 1
  done
  return 1
}
tap() {
  local xy
  xy="$(python3 "$here/ui.py" "$out/$2.xml" "$1")" || fail "no \"$1\" on screen"
  echo "tap \"$1\" at $xy"
  adb shell input tap $xy
}

adb install -r "$apk" || fail "install failed"
adb logcat -c
adb shell am start -W -n "$pkg/.MainActivity" || fail "launch failed"

wait_for "Try demo mode" 90 connect || fail "the Connect screen never appeared"
shot connect
echo "--- Connect screen text:"; python3 "$here/ui.py" "$out/connect.xml" --text | head -40
tap "Try demo mode" connect

wait_for "LIVE" 60 live || fail "the demo never went live"
echo "Live."
sleep 20
dump dashboard
shot dashboard
echo "--- Dashboard text:"; python3 "$here/ui.py" "$out/dashboard.xml" --text | head -60

# The demo laps take about 90 s; give it two laps and look in Activity.
sleep 150
alive || fail "ApexNorth stopped during the demo race"
shot racing
dump racing
python3 "$here/ui.py" "$out/racing.xml" "Activity" >/dev/null || tap "Menu" racing
dump menu
tap "Activity" menu
sleep 3
dump activity
shot activity
echo "--- Activity text:"; python3 "$here/ui.py" "$out/activity.xml" --text | head -40
grep -qiE "lap|session|race" "$out/activity.xml" || fail "no recorded session in Activity"

# Rotate to landscape: the dashboard should still be there.
adb shell settings put system accelerometer_rotation 0
adb shell settings put system user_rotation 1
sleep 3
tap "Dashboard" activity 2>/dev/null || true
sleep 3
shot landscape
adb shell settings put system user_rotation 0

adb logcat -d > "$out/logcat.txt"
if grep -E "panicked|FATAL EXCEPTION" "$out/logcat.txt"; then fail "crash in logcat"; fi
alive || fail "ApexNorth stopped"
echo "Android demo race OK."
