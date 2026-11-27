#!/usr/bin/env python3
"""compare_twin.py — the twin gate's comparison instrument
(runtime plan Rung R2).

Reads `runtime-twin` / `loop-twin` emit reports and judges
them in the plan's two tiers:

- `tier1 <file...>`: the identity lines (core, corpus hash,
  genesis, the per-tick `t1` chain, the checkpoint save hash,
  the replay verdict) must be identical in every file. Tier 1
  admits no tolerance.
- `band <abs> <rel> <file...>`: the Tier 2 numeric check.
  Every gauge value (carried as raw bit patterns on `t2v`
  lines) is compared across files; the largest absolute and
  relative differences are printed as the measurement of
  record, and the verdict is PASS when the measured distance
  sits inside the band on either leg (the combined-tolerance
  rule `torogao_runtime::BandReport::within` implements).
  Per-tick digest mismatches are counted and printed as
  information: across architectures a digest difference is
  the expected shape of a passing band comparison, never the
  verdict itself.
"""

import struct
import sys

IDENT_PREFIXES = (
    "core ",
    "corpus-sha ",
    "genesis ",
    "t1 ",
    "save-at ",
    "save-sha ",
    "replay ",
)


def load(path):
    ident = []
    values = {}
    digests = {}
    with open(path) as handle:
        for line in handle.read().splitlines():
            if line.startswith(IDENT_PREFIXES):
                ident.append(line)
            parts = line.split(" ")
            if parts[0] == "t2v" and len(parts) == 4:
                key = (int(parts[1]), parts[2])
                values[key] = struct.unpack(">d", bytes.fromhex(parts[3]))[0]
            elif parts[0] == "t2" and len(parts) == 3:
                digests[int(parts[1])] = parts[2]
    return ident, values, digests


def tier1(paths):
    reference, _, _ = load(paths[0])
    ok = True
    for path in paths[1:]:
        ident, _, _ = load(path)
        if ident != reference:
            ok = False
            print(f"TIER1 MISMATCH: {path} differs from {paths[0]}")
    print("TIER1 PASS" if ok else "TIER1 FAIL")
    return 0 if ok else 1


def band(abs_band, rel_band, paths):
    _, ref_values, ref_digests = load(paths[0])
    max_abs = 0.0
    max_rel = 0.0
    compared = 0
    digest_mismatches = 0
    for path in paths[1:]:
        _, values, digests = load(path)
        for tick, digest in ref_digests.items():
            if digests.get(tick) != digest:
                digest_mismatches += 1
        for key, ref in ref_values.items():
            if key not in values:
                print(f"BAND FAIL: gauge {key} missing from {path}")
                return 1
            other = values[key]
            diff = abs(ref - other)
            scale = max(abs(ref), abs(other))
            rel = diff / scale if scale > 0 else 0.0
            compared += 1
            max_abs = max(max_abs, diff)
            max_rel = max(max_rel, rel)
    within = max_abs <= abs_band or max_rel <= rel_band
    print(
        f"BAND gauges_compared={compared} "
        f"digest_mismatch_ticks={digest_mismatches} "
        f"max_abs={max_abs:.3e} max_rel={max_rel:.3e} "
        f"band_abs={abs_band:.1e} band_rel={rel_band:.1e}"
    )
    print("BAND PASS" if within else "BAND FAIL")
    return 0 if within else 1


def main(argv):
    if len(argv) >= 3 and argv[1] == "tier1":
        return tier1(argv[2:])
    if len(argv) >= 5 and argv[1] == "band":
        return band(float(argv[2]), float(argv[3]), argv[4:])
    print(__doc__)
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv))
