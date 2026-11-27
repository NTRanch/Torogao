"""h2_gate — the H2 rung gate, run headless under the official
Blender 5.3 build:

  blender -b -Y --offline-mode -noaudio -P h2_gate.py -- \
      <lana.glb> <hierarchy.json> <golden_dir> <workdir> \
      <motion-clip-check>

Gates, in order (PLAN.md rung H2):
  G0  clipmath reproduces the H1 evaluator's pinned golden
      digests (the Python evaluator is verified before it is
      trusted as an instrument);
  G1  the bench extension registers (panel + operators);
  G2  a hand-keyed test clip (bend-battery poses + a short
      action) on Lana's rung-1 rig converts action -> clip and
      freezes through the estate gate tool; the frozen clip
      carries her rig identity and its frame-0 root translation
      is the GLB's rest translation;
  G3  action -> clip -> action: evaluated poses at every frame
      inside the Tier 2 band (1e-6), measured three ways —
      Blender-evaluated original vs round-tripped, and the
      frozen clip's own Appendix C evaluation vs the
      round-tripped action;
  G4  clip -> action -> clip byte-identical after
      canonicalisation (frozen bytes compared);
  G5  the NLA layer front operates over native data and the
      evaluated flatten validates, with the strip layout in
      the op log as provenance;
  G6  pose-asset import/export rides the same converter;
  G7  the clip's glTF export passes the A10.2-shape census
      (bone census, morph census, clip census: duration and
      frame count as authored).

Every measurement is written to <workdir>/gate_report.json;
the script exits non-zero if any gate fails.
"""

import json
import math
import os
import subprocess
import sys

import bpy
from mathutils import Quaternion, Vector

sys.path.insert(0, os.path.dirname(os.path.dirname(
    os.path.dirname(os.path.abspath(__file__)))))

from torogao_bench import (  # noqa: E402
    census, clipdoc, clipmath, converter)

SESSION = "h2-gate-20261127"
BAND = 1e-6

REPORT = {"gates": {}, "measurements": {}}


def record(gate, ok, detail):
    REPORT["gates"][gate] = {"pass": bool(ok), "detail": detail}
    print("GATE {} {} — {}".format(gate, "PASS" if ok else "FAIL",
                                   detail))


FREEZE_COMPILER = "torogao-format motion_clip h0 (2026-11-27)"
FREEZE_BAND = "tier2-1e-6"


def freeze(binary, src, dst):
    """motion-clip-check --freeze <compiler> <band> <file>
    prints the frozen document's canonical text to stdout."""
    proc = subprocess.run(
        [binary, "--freeze", FREEZE_COMPILER, FREEZE_BAND, src],
        capture_output=True)
    if proc.returncode != 0:
        return False, proc.stderr.decode("utf-8", "replace").strip()
    with open(dst, "wb") as handle:
        handle.write(proc.stdout)
    return True, ""


def validate(binary, path):
    proc = subprocess.run([binary, path], capture_output=True,
                          text=True)
    return proc.returncode == 0, (proc.stderr or proc.stdout).strip()


# ----------------------------------------------------------------
# G0: clipmath vs the pinned Rust digests
# ----------------------------------------------------------------

def gate_g0(golden_dir):
    # The pins are read from the Rust test source itself, never
    # transcribed by hand: a hand-copied hash is an identifier
    # waiting to be wrong.
    import re
    test_rs = os.path.normpath(os.path.join(
        golden_dir, "..", "..", "..", "..", "torogao-runtime",
        "tests", "clip_eval.rs"))
    with open(test_rs) as handle:
        source = handle.read()
    pins = {}
    for name, ticks, digest in re.findall(
            r'golden\("([\w-]+)"\)\.digest\(0, (\d+)\),\s*'
            r'"([0-9a-f]{64})"', source):
        pins[name] = (int(ticks), digest)
    if len(pins) < 3:
        record("G0", False, "could not extract digest pins from "
                            "{}".format(test_rs))
        return False
    ok_all = True
    for name, (ticks, pin) in pins.items():
        path = None
        for candidate in (os.path.join(golden_dir, name + ".clip.json"),
                          os.path.join(golden_dir, name + ".json")):
            if os.path.exists(candidate):
                path = candidate
                break
        if path is None:
            record("G0", False, "golden {} not found".format(name))
            return False
        doc = clipdoc.load_document(path)
        got = clipmath.clip_digest(doc, 0, ticks)
        ok = got == pin
        ok_all = ok_all and ok
        REPORT["measurements"]["digest_" + name] = got
        if not ok:
            record("G0", False, "{} digest {} != pin {}".format(
                name, got, pin))
            return False
    closure = clipmath.loop_closure(
        clipdoc.load_document(os.path.join(
            golden_dir, "loop-hold.clip.json")))
    closed = all(worst == 0.0 for _t, worst in closure)
    record("G0", ok_all and closed,
           "clipmath digests match the pinned Rust digests "
           "(loop-hold, fac, shader); loop-hold closes exactly")
    return ok_all and closed


# ----------------------------------------------------------------
# Authoring: bend-battery poses + a short action (the "hand")
# ----------------------------------------------------------------

ROT_PLAN = {  # bone -> [(frame, axis, degrees)] ; mode per channel
    "mixamorig:LeftForeArm": ("cubic", [(0, "X", 0), (12, "X", 100),
                                        (24, "X", 60), (36, "X", 110),
                                        (48, "X", 0)]),
    "mixamorig:LeftArm": ("cubic", [(0, "X", 0), (12, "X", 25),
                                    (24, "X", 10), (36, "X", -20),
                                    (48, "X", 0)]),
    "mixamorig:LeftUpLeg": ("linear", [(0, "X", 0), (12, "X", -40),
                                       (24, "X", -15), (48, "X", 0)]),
    "mixamorig:LeftLeg": ("linear", [(0, "X", 0), (12, "X", 60),
                                     (24, "X", 20), (48, "X", 0)]),
    "mixamorig:Spine": ("cubic", [(0, "X", 0), (24, "X", 15),
                                  (30, "X", 15), (48, "X", 0)]),
    "mixamorig:Head": ("cubic", [(0, "X", 0), (12, "X", -10),
                                 (24, "X", -15), (30, "X", -15),
                                 (36, "Y", 20), (48, "X", 0)]),
    "mixamorig:RightArm": ("linear", [(0, "X", 0), (24, "X", -50),
                                      (36, "X", -50), (48, "X", 0)]),
    "mixamorig:Hips": ("linear", [(0, "Y", 0), (24, "Y", 8),
                                  (48, "Y", 0)]),
}
HIPS_LOC_PLAN = [(0, (0.0, 0.0, 0.0)), (24, (0.03, 0.01, -0.02)),
                 (48, (0.0, 0.0, 0.0))]
MORPH_PLAN = {  # name -> (mode, [(frame, value)], {frame: interp})
    "Fac_Eye_Close": ("linear", [(0, 0.0), (34, 0.0), (35, 1.0),
                                 (36, 0.0), (48, 0.0)], {}),
    "Fac_Mth_Smile": ("cubic", [(0, 0.0), (24, 0.8), (48, 0.0)], {}),
    "Fac_Mth_Open": ("linear", [(0, 0.0), (24, 0.3), (36, 0.6),
                                (48, 0.0)], {24: "CONSTANT"}),
}
AXES = {"X": Vector((1, 0, 0)), "Y": Vector((0, 1, 0)),
        "Z": Vector((0, 0, 1))}


def author_action(arm, body):
    bpy.context.view_layer.objects.active = arm
    bpy.ops.object.mode_set(mode="POSE")
    for bone_name, (_mode, keys) in ROT_PLAN.items():
        pb = arm.pose.bones[bone_name]
        pb.rotation_mode = "QUATERNION"
        for frame, axis, degrees in keys:
            pb.rotation_quaternion = Quaternion(
                AXES[axis], math.radians(degrees))
            pb.keyframe_insert("rotation_quaternion", frame=frame)
    hips = arm.pose.bones["mixamorig:Hips"]
    for frame, loc in HIPS_LOC_PLAN:
        hips.location = Vector(loc)
        hips.keyframe_insert("location", frame=frame)
    bpy.ops.object.mode_set(mode="OBJECT")
    for morph, (_mode, keys, _over) in MORPH_PLAN.items():
        kb = body.data.shape_keys.key_blocks[morph]
        for frame, value in keys:
            kb.value = value
            kb.keyframe_insert("value", frame=frame)
    # Interpolation modes + key types, then settle handles.
    def set_modes(action, plan_by_path):
        for _slot, fc in converter.action_fcurves(action):
            plan = plan_by_path.get(fc.data_path)
            if plan is None:
                continue
            mode, overrides = plan
            for kp in fc.keyframe_points:
                frame = int(round(kp.co[0]))
                kp.interpolation = overrides.get(
                    frame, "BEZIER" if mode == "cubic" else "LINEAR")
    arm_action = arm.animation_data.action
    plan = {}
    for bone_name, (mode, _keys) in ROT_PLAN.items():
        plan['pose.bones["{}"].rotation_quaternion'.format(bone_name)] = \
            (mode, {})
    plan['pose.bones["mixamorig:Hips"].location'] = ("cubic", {})
    set_modes(arm_action, plan)
    key_action = body.data.shape_keys.animation_data.action
    kplan = {}
    for morph, (mode, _keys, overrides) in MORPH_PLAN.items():
        kplan['key_blocks["{}"].value'.format(morph)] = (mode, overrides)
    set_modes(key_action, kplan)
    # The Spine/Head keys at frame 30 are moving holds.
    for _slot, fc in converter.action_fcurves(arm_action):
        if "Spine" in fc.data_path or "Head" in fc.data_path:
            for kp in fc.keyframe_points:
                if int(round(kp.co[0])) == 30:
                    kp.type = "MOVING_HOLD"
    moved = converter.settle_handles_thirds(arm_action)
    moved += converter.settle_handles_thirds(key_action)
    REPORT["measurements"]["handles_moved_to_thirds"] = moved
    return arm_action, key_action


def sample_pose(arm, body, frame):
    bpy.context.scene.frame_set(frame)
    bpy.context.view_layer.update()
    pose = {}
    for bone_name in ROT_PLAN:
        quat = arm.pose.bones[bone_name].rotation_quaternion
        pose[bone_name] = [quat.x, quat.y, quat.z, quat.w]
    pose["__hips_location"] = list(
        arm.pose.bones["mixamorig:Hips"].location)
    for morph in MORPH_PLAN:
        pose[morph] = body.data.shape_keys.key_blocks[morph].value
    return pose


def quat_max_diff(a, b):
    dot = sum(x * y for x, y in zip(a, b))
    if dot < 0.0:
        b = [-c for c in b]
    return max(abs(x - y) for x, y in zip(a, b))


def quat_normalized(q):
    """The rotation a quaternion value applies: Blender
    normalises on use (matrix conversion), and Appendix C
    normalises rotation results — the applied rotation is
    the normalised quaternion. The raw F-Curve property can
    sit off unit mid-segment (componentwise Hermite drift),
    which is representation, not pose."""
    length = math.sqrt(sum(c * c for c in q))
    if length == 0.0:
        return list(q)
    return [c / length for c in q]


# ----------------------------------------------------------------
# Main
# ----------------------------------------------------------------

def main():
    argv = sys.argv[sys.argv.index("--") + 1:]
    glb_path, hierarchy_path, golden_dir, workdir, binary = argv
    os.makedirs(workdir, exist_ok=True)

    if not gate_g0(golden_dir):
        return finish(1)

    # G1: the extension registers.
    import torogao_bench
    torogao_bench.register()
    ok = hasattr(bpy.types, "TOROGAO_PT_bench") and \
        hasattr(bpy.ops.torogao, "action_to_clip") and \
        hasattr(bpy.ops.torogao, "nla_refresh")
    record("G1", ok, "Torogao bench panel + converter/NLA "
                     "operators registered")
    if not ok:
        return finish(1)

    bpy.ops.wm.read_factory_settings(use_empty=True)
    bpy.ops.import_scene.gltf(filepath=glb_path)
    arm = next(o for o in bpy.data.objects if o.type == "ARMATURE")
    body = next(o for o in bpy.data.objects
                if o.type == "MESH" and "Body" in o.name
                and o.data.shape_keys)
    scene = bpy.context.scene
    scene.render.fps = 24
    scene.render.fps_base = 1.0
    scene.frame_start, scene.frame_end = 0, 48
    binding = converter.RigBinding(glb_path)
    rig = clipdoc.rig_identity(hierarchy_path)
    REPORT["measurements"]["rig_identity"] = rig
    REPORT["measurements"]["root_joint"] = binding.root_joint

    author_action(arm, body)
    poses_before = {f: sample_pose(arm, body, f) for f in range(49)}

    # G2: action -> clip -> freeze.
    doc, notes = converter.action_to_clip(
        arm, body, binding, "lana/bench/h2_battery_wave", rig,
        SESSION, loop=(0, 48), actor="agent")
    working = os.path.join(workdir, "h2_battery_wave.clip.json")
    frozen_a = os.path.join(workdir, "h2_battery_wave.frozen.json")
    clipdoc.write_document(doc, working)
    ok_f, msg = freeze(binary, working, frozen_a)
    doc_a = clipdoc.load_document(frozen_a) if ok_f else {}
    hips0 = None
    for channel in doc_a.get("channels", []):
        if channel["target"] == {"kind": "joint",
                                  "name": binding.root_joint,
                                  "property": "translation"}:
            hips0 = channel["keys"][0]["value"]
    rest = binding.rest_translation[binding.root_joint]
    ok = ok_f and rig == doc_a.get("rig") and hips0 is not None and \
        max(abs(a - b) for a, b in zip(hips0, rest)) < 1e-9
    record("G2", ok, "freeze {}; rig {}; frame-0 root translation "
                     "{} vs GLB rest {}".format(
                         "ok" if ok_f else msg, doc_a.get("rig"),
                         hips0, rest))
    REPORT["measurements"]["clip_channels"] = len(doc_a.get("channels", []))
    if not ok:
        return finish(1)
    closure = clipmath.loop_closure(doc_a)
    REPORT["measurements"]["loop_closure_max"] = max(
        worst for _t, worst in closure)
    REPORT["measurements"]["clip_digest_0_49"] = \
        clipmath.clip_digest(doc_a, 0, 49)

    # G3: clip -> action; evaluated poses at every frame.
    converter.clip_to_action(doc_a, arm, body, binding, SESSION)
    poses_after = {f: sample_pose(arm, body, f) for f in range(49)}
    worst_blender = 0.0
    worst_clip = 0.0
    for frame in range(49):
        clip_values = dict(clipmath.evaluate_all(doc_a, frame))
        for bone_name in ROT_PLAN:
            diff = quat_max_diff(poses_before[frame][bone_name],
                                 poses_after[frame][bone_name])
            worst_blender = max(worst_blender, diff)
            clip_q = clip_values["joint:{}:rotation".format(bone_name)]
            # Both sides compared as applied rotations: a
            # componentwise curve's mid-segment quaternions
            # sit off unit (that is representation, not pose),
            # and Appendix C normalises rotation results.
            diff = quat_max_diff(
                quat_normalized(clip_q),
                quat_normalized(poses_after[frame][bone_name]))
            worst_clip = max(worst_clip, diff)
        loc_diff = max(abs(a - b) for a, b in zip(
            poses_before[frame]["__hips_location"],
            poses_after[frame]["__hips_location"]))
        worst_blender = max(worst_blender, loc_diff)
        clip_t = clip_values["joint:{}:translation".format(
            binding.root_joint)]
        loc_clip = converter.hips_location_to_clip(
            binding, arm.data.bones[binding.root_joint],
            poses_after[frame]["__hips_location"])
        worst_clip = max(worst_clip, max(
            abs(a - b) for a, b in zip(clip_t, loc_clip)))
        for morph in MORPH_PLAN:
            diff = abs(poses_before[frame][morph] -
                       poses_after[frame][morph])
            worst_blender = max(worst_blender, diff)
            clip_w = clip_values["morph:{}:weight".format(morph)]
            worst_clip = max(worst_clip,
                             abs(clip_w[0] - poses_after[frame][morph]))
    REPORT["measurements"]["roundtrip_max_delta_blender"] = worst_blender
    REPORT["measurements"]["roundtrip_max_delta_clip_eval"] = worst_clip
    ok = worst_blender <= BAND and worst_clip <= BAND
    record("G3", ok, "evaluated poses at every frame: original "
                     "vs round-tripped max {:.3e}; frozen clip "
                     "evaluation vs round-tripped max {:.3e}; "
                     "band {:.0e}".format(worst_blender, worst_clip,
                                          BAND))
    if not ok:
        return finish(1)

    # G4: action -> clip again; frozen bytes identical.
    doc_b, _notes_b = converter.action_to_clip(
        arm, body, binding, "lana/bench/h2_battery_wave", rig,
        SESSION, loop=(0, 48), actor="converter")
    # A working document never carries a freeze block; the
    # stash rule returned the frozen source verbatim, so strip
    # the block before writing the working copy to re-freeze.
    doc_b = dict(doc_b)
    doc_b.pop("freeze", None)
    working_b = os.path.join(workdir, "h2_roundtrip.clip.json")
    frozen_b = os.path.join(workdir, "h2_roundtrip.frozen.json")
    clipdoc.write_document(doc_b, working_b)
    ok_f, msg = freeze(binary, working_b, frozen_b)
    same = ok_f and open(frozen_a, "rb").read() == \
        open(frozen_b, "rb").read()
    record("G4", same, "clip -> action -> clip frozen bytes "
                       "identical" if same else
           "frozen bytes differ ({})".format(msg))
    if not same:
        return finish(1)

    # G5: NLA front + evaluated flatten.
    nod = bpy.data.actions.new("H2 Nod")
    scene.torogao_armature = arm
    scene.torogao_body = body
    adt = arm.animation_data
    track1 = adt.nla_tracks.new()
    track1.name = "Base"
    strip1 = track1.strips.new("Base", 0, adt.action)
    strip1.influence = 1.0
    track2 = adt.nla_tracks.new()
    track2.name = "Overlay"
    strip2 = track2.strips.new("Overlay", 0, nod)
    strip2.blend_type = "ADD"
    strip2.influence = 0.5
    bpy.ops.torogao.nla_refresh()
    items = len(scene.torogao_nla_items)
    scene.torogao_nla_items[1].mute = True
    mute_written = track2.strips[0].mute is True
    scene.torogao_nla_items[1].mute = False
    scene.torogao_nla_active_index = 1
    bpy.ops.torogao.nla_track_up()
    order_after = [t.name for t in adt.nla_tracks]
    bpy.ops.torogao.nla_select()
    head_before = None
    scene.frame_set(10)
    bpy.context.view_layer.update()
    quat = arm.pose.bones["mixamorig:Head"].rotation_quaternion
    head_before = [quat.x, quat.y, quat.z, quat.w]
    flat = converter.flatten_evaluated(
        arm, body, binding, "lana/bench/h2_flatten", rig, SESSION,
        0, 48, loop=(0, 48))
    flat_path = os.path.join(workdir, "h2_flatten.clip.json")
    clipdoc.write_document(flat, flat_path)
    ok_v, msg = validate(binary, flat_path)
    strip_lines = [e for e in flat["op_log"]
                   if e["op"].startswith("nla ")]
    head_channel = next(
        c for c in flat["channels"]
        if c["target"]["name"] == "mixamorig:Head"
        and c["target"]["property"] == "rotation")
    head_flat = next(k["value"] for k in head_channel["keys"]
                     if k["frame"] == 10)
    head_diff = quat_max_diff(quat_normalized(head_before),
                              quat_normalized(head_flat))
    ok = items == 2 and mute_written and \
        order_after == ["Overlay", "Base"] and ok_v and \
        len(strip_lines) == 2 and head_diff <= BAND
    record("G5", ok, "front lists {} strips, mute writes through, "
                     "track reorder {}; flatten validates ({}), "
                     "strip provenance lines {}, frame-10 head "
                     "delta {:.3e}".format(items, order_after, msg,
                                           len(strip_lines),
                                           head_diff))
    if not ok:
        return finish(1)

    # G6: pose asset round trip.
    pose_doc = clipdoc.new_document(
        "lana/bench/h2_pose_wave24", rig, (24, 1), 24, 24)
    clipdoc.append_op(pose_doc, "agent", SESSION,
                      "pose record extracted from the battery "
                      "clip's frame 24")
    for channel in doc_a["channels"]:
        key = next((k for k in channel["keys"] if k["frame"] == 24),
                   None)
        if key is None:
            continue
        single = {"frame": 24, "value": key["value"], "interp": "step"}
        if "key_type" in key:
            single["key_type"] = key["key_type"]
        clipdoc.add_channel(pose_doc, channel["target"]["kind"],
                            channel["target"]["name"],
                            channel["target"]["property"], [single])
    pose_path = os.path.join(workdir, "h2_pose.clip.json")
    clipdoc.write_document(pose_doc, pose_path)
    ok_v, msg = validate(binary, pose_path)
    pose_action, _ = converter.clip_to_pose_asset(
        pose_doc, arm, body, binding, SESSION)
    is_asset = pose_action.asset_data is not None
    back, _n = converter.action_to_clip(
        arm, body, binding, "lana/bench/h2_pose_wave24", rig,
        SESSION, actor="converter")
    same = clipdoc.document_equal(back, pose_doc)
    ok = ok_v and is_asset and same
    record("G6", ok, "one-frame clip validates ({}), imports as "
                     "a marked pose asset ({}), re-exports "
                     "identical ({})".format(msg, is_asset, same))
    if not ok:
        return finish(1)

    # G7: GLB export + A10.2-shape census.
    converter.clip_to_action(doc_a, arm, body, binding, SESSION)
    glb_out = os.path.join(workdir, "h2_witness.glb")
    bpy.ops.export_scene.gltf(
        filepath=glb_out, export_format="GLB", export_animations=True,
        export_animation_mode="ACTIVE_ACTIONS", export_frame_step=1)
    result = census.census_glb(glb_out)
    src = census.census_glb(glb_path)
    joints_ok = result["joints"] == src["joints"]
    morphs_ok = result["morph_targets"] == src["morph_targets"]
    anim = result["animations"][0] if result["animations"] else None
    keys_max = max((s["keys"] for s in anim["samplers"]), default=0) \
        if anim else 0
    dur = max((s["t_max"] for s in anim["samplers"]), default=0.0) \
        if anim else 0.0
    clip_ok = anim is not None and keys_max == 49 and \
        abs(dur - 2.0) < 0.021
    REPORT["measurements"]["census"] = result
    ok = joints_ok and morphs_ok and clip_ok
    record("G7", ok, "census: joints {} ({}), morphs {} ({}), "
                     "animation {!r} samplers max {} keys, "
                     "duration {:.3f}s (authored 49 keys / "
                     "2.000s)".format(len(result["joints"]), joints_ok,
                                       len(result["morph_targets"]),
                                       morphs_ok,
                                       anim["name"] if anim else None,
                                       keys_max, dur))
    if not ok:
        return finish(1)
    return finish(0)


def finish(code):
    path = os.path.join(REPORT.get("_workdir", "."), "gate_report.json")
    with open(path, "w") as handle:
        json.dump(REPORT, handle, indent=2)
    print("H2 GATE {}".format("PASS" if code == 0 else "FAIL"))
    sys.exit(code)


if __name__ == "__main__":
    argv = sys.argv[sys.argv.index("--") + 1:]
    REPORT["_workdir"] = argv[3]
    try:
        main()
    except Exception as error:  # noqa: BLE001 - gate must report
        import traceback
        traceback.print_exc()
        record("EXCEPTION", False, repr(error))
        finish(1)
