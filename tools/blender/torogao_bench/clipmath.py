"""clipmath — pure curve mathematics for the Torogao bench.

No Blender imports: this module is importable by plain Python so
the H2 gate can verify it against the estate evaluator before
trusting it inside Blender.

Two halves:

1. The Appendix C evaluation of DESIGN.md, replicated operation
   for operation from `crates/torogao-runtime/src/clip_eval.rs`
   (the H1 evaluator): step / componentwise linear / quaternion
   slerp with shortest-arc correction and the near-parallel
   normalised-lerp fallback / cubic Hermite with tangents read
   as derivatives with respect to frames, scaled by the segment
   length, rotation results normalised. Keyframe exactness is
   by construction (a frame on a key returns the key verbatim).
   `clip_digest` reproduces the Rust Tier 2 digest: SHA-256
   over the f64 little-endian bit patterns of every channel's
   value at every witness tick, frames resolved through the
   clip's loop rule. The H2 gate pins this module against the
   H0/H1 golden digests before using it as an instrument.

2. The Bezier <-> Hermite conversion pair of DESIGN.md section
   3.2. A cubic Bezier segment with control points P0..P3 in
   (frame, value) space is, parametrically, the Hermite segment
   whose tangents (derivatives with respect to frames) are
   m0 = 3*(P1.y - P0.y) / (P3.x - P0.x) and
   m1 = 3*(P3.y - P2.y) / (P3.x - P0.x); inversely, a Hermite
   segment with tangents m0/m1 has Bezier control points
   P1 = P0 + (dt/3, m0*dt/3), P2 = P3 - (dt/3, m1*dt/3) with
   dt = P3.x - P0.x. Handle x-positions are not clip data: the
   clip carries the parametric curve, and the converter writes
   handles back at the segment thirds, where Blender's graph
   evaluation of the Bezier is the same curve (with handle
   x at the thirds, x(t) is linear in t). Blender's own AUTO /
   AUTO_CLAMPED handles sit at the thirds, so default-authored
   actions convert exactly; hand-dragged free handles off the
   thirds are normalised to the parametric curve on export.
"""

import hashlib
import math
import struct


# ----------------------------------------------------------------
# Appendix C evaluation (mirror of clip_eval.rs)
# ----------------------------------------------------------------

def _dot(a, b):
    total = 0.0
    for x, y in zip(a, b):
        total += x * y
    return total


def _lerp(a, b, t):
    return [x + (y - x) * t for x, y in zip(a, b)]


def _normalize(value):
    length = math.sqrt(_dot(value, value))
    if length == 0.0:
        return list(value)
    return [c / length for c in value]


def _slerp(q0, q1, t):
    b = list(q1)
    cosine = _dot(q0, b)
    if cosine < 0.0:
        b = [-c for c in b]
        cosine = -cosine
    if cosine > 0.9995:
        blended = _lerp(q0, b, t)
    else:
        theta = math.acos(cosine)
        sine = math.sin(theta)
        a_weight = math.sin((1.0 - t) * theta) / sine
        b_weight = math.sin(t * theta) / sine
        blended = [a_weight * x + b_weight * y for x, y in zip(q0, b)]
    return _normalize(blended)


def _hermite(p0, m0, p1, m1, delta, t):
    t2 = t * t
    t3 = t2 * t
    h00 = 2.0 * t3 - 3.0 * t2 + 1.0
    h10 = t3 - 2.0 * t2 + t
    h01 = -2.0 * t3 + 3.0 * t2
    h11 = t3 - t2
    return [
        h00 * x + h10 * delta * m0[i] + h01 * p1[i] + h11 * delta * m1[i]
        for i, x in enumerate(p0)
    ]


def _as_components(value):
    """A clip value in evaluation shape: scalar properties are
    bare numbers in the document, vec3/quaternion are arrays."""
    if isinstance(value, (int, float)):
        return [float(value)]
    return [float(c) for c in value]


def evaluate_channel(channel, frame):
    """One channel's value (component list) at an integer frame,
    exactly as clip_eval.rs `evaluate_channel` computes it."""
    keys = channel["keys"]
    prop = channel["target"]["property"]
    first = keys[0]
    if frame <= first["frame"]:
        return _as_components(first["value"])
    last = keys[-1]
    if frame >= last["frame"]:
        return _as_components(last["value"])
    # partition_point(|key| key.frame <= frame) - 1
    segment = 0
    for i, key in enumerate(keys):
        if key["frame"] <= frame:
            segment = i
        else:
            break
    k0 = keys[segment]
    if frame == k0["frame"]:
        return _as_components(k0["value"])
    k1 = keys[segment + 1]
    delta = float(k1["frame"] - k0["frame"])
    t = (frame - k0["frame"]) / delta
    interp = k0["interp"]
    if interp == "step":
        return _as_components(k0["value"])
    if interp == "linear":
        if prop == "rotation":
            return _slerp(_as_components(k0["value"]),
                          _as_components(k1["value"]), t)
        return _lerp(_as_components(k0["value"]),
                     _as_components(k1["value"]), t)
    # cubic
    m0 = _as_components(k0["out_tangent"])
    m1 = _as_components(k1["in_tangent"])
    value = _hermite(_as_components(k0["value"]), m0,
                     _as_components(k1["value"]), m1, delta, t)
    if prop == "rotation":
        return _normalize(value)
    return value


def frame_at(doc, tick):
    """The frame a witness tick maps to (clip_eval.rs frame_at)."""
    loop = doc.get("loop")
    if loop:
        span = loop["end"] - loop["start"] + 1
        return loop["start"] + (tick % span)
    start = doc["range"]["start"]
    end = doc["range"]["end"]
    frame = start + tick
    return end if frame > end else frame


def evaluate_all(doc, frame):
    """Every channel's value at one frame, in document order,
    as (target_id, components) pairs."""
    out = []
    for channel in doc["channels"]:
        target = channel["target"]
        target_id = "{}:{}:{}".format(
            target["kind"], target["name"], target["property"])
        out.append((target_id, evaluate_channel(channel, frame)))
    return out


def clip_digest(doc, from_tick, ticks):
    """The Tier 2 evaluation digest of clip_eval.rs `digest`:
    SHA-256 over per-tick f64 little-endian bit patterns."""
    hasher = hashlib.sha256()
    for offset in range(ticks):
        frame = frame_at(doc, from_tick + offset)
        for _target_id, value in evaluate_all(doc, frame):
            for component in value:
                hasher.update(struct.pack("<d", component))
    return hasher.hexdigest()


def loop_closure(doc):
    """Per-channel loop closure (clip_eval.rs loop_closure):
    max abs component difference between loop start and end
    values, rotation sign-aligned. None for a non-looping clip."""
    loop = doc.get("loop")
    if not loop:
        return None
    report = []
    for channel in doc["channels"]:
        a = evaluate_channel(channel, loop["start"])
        b = evaluate_channel(channel, loop["end"])
        if channel["target"]["property"] == "rotation" and _dot(a, b) < 0.0:
            b = [-c for c in b]
        worst = 0.0
        for x, y in zip(a, b):
            diff = abs(x - y)
            if diff > worst:
                worst = diff
        target = channel["target"]
        report.append(("{}:{}:{}".format(
            target["kind"], target["name"], target["property"]), worst))
    return report


# ----------------------------------------------------------------
# Bezier <-> Hermite (DESIGN.md section 3.2)
# ----------------------------------------------------------------

def bezier_out_tangent(value, handle_y, next_value, next_handle_y, dt):
    """Leaving tangent of a Bezier segment, per component, from
    the opening key's right-handle y. (next_* unused for the
    leaving tangent; kept in the signature for symmetry with
    the entering tangent and for callers that validate both.)"""
    return 3.0 * (handle_y - value) / dt


def bezier_in_tangent(value, handle_y, dt):
    """Entering tangent of a Bezier segment, per component, from
    the closing key's left-handle y."""
    return 3.0 * (value - handle_y) / dt


def hermite_handle_right(frame, value, out_tangent, dt):
    """The (frame, value) right handle a Hermite leaving tangent
    writes back as: at the segment third, value + m*dt/3."""
    return (frame + dt / 3.0, value + out_tangent * dt / 3.0)


def hermite_handle_left(frame, value, in_tangent, dt):
    """The (frame, value) left handle a Hermite entering tangent
    writes back as: at the segment third, value - m*dt/3."""
    return (frame - dt / 3.0, value - in_tangent * dt / 3.0)
