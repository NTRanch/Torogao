#!/usr/bin/env python3
"""Regenerates the M fixture rigs deterministically.

Usage: generate.py <torogao-cli> <m-rigs-dir>
Runs create-character once per manifest fixture into
<m-rigs-dir>/rigs/. The rigs themselves are build products,
not source: the repo carries identities, ground truth, the
manifest and this generator.
"""
import json
import os
import subprocess
import sys


def main() -> int:
    cli, root = sys.argv[1], sys.argv[2]
    manifest = json.load(open(os.path.join(root, "manifest.json")))
    out_dir = os.path.join(root, "rigs")
    os.makedirs(out_dir, exist_ok=True)
    for fx in manifest["fixtures"]:
        out = os.path.join(root, fx["rig"])
        cmd = [cli, "create-character", "--identity",
               os.path.join(root, fx["identity"]), "--pack", fx["pack"],
               "--out", out]
        for preset in fx["presets"]:
            cmd += ["--preset", preset]
        proc = subprocess.run(cmd, capture_output=True, text=True)
        if proc.returncode != 0:
            print(f"FAIL {fx['id']}: {proc.stdout} {proc.stderr}")
            return 1
        print(f"generated {fx['id']} ({fx['pack']}, "
              f"presets={','.join(fx['presets']) or 'none'})")
    return 0


if __name__ == "__main__":
    sys.exit(main())
