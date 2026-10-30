#!/usr/bin/env python3
"""D-4 arm E1: extend the repertoire rigs with the gaze pair
and the blush parameter.

Reads the landed Phase F repertoire fixtures (arm E0 inputs,
never modified) and writes extended copies:

- `ParamGaze` (uuid 2201, vec2) deforming IrisL/IrisR,
  encoded in the estate's [0, 1] parameter idiom with the
  neutral at 0.5: the section C gaze g in [-1, 1] is the
  affine map g = 2v - 1 of the parameter value v. (The
  engine samples binding grids through a range-mapped
  offset lookup; a [-1, 1]-ranged parameter's grid is
  sampled compressed and off-centre — the landed
  ParamMouthForm shows it: value 0 evaluates to a quarter
  of its +1 keyform, not its centre keyform. The [0, 1]
  encoding is exact in both the INP engine and the lumen
  keyform layer; the [-1, 1] encoding would displace the
  irises at rest in the native runtime.) Travel per axis equals the rig's own
  eye-dialect excursion (EyeCross max |x| for GazeX, EyeRoll
  max |y| for GazeY), so the gaze range is the range the
  rig's eyes already move through. Neutral (0, 0) writes
  zero offsets: the rest state is byte-identical in effect.
- `ParamCheek` (uuid 2203, scalar [0, 1], default 0)
  driving the opacity of two new BlushL/BlushR parts
  (uuids 3301/3302) placed under HeadAnchor from the rig's
  own iris position (7 units outboard, 16 below, quad
  22x12). Base opacity 1.0 with binding values 0 / 0.5 / 1:
  cheek 0 evaluates to opacity 0, so the blush is invisible
  at rest — exactly the current visual state, which has no
  blush mesh at all.
- One added texture slot: a soft pink radial-gradient disc
  (48x32 RGBA), the blush art. Generated deterministically
  from pixel coordinates.

Usage: extend_rigs.py <source_dir> <out_dir>
Prints a JSON summary (per-rig input/output SHA-256 and the
extracted travels) to stdout.
"""

import hashlib
import json
import struct
import sys
from io import BytesIO
from pathlib import Path

MAGIC = b"TRNSRTS\x00"
TEX_MAGIC = b"TEX_SECT"
EXT_MAGIC = b"EXT_SECT"

GAZE_UUID = 2201
CHEEK_UUID = 2203
BLUSH_L_UUID = 3301
BLUSH_R_UUID = 3302
IRIS_L_UUID = 3001
IRIS_R_UUID = 3002
HEAD_ANCHOR_UUID = 1020


def parse_container(raw: bytes):
    assert raw[:8] == MAGIC, "bad container magic"
    (plen,) = struct.unpack(">I", raw[8:12])
    payload = json.loads(raw[12 : 12 + plen])
    tail = raw[12 + plen :]
    assert tail[:8] == TEX_MAGIC, "bad texture magic"
    off = 8
    (count,) = struct.unpack(">I", tail[off : off + 4])
    off += 4
    textures = []
    for _ in range(count):
        (length,) = struct.unpack(">I", tail[off : off + 4])
        tag = tail[off + 4]
        data = tail[off + 5 : off + 5 + length]
        assert len(data) == length
        textures.append((tag, data))
        off += 5 + length
    ext_sections = []
    if tail[off : off + 8] == EXT_MAGIC:
        off += 8
        (ecount,) = struct.unpack(">I", tail[off : off + 4])
        off += 4
        for _ in range(ecount):
            (nlen,) = struct.unpack(">I", tail[off : off + 4])
            off += 4
            name = tail[off : off + nlen].decode()
            off += nlen
            (dlen,) = struct.unpack(">I", tail[off : off + 4])
            off += 4
            data = tail[off : off + dlen]
            off += dlen
            ext_sections.append((name, data))
    return payload, textures, ext_sections


def build_container(payload, textures, ext_sections) -> bytes:
    body = json.dumps(payload, separators=(",", ":")).encode()
    out = bytearray()
    out += MAGIC
    out += struct.pack(">I", len(body))
    out += body
    out += TEX_MAGIC
    out += struct.pack(">I", len(textures))
    for tag, data in textures:
        out += struct.pack(">I", len(data))
        out.append(tag)
        out += data
    if ext_sections:
        out += EXT_MAGIC
        out += struct.pack(">I", len(ext_sections))
        for name, data in ext_sections:
            encoded = name.encode()
            out += struct.pack(">I", len(encoded))
            out += encoded
            out += struct.pack(">I", len(data))
            out += data
    return bytes(out)


def blush_png() -> bytes:
    from PIL import Image

    width, height = 48, 32
    image = Image.new("RGBA", (width, height))
    pixels = image.load()
    for y in range(height):
        for x in range(width):
            dx = (x - (width - 1) / 2) / (width / 2)
            dy = (y - (height - 1) / 2) / (height / 2)
            r2 = dx * dx + dy * dy
            alpha = 0 if r2 >= 1 else int(round((1 - r2) ** 1.5 * 255))
            pixels[x, y] = (255, 105, 140, alpha)
    buffer = BytesIO()
    image.save(buffer, format="PNG")
    return buffer.getvalue()


def find_node(node, uuid):
    if node.get("uuid") == uuid:
        return node
    for child in node.get("children") or []:
        found = find_node(child, uuid)
        if found is not None:
            return found
    return None


def dialect_travels(payload):
    travels = {}
    for param in payload["param"]:
        if param["name"] in ("ParamEyeRoll", "ParamEyeCross"):
            for binding in param["bindings"]:
                if binding["node"] in (IRIS_L_UUID, IRIS_R_UUID):
                    cells = [c for row in binding["values"] for c in row]
                    mx = max(abs(v[0]) for cell in cells for v in cell)
                    my = max(abs(v[1]) for cell in cells for v in cell)
                    travels[param["name"]] = (mx, my)
    return travels["ParamEyeCross"][0], travels["ParamEyeRoll"][1]


def extend(payload, travel_x: float, travel_y: float, blush_slot: int):
    param_uuids = {p["uuid"] for p in payload["param"]}
    assert GAZE_UUID not in param_uuids and CHEEK_UUID not in param_uuids
    assert find_node(payload["nodes"], BLUSH_L_UUID) is None
    assert find_node(payload["nodes"], BLUSH_R_UUID) is None
    iris_l = find_node(payload["nodes"], IRIS_L_UUID)
    iris_r = find_node(payload["nodes"], IRIS_R_UUID)
    anchor = find_node(payload["nodes"], HEAD_ANCHOR_UUID)
    assert iris_l is not None and iris_r is not None and anchor is not None

    def gaze_binding(node_uuid):
        # Grid points are the mapped positions 0 / 0.5 / 1,
        # i.e. section C gaze -1 / 0 / +1 per axis.
        values, isset = [], []
        for mx in (0.0, 0.5, 1.0):
            row, set_row = [], []
            for my in (0.0, 0.5, 1.0):
                offset = [(2 * mx - 1) * travel_x, -(2 * my - 1) * travel_y]
                row.append([offset, offset, offset, offset])
                set_row.append(True)
            values.append(row)
            isset.append(set_row)
        return {
            "node": node_uuid,
            "param_name": "deform",
            "values": values,
            "isSet": isset,
        }

    payload["param"].append(
        {
            "uuid": GAZE_UUID,
            "name": "ParamGaze",
            "is_vec2": True,
            "min": [0, 0],
            "max": [1, 1],
            "defaults": [0.5, 0.5],
            "axis_points": [[0, 0.5, 1], [0, 0.5, 1]],
            "bindings": [gaze_binding(IRIS_L_UUID), gaze_binding(IRIS_R_UUID)],
        }
    )
    payload["param"].append(
        {
            "uuid": CHEEK_UUID,
            "name": "ParamCheek",
            "is_vec2": False,
            "min": [0, 0],
            "max": [1, 0],
            "defaults": [0, 0],
            "axis_points": [[0, 0.5, 1], [0]],
            "bindings": [
                {
                    "node": node_uuid,
                    "param_name": "opacity",
                    "values": [[0.0], [0.5], [1.0]],
                    "isSet": [[True], [True], [True]],
                }
                for node_uuid in (BLUSH_L_UUID, BLUSH_R_UUID)
            ],
        }
    )

    def blush_part(uuid, name, iris):
        trans = iris["transform"]["trans"]
        center_x = (abs(trans[0]) + 7) * (1 if trans[0] >= 0 else -1)
        center_y = trans[1] + 16
        return {
            "blend_mode": "Normal",
            "emissionStrength": 1.0,
            "enabled": True,
            "lockToRoot": False,
            "mask_threshold": 0.5,
            "mesh": {
                "indices": [0, 1, 2, 2, 1, 3],
                "origin": [0, 0],
                "uvs": [0, 0, 1, 0, 0, 1, 1, 1],
                "verts": [-11, -6, 11, -6, -11, 6, 11, 6],
            },
            "name": name,
            "opacity": 1.0,
            "screenTint": [0, 0, 0],
            "textures": [blush_slot, 4294967295, 4294967295],
            "tint": [1, 1, 1],
            "transform": {
                "rot": [0, 0, 0],
                "scale": [1, 1],
                "trans": [center_x, center_y, 0],
            },
            "type": "Part",
            "uuid": uuid,
            "zsort": 0.05,
        }

    anchor["children"].append(blush_part(BLUSH_L_UUID, "BlushL", iris_l))
    anchor["children"].append(blush_part(BLUSH_R_UUID, "BlushR", iris_r))


def main() -> int:
    source_dir, out_dir = Path(sys.argv[1]), Path(sys.argv[2])
    out_dir.mkdir(parents=True, exist_ok=True)
    png = blush_png()
    summary = {"blush_png_sha256": hashlib.sha256(png).hexdigest(), "rigs": {}}
    for rig in ("prima", "yui", "alma"):
        raw = (source_dir / f"{rig}-repertoire.inx").read_bytes()
        payload, textures, ext_sections = parse_container(raw)
        travel_x, travel_y = dialect_travels(payload)
        extend(payload, travel_x, travel_y, blush_slot=len(textures))
        textures.append((0, png))
        built = build_container(payload, textures, ext_sections)
        out_path = out_dir / f"{rig}-repertoire-extended.inx"
        out_path.write_bytes(built)
        summary["rigs"][rig] = {
            "input_sha256": hashlib.sha256(raw).hexdigest(),
            "output_sha256": hashlib.sha256(built).hexdigest(),
            "travel_x": travel_x,
            "travel_y": travel_y,
            "params_before": len(payload["param"]) - 2,
            "params_after": len(payload["param"]),
        }
    print(json.dumps(summary, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
