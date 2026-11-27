#!/usr/bin/env python3
"""Hand-walk the PlayAudio object 5024 bytes under candidate
AnimatedParameter layouts and print spans. Evidence tool."""
import os
import struct

W = os.path.expanduser("~/.local/share/pax/bundle-native-rung2-20261110")
buf = None
for line in open(W + "/evidence/msdump-pes-level0-mb.txt"):
    parts = line.split()
    if len(parts) == 3 and parts[1] == "5024":
        buf = bytes.fromhex(parts[2])
        break
assert buf is not None
print("object size", len(buf))


def rd_str(b, o):
    (n,) = struct.unpack_from("<i", b, o)
    o += 4
    s = b[o : o + n]
    o += n
    o = (o + 3) // 4 * 4
    return s, o


def rd_f32arr(b, o):
    (n,) = struct.unpack_from("<i", b, o)
    o += 4
    vals = struct.unpack_from(f"<{n}f", b, o)
    raws = struct.unpack_from(f"<{n}I", b, o)
    o += 4 * n
    o = (o + 3) // 4 * 4
    return list(zip(vals, raws)), o


# preamble: pptr(12) bool(1+3) pptr(12) name str
o = 12 + 4 + 12
name, o = rd_str(buf, o)
print("m_Name", name, "->", o)
(st,) = struct.unpack_from("<f", buf, o)
print("_startTime", st)
o += 4
(cnt,) = struct.unpack_from("<i", buf, o)
print("animatedParameters count", cnt)
o += 4
for e in range(cnt):
    start = o
    s, o = rd_str(buf, o)
    print(f" elem{e} _serializedData len={len(s)} -> {o}")
    isd = buf[o]
    o += 1
    o = (o + 3) // 4 * 4
    print(f" elem{e} _isDisabled={isd} -> {o}")
    s2, o = rd_str(buf, o)
    print(f" elem{e} _scriptExpression={s2!r} -> {o}")
    snap, o = rd_f32arr(buf, o)
    print(f" elem{e} snapshot={snap} -> {o}")
    last, o = rd_f32arr(buf, o)
    print(f" elem{e} lastEval={last} -> {o}")
    print(f" elem{e} span {start}..{o} ({o-start} bytes)")
print("after elements:", o)
rest = buf[o : o + 60]
print("next bytes:", rest.hex(" "))
# remaining PlayAudio fields per layout: _length,_blendIn,_blendOut (3 floats),
# audioClip pptr(12), clipOffset, volume, pitch, stereoPan (4 floats),
# subtitlesText str, subtitlesColor 4 floats
vals = struct.unpack_from("<3f", buf, o)
print("length/blendIn/blendOut", vals)
o += 12
fid, pid = struct.unpack_from("<iq", buf, o)
print("audioClip pptr", fid, pid)
o += 12
vals = struct.unpack_from("<4f", buf, o)
print("clipOffset/volume/pitch/stereoPan", vals)
o += 16
sub, o = rd_str(buf, o)
print("subtitlesText", sub)
vals = struct.unpack_from("<4f", buf, o)
print("subtitlesColor", vals)
o += 16
print("final consumed", o, "of", len(buf))
