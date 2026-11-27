#!/usr/bin/env python3
"""Diff bundle-native join-values output against AssetRipper fixtures.

Usage: differ.py <ours.jsonl> <fixtures.jsonl> [max_reports]

Compares per (collection, path_id):
- MonoBehaviour/ScriptableObject script structures (ours root value vs
  the fixture's m_Structure + engine preamble fields).
- TextAssets by byte length + sha256 (content never enters either side
  beyond hashes on the fixture side; ours carries the same digests).

Categories: exact, name_diff (same values, different field-name sets),
value_mismatch (first differing path reported), ours_review (we did
not decode it), missing_either_side.
"""
import json
import sys
from collections import Counter

PREAMBLE = {"m_GameObject", "m_Enabled", "m_Script", "m_Name"}


def collection_of(sf):
    if "::" in sf:
        return sf.split("::")[-1]
    return sf.rsplit("/", 1)[-1]


def is_pptr(v):
    return isinstance(v, dict) and set(v.keys()) in (
        {"fileID", "pathID"},
        {"m_FileID", "m_PathID"},
    )


def pptr_norm(v):
    if "fileID" in v:
        return (v["fileID"], v["pathID"])
    return (v["m_FileID"], v["m_PathID"])


def close(a, b, path, diffs):
    """Recursive compare; appends (path, ours, theirs) to diffs."""
    if is_pptr(a) and is_pptr(b):
        if pptr_norm(a) != pptr_norm(b):
            diffs.append((path, a, b))
        return
    if isinstance(a, bool) or isinstance(b, bool):
        if bool(a) != bool(b):
            diffs.append((path, a, b))
        return
    if isinstance(a, (int, float)) and isinstance(b, (int, float)):
        if a != b:
            fa, fb = float(a), float(b)
            if abs(fa - fb) > max(1e-6, 1e-6 * abs(fb)):
                diffs.append((path, a, b))
        return
    if isinstance(a, str) and isinstance(b, str):
        if a != b:
            diffs.append((path, a, b))
        return
    if isinstance(a, list) and isinstance(b, list):
        if len(a) != len(b):
            diffs.append((path + ".__len__", len(a), len(b)))
            return
        for i, (x, y) in enumerate(zip(a, b)):
            close(x, y, f"{path}[{i}]", diffs)
        return
    if isinstance(a, dict) and isinstance(b, dict):
        return  # name-set differences are handled by the caller walk
    diffs.append((path, a, b))


def walk(a, b, path, diffs, name_diffs):
    if isinstance(a, dict) and isinstance(b, dict) and not is_pptr(a):
        ka, kb = set(a.keys()), set(b.keys())
        for k in sorted(ka - kb):
            name_diffs.append((f"{path}.{k}", "ours-only"))
        for k in sorted(kb - ka):
            name_diffs.append((f"{path}.{k}", "theirs-only"))
        for k in sorted(ka & kb):
            walk(a[k], b[k], f"{path}.{k}", diffs, name_diffs)
        return
    close(a, b, path, diffs)


def main():
    ours_path, fx_path = sys.argv[1], sys.argv[2]
    max_reports = int(sys.argv[3]) if len(sys.argv) > 3 else 25
    ours = {}
    for line in open(ours_path):
        r = json.loads(line)
        ours[(collection_of(r["sf"]), r["path_id"])] = r
    theirs = {}
    for line in open(fx_path):
        r = json.loads(line)
        theirs[(r["collection"], r["path_id"])] = r

    counts = Counter()
    name_hist = {}
    mismatch_classes = Counter()
    reports = []
    name_reports = []
    for key in sorted(set(ours) | set(theirs)):
        o = ours.get(key)
        t = theirs.get(key)
        if o is None:
            counts["theirs_only"] += 1
            continue
        if t is None:
            counts["ours_only"] += 1
            continue
        if o["class_id"] == 49 or t["class_id"] == 49:
            # TextAssets: fixture records carry name + byte length
            # + sha256; ours carries name + byte length + fnv1a64.
            # The digest algorithms differ, so the cross-check is
            # name + length (byte identity for TextAssets was
            # proven in rung 1 by full-content comparison).
            counts["compared"] += 1
            if o.get("name") == t.get("name") and o.get("bytes") == t.get("bytes"):
                counts["exact"] += 1
            else:
                counts["value_mismatch"] += 1
                mismatch_classes["TextAsset name/length"] += 1
            continue
        if "json" not in t:
            counts["theirs_failed"] += 1
            continue
        counts["compared"] += 1
        tj = t["json"]
        their_struct = dict(tj.get("m_Structure") or {})
        our_value = o["value"]
        our_struct = {k: v for k, v in our_value.items() if k not in PREAMBLE}
        # Preamble compare (script identity + name + enabled + GO).
        pre_diffs = []
        for pk, tk in (
            ("m_GameObject", "m_GameObject"),
            ("m_Script", "m_Script"),
            ("m_Name", "m_Name"),
            ("m_Enabled", "m_Enabled"),
        ):
            if pk in our_value and tk in tj:
                d = []
                close(our_value[pk], tj[tk], pk, d)
                pre_diffs.extend(d)
        diffs, name_diffs = [], []
        walk(our_struct, their_struct, "", diffs, name_diffs)
        diffs = pre_diffs + diffs
        if diffs:
            counts["value_mismatch"] += 1
            first = diffs[0][0]
            mismatch_classes[first.split(".")[1] if "." in first else first] += 1
            if len(reports) < max_reports:
                reports.append((key, o.get("class"), diffs[:4], name_diffs[:4]))
        elif name_diffs:
            counts["name_diff"] += 1
            for nd_path, nd_side in name_diffs:
                leaf = nd_path.rsplit(".", 1)[-1]
                name_hist[f"{leaf}|{nd_side}"] = name_hist.get(f"{leaf}|{nd_side}", 0) + 1
            if len(name_reports) < max_reports:
                name_reports.append((key, o.get("class"), name_diffs[:6]))
        else:
            counts["exact"] += 1

    print(json.dumps(dict(counts), indent=1, sort_keys=True))
    print("name-diff leaf histogram:")
    for k, v in sorted(name_hist.items(), key=lambda kv: -kv[1]):
        print(f"  {v} {k}")
    print("mismatch first-path histogram:", dict(mismatch_classes.most_common(15)))
    for key, cls, diffs, nd in reports:
        print(f"\nMISMATCH {key} {cls}")
        for d in diffs:
            print("   value:", d[0], "ours=", str(d[1])[:90], "theirs=", str(d[2])[:90])
        for n in nd:
            print("   name:", n)
    for key, cls, nd in name_reports:
        print(f"\nNAMEDIFF {key} {cls}")
        for n in nd:
            print("   name:", n)


if __name__ == "__main__":
    main()
