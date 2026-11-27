#!/usr/bin/env python3
"""Project rung-2 value dumps into downstream payload extracts.

Values only: scalars, enums, PPtrs and small scalar lists are
kept whole; bulk arrays (mesh/particle data) are reduced to
their lengths. The projection rule is uniform so the extracts
stay honest: nothing is silently dropped — every reduced array
says how long it was.
"""
import json
import sys

MAX_SCALAR_LIST = 64


def is_scalar(v):
    return v is None or isinstance(v, (bool, int, float, str))


def project(v, depth=0):
    if is_scalar(v):
        return v
    if isinstance(v, list):
        if len(v) <= MAX_SCALAR_LIST and all(is_scalar(e) for e in v):
            return v
        if (
            len(v) <= 32
            and v
            and all(
                isinstance(e, dict) and all(is_scalar(x) for x in e.values())
                for e in v
            )
        ):
            return v
        return {"$list_length": len(v)}
    if isinstance(v, dict):
        # PPtr leaf shape.
        if set(v.keys()) == {"fileID", "pathID"}:
            return v
        return {k: project(val, depth + 1) for k, val in v.items()}
    return str(v)


def main():
    values_path, class_file, out_path = sys.argv[1], sys.argv[2], sys.argv[3]
    wanted = set()
    with open(class_file) as f:
        for line in f:
            line = line.strip()
            if line and not line.startswith("#"):
                wanted.add(line)
    out = []
    with open(values_path) as f:
        for line in f:
            rec = json.loads(line)
            if rec.get("class") in wanted and rec.get("value") is not None:
                sf = rec.get("sf", "")
                out.append(
                    {
                        "collection": sf.rsplit("::", 1)[-1],
                        "path_id": rec.get("path_id"),
                        "class": rec.get("class"),
                        "decode": rec.get("decode"),
                        "value": project(rec["value"]),
                    }
                )
    with open(out_path, "w") as f:
        json.dump(out, f, indent=1, ensure_ascii=False)
    print(f"{out_path}: {len(out)} records")


if __name__ == "__main__":
    main()
