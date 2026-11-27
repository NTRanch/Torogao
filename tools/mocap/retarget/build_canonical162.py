"""Rebuild retarget/canonical162.json from the canonical-base GLB.

The rest pose is taken from the skin's inverse bind matrices
(world_j = inverse(IBM_j)), NOT from node world transforms: in
prima-rung6.glb the two disagree by up to ~1.9 cm on the arm and
foot joints (the nodes carry a posed offset; the IBMs encode the
true bind pose, and Blender's importer agrees with the IBMs).
Found by the ground-truth loop, 2026-11-27; see ../README.md.

The rig identity hash is over the hierarchy alone, so correcting
the rest pose does not change the rig id clips are stamped with.
"""
import hashlib
import json
import struct
import sys

import numpy as np
from scipy.spatial.transform import Rotation

GLB = ("/home/phaedrus/.local/share/pax/prima-canonical-base-20261124/"
       "base-template-v2/export/prima-rung6.glb")


def load_glb(path):
    data = open(path, "rb").read()
    clen, _ = struct.unpack_from("<I4s", data, 12)
    gltf = json.loads(data[20:20 + clen])
    off = 20 + clen
    blen, _ = struct.unpack_from("<I4s", data, off)
    return gltf, data[off + 8:off + 8 + blen]


def accessor(gltf, binchunk, idx):
    a = gltf["accessors"][idx]
    bv = gltf["bufferViews"][a["bufferView"]]
    o = bv.get("byteOffset", 0) + a.get("byteOffset", 0)
    cols = {"SCALAR": 1, "VEC2": 2, "VEC3": 3, "VEC4": 4,
            "MAT4": 16}[a["type"]]
    dt = {5126: np.float32, 5123: np.uint16, 5125: np.uint32}[
        a["componentType"]]
    return np.frombuffer(binchunk, dtype=dt, count=a["count"] * cols,
                         offset=o).reshape(a["count"], cols)


def main():
    out = sys.argv[1] if len(sys.argv) > 1 else "canonical162.json"
    gltf, binchunk = load_glb(GLB)
    skin = gltf["skins"][0]
    joints = skin["joints"]
    names = [gltf["nodes"][j].get("name") for j in joints]
    parent_of = {}
    for ni, node in enumerate(gltf["nodes"]):
        for c in node.get("children", []):
            parent_of[c] = ni
    name_of = {ni: gltf["nodes"][ni].get("name")
               for ni in range(len(gltf["nodes"]))}
    jointset = set(joints)
    hierarchy = []
    for j in joints:
        # parent = nearest ancestor that is itself a skin joint
        # (some joints hang under unskinned helper nodes; the
        # retarget FK needs joint parents).
        p = parent_of.get(j)
        while p is not None and p not in jointset:
            p = parent_of.get(p)
        hierarchy.append({"name": name_of[j],
                          "parent": name_of.get(p) if p is not None else None})
    flat = accessor(gltf, binchunk, skin["inverseBindMatrices"])
    ibm = flat.reshape(-1, 4, 4).transpose(0, 2, 1)  # column-major
    rest = {}
    for i, nm in enumerate(names):
        world = np.linalg.inv(ibm[i])
        q = Rotation.from_matrix(world[:3, :3]).as_quat()
        rest[nm] = {"pos": [float(v) for v in world[:3, 3]],
                    "quat_xyzw": [float(v) for v in q]}
    canon = json.dumps(hierarchy, sort_keys=True,
                       separators=(",", ":")).encode()
    doc = {"hierarchy": hierarchy, "rest": rest,
           "hierarchy_sha256": hashlib.sha256(canon).hexdigest(),
           "source_glb": GLB,
           "rest_source": "inverse bind matrices (world = inv(IBM))"}
    with open(out, "w") as fh:
        json.dump(doc, fh, indent=1, sort_keys=True)
    print(f"wrote {out}: {len(names)} joints, "
          f"hierarchy sha {doc['hierarchy_sha256'][:16]}...")


if __name__ == "__main__":
    main()
