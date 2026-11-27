"""converter — the Blender round-trip converter of the Torogao
bench: Blender Action <-> torogao-motion-clip/2, per DESIGN.md
section 5.1, plus pose-asset import/export riding the same
converter, plus the evaluated NLA flatten.

Binding law (from the estate binders that froze Lana's clips,
build/bind_rung1.py and build/bind_proof.py in the rung-1 lane,
and the format's FORMATS.md entry):

- a joint rotation channel's value IS the pose bone's local
  rotation quaternion (a delta from rest), stored [x, y, z, w];
- the root joint's translation channel holds absolute glTF
  node translations; a pose bone location binds as a delta
  from the rest translation, converted glTF -> Blender as
  (x, -z, y) and rotated by the inverse of the bone's
  matrix_local 3x3. The converter inverts exactly that.
  Translation channels are bound for the root joint only;
  keyed locations on any other joint are a named refusal,
  because no estate binder reads them;
- morph weight channels are shape key values, addressed by
  target name; parameter channels are not authored in Blender
  (they are reaction/toon data) and pass through the stash
  untouched on a round trip.

Curve law (DESIGN.md sections 3.2 and 3.4): Bezier segments
convert to cubic Hermite parametrically (clipmath), and write
back with handles at the segment thirds. Easing-family
interpolations and uneven component key sets are edit-time
shapes: the converter bakes them to per-frame linear keys and
says so in the op log, carrying the easing name as the opening
key's provenance tag. One more format-forced sampling: a flat
Bezier handle on a rotation channel (Blender's clamped end
keys, a held pose) yields a zero quaternion tangent, which
the format refuses (TG-CLIP-ZERO-QUATERNION — evaluation
normalises, and zero has no normal); such a segment is
sampled to per-frame linear keys, exact at every frame, and
the sampling is named in the op log. Linear rotation
segments are sampled in both directions for the mirror
reason: the clip's linear rotation is slerp (Appendix C),
an F-Curve's linear is componentwise lerp — different curves
between sparse keys, one curve at frame density.

The evaluated read (DESIGN.md section 5.1: the converter reads
evaluated bone transforms; IK is a posing aid, what freezes
is FK keys) is `flatten_evaluated`: the whole animation stack
(action + NLA) sampled from the evaluated pose at every frame,
with the strip layout recorded in the op log as provenance.
"""

import json
import math
import re
import struct

import bpy
from mathutils import Matrix, Quaternion, Vector

from . import clipdoc
from .clipdoc import ClipError

# ----------------------------------------------------------------
# Rig binding: rest data read from the rig's GLB of record.
# ----------------------------------------------------------------


def _glb_json(path):
    with open(path, "rb") as handle:
        data = handle.read()
    if data[:4] != b"glTF":
        raise ClipError("{} is not a GLB".format(path))
    length = struct.unpack("<I", data[12:16])[0]
    return json.loads(data[20:20 + length])


class RigBinding:
    """Rest-state facts about one rig, from its GLB: the joint
    set, the root joint, rest node translations, morph names."""

    def __init__(self, glb_path):
        gltf = _glb_json(glb_path)
        nodes = gltf["nodes"]
        skin = gltf["skins"][0]
        self.joint_names = [nodes[j]["name"] for j in skin["joints"]]
        child_of = {}
        for i, node in enumerate(nodes):
            for child in node.get("children", []):
                child_of[child] = i
        joint_set = set(skin["joints"])
        self.root_joint = None
        self.rest_translation = {}
        for j in skin["joints"]:
            node = nodes[j]
            self.rest_translation[node["name"]] = list(
                node.get("translation", [0.0, 0.0, 0.0]))
            if child_of.get(j) not in joint_set:
                self.root_joint = node["name"]
        self.morph_names = []
        for mesh in gltf["meshes"]:
            names = (mesh.get("extras") or {}).get("targetNames")
            if names:
                self.morph_names = list(names)
                break


# ----------------------------------------------------------------
# Basis conversion (the bind law's glTF <-> Blender frame change)
# ----------------------------------------------------------------


def _gltf_delta_to_blender(delta):
    return Vector((delta[0], -delta[2], delta[1]))


def _blender_delta_to_gltf(vec):
    return [vec.x, vec.z, -vec.y]


def hips_location_to_clip(binding, bone, location):
    """Pose bone location -> clip translation value for the
    root joint: rest glTF translation + the glTF-frame delta,
    the exact inverse of the binders' binding."""
    rest = binding.rest_translation[bone.name]
    rot = bone.matrix_local.to_3x3()
    d_bl = rot @ Vector(location)
    delta = _blender_delta_to_gltf(d_bl)
    return [rest[i] + delta[i] for i in range(3)]


def clip_to_hips_location(binding, bone, value):
    """Clip translation value -> pose bone location (the
    binders' formula verbatim)."""
    rest = binding.rest_translation[bone.name]
    delta = [value[i] - rest[i] for i in range(3)]
    d_bl = _gltf_delta_to_blender(delta)
    rinv = bone.matrix_local.to_3x3().inverted()
    return rinv @ d_bl


# ----------------------------------------------------------------
# Action reading
# ----------------------------------------------------------------

_POSE_RE = re.compile(
    r'^pose\.bones\["(.*)"\]\.'
    r'(rotation_quaternion|rotation_euler|location|scale)$')
_KEY_RE = re.compile(r'^key_blocks\["(.*)"\]\.value$')

_EASING_MODES = {
    "SINE", "QUAD", "CUBIC", "QUART", "QUINT", "EXPO", "CIRC",
    "BACK", "BOUNCE", "ELASTIC",
}
_KEY_TYPE_OUT = {
    "KEYFRAME": "keyframe",
    "BREAKDOWN": "breakdown",
    "MOVING_HOLD": "moving_hold",
    "EXTREME": "extreme",
}
_KEY_TYPE_IN = {v: k for k, v in _KEY_TYPE_OUT.items()}


def action_fcurves(action):
    """Every (slot, fcurve) in a slotted action, walking
    layers -> strips -> channelbags (the Blender 5.3 data
    model; the layered-actions UI is not in 5.3, the data is)."""
    out = []
    if action is None:
        return out
    for layer in action.layers:
        for strip in layer.strips:
            if strip.type != "KEYFRAME":
                continue
            for slot in action.slots:
                try:
                    bag = strip.channelbag(slot)
                except (TypeError, RuntimeError):
                    continue
                if bag is None:
                    continue
                for fcurve in bag.fcurves:
                    out.append((slot, fcurve))
    return out


def _int_frame(x, what):
    frame = int(round(x))
    if abs(x - frame) > 1e-4:
        raise ClipError(
            "{} sits at non-integer frame {} — clip keys are "
            "integer frames; move the key".format(what, x))
    return frame


def _group_curves(action):
    """Group an action's fcurves by animated target:
    {(kind, name, property): {array_index: fcurve}} with kind
    'joint' (pose bones) or 'morph' (shape keys)."""
    groups = {}
    for _slot, fcurve in action_fcurves(action):
        match = _POSE_RE.match(fcurve.data_path)
        if match:
            key = ("joint", match.group(1), match.group(2))
        else:
            match = _KEY_RE.match(fcurve.data_path)
            if not match:
                continue
            key = ("morph", match.group(1), "value")
        groups.setdefault(key, {})[fcurve.array_index] = fcurve
    return groups


def _channel_keys_from_curves(curves, shape, notes):
    """Assemble clip keys for one channel from its component
    fcurves. `shape` is 'vec3', 'quat' (Blender wxyz order) or
    'scalar'. Returns the clip key list. Bakes segments whose
    interpolation is not uniformly step/linear/Bezier across
    components, and channels whose components key unevenly."""
    # Settle auto handles before reading them.
    for fcurve in curves.values():
        fcurve.update()
    comp_frames = {}
    for index, fcurve in curves.items():
        comp_frames[index] = [
            _int_frame(kp.co[0], "key of {}".format(fcurve.data_path))
            for kp in fcurve.keyframe_points]
    union = sorted({f for frames in comp_frames.values() for f in frames})
    even = all(comp_frames[i] == union for i in comp_frames)

    def point_at(index, frame):
        for kp in curves[index].keyframe_points:
            if _int_frame(kp.co[0], "key") == frame:
                return kp
        return None

    def value_at(frame):
        comps = []
        for index in sorted(curves):
            kp = point_at(index, frame)
            if kp is not None:
                comps.append(kp.co[1])
            else:
                comps.append(curves[index].evaluate(frame))
        if shape == "quat":
            w, x, y, z = comps
            return [x, y, z, w]
        if shape == "scalar":
            return comps[0]
        return comps

    keys = {}
    pending_in = {}
    baked = False
    for si in range(len(union) - 1):
        f0, f1 = union[si], union[si + 1]
        dt = float(f1 - f0)
        modes = set()
        easing = None
        if even:
            for index in curves:
                kp = point_at(index, f0)
                modes.add(kp.interpolation)
                if kp.interpolation in _EASING_MODES:
                    easing = kp.interpolation
        if not even or len(modes) != 1 or easing is not None:
            baked = True
            for frame in range(f0, f1):
                keys[frame] = {"frame": frame, "value": value_at(frame),
                               "interp": "linear"}
            if easing is not None and f0 in keys:
                keys[f0]["easing"] = "blender:{}".format(easing)
            continue
        mode = modes.pop()
        if mode == "CONSTANT":
            keys[f0] = {"frame": f0, "value": value_at(f0),
                        "interp": "step"}
        elif mode == "LINEAR" and shape == "quat":
            # A linear rotation segment means slerp in the
            # clip (Appendix C) and componentwise lerp in an
            # F-Curve: different curves between the keys. The
            # converter samples the segment to per-frame keys
            # — exact at every frame in both languages — and
            # says so in the op log.
            for frame in range(f0, f1):
                keys[frame] = {"frame": frame,
                               "value": value_at(frame),
                               "interp": "linear"}
            notes.append(
                "segment {}..{} sampled to per-frame linear "
                "keys: clip linear rotation is slerp, F-Curve "
                "linear is componentwise".format(f0, f1))
        elif mode == "LINEAR":
            keys[f0] = {"frame": f0, "value": value_at(f0),
                        "interp": "linear"}
        elif mode == "BEZIER":
            out_t = []
            for index in sorted(curves):
                kp = point_at(index, f0)
                out_t.append(3.0 * (kp.handle_right[1] - kp.co[1]) / dt)
            in_t = []
            for index in sorted(curves):
                kp = point_at(index, f1)
                in_t.append(3.0 * (kp.co[1] - kp.handle_left[1]) / dt)
            if shape == "quat" and (
                    all(c == 0.0 for c in out_t)
                    or all(c == 0.0 for c in in_t)):
                # A flat Bezier handle (Blender's clamped end
                # keys, a held pose) gives a zero tangent, and
                # the format refuses a zero quaternion tangent
                # (TG-CLIP-ZERO-QUATERNION: evaluation
                # normalises, zero has no normal). The segment
                # is sampled to per-frame linear keys — exact
                # at every frame — and the sampling is named
                # in the op log.
                for frame in range(f0, f1):
                    keys[frame] = {"frame": frame,
                                   "value": value_at(frame),
                                   "interp": "linear"}
                notes.append(
                    "segment {}..{} sampled to per-frame linear "
                    "keys: zero quaternion tangent (flat handle) "
                    "is refused by the format".format(f0, f1))
                continue
            if shape == "scalar":
                # Scalar properties carry bare numbers, values
                # and tangents alike (the format's two shapes).
                out_t = out_t[0]
                in_t = in_t[0]
            key = {"frame": f0, "value": value_at(f0),
                   "interp": "cubic", "out_tangent": out_t}
            if shape == "quat":
                key["out_tangent"] = _quat_wxyz_to_xyzw(out_t)
                in_t = _quat_wxyz_to_xyzw(in_t)
            keys[f0] = key
            # The closing key's in_tangent is applied after the
            # segment loop: the next segment writes that key as
            # its own opening key, and the tangent must survive.
            pending_in[f1] = in_t
        else:  # pragma: no cover - modes are the four above
            raise ClipError("unhandled interpolation {!r}".format(mode))
    last = union[-1]
    if last not in keys:
        keys[last] = {"frame": last, "value": value_at(last),
                      "interp": "step"}
    for frame, in_t in pending_in.items():
        if frame in keys:
            keys[frame]["in_tangent"] = in_t
    ordered = [keys[f] for f in sorted(keys)]
    # Key types ride the opening key of each segment.
    if even:
        for key in ordered:
            kp = point_at(sorted(curves)[0], key["frame"])
            if kp is not None and kp.type in _KEY_TYPE_OUT:
                key["key_type"] = _KEY_TYPE_OUT[kp.type]
    if baked:
        notes.append("baked uneven/eased segments to per-frame "
                     "linear keys")
    return ordered


def _quat_wxyz_to_xyzw(vec):
    w, x, y, z = vec
    return [x, y, z, w]


def _hemisphere_continuity(keys, notes, label):
    """Rotation keys: negate any key whose quaternion flips
    hemisphere against its predecessor (and its tangents with
    it). Same rotations, one continuous representation."""
    flips = 0
    prev = None
    for key in keys:
        value = key["value"]
        if prev is not None:
            dot = sum(a * b for a, b in zip(prev, value))
            if dot < 0.0:
                key["value"] = [-c for c in value]
                for field in ("in_tangent", "out_tangent"):
                    if field in key:
                        key[field] = [-c for c in key[field]]
                flips += 1
                value = key["value"]
        prev = value
    if flips:
        notes.append("{}: {} quaternion sign flips normalised "
                     "for hemisphere continuity".format(label, flips))


def action_to_clip(arm, body, binding, name, rig, session,
                   loop=None, actor="converter"):
    """Convert the armature's active action (+ the body's shape
    key action) into a working clip document. Returns
    (doc, notes)."""
    notes = []
    groups = {}
    arm_action = arm.animation_data.action if arm.animation_data else None
    for key, curves in _group_curves(arm_action).items():
        groups[key] = curves
    if body is not None and body.data.shape_keys is not None:
        key_action = body.data.shape_keys.animation_data
        key_action = key_action.action if key_action else None
        for key, curves in _group_curves(key_action).items():
            groups[key] = curves
    if not groups:
        raise ClipError("no keyframed pose-bone or shape-key "
                        "curves on the active actions")

    # Range from the union of all key frames.
    all_frames = []
    for curves in groups.values():
        for fcurve in curves.values():
            for kp in fcurve.keyframe_points:
                all_frames.append(_int_frame(kp.co[0], "key"))
    start, end = min(all_frames), max(all_frames)

    scene = bpy.context.scene
    fps = scene.render.fps
    base = scene.render.fps_base
    rate = (int(round(fps)), int(round(base))) if abs(base - round(base)) < 1e-6 \
        else (int(round(fps * 1000)), int(round(base * 1000)))
    doc = clipdoc.new_document(name, rig, rate, start, end, loop)
    clipdoc.append_op(doc, actor, session,
                      "action->clip from action {!r} on rig {}".format(
                          arm_action.name if arm_action else None,
                          binding.root_joint))

    stashed = _read_stash(arm_action)
    root = binding.root_joint
    bad_locations = []
    for (kind, bone_name, prop), curves in sorted(groups.items()):
        if kind == "morph":
            keys = _channel_keys_from_curves(curves, "scalar", notes)
            clipdoc.add_channel(doc, "morph", bone_name, "weight", keys)
            continue
        bone = arm.data.bones.get(bone_name)
        if bone is None:
            raise ClipError("action keys unknown bone {!r}".format(bone_name))
        if prop == "rotation_quaternion":
            keys = _channel_keys_from_curves(curves, "quat", notes)
            _hemisphere_continuity(keys, notes, bone_name)
            clipdoc.add_channel(doc, "joint", bone_name, "rotation", keys)
        elif prop == "rotation_euler":
            keys = _euler_channel_keys(arm, bone_name, curves)
            _hemisphere_continuity(keys, notes, bone_name)
            clipdoc.add_channel(doc, "joint", bone_name, "rotation", keys)
            notes.append("{}: euler curves converted by sampling "
                         "to linear quaternion keys".format(bone_name))
        elif prop == "location":
            if bone_name != root:
                vals = [curves[i].evaluate(f)
                        for i in curves for f in range(start, end + 1)]
                if any(abs(v) > 1e-9 for v in vals):
                    bad_locations.append(bone_name)
                continue
            keys = _channel_keys_from_curves(curves, "vec3", notes)
            pbone = arm.pose.bones[bone_name]
            for key in keys:
                key["value"] = hips_location_to_clip(
                    binding, pbone.bone, key["value"])
                for field in ("in_tangent", "out_tangent"):
                    if field in key:
                        rot = pbone.bone.matrix_local.to_3x3()
                        key[field] = _blender_delta_to_gltf(
                            rot @ Vector(key[field]))
            clipdoc.add_channel(doc, "joint", bone_name,
                                "translation", keys)
        elif prop == "scale":
            keys = _channel_keys_from_curves(curves, "vec3", notes)
            clipdoc.add_channel(doc, "joint", bone_name, "scale", keys)
    if bad_locations:
        raise ClipError(
            "location keys on non-root joints {} — translation "
            "channels are bound for the root joint ({}) only "
            "(estate bind law); bake those bones' motion into "
            "rotations or flatten by evaluation".format(
                sorted(bad_locations), root))
    # Parameter channels and shader blocks are not
    # Blender-authored; a stashed document's ride through an
    # edit untouched, and the channel order of the source is
    # preserved so an unedited round trip stays identical.
    if stashed is not None:
        doc["shaders"] = stashed.get("shaders", [])
        regen = {}
        for channel in doc["channels"]:
            t = channel["target"]
            regen[(t["kind"], t["name"], t["property"])] = channel
        ordered = []
        for schannel in stashed["channels"]:
            t = schannel["target"]
            key = (t["kind"], t["name"], t["property"])
            if key in regen:
                ordered.append(regen.pop(key))
            elif t["kind"] == "parameter":
                ordered.append(schannel)
            # else: the channel was deleted in Blender; the
            # edit stands and the channel is gone.
        ordered.extend(regen.values())
        doc["channels"] = ordered
    for note in notes:
        clipdoc.append_op(doc, actor, session, note)
    # Stash rule: an unedited round trip re-emits the source.
    if stashed is not None and _stash_matches(doc, stashed):
        return stashed, notes + ["stash rule: regenerated document "
                                 "identical to source; source "
                                 "re-emitted verbatim"]
    if stashed is not None:
        doc["op_log"] = stashed["op_log"] + doc["op_log"]
        for i, entry in enumerate(doc["op_log"]):
            entry["seq"] = i
    return doc, notes


def _euler_channel_keys(arm, bone_name, curves):
    frames = sorted({_int_frame(kp.co[0], "key")
                     for fc in curves.values()
                     for kp in fc.keyframe_points})
    pbone = arm.pose.bones[bone_name]
    order = pbone.rotation_mode
    keys = []
    for frame in frames:
        euler = [curves[i].evaluate(frame) if i in curves else 0.0
                 for i in range(3)]
        from mathutils import Euler
        quat = Euler(euler, order).to_quaternion()
        keys.append({"frame": frame,
                     "value": [quat.x, quat.y, quat.z, quat.w],
                     "interp": "linear"})
    return keys


def settle_handles_thirds(action):
    """The bench's keying discipline: place every Bezier
    handle's frame coordinate at its segment third, keeping
    the handle values. Handle frame placement is not clip
    data — the clip carries the parametric curve through the
    handle values (DESIGN.md section 3.2) — and with handles
    at the thirds, Blender's graph evaluation of the action
    is that same curve, so an action authored on the bench
    converts exactly. Blender's AUTO / AUTO_CLAMPED handles
    already sit at the thirds; this normalises hand-dragged
    free handles. Returns the number of handles moved."""
    moved = 0
    for _slot, fcurve in action_fcurves(action):
        fcurve.update()
        points = fcurve.keyframe_points
        for i, kp in enumerate(points):
            if kp.interpolation != "BEZIER":
                continue
            if i + 1 < len(points):
                dt = points[i + 1].co[0] - kp.co[0]
                want = kp.co[0] + dt / 3.0
                if abs(kp.handle_right[0] - want) > 1e-6:
                    kp.handle_right[0] = want
                    moved += 1
            if i > 0:
                dt = kp.co[0] - points[i - 1].co[0]
                want = kp.co[0] - dt / 3.0
                if abs(kp.handle_left[0] - want) > 1e-6:
                    kp.handle_left[0] = want
                    moved += 1
        fcurve.update()
    return moved


# ----------------------------------------------------------------
# Action writing
# ----------------------------------------------------------------

_STASH_PROP = "torogao_clip_stash"


def _read_stash(action):
    if action is None or _STASH_PROP not in action:
        return None
    try:
        return json.loads(action[_STASH_PROP])
    except (ValueError, TypeError):
        return None


def _expand_linear_rotation(keys):
    """A clip channel's keys with sparse linear rotation
    segments expanded to per-frame slerp samples (Appendix C
    evaluation of the two-key segment at each interior frame).
    Used on import — Blender's componentwise curves reproduce
    the clip at every frame only at frame density — and on
    the stash comparison, where both sides are expanded the
    same way before they are compared."""
    from . import clipmath
    out = []
    for i, key in enumerate(keys):
        out.append(key)
        if key["interp"] == "linear" and i + 1 < len(keys):
            nxt = keys[i + 1]
            pair = {"target": {"kind": "joint", "name": "",
                               "property": "rotation"},
                    "keys": [key, nxt]}
            for frame in range(key["frame"] + 1, nxt["frame"]):
                out.append({"frame": frame,
                            "value": clipmath.evaluate_channel(
                                pair, frame),
                            "interp": "linear"})
    return out


def _numbers_close(a, b, tol=1e-6):
    if isinstance(a, list) and isinstance(b, list):
        return len(a) == len(b) and all(
            _numbers_close(x, y, tol) for x, y in zip(a, b))
    if isinstance(a, (int, float)) and isinstance(b, (int, float)):
        return abs(a - b) <= tol
    return a == b


def _channels_match(a_channels, b_channels):
    """Stash comparison: same targets in the same order; keys
    equal in frame/interp/key_type; values and tangents equal
    within the Tier 2 band (an F-Curve stores f32, so a round
    trip through Blender rounds at ~1e-7 — inside the band,
    the document is the same document). Linear rotation
    segments are compared after slerp expansion, the form
    both directions of the converter produce."""
    if len(a_channels) != len(b_channels):
        return False
    for chan_a, chan_b in zip(a_channels, b_channels):
        if chan_a["target"] != chan_b["target"]:
            return False
        keys_a = chan_a["keys"]
        keys_b = chan_b["keys"]
        if chan_a["target"]["property"] == "rotation":
            keys_a = _expand_linear_rotation(keys_a)
            keys_b = _expand_linear_rotation(keys_b)
        if len(keys_a) != len(keys_b):
            return False
        for key_a, key_b in zip(keys_a, keys_b):
            # A missing key_type is the default keyframe: the
            # converter tags keys from real Blender keyframe
            # points, and a sampled interior key has no point
            # on first export but does (a default one) after a
            # round trip — the document is the same document.
            if key_a["frame"] != key_b["frame"] or \
                    key_a["interp"] != key_b["interp"] or \
                    key_a.get("key_type", "keyframe") != \
                    key_b.get("key_type", "keyframe"):
                return False
            for field in ("value", "in_tangent", "out_tangent"):
                if (field in key_a) != (field in key_b):
                    return False
                if field in key_a and not _numbers_close(
                        key_a[field], key_b[field]):
                    return False
    return True


def _stash_matches(doc, stashed):
    keys = ("format", "name", "rig", "rate", "range", "loop",
            "shaders")
    return all(doc.get(k) == stashed.get(k) for k in keys) and \
        _channels_match(doc["channels"], stashed["channels"])


def _new_slotted_action(name, id_type, slot_name):
    action = bpy.data.actions.new(name)
    slot = action.slots.new(id_type, slot_name)
    layer = action.layers.new("Layer")
    strip = layer.strips.new(type="KEYFRAME")
    bag = strip.channelbag(slot, ensure=True)
    return action, slot, bag


def _write_fcurve(bag, data_path, index, keys, group):
    """Write one component curve from clip keys. `keys` are
    (frame, value, interp, key_type, in_tangent, out_tangent)
    tuples in clip value shape; this writes one component."""
    fcurve = bag.fcurves.new(data_path, index=index, group_name=group)
    points = fcurve.keyframe_points
    for frame, value, _i, _t, _in, _out in keys:
        points.insert(frame, value, options={"FAST"})
    by_frame = {int(round(kp.co[0])): kp for kp in points}
    for i, (frame, _v, interp, key_type, in_t, out_t) in enumerate(keys):
        kp = by_frame[frame]
        if interp == "cubic":
            kp.interpolation = "BEZIER"
            kp.handle_left_type = "FREE"
            kp.handle_right_type = "FREE"
            if out_t is not None and i + 1 < len(keys):
                dt = float(keys[i + 1][0] - frame)
                kp.handle_right = (frame + dt / 3.0,
                                   kp.co[1] + out_t * dt / 3.0)
            if in_t is not None and i > 0:
                dt = float(frame - keys[i - 1][0])
                kp.handle_left = (frame - dt / 3.0,
                                  kp.co[1] - in_t * dt / 3.0)
        elif interp == "linear":
            kp.interpolation = "LINEAR"
        else:
            kp.interpolation = "CONSTANT"
        if key_type is not None:
            kp.type = _KEY_TYPE_IN[key_type]
    fcurve.update()
    return fcurve


def clip_to_action(doc, arm, body, binding, session,
                   actor="converter", action_name=None):
    """Build Blender actions from a clip document: an armature
    action for joint channels, a shape-key action for morph
    channels. Returns (arm_action, key_action). The source
    document is stashed on both for the round-trip rule."""
    for channel in doc["channels"]:
        target = channel["target"]
        if target["kind"] == "joint" and \
                target["name"] not in binding.joint_names:
            raise ClipError("clip names joint {!r}, not on this "
                            "rig".format(target["name"]))
        if target["kind"] == "morph" and binding.morph_names and \
                target["name"] not in binding.morph_names:
            raise ClipError("clip names morph {!r}, not on this "
                            "rig".format(target["name"]))
    name = action_name or doc["name"]
    arm_action, arm_slot, arm_bag = _new_slotted_action(
        name, "OBJECT", arm.name)
    key_action = None
    key_bag = None
    stash = json.dumps(doc)

    def key_rows(channel, component):
        rows = []
        for key in channel["keys"]:
            value = key["value"]
            comp = value[component] if isinstance(value, list) else value
            in_t = key.get("in_tangent")
            out_t = key.get("out_tangent")
            rows.append((
                key["frame"], comp, key["interp"],
                key.get("key_type"),
                in_t[component] if isinstance(in_t, list) else in_t,
                out_t[component] if isinstance(out_t, list) else out_t))
        return rows

    for channel in doc["channels"]:
        target = channel["target"]
        kind, cname, prop = (target["kind"], target["name"],
                             target["property"])
        if kind == "parameter":
            continue  # not Blender-authored; carried by the stash
        if kind == "morph":
            if body is None or body.data.shape_keys is None:
                raise ClipError("clip has morph channels but no "
                                "shape-keyed body was given")
            if key_action is None:
                key_action, _ks, key_bag = _new_slotted_action(
                    name + " Keys", "KEY", body.data.shape_keys.name)
            path = 'key_blocks["{}"].value'.format(cname)
            _write_fcurve(key_bag, path, 0, key_rows(channel, 0),
                          cname)
            continue
        bone = arm.pose.bones[cname]
        if prop == "rotation":
            path = 'pose.bones["{}"].rotation_quaternion'.format(cname)
            # Sparse linear rotation segments are expanded to
            # per-frame slerp samples: the clip's linear is
            # slerp, an F-Curve's linear is componentwise, and
            # only frame density makes the action reproduce
            # the clip at every frame.
            expanded = dict(channel)
            expanded["keys"] = _expand_linear_rotation(
                channel["keys"])
            # Clip order [x, y, z, w] -> Blender indices w=0..z=3.
            for bl_index, clip_comp in ((0, 3), (1, 0), (2, 1), (3, 2)):
                _write_fcurve(arm_bag, path, bl_index,
                              key_rows(expanded, clip_comp), cname)
            bone.rotation_mode = "QUATERNION"
        elif prop == "translation":
            if cname != binding.root_joint:
                raise ClipError(
                    "clip carries a translation channel for "
                    "non-root joint {!r}; translation binds for "
                    "the root joint ({}) only".format(
                        cname, binding.root_joint))
            path = 'pose.bones["{}"].location'.format(cname)
            rows_by_comp = {0: [], 1: [], 2: []}
            for key in channel["keys"]:
                loc = clip_to_hips_location(binding, bone.bone,
                                            key["value"])
                for comp in range(3):
                    in_t = key.get("in_tangent")
                    out_t = key.get("out_tangent")
                    def conv_t(t):
                        if t is None:
                            return None
                        rinv = bone.bone.matrix_local.to_3x3().inverted()
                        return (rinv @ _gltf_delta_to_blender(t))[comp]
                    rows_by_comp[comp].append((
                        key["frame"], loc[comp], key["interp"],
                        key.get("key_type"), conv_t(in_t),
                        conv_t(out_t)))
            for comp in range(3):
                _write_fcurve(arm_bag, path, comp,
                              rows_by_comp[comp], cname)
        elif prop == "scale":
            path = 'pose.bones["{}"].scale'.format(cname)
            for comp in range(3):
                _write_fcurve(arm_bag, path, comp,
                              key_rows(channel, comp), cname)
    arm_action[_STASH_PROP] = stash
    adt = arm.animation_data or arm.animation_data_create()
    adt.action = arm_action
    try:
        adt.action_slot = arm_slot
    except (TypeError, AttributeError):
        pass
    if key_action is not None:
        key_action[_STASH_PROP] = stash
        keys_id = body.data.shape_keys
        kdt = keys_id.animation_data or keys_id.animation_data_create()
        kdt.action = key_action
        try:
            kdt.action_slot = key_action.slots[0]
        except (TypeError, AttributeError, IndexError):
            pass
    return arm_action, key_action


# ----------------------------------------------------------------
# Pose assets: a pose asset is a one-frame action; the same
# converter carries it (DESIGN.md section 5.1).
# ----------------------------------------------------------------


def clip_to_pose_asset(doc, arm, body, binding, session):
    """Import a one-frame clip as a Blender pose asset."""
    if doc["range"]["start"] != doc["range"]["end"]:
        raise ClipError("a pose asset clip spans exactly one "
                        "frame; this clip spans {}..{}".format(
                            doc["range"]["start"], doc["range"]["end"]))
    arm_action, key_action = clip_to_action(
        doc, arm, body, binding, session)
    for action in (arm_action, key_action):
        if action is not None:
            action.asset_mark()
            if action.asset_data:
                action.asset_data.description = \
                    "Torogao pose asset from clip {}".format(doc["name"])
    return arm_action, key_action


# ----------------------------------------------------------------
# Evaluated read: NLA flatten (DESIGN.md section 5.1)
# ----------------------------------------------------------------


def strip_layout_lines(arm):
    """The NLA strip layout as provenance lines: one per strip,
    naming track, strip, action, frames, blend, influence, and
    mute/solo state — recorded in the flattened clip's op log."""
    lines = []
    adt = arm.animation_data
    if adt is None:
        return lines
    for track in adt.nla_tracks:
        for strip in track.strips:
            lines.append(
                "nla {}/{} act={!r} {}..{} {} inf={:.2f} "
                "mute={} tmute={} solo={}".format(
                    track.name, strip.name,
                    strip.action.name if strip.action else None,
                    int(strip.frame_start), int(strip.frame_end),
                    strip.blend_type, strip.influence,
                    strip.mute, track.mute, track.is_solo))
    return lines


def flatten_evaluated(arm, body, binding, name, rig, session,
                      start, end, loop=None, actor="converter"):
    """Sample the evaluated pose (active action + full NLA
    stack, constraints included) at every frame in [start, end]
    and write linear keys. What freezes is FK keys: the
    evaluated transforms, never the drivers."""
    scene = bpy.context.scene
    doc = clipdoc.new_document(
        name, rig, (int(round(scene.render.fps)),
                    int(round(scene.render.fps_base))), start, end,
        loop)
    clipdoc.append_op(doc, actor, session,
                      "evaluated flatten of the nla stack, "
                      "frames {}..{}".format(start, end))
    for line in strip_layout_lines(arm):
        clipdoc.append_op(doc, actor, session, line)
    root = binding.root_joint
    rot_keys = {pb.name: [] for pb in arm.pose.bones}
    trans_keys = []
    morph_keys = {}
    if body is not None and body.data.shape_keys is not None:
        morph_keys = {kb.name: [] for kb in
                      body.data.shape_keys.key_blocks[1:]}
    for frame in range(start, end + 1):
        scene.frame_set(frame)
        bpy.context.view_layer.update()
        for pb in arm.pose.bones:
            quat = pb.matrix_basis.to_quaternion()
            rot_keys[pb.name].append({
                "frame": frame,
                "value": [quat.x, quat.y, quat.z, quat.w],
                "interp": "linear"})
            if pb.name == root:
                loc = pb.matrix_basis.to_translation()
                trans_keys.append({
                    "frame": frame,
                    "value": hips_location_to_clip(
                        binding, pb.bone, loc),
                    "interp": "linear"})
        for kb_name, rows in morph_keys.items():
            rows.append({
                "frame": frame,
                "value": body.data.shape_keys.key_blocks[kb_name].value,
                "interp": "linear"})
    for bone_name, keys in rot_keys.items():
        _hemisphere_continuity(keys, [], bone_name)
        clipdoc.add_channel(doc, "joint", bone_name, "rotation",
                            keys)
    clipdoc.add_channel(doc, "joint", root, "translation",
                        trans_keys)
    for kb_name, keys in morph_keys.items():
        if any(abs(k["value"]) > 1e-9 for k in keys):
            clipdoc.add_channel(doc, "morph", kb_name, "weight",
                                keys)
    return doc
