#!/usr/bin/env python3
"""Find a node in a `uiautomator dump` by its text and print its centre.

    ui.py <dump.xml> <text>      → "x y" of the first node whose text or
                                   content-desc contains <text> (case-insensitive)
    ui.py <dump.xml> --text      → every visible string, one per line
"""
import re
import sys
import xml.etree.ElementTree as ET

path, query = sys.argv[1], sys.argv[2]
nodes = list(ET.parse(path).iter("node"))
if query == "--text":
    for n in nodes:
        for k in ("text", "content-desc"):
            if n.get(k):
                print(n.get(k))
    sys.exit(0)
for n in nodes:
    label = f"{n.get('text', '')} {n.get('content-desc', '')}"
    if query.lower() in label.lower():
        x1, y1, x2, y2 = map(int, re.findall(r"\d+", n.get("bounds", "")))
        print((x1 + x2) // 2, (y1 + y2) // 2)
        sys.exit(0)
sys.exit(1)
