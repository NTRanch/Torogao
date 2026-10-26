#!/usr/bin/env python3
"""Generate the Torogao P2 template data files.

Emits into the repository's templates/ directory:
  base-humanoid.json   the v1 puppet template (payload-shaped)
  hair-sway-2step.json, hair-sway-3step.json, hair-bounce.json,
  breast-x.json, breast-y.json, skirt-sway.json
                       the six physics-chain presets, data-
                       faithful to the physics annex in
                       ~/workspace/fathom-anim-re-20261025/batch2/
                       PHYSICS-ANNEX.md (provenance per field).

Usage: make_templates.py <templates-dir>

Every physics setting is stored as
  {"chosen": <number>, "range": [lo, hi] | null,
   "provenance": "DOC" | "EXPORT" | "TUTORIAL" | "chosen",
   "note"?: str}
Ranges and attested values come from the annex; `chosen`
values inside a range, and every value the annex does not
attest, are marked as chosen. The tutorial's on-screen
numeric settings were never recovered (annex section 0);
each preset records that as its `unrecovered` note instead
of inventing numbers.
"""
import json
import os
import sys

NO_TEXTURE = 4294967295
OUT = sys.argv[1]


# ------------------------------------------------------------ helpers
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


def moved(uuid, name, type_, x, y, **kw):
    n = node(uuid, name, type_, **kw)
    n["transform"] = {"trans": [x, y, 0], "rot": [0, 0, 0], "scale": [1, 1]}
    return n


def quad_mesh(w, h):
    return {
        "verts": [-w, -h, w, -h, -w, h, w, h],
        "uvs": [0, 0, 1, 0, 0, 1, 1, 1],
        "indices": [0, 1, 2, 2, 1, 3],
        "origin": [0, 0],
    }


def part(uuid, name, mesh, x=0, y=0, **kw):
    d = moved(uuid, name, "Part", x, y, mesh=mesh,
              textures=[0, NO_TEXTURE, NO_TEXTURE],
              blend_mode="Normal", opacity=1.0, mask_threshold=0.5,
              tint=[1, 1, 1], screenTint=[0, 0, 0], emissionStrength=1.0)
    d.update(kw)
    return d


def vb(node_uuid, param, pairs):
    """Value binding from a list of (axis0_value, bound_value)."""
    grid = [[v] for _, v in pairs]
    is_set = [[True] for _ in pairs]
    return {"node": node_uuid, "param_name": param, "values": grid,
            "isSet": is_set}


def db(node_uuid, keyforms):
    """Deformation binding; keyforms = per-axis0-point vertex offsets."""
    grid = [[kf] for kf in keyforms]
    is_set = [[True] for _ in keyforms]
    return {"node": node_uuid, "param_name": "deform", "values": grid,
            "isSet": is_set}


def param(uuid, name, lo, hi, default, axis, bindings):
    return {"uuid": uuid, "name": name, "is_vec2": False,
            "min": [lo, 0], "max": [hi, 0], "defaults": [default, 0],
            "axis_points": [axis, [0]], "bindings": bindings}


def setting(chosen, rng, provenance, note=None):
    s = {"chosen": chosen, "range": rng, "provenance": provenance}
    if note:
        s["note"] = note
    return s


ZEROS4 = [[0.0, 0.0] for _ in range(4)]

# ------------------------------------------------------------ base puppet
U = {
    "root": 1001, "body_root": 1010, "body": 1011, "breast_anchor": 1012,
    "skirt_anchor": 1013, "head_anchor": 1020, "head": 1021, "eye_l": 1022,
    "eye_r": 1023, "brow_l": 1024, "brow_r": 1025, "mouth": 1026,
    "hair_anchor": 1030,
}

nodes = node(U["root"], "Root", "Node", children=[
    moved(U["body_root"], "BodyRoot", "Node", 0, 0, children=[
        part(U["body"], "Body", quad_mesh(60, 80)),
        moved(U["breast_anchor"], "BreastAnchor", "Node", 0, -55),
        moved(U["skirt_anchor"], "SkirtAnchor", "Node", 0, 55),
        moved(U["head_anchor"], "HeadAnchor", "Node", 0, -135, children=[
            part(U["head"], "Head", quad_mesh(50, 42)),
            part(U["eye_l"], "EyeL", quad_mesh(8, 5), -20, -8),
            part(U["eye_r"], "EyeR", quad_mesh(8, 5), 20, -8),
            part(U["brow_l"], "BrowL", quad_mesh(9, 2), -20, -20),
            part(U["brow_r"], "BrowR", quad_mesh(9, 2), 20, -20),
            part(U["mouth"], "Mouth", quad_mesh(12, 5), 0, 18),
            moved(U["hair_anchor"], "HairAnchor", "Node", 0, -34),
        ]),
    ]),
])

eye_closed = [[0.0, 4.5], [0.0, 4.5], [0.0, -4.5], [0.0, -4.5]]
eye_smile = [[0.0, -1.0], [0.0, -1.0], [0.0, -3.5], [0.0, -3.5]]
mouth_open = [[0.0, 0.0], [0.0, 0.0], [0.0, 7.0], [0.0, 7.0]]
mouth_round = [[7.0, 0.0], [-7.0, 0.0], [7.0, -1.0], [-7.0, -1.0]]
mouth_wide = [[-4.0, 0.0], [4.0, 0.0], [-4.0, 1.0], [4.0, 1.0]]

params = [
    param(2001, "ParamAngleX", -30, 30, 0, [-30, 0, 30], [
        vb(U["head_anchor"], "transform.t.x",
           [(-30, -10.0), (0, 0.0), (30, 10.0)]),
        db(U["head"], [[[ -3.0, 0.0] for _ in range(4)], ZEROS4,
                       [[3.0, 0.0] for _ in range(4)]]),
    ]),
    param(2002, "ParamAngleY", -30, 30, 0, [-30, 0, 30], [
        vb(U["head_anchor"], "transform.t.y",
           [(-30, 8.0), (0, 0.0), (30, -8.0)]),
    ]),
    param(2003, "ParamAngleZ", -30, 30, 0, [-30, 0, 30], [
        vb(U["head_anchor"], "transform.r.z",
           [(-30, -0.30), (0, 0.0), (30, 0.30)]),
    ]),
    param(2004, "ParamBodyAngleX", -30, 30, 0, [-30, 0, 30], [
        vb(U["body_root"], "transform.t.x",
           [(-30, -6.0), (0, 0.0), (30, 6.0)]),
    ]),
    param(2005, "ParamBodyAngleY", -30, 30, 0, [-30, 0, 30], [
        vb(U["body_root"], "transform.t.y",
           [(-30, 5.0), (0, 0.0), (30, -5.0)]),
    ]),
    param(2006, "ParamBodyAngleZ", -30, 30, 0, [-30, 0, 30], [
        vb(U["body_root"], "transform.r.z",
           [(-30, -0.17), (0, 0.0), (30, 0.17)]),
    ]),
    param(2007, "ParamBreath", 0, 1, 0, [0, 1], [
        vb(U["body_root"], "transform.s.y", [(0, 1.0), (1, 1.03)]),
    ]),
    param(2008, "ParamEyeLOpen", 0, 1, 1, [0, 1], [
        db(U["eye_l"], [eye_closed, ZEROS4]),
    ]),
    param(2009, "ParamEyeROpen", 0, 1, 1, [0, 1], [
        db(U["eye_r"], [eye_closed, ZEROS4]),
    ]),
    param(2010, "ParamEyeLSmile", 0, 1, 0, [0, 1], [
        db(U["eye_l"], [ZEROS4, eye_smile]),
    ]),
    param(2011, "ParamEyeRSmile", 0, 1, 0, [0, 1], [
        db(U["eye_r"], [ZEROS4, eye_smile]),
    ]),
    param(2012, "ParamBrowLY", -1, 1, 0, [-1, 0, 1], [
        vb(U["brow_l"], "transform.t.y",
           [(-1, 4.0), (0, 0.0), (1, -4.0)]),
    ]),
    param(2013, "ParamBrowRY", -1, 1, 0, [-1, 0, 1], [
        vb(U["brow_r"], "transform.t.y",
           [(-1, 4.0), (0, 0.0), (1, -4.0)]),
    ]),
    param(2014, "ParamMouthOpenY", 0, 1, 0, [0, 1], [
        db(U["mouth"], [ZEROS4, mouth_open]),
    ]),
    param(2015, "ParamMouthForm", -1, 1, 0, [-1, 0, 1], [
        db(U["mouth"], [mouth_round, ZEROS4, mouth_wide]),
    ]),
    param(2016, "ParamMouthX", 0, 1, 0.5, [0, 0.5, 1], [
        vb(U["mouth"], "transform.s.x",
           [(0, 0.75), (0.5, 1.0), (1, 1.3)]),
    ]),
]

base = {
    "format": "torogao-template/1",
    "kind": "puppet",
    "name": "base-humanoid",
    "title": "Base Humanoid",
    "description": "Minimal humanoid puppet skeleton: body and head "
                   "angle parameters, breath, eye/brow parameters, and "
                   "the mouth parameter set the viseme contract maps "
                   "onto. Carries the named anchors the physics-chain "
                   "presets attach to (HairAnchor, BreastAnchor, "
                   "SkirtAnchor). Geometry is nominal placeholder "
                   "quads; rig real artwork over the same node names.",
    "meta": {"name": "Base Humanoid", "version": "INP 0.8",
             "rigger": "Torogao templates"},
    "physics": {"pixelsPerMeter": 100, "gravity": 9.8},
    "anchors": {"hair": "HairAnchor", "breast": "BreastAnchor",
                "skirt": "SkirtAnchor"},
    "parameters": params,
    "nodes": nodes,
}

# ------------------------------------------------------------ presets
UNRECOVERED = ("The reference tutorial's concrete numeric settings are "
               "shown on screen (07:02-07:15) and were not recovered "
               "from its caption track; no number in this preset is "
               "attributed to the tutorial.")
NORM_DOC = {"minimum": -10.0, "center": 0.0, "maximum": 10.0}


def chain_stage(mob, delay, accel, ease, seg_len):
    return {"segmentLength": seg_len, "mobility": mob, "delay": delay,
            "acceleration": accel, "easeOfSwinging": ease}


def mob_root():
    return setting(1.0, [0.9, 1.0], "EXPORT",
                   "root mobility 1.0 is attested in shipped sample rigs")


def mob_mid(v):
    return setting(v, [0.9, 1.0], "chosen",
                   "interpolated between the attested root (1.0) and "
                   "tip (0.98) values")


def mob_tip():
    return setting(0.98, [0.9, 1.0], "EXPORT",
                   "tip mobility 0.98 is attested in shipped sample rigs")


def delay_v(v, note=None):
    return setting(v, [0.6, 1.0], "EXPORT", note)


def accel_v():
    return setting(1.0, [0.6, 1.4], "EXPORT",
                   "mid-band choice inside the exported range")


def ease_v():
    return setting(0.85, [0.7, 0.99], "DOC",
                   "inside the documented 0.7-0.99 rule-of-thumb band")


def scale_v(v, provenance, note=None):
    return {"value": v, "provenance": provenance, "note": note} \
        if note else {"value": v, "provenance": provenance}


def preset(name, title, desc, prov, attach, inputs, stages, outputs):
    return {
        "format": "torogao-template/1",
        "kind": "physics-chain",
        "name": name,
        "title": title,
        "description": desc,
        "provenance": prov,
        "attachTo": attach,
        "inputs": inputs,
        "stages": stages,
        "outputs": outputs,
        "unrecovered": UNRECOVERED,
    }


def inp(parameter, influence, prov, note=None):
    d = {"parameter": parameter, "type": "Angle",
         "influencePercent": influence, "flip": False,
         "normalization": NORM_DOC, "provenance": prov}
    if note:
        d["note"] = note
    return d


def out(parameter, stage, scale):
    return {"parameter": parameter, "stage": stage, "scale": scale,
            "influencePercent": 100, "flip": False}


hair_inputs = [
    inp("ParamAngleX", 60, "DOC",
        "head-input preset shape; the 60/40 split is chosen"),
    inp("ParamAngleZ", 40, "DOC"),
]

hair_sway_2 = preset(
    "hair-sway-2step", "Hair sway — 2-step pendulum",
    "Two-stage sway chain for short hair, after the editor's shipped "
    "'Hair 2-step pendulum' physical-model preset.",
    {"class": "SETUP",
     "sources": ["Official 'How to Set Up Physics' (shipped preset "
                 "names + multi-stage procedure)",
                 "Physics annex sections 1 and 3 (exported ranges)"]},
    "HairAnchor", hair_inputs,
    [chain_stage(mob_root(), delay_v(0.6), accel_v(), ease_v(), 35.0),
     chain_stage(mob_tip(), delay_v(0.8, "rising toward the tip"),
                 accel_v(), ease_v(), 35.0)],
    [out("ParamHairSway1", 1, scale_v(5, "EXPORT",
                                      "graduated scale start, attested")),
     out("ParamHairSway2", 2, scale_v(90, "EXPORT",
                                      "graduated scale end, attested"))],
)

hair_sway_3 = preset(
    "hair-sway-3step", "Hair sway — 3-step pendulum",
    "Three-stage sway chain for longer hair, after the editor's "
    "shipped 'Hair 3-step pendulum' physical-model preset; bend "
    "points get denser toward the tip.",
    {"class": "SETUP",
     "sources": ["Official 'How to Set Up Physics' (shipped preset "
                 "names + multi-stage procedure)",
                 "Physics annex sections 1 and 3 (exported ranges)"]},
    "HairAnchor", hair_inputs,
    [chain_stage(mob_root(), delay_v(0.6), accel_v(), ease_v(), 35.0),
     chain_stage(mob_mid(0.99), delay_v(0.8, "rising toward the tip"),
                 accel_v(), ease_v(), 35.0),
     chain_stage(mob_tip(), delay_v(1.0, "tip of the attested band"),
                 accel_v(), ease_v(), 35.0)],
    [out("ParamHairSway1", 1, scale_v(5, "EXPORT",
                                      "graduated scale start, attested")),
     out("ParamHairSway2", 2, scale_v(42, "chosen",
                                      "graduated midpoint between the "
                                      "attested 5 and 90")),
     out("ParamHairSway3", 3, scale_v(90, "EXPORT",
                                      "graduated scale end, attested"))],
)

bounce_unrecovered_note = ("no bounce-specific exported range was "
                           "captured; value chosen inside the "
                           "exported sway-class band")
hair_bounce = preset(
    "hair-bounce", "Hair bounce — up/down + fan",
    "Bounce component of the hair rig: two stages (the long-hair "
    "form), one output for up/down and one for fan-out/squeeze-in, "
    "driven by Body Y.",
    {"class": "TUTORIAL",
     "sources": ["Physics annex section 2 (hair = sway + bounce, "
                 "rigged separately; two parameters per deformer)",
                 "Physics annex sections 1 and 3 (bands)"]},
    "HairAnchor",
    [inp("ParamBodyAngleY", 100, "TUTORIAL",
         "Body Y drives the bounce component")],
    [chain_stage(setting(1.0, None, "chosen", bounce_unrecovered_note),
                 setting(0.6, None, "chosen", bounce_unrecovered_note),
                 setting(1.0, None, "chosen", bounce_unrecovered_note),
                 ease_v(), 30.0),
     chain_stage(setting(0.98, None, "chosen", bounce_unrecovered_note),
                 setting(0.8, None, "chosen", bounce_unrecovered_note),
                 setting(1.0, None, "chosen", bounce_unrecovered_note),
                 ease_v(), 30.0)],
    [out("ParamHairBounceY", 1, scale_v(45, "chosen",
                                        "no bounce output scale is "
                                        "attested in the annex")),
     out("ParamHairFan", 2, scale_v(45, "chosen",
                                    "no bounce output scale is "
                                    "attested in the annex"))],
)

breast_note = "shipped-rig breast chains sit in the ~0.9 mobility class"
breast_x = preset(
    "breast-x", "Breast — X component",
    "Lateral breast component: one chain, outputs staggered across "
    "stages for the left and right parameters, driven by Body X + "
    "Body Z.",
    {"class": "TUTORIAL",
     "sources": ["Physics annex section 2 (Breast X group inputs "
                 "Body X + Body Z; distinct pendulum stage per "
                 "output)",
                 "Physics annex section 3 (exported bands)"]},
    "BreastAnchor",
    [inp("ParamBodyAngleX", 60, "TUTORIAL",
         "the 60/40 split between the two attested inputs is chosen"),
     inp("ParamBodyAngleZ", 40, "TUTORIAL")],
    [chain_stage(setting(0.9, [0.9, 1.0], "EXPORT", breast_note),
                 delay_v(0.6), accel_v(), ease_v(), 25.0),
     chain_stage(setting(0.9, [0.9, 1.0], "EXPORT", breast_note),
                 delay_v(0.8), accel_v(), ease_v(), 25.0)],
    [out("ParamBreastL", 1, scale_v(45, "chosen",
                                    "no breast output scale is "
                                    "attested in the annex")),
     out("ParamBreastR", 2, scale_v(45, "chosen",
                                    "no breast output scale is "
                                    "attested in the annex"))],
)

breast_y = preset(
    "breast-y", "Breast — Y component",
    "Vertical breast component: up/down and together/apart outputs "
    "on staggered stages, driven by Body Y alone.",
    {"class": "TUTORIAL",
     "sources": ["Physics annex section 2 (Breast Y group input "
                 "Body Y alone; two parameters per deformer)",
                 "Physics annex section 3 (exported bands)"]},
    "BreastAnchor",
    [inp("ParamBodyAngleY", 100, "TUTORIAL")],
    [chain_stage(setting(0.9, [0.9, 1.0], "EXPORT", breast_note),
                 delay_v(0.6), accel_v(), ease_v(), 25.0),
     chain_stage(setting(0.9, [0.9, 1.0], "EXPORT", breast_note),
                 delay_v(0.8), accel_v(), ease_v(), 25.0)],
    [out("ParamBreastY", 1, scale_v(45, "chosen",
                                    "no breast output scale is "
                                    "attested in the annex")),
     out("ParamBreastApart", 2, scale_v(45, "chosen",
                                        "no breast output scale is "
                                        "attested in the annex"))],
)

skirt_sway = preset(
    "skirt-sway", "Skirt sway — 3-stage chain",
    "Sway chain for a skirt or hem, hung from the hips anchor. The "
    "physics group itself is the documented canonical example; the "
    "chain values follow the exported sway class.",
    {"class": "DOC",
     "sources": ["Official physics documentation ('swinging skirt' "
                 "is the canonical physics-group example)",
                 "Physics annex section 3 (sway-class exported "
                 "ranges)"],
     "notes": "No skirt-specific exported ranges were captured; "
              "mobility/delay/acceleration follow the sway class "
              "and are marked chosen where interpolated."},
    "SkirtAnchor",
    [inp("ParamBodyAngleX", 60, "DOC",
         "torso-input preset shape; the 60/40 split is chosen"),
     inp("ParamBodyAngleZ", 40, "DOC")],
    [chain_stage(mob_root(), delay_v(0.6), accel_v(), ease_v(), 30.0),
     chain_stage(mob_mid(0.99), delay_v(0.8), accel_v(), ease_v(), 30.0),
     chain_stage(mob_tip(), delay_v(1.0), accel_v(), ease_v(), 30.0)],
    [out("ParamSkirtSway1", 1, scale_v(5, "EXPORT",
                                       "sway-class graduated start")),
     out("ParamSkirtSway2", 2, scale_v(42, "chosen",
                                       "graduated midpoint")),
     out("ParamSkirtSway3", 3, scale_v(90, "EXPORT",
                                       "sway-class graduated end"))],
)

FILES = {
    "base-humanoid.json": base,
    "hair-sway-2step.json": hair_sway_2,
    "hair-sway-3step.json": hair_sway_3,
    "hair-bounce.json": hair_bounce,
    "breast-x.json": breast_x,
    "breast-y.json": breast_y,
    "skirt-sway.json": skirt_sway,
}

os.makedirs(OUT, exist_ok=True)
for fname, doc in FILES.items():
    with open(os.path.join(OUT, fname), "w") as fh:
        json.dump(doc, fh, indent=1)
        fh.write("\n")
    print("wrote", fname)
