#!/usr/bin/env python3
"""Print layout records for the type indices given on argv.
Usage: idxprobe.py <layout> <idx> [idx...]
"""
import importlib.util
import os
import sys

spec = importlib.util.spec_from_file_location(
    "ld", os.path.join(os.path.dirname(os.path.abspath(__file__)), "layoutdump.py")
)
ld = importlib.util.module_from_spec(spec)
spec.loader.exec_module(ld)

types = ld.load(sys.argv[1])
for raw in sys.argv[2:]:
    i = int(raw)
    t = types[i]
    print(
        i,
        t["full"],
        t["kind"],
        "ser=",
        t["ser"],
        "base=",
        types[t["base"]]["full"] if t["base"] is not None else None,
        "under=",
        types[t["under"]]["full"] if t["under"] is not None else None,
    )
    for ln in t["fields"][:30]:
        print("   ", ln)
