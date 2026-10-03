#!/usr/bin/env python3
"""Rebuild the bundled GT7 car and track catalogs.

GT7 only sends a numeric car code and no track at all, so ApexNorth ships two
reference tables, both compiled into the Rust binary:

  src-tauri/data/gt7_cars.csv    car code -> "Maker Model 'YY"
  src-tauri/data/gt7_tracks.json every track layout, plus the racing line (in
                                 GT7 world x/z, one point every ~20 m, driving
                                 order) for the layouts we have geometry for

Sources (clone them next to each other and pass the directory):
  https://github.com/ddm999/gt7info              (branch web-new, MIT-0)  -> cars
  https://github.com/jbhoorasingh/gt7-datalogger-track-data  (CC0)        -> tracks

    python3 scripts/build_gt7_data.py /path/to/sources
"""
import csv
import json
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "src-tauri" / "data"


def cars(src: Path) -> int:
    db = src / "gt7info" / "_data" / "db"
    with open(db / "maker.csv", newline="", encoding="utf-8") as f:
        makers = {int(r["ID"]): r["Name"].strip() for r in csv.DictReader(f)}
    rows = []
    with open(db / "cars.csv", newline="", encoding="utf-8") as f:
        for r in csv.DictReader(f):
            maker = makers.get(int(r["Maker"]), "")
            model = r["ShortName"].strip()
            name = model if not maker or model.startswith(maker) else f"{maker} {model}"
            rows.append((int(r["ID"]), name))
    rows.sort()
    with open(OUT / "gt7_cars.csv", "w", newline="", encoding="utf-8") as f:
        w = csv.writer(f, lineterminator="\n")
        w.writerow(["id", "name"])
        w.writerows(rows)
    return len(rows)


def tracks(src: Path) -> tuple[int, int]:
    base = src / "gt7-datalogger-track-data"
    index = json.loads((base / "index.json").read_text(encoding="utf-8"))["configurations"]
    sigs = {s["official_id"]: s for s in json.loads((base / "signatures.json").read_text(encoding="utf-8"))["signatures"]}
    # A reverse layout without its own capture is its forward twin driven backwards.
    reverse_of = {}
    for s in sigs.values():
        twin = s.get("reverse")
        if twin and twin["official_id"] not in sigs:
            reverse_of[twin["official_id"]] = s["official_id"]
    layouts = []
    with_geometry = 0
    for c in index:
        lid = c["official_id"]
        entry = {
            "id": lid,
            "name": c["official_name"],
            "venue": c["track"],
            "country": c["country"],
            "lengthM": c["length_m"],
            "reverse": bool(c["reverse"]),
        }
        sig = sigs.get(lid)
        if sig and sig.get("path"):
            entry["lineM"] = sig["length_m"]
            entry["path"] = [[round(x), round(z)] for x, z in sig["path"]]
            with_geometry += 1
        elif lid in reverse_of:
            entry["reverseOf"] = reverse_of[lid]
            with_geometry += 1
        layouts.append(entry)
    layouts.sort(key=lambda l: l["name"].casefold())
    doc = {
        "source": "jbhoorasingh/gt7-datalogger-track-data (CC0) incl. zetetos/gt-telemetry (MIT)",
        "layouts": layouts,
    }
    (OUT / "gt7_tracks.json").write_text(json.dumps(doc, ensure_ascii=False, separators=(",", ":")) + "\n", encoding="utf-8")
    return len(layouts), with_geometry


if __name__ == "__main__":
    src = Path(sys.argv[1] if len(sys.argv) > 1 else ".").resolve()
    OUT.mkdir(parents=True, exist_ok=True)
    print(f"cars:   {cars(src)}")
    n, g = tracks(src)
    print(f"tracks: {n} layouts, {g} recognisable from geometry")
