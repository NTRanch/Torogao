#!/usr/bin/env python3
"""Generate the Torogao P2 physics-preview fixture.

physics-chain.inx: a puppet whose head anchor carries a
three-stage SimplePhysics chain (the hair-sway-3step shape):
each stage's physics node drives a parameter, and that
parameter rotates the node the next stage hangs from, so
motion cascades down the chain. One input parameter
(HeadSway) moves and tilts the head anchor.

physics-track.json: the scripted input track for the
preview — 240 steps at 60 fps, HeadSway following a
0.75 Hz sine at 0.85 amplitude.

INP packing and payload grammar mirror the P1 fixture
generator (fixtures/make_fixtures.py in the P1 work dir).
"""
import json
import math
import struct

NO_TEXTURE = 4294967295


def pack_inp(payload: dict, textures: list) -> bytes:
    body = json.dumps(payload).encode()
    out = b"TRNSRTS\x00" + struct.pack(">I", len(body)) + body
    out += b"TEX_SECT" + struct.pack(">I", len(textures))
    for type_byte, data in textures:
        out += struct.pack(">I", len(data)) + bytes([type_byte]) + data
    return out


def node(uuid, name, type_, **kw):
    d = {
        "type": type_,
        "uuid": uuid,
        "name": name,
        "enabled": True,
        "zsort": 0,
        "transform": {"trans": [0, 0, 0], "rot": [0, 0, 0], "scale": [1, 1]},
        "lockToRoot": False,
    }
    d.update(kw)
    return d


def quad_mesh(w, h):
    verts = [-w, -h, w, -h, -w, h, w, h]
    return {
        "verts": verts,
        "uvs": [0, 0, 1, 0, 0, 1, 1, 1],
        "indices": [0, 1, 2, 2, 1, 3],
        "origin": [0, 0],
    }


def part(uuid, name, mesh, **kw):
    d = node(uuid, name, "Part", mesh=mesh,
             textures=[0, NO_TEXTURE, NO_TEXTURE],
             blend_mode="Normal", opacity=1.0,
             mask_threshold=0.5, tint=[1, 1, 1], screenTint=[0, 0, 0],
             emissionStrength=1.0)
    d.update(kw)
    return d


def vb(node_uuid, param, values):
    xs = set(); ys = set()
    for key in values:
        x, y = key.strip("[]").split(", ")
        xs.add(int(x)); ys.add(int(y))
    grid = [[None] * (max(ys) + 1) for _ in range(max(xs) + 1)]
    for key, v in values.items():
        x, y = key.strip("[]").split(", ")
        grid[int(x)][int(y)] = v
    is_set = [[True] * len(row) for row in grid]
    return {"node": node_uuid, "param_name": param, "values": grid,
            "isSet": is_set}


def phys(uuid, name, param_uuid, length=35.0, angle_damping=0.15):
    return node(uuid, name, "SimplePhysics",
                param=param_uuid, model_type="Pendulum",
                map_mode="AngleLength", gravity=1.0, length=length,
                frequency=1.0, angle_damping=angle_damping,
                length_damping=0.5, output_scale=[1.0, 1.0],
                local_only=False)


U = {
    "root": 1001, "body": 1010, "head_anchor": 1020, "head": 1021,
    "rot1": 1022, "seg2": 1023, "part2": 1024, "rot2": 1025,
    "seg3": 1026, "part3": 1027, "part1": 1028,
    "phys1": 1030, "phys2": 1031, "phys3": 1032,
}
P_HEAD = 2001
P_SWAY = {1: 2011, 2: 2012, 3: 2013}

SEG = 35.0


def moved(uuid, name, type_, x, y, **kw):
    n = node(uuid, name, type_, **kw)
    n["transform"] = {"trans": [x, y, 0], "rot": [0, 0, 0], "scale": [1, 1]}
    return n


def moved_part(uuid, name, mesh, x, y, **kw):
    n = part(uuid, name, mesh, **kw)
    n["transform"] = {"trans": [x, y, 0], "rot": [0, 0, 0], "scale": [1, 1]}
    return n


nodes = node(U["root"], "Root", "Node", children=[
    part(U["body"], "Body", quad_mesh(60, 80)),
    moved(U["head_anchor"], "HeadAnchor", "Node", 0, 120, children=[
        part(U["head"], "Head", quad_mesh(50, 40)),
        moved_part(U["part1"], "HairSeg1", quad_mesh(35, 17), 0, -17),
        phys(U["phys1"], "HairPhys1", P_SWAY[1], SEG),
        node(U["rot1"], "HairRot1", "Node", children=[
            moved(U["seg2"], "HairSeg2Anchor", "Node", 0, -SEG, children=[
                phys(U["phys2"], "HairPhys2", P_SWAY[2], SEG),
                moved_part(U["part2"], "HairSeg2", quad_mesh(32, 17), 0, -17),
                node(U["rot2"], "HairRot2", "Node", children=[
                    moved(U["seg3"], "HairSeg3Anchor", "Node", 0, -SEG,
                          children=[
                              phys(U["phys3"], "HairPhys3", P_SWAY[3], SEG),
                              moved_part(U["part3"], "HairSeg3",
                                    quad_mesh(28, 17), 0, -17),
                          ]),
                ]),
            ]),
        ]),
    ]),
])


def sway_param(uuid, name, rot_uuid):
    return {"uuid": uuid, "name": name, "is_vec2": False,
            "min": [-1, 0], "max": [1, 0], "defaults": [0, 0],
            "axis_points": [[-1, 0, 1], [0]],
            "bindings": [
                vb(rot_uuid, "transform.r.z", {
                    "[0, 0]": -0.6, "[1, 0]": 0.0, "[2, 0]": 0.6}),
            ]}


params = [
    {"uuid": P_HEAD, "name": "HeadSway", "is_vec2": False,
     "min": [-1, 0], "max": [1, 0], "defaults": [0, 0],
     "axis_points": [[-1, 0, 1], [0]],
     "bindings": [
         vb(U["head_anchor"], "transform.t.x", {
             "[0, 0]": -25.0, "[1, 0]": 0.0, "[2, 0]": 25.0}),
         vb(U["head_anchor"], "transform.r.z", {
             "[0, 0]": -0.35, "[1, 0]": 0.0, "[2, 0]": 0.35}),
     ]},
    sway_param(P_SWAY[1], "HairSway1", U["rot1"]),
    sway_param(P_SWAY[2], "HairSway2", U["rot2"]),
    # HairSway3 drives the tip segment part's rotation directly.
    sway_param(P_SWAY[3], "HairSway3", U["part3"]),
]

payload = {
    "meta": {"name": "Physics Chain Fixture", "version": "INP 0.8",
             "rigger": "torogao-p2"},
    "physics": {"pixelsPerMeter": 100, "gravity": 9.8},
    "nodes": nodes,
    "param": params,
}

fake_tex = b"\x89PNG\r\n\x1a\n" + b"\x00" * 64

with open("physics-chain.inx", "wb") as fh:
    fh.write(pack_inp(payload, [(0, fake_tex)]))

steps = 240
track = {
    "format": "torogao-input-track/1",
    "fps": 60,
    "steps": steps,
    "inputs": {
        "HeadSway": [
            round(0.85 * math.sin(2 * math.pi * 0.75 * (i / 60.0)), 6)
            for i in range(steps)
        ],
    },
}
with open("physics-track.json", "w") as fh:
    json.dump(track, fh, indent=1)
    fh.write("\n")

# A short track for the rich P1 fixture: drives HeadTurn.
rich_track = {
    "format": "torogao-input-track/1",
    "fps": 60,
    "steps": 120,
    "inputs": {
        "HeadTurn": [
            round(0.9 * math.sin(2 * math.pi * 0.5 * (i / 60.0)), 6)
            for i in range(120)
        ],
    },
}
with open("rich-track.json", "w") as fh:
    json.dump(rich_track, fh, indent=1)
    fh.write("\n")

print("physics fixtures written")
