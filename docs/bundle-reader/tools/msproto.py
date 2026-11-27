#!/usr/bin/env python3
"""Prototype: derive the stripped MonoScript serialized layout.

Input: lines from msdump.rs ("<sf-name> <path_id> <hex>") on stdin,
plus the title's il2cpp-index JSON. Tests candidate layouts by
exact-consumption rate and by how often the decoded
(assembly, namespace, class) triple resolves in the index.
"""

import json
import os
import struct
import sys

HOME = os.path.expanduser("~")


def read_string(buf, o):
    if o + 4 > len(buf):
        return None, o
    (n,) = struct.unpack_from("<i", buf, o)
    o += 4
    if n < 0 or n > 8192 or o + n > len(buf):
        return None, o
    s = buf[o : o + n]
    o += n
    o = (o + 3) // 4 * 4
    try:
        return s.decode("utf-8"), o
    except UnicodeDecodeError:
        return None, o


def try_layout(buf, layout):
    o = 0
    out = {}
    for kind, name in layout:
        if kind == "str":
            v, o = read_string(buf, o)
            if v is None:
                return None
            out[name] = v
        elif kind == "i32":
            if o + 4 > len(buf):
                return None
            (out[name],) = struct.unpack_from("<i", buf, o)
            o += 4
        elif kind == "hash":
            if o + 16 > len(buf):
                return None
            out[name] = buf[o : o + 16].hex()
            o += 16
        elif kind == "bool_pad":
            if o + 1 > len(buf):
                return None
            out[name] = buf[o]
            o += 1
            o = (o + 3) // 4 * 4
        elif kind == "bool_raw":
            if o + 1 > len(buf):
                return None
            out[name] = buf[o]
            o += 1
        else:
            raise AssertionError(kind)
    out["_consumed"] = o
    return out


LAYOUTS = {
    "A name,exec,hash,cls,ns,asm,editor(pad)": [
        ("str", "m_Name"),
        ("i32", "m_ExecutionOrder"),
        ("hash", "m_PropertiesHash"),
        ("str", "m_ClassName"),
        ("str", "m_Namespace"),
        ("str", "m_AssemblyName"),
        ("bool_pad", "m_IsEditorScript"),
    ],
    "B name,exec,hash,cls,ns,asm": [
        ("str", "m_Name"),
        ("i32", "m_ExecutionOrder"),
        ("hash", "m_PropertiesHash"),
        ("str", "m_ClassName"),
        ("str", "m_Namespace"),
        ("str", "m_AssemblyName"),
    ],
    "C name,exec,cls,ns,asm,hash,editor(pad)": [
        ("str", "m_Name"),
        ("i32", "m_ExecutionOrder"),
        ("str", "m_ClassName"),
        ("str", "m_Namespace"),
        ("str", "m_AssemblyName"),
        ("hash", "m_PropertiesHash"),
        ("bool_pad", "m_IsEditorScript"),
    ],
    "D name,exec,hash,cls,ns,asm,editor(raw)": [
        ("str", "m_Name"),
        ("i32", "m_ExecutionOrder"),
        ("hash", "m_PropertiesHash"),
        ("str", "m_ClassName"),
        ("str", "m_Namespace"),
        ("str", "m_AssemblyName"),
        ("bool_raw", "m_IsEditorScript"),
    ],
}


def main():
    index_path = sys.argv[1]
    index = json.load(open(index_path))
    images = index.get("images", [])
    triples = set()
    for t in index["types"]:
        triples.add((t["assembly"], t["namespace"], t["name"]))
    rows = []
    for line in sys.stdin:
        parts = line.split()
        if len(parts) != 3:
            continue
        rows.append(bytes.fromhex(parts[2]))
    print(f"monoscript objects: {len(rows)}")
    for label, layout in LAYOUTS.items():
        exact = 0
        resolved = 0
        near = 0
        for buf in rows:
            d = try_layout(buf, layout)
            if d is None:
                continue
            if d["_consumed"] == len(buf):
                exact += 1
                asm = d.get("m_AssemblyName", "")
                asm = asm[:-4] if asm.endswith(".dll") else asm
                if (asm, d.get("m_Namespace", ""), d.get("m_ClassName", "")) in triples:
                    resolved += 1
            elif abs(d["_consumed"] - len(buf)) <= 4:
                near += 1
        print(f"{label}: exact {exact}/{len(rows)} resolved {resolved} near {near}")


if __name__ == "__main__":
    main()
