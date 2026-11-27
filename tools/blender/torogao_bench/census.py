"""census — a plain-Python GLB census in the fleet A10.2
instrument's shape (no Blender): joint census, morph census,
and the clip census of the glTF animations a GLB carries —
per animation, its channels and, per sampler, the key count
and the input time span, so a gate can assert the exported
clip's duration and frame count are as authored.
"""

import json
import struct


def _load(path):
    with open(path, "rb") as handle:
        data = handle.read()
    assert data[:4] == b"glTF", "not a GLB"
    jlen = struct.unpack("<I", data[12:16])[0]
    gltf = json.loads(data[20:20 + jlen])
    off = 20 + jlen
    blen = struct.unpack("<I", data[off:off + 4])[0]
    binchunk = data[off + 8:off + 8 + blen]
    return gltf, binchunk


def _accessor_floats(gltf, binchunk, index):
    acc = gltf["accessors"][index]
    view = gltf["bufferViews"][acc["bufferView"]]
    start = view.get("byteOffset", 0) + acc.get("byteOffset", 0)
    count = acc["count"]
    assert acc["componentType"] == 5126 and acc["type"] == "SCALAR"
    return struct.unpack_from("<{}f".format(count), binchunk, start)


def census_glb(path):
    gltf, binchunk = _load(path)
    nodes = gltf["nodes"]
    joints = []
    if gltf.get("skins"):
        joints = [nodes[j]["name"] for j in gltf["skins"][0]["joints"]]
    morphs = []
    for mesh in gltf["meshes"]:
        names = (mesh.get("extras") or {}).get("targetNames")
        if names:
            morphs = list(names)
            break
    animations = []
    for anim in gltf.get("animations", []):
        samplers = []
        for sampler in anim["samplers"]:
            times = _accessor_floats(gltf, binchunk, sampler["input"])
            samplers.append({
                "keys": len(times),
                "t_min": min(times) if times else None,
                "t_max": max(times) if times else None,
                "interpolation": sampler.get("interpolation", "LINEAR"),
            })
        animations.append({
            "name": anim.get("name"),
            "channels": len(anim["channels"]),
            "samplers": samplers,
        })
    return {"path": path, "joints": joints, "morph_targets": morphs,
            "animations": animations}
