#!/usr/bin/env python3
"""Inspect an il2cpp-layout 1 file: print one type's record and its
flattened field list (base-first), following the same rules the
synthesizer uses. Usage: layoutdump.py <layout> <full-or-partial-name>
"""
import sys

STOP = {
    "System.Object",
    "System.ValueType",
    "UnityEngine.Object",
    "UnityEngine.Component",
    "UnityEngine.Behaviour",
    "UnityEngine.MonoBehaviour",
    "UnityEngine.ScriptableObject",
}


def load(path):
    types = []
    fields = {}
    cur = None
    with open(path) as f:
        lines = [ln.rstrip("\n") for ln in f]
    assert lines[0] in ("il2cpp-layout 1", "il2cpp-layout 2")
    n = int(lines[2].split()[1])
    i = 3
    raw = {}
    for idx in range(n):
        t = lines[i].split()
        a = lines[i + 1]
        nn = lines[i + 2]
        i += 3
        asm = a.split(" ", 2)[2]
        ns, name = nn.split(" ", 2)[2].split(" ", 1)
        raw[idx] = {
            "kind": t[2],
            "ser": t[3] == "1",
            "base": (int(t[6]) if t[5] == "t" else None) if len(t) > 6 else (None if t[4] == "-1" else int(t[4])),
            "under": None if t[4] == "-1" else int(t[4]) if len(t) > 6 else (None if t[5] == "-1" else int(t[5])),
            "asm": asm,
            "ns": "" if ns == "-" else ns,
            "name": name,
        }
    for idx in range(n):
        d = lines[i].split()
        i += 1
        cnt = int(d[2])
        fl = []
        for _ in range(cnt):
            fl.append(lines[i])
            i += 1
        fields[idx] = fl
    for idx in range(n):
        r = raw[idx]
        r["full"] = (r["ns"] + "." + r["name"]) if r["ns"] else r["name"]
        r["fields"] = fields[idx]
        types.append(r)
    return types


def main():
    types = load(sys.argv[1])
    pat = sys.argv[2]
    hits = [i for i, t in enumerate(types) if pat in t["full"]]
    if not hits:
        print("no match")
        return
    idx = hits[0]
    t = types[idx]
    print(f"#{idx} {t['full']} kind={t['kind']} ser={t['ser']} asm={t['asm']}")
    chain = []
    cur = idx
    while cur is not None:
        chain.append(cur)
        b = types[cur]["base"]
        if b is None or types[b]["full"] in STOP:
            break
        cur = b
    print("chain:", " -> ".join(types[c]["full"] for c in chain))
    for c in reversed(chain):
        print(f"-- {types[c]['full']}")
        for ln in types[c]["fields"]:
            print("   ", ln)


if __name__ == "__main__":
    main()
