#!/usr/bin/env python3
"""Download the built-in voice engine and the bundled voice into
src-tauri/resources/tts, so `tauri build` ships them and nobody has to
install Piper.

    python3 scripts/fetch-voice.py                 # for this computer
    python3 scripts/fetch-voice.py --target x86_64-apple-darwin

What lands in resources/tts:
    lib/             sherpa-onnx C API + ONNX Runtime for the target (from npm)
    espeak-ng-data/  phoneme data for espeak-ng
    voices/          the bundled Piper voice (.onnx + .onnx.json)
    NOTICE.txt       where each part comes from, and its license

Only the Python standard library is used. Re-running skips what's there.
"""

import argparse
import io
import json
import platform
import shutil
import sys
import tarfile
import urllib.request
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "src-tauri" / "resources" / "tts"
CATALOG = json.loads((ROOT / "src-tauri" / "voices.json").read_text())

# Must match the struct layouts in src-tauri/src/engineer/tts.rs.
SHERPA_VERSION = "1.12.9"

# Rust target -> (npm package suffix, library files the app loads)
TARGETS = {
    "aarch64-apple-darwin": ("darwin-arm64", ["libonnxruntime.1.17.1.dylib", "libsherpa-onnx-c-api.dylib"]),
    "x86_64-apple-darwin": ("darwin-x64", ["libonnxruntime.1.17.1.dylib", "libsherpa-onnx-c-api.dylib"]),
    "x86_64-pc-windows-msvc": ("win-x64", ["onnxruntime.dll", "sherpa-onnx-c-api.dll"]),
    "x86_64-unknown-linux-gnu": ("linux-x64", ["libonnxruntime.so", "libsherpa-onnx-c-api.so"]),
}

ESPEAK_DATA = [
    # Built for sherpa-onnx's espeak-ng.
    ("tar.bz2", "https://github.com/k2-fsa/sherpa-onnx/releases/download/tts-models/espeak-ng-data.tar.bz2"),
]
# Fallback: the data Piper itself ships in its Python wheel.
PIPER_WHEEL = ("piper-tts", "1.8.0")


def host_target():
    m = platform.machine().lower()
    if sys.platform == "darwin":
        return "aarch64-apple-darwin" if m in ("arm64", "aarch64") else "x86_64-apple-darwin"
    if sys.platform == "win32":
        return "x86_64-pc-windows-msvc"
    return "x86_64-unknown-linux-gnu"


def get(url):
    print(f"  downloading {url}", flush=True)
    req = urllib.request.Request(url, headers={"User-Agent": "apexnorth-fetch-voice"})
    with urllib.request.urlopen(req, timeout=300) as r:
        return r.read()


def fetch_libs(target):
    pkg, files = TARGETS[target]
    dest = OUT / "lib"
    marker = dest / "TARGET"
    if all((dest / f).is_file() for f in files) and marker.is_file() and marker.read_text().strip() == f"{target} {SHERPA_VERSION}":
        print("engine libraries: already there")
        return
    print(f"engine libraries: sherpa-onnx {SHERPA_VERSION} for {target}")
    meta = json.loads(get(f"https://registry.npmjs.org/sherpa-onnx-{pkg}/{SHERPA_VERSION}"))
    tgz = get(meta["dist"]["tarball"])
    if dest.exists():
        shutil.rmtree(dest)
    dest.mkdir(parents=True)
    with tarfile.open(fileobj=io.BytesIO(tgz), mode="r:gz") as t:
        for f in files:
            m = t.getmember(f"package/{f}")
            (dest / f).write_bytes(t.extractfile(m).read())
            (dest / f).chmod(0o755)
    marker.write_text(f"{target} {SHERPA_VERSION}\n")


def fetch_espeak():
    dest = OUT / "espeak-ng-data"
    if (dest / "phontab").is_file():
        print("espeak-ng data: already there")
        return
    print("espeak-ng data")
    for kind, url in ESPEAK_DATA:
        try:
            data = get(url)
            with tarfile.open(fileobj=io.BytesIO(data), mode="r:bz2") as t:
                members = [m for m in t.getmembers() if m.name.startswith("espeak-ng-data/") and (m.isfile() or m.isdir())]
                t.extractall(OUT, members=members)
            if (dest / "phontab").is_file():
                return
        except Exception as e:  # noqa: BLE001
            print(f"  {url} failed ({e}); trying Piper's wheel")
    name, version = PIPER_WHEEL
    meta = json.loads(get(f"https://pypi.org/pypi/{name}/{version}/json"))
    wheel = next(u["url"] for u in meta["urls"] if u["packagetype"] == "bdist_wheel")
    with zipfile.ZipFile(io.BytesIO(get(wheel))) as z:
        for n in z.namelist():
            if n.startswith("piper/espeak-ng-data/") and not n.endswith("/"):
                p = OUT / n[len("piper/"):]
                p.parent.mkdir(parents=True, exist_ok=True)
                p.write_bytes(z.read(n))
    if not (dest / "phontab").is_file():
        sys.exit("espeak-ng data download failed")


def fetch_voice(voice_id):
    v = next(v for v in CATALOG["voices"] if v["id"] == voice_id)
    dest = OUT / "voices"
    dest.mkdir(parents=True, exist_ok=True)
    model, cfg = dest / f"{voice_id}.onnx", dest / f"{voice_id}.onnx.json"
    if model.is_file() and cfg.is_file():
        print(f"voice {voice_id}: already there")
        return
    print(f"voice {voice_id}")
    base = f"{CATALOG['source']}/{v['path']}/{voice_id}"
    cfg.write_bytes(get(f"{base}.onnx.json"))
    part = dest / f"{voice_id}.onnx.part"
    part.write_bytes(get(f"{base}.onnx"))
    part.replace(model)
    card = dest / f"{voice_id}.MODEL_CARD.txt"
    try:
        card.write_bytes(get(f"{CATALOG['source']}/{v['path']}/MODEL_CARD"))
    except Exception as e:  # noqa: BLE001
        print(f"  no model card ({e})")


def check_catalog():
    """Every downloadable voice must exist (run in CI)."""
    bad = []
    for v in CATALOG["voices"]:
        for ext in (".onnx", ".onnx.json"):
            url = f"{CATALOG['source']}/{v['path']}/{v['id']}{ext}"
            req = urllib.request.Request(url, method="HEAD", headers={"User-Agent": "apexnorth-fetch-voice"})
            try:
                with urllib.request.urlopen(req, timeout=60) as r:
                    print(f"  ok {r.status} {url}")
            except Exception as e:  # noqa: BLE001
                print(f"  MISSING {url}: {e}")
                bad.append(url)
    if bad:
        sys.exit(f"{len(bad)} catalog voice file(s) missing")


NOTICE = """ApexNorth built-in voice
========================

lib/             sherpa-onnx {ver} (Apache-2.0) https://github.com/k2-fsa/sherpa-onnx
                 with ONNX Runtime (MIT) https://github.com/microsoft/onnxruntime
                 and espeak-ng (GPL-3.0) https://github.com/espeak-ng/espeak-ng
espeak-ng-data/  espeak-ng phoneme data (GPL-3.0)
voices/          Piper voices https://github.com/rhasspy/piper
                 Each voice's MODEL_CARD lists its dataset and license.
"""


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--target", default=host_target(), choices=sorted(TARGETS))
    ap.add_argument("--voice", default=CATALOG["bundled"])
    ap.add_argument("--check-catalog", action="store_true", help="only check that every catalog voice can be downloaded")
    a = ap.parse_args()
    if a.check_catalog:
        check_catalog()
        return
    OUT.mkdir(parents=True, exist_ok=True)
    fetch_libs(a.target)
    fetch_espeak()
    fetch_voice(a.voice)
    (OUT / "NOTICE.txt").write_text(NOTICE.format(ver=SHERPA_VERSION))
    print(f"done: {OUT}")


if __name__ == "__main__":
    main()
