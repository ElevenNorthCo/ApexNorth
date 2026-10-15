#!/usr/bin/env bash
# Run on a booted Android emulator (CI: .github/workflows/mobile.yml):
# install the APK, start it with the demo-mode test hook
# (debug.apexnorth.autodemo=1), and check from logcat that it goes live,
# records laps and the engineer talks, without crashing. Screenshots of the
# portrait and landscape dashboard go to <out dir>.
#
#   scripts/mobile/android-smoke.sh <apk> <out dir>
set -uo pipefail
apk=$1
out=$2
pkg=com.elevennorth.apexnorth
mkdir -p "$out"

shot() {
  adb exec-out screencap -p > "$out/$1.png"
  if command -v convert >/dev/null; then convert "$out/$1.png" -resize 360x -quality 70 "$out/$1.jpg"; fi
}
alive() { adb shell pidof "$pkg" >/dev/null; }
smoke() { adb logcat -d -s ApexNorth:I > "$out/smoke.txt"; }
fail() {
  echo "::error::$1"
  shot failure
  adb logcat -d > "$out/logcat.txt"
  grep -E "AndroidRuntime|panicked|FATAL|ApexNorth|$pkg|chromium" "$out/logcat.txt" | tail -80
  exit 1
}
# Wait up to $2 seconds for a smoke line matching $1.
wait_for() {
  for _ in $(seq 1 "$2"); do
    alive || fail "ApexNorth stopped running"
    smoke
    grep -qE "$1" "$out/smoke.txt" && return 0
    sleep 1
  done
  return 1
}

adb install -r "$apk" || fail "install failed"
adb shell setprop debug.apexnorth.autodemo 1
adb logcat -c
adb shell am start -W -n "$pkg/.MainActivity" || fail "launch failed"

wait_for "demo started" 60 || fail "demo mode didn't start"
wait_for "connection Live" 60 || fail "the demo never went live"
echo "Live."
sleep 25
shot dashboard

wait_for "lap [0-9]+ recorded" 240 || fail "no lap recorded within 4 minutes"
wait_for "engineer: " 30 || fail "the engineer never spoke"
shot racing

# Rotate to landscape: the sim-dash layout.
adb shell settings put system accelerometer_rotation 0
adb shell settings put system user_rotation 1
sleep 5
shot landscape
adb shell settings put system user_rotation 0
sleep 2

alive || fail "ApexNorth stopped"
adb logcat -d > "$out/logcat.txt"
if grep -E "panicked|FATAL EXCEPTION" "$out/logcat.txt"; then fail "crash in logcat"; fi
echo "--- smoke log:"
cat "$out/smoke.txt"
echo "Android demo race OK."
