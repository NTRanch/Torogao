#!/usr/bin/env python3
"""Strip the mouth parameters from an instantiated puppet.

Produces the purpose-broken viseme fixture: the base
template without ParamMouthOpenY / ParamMouthForm /
ParamMouthX, which viseme-check must reject with
TG-VISEME-PARAM-MISSING.

Usage: make_broken_viseme_fixture.py <in.inx> <out.inx>
"""
import json
import struct
import sys

MOUTH = {"ParamMouthOpenY", "ParamMouthForm", "ParamMouthX"}


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
    src, dst = sys.argv[1], sys.argv[2]
    with open(src, "rb") as fh:
        payload, textures, tail = unpack_inp(fh.read())
    before = len(payload["param"])
    payload["param"] = [p for p in payload["param"]
                        if p["name"] not in MOUTH]
    assert len(payload["param"]) == before - 3
    with open(dst, "wb") as fh:
        fh.write(pack_inp(payload, textures, tail))
    print(f"wrote {dst} (removed 3 mouth parameters)")


if __name__ == "__main__":
    main()
