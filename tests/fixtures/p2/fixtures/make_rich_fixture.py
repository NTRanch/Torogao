#!/usr/bin/env python3
"""Produce the P2 copy of the rich P1 fixture.

The P1 rich.inx (copied through the P1.5 fixtures) writes its
sine automation binding's "param" as the parameter's numeric
uuid. The format carries the parameter NAME there
(AutomationBinding.serialize writes param.name, and finalize
resolves by name); with the uuid the binding never resolves,
its param stays null, and the first puppet update that runs
the automation dereferences it. Load / validate / export
never step the puppet, so the defect was latent until the
P2 physics preview first ran update() on the fixture.

This script copies the fixture and corrects that one field.

Update (Torogao P3, Item 0): the P1 generator itself has since
been corrected at the source and all three rich copies (P1,
P1.5, P2) regenerated from it; they are now byte-identical and
already carry the name form, so the correction below is a
no-op safeguard that keeps this script safe to re-run against
any older copy.

Source: ~/workspace/torogao-p15-20261026/fixtures/rich.inx
Output: rich.inx in this directory.
"""
import json
import os
import struct

SRC = os.path.expanduser("~/workspace/torogao-p15-20261026/fixtures/rich.inx")
DST = os.path.join(os.path.dirname(os.path.abspath(__file__)), "rich.inx")


def unpack_inp(data: bytes):
    assert data[:8] == b"TRNSRTS\x00"
    (length,) = struct.unpack(">I", data[8:12])
    payload = json.loads(data[12:12 + length])
    rest = data[12 + length:]
    assert rest[:8] == b"TEX_SECT"
    rest = rest[8:]
    (count,) = struct.unpack(">I", rest[:4])
    rest = rest[4:]
    textures = []
    for _ in range(count):
        (ln,) = struct.unpack(">I", rest[:4])
        textures.append((rest[4], rest[5:5 + ln]))
        rest = rest[5 + ln:]
    return payload, textures, rest


def pack_inp(payload: dict, textures: list, tail: bytes = b"") -> bytes:
    body = json.dumps(payload).encode()
    out = b"TRNSRTS\x00" + struct.pack(">I", len(body)) + body
    out += b"TEX_SECT" + struct.pack(">I", len(textures))
    for type_byte, data in textures:
        out += struct.pack(">I", len(data)) + bytes([type_byte]) + data
    return out + tail


def main():
    with open(SRC, "rb") as fh:
        payload, textures, tail = unpack_inp(fh.read())
    names = {p["uuid"]: p["name"] for p in payload["param"]}
    for auto in payload.get("automation", []):
        for binding in auto.get("bindings", []):
            ref = binding.get("param")
            if isinstance(ref, int) and ref in names:
                binding["param"] = names[ref]
    with open(DST, "wb") as fh:
        fh.write(pack_inp(payload, textures, tail))
    print(f"wrote {DST}")


if __name__ == "__main__":
    main()
