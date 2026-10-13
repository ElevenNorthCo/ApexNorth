#!/usr/bin/env python3
"""Write the in-app updater's feed, out/latest.json, from the signed update
files the release workflow collected (each with a .sig next to it).

    update-feed.py <dir> <version> <download base URL> [beta build number]

The app (src-tauri/src/updater.rs) reads it from the Latest release, or from
beta-latest on the beta channel. Does nothing when the build wasn't signed.

The version must be exactly the one the files were signed for (the updater
rejects a mismatch), so a beta's build number goes in the notes instead,
as "beta.<n>: <change>", where the app's beta channel reads it.
"""
import datetime
import json
import os
import subprocess
import sys

# Update file → the updater's platform keys. Linux .deb installs need the
# "-deb" key; a plain linux-x86_64 is the AppImage.
PLATFORMS = {
    "ApexNorth-macOS-AppleSilicon.app.tar.gz": ["darwin-aarch64"],
    "ApexNorth-macOS-Intel.app.tar.gz": ["darwin-x86_64"],
    "ApexNorth-Windows-Setup.exe": ["windows-x86_64"],
    "ApexNorth-Linux.AppImage": ["linux-x86_64"],
    "ApexNorth-Linux.deb": ["linux-x86_64-deb"],
}


def main(out, version, base, beta_build=None):
    platforms = {}
    for name, keys in PLATFORMS.items():
        sig = os.path.join(out, name + ".sig")
        if not os.path.isfile(sig):
            continue
        with open(sig) as f:
            signature = f.read().strip()
        for key in keys:
            platforms[key] = {"signature": signature, "url": f"{base}/{name}"}
    if not platforms:
        print("No signed update files; skipping latest.json.")
        return
    try:
        notes = subprocess.run(["git", "log", "-1", "--format=%s"], capture_output=True, text=True).stdout.strip()
    except OSError:
        notes = ""
    if beta_build:
        notes = f"beta.{beta_build}: {notes}"
    feed = {
        "version": version,
        "notes": notes,
        "pub_date": datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
        "platforms": platforms,
    }
    with open(os.path.join(out, "latest.json"), "w") as f:
        json.dump(feed, f, indent=2)
    print(json.dumps(feed, indent=2))


if __name__ == "__main__":
    if len(sys.argv) not in (4, 5):
        sys.exit(__doc__)
    main(*sys.argv[1:])
