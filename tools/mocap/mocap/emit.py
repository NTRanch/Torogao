"""Emit stage: solved joint curves -> torogao-motion-clip/2 source
document (the DRAFT) + a loss report.

The draft is a working document in the format's terms: keys at
every frame, linear interpolation (the sampling loss is named in
the loss report), no freeze block. Production inserts hand/code
finishing between this draft and the freeze; the pipeline's own
freeze step exists to prove the draft passes the H0 bench.

Channels emitted:
  - rotation for every driven body joint (retarget.DRIVEN),
  - translation for mixamorig:Hips (weak-perspective unprojection
    of the image-space hip track; the z-plane assumption is a
    named loss),
  - rotation for finger joints when hand capture is enabled
    (flexion deltas about rest-calibrated axes; twist and spread
    are not observed — named loss),
  - scalar parameter channels `mocapface.<blendshape>` when face
    capture is enabled (ARKit-named blendshape scores awaiting
    binding to rig morph targets — named loss).
Everything not listed is not driven: hair, cloth, twist and eye
joints, ToeBase, and all morph targets keep their rest values.
"""
import numpy as np

from . import pose_math as pm
from . import retarget as rt

FACE_SUBSET = ["eyeBlinkLeft", "eyeBlinkRight", "eyeWideLeft",
               "eyeWideRight", "eyeSquintLeft", "eyeSquintRight",
               "jawOpen", "mouthSmileLeft", "mouthSmileRight",
               "mouthFunnel", "mouthPucker", "browInnerUp",
               "browDownLeft", "browDownRight", "cheekPuff"]

# MediaPipe hand landmark chains (wrist=0): joint landmarks per
# finger in chain order, tip last.
HAND_CHAINS = {
    "Thumb": [1, 2, 3, 4], "Index": [5, 6, 7, 8],
    "Middle": [9, 10, 11, 12], "Ring": [13, 14, 15, 16],
    "Pinky": [17, 18, 19, 20],
}


def _keys(values, fmt):
    keys = []
    n = len(values)
    for i, v in enumerate(values):
        keys.append({"frame": i,
                     "interp": "step" if i == n - 1 else "linear",
                     "key_type": "keyframe",
                     "value": fmt(v)})
    return keys


def _rot_keys(quats):
    return _keys(quats, lambda q: [float(c) for c in q])


def _angle(p_prev, p_cur, p_next):
    a = p_prev - p_cur
    b = p_next - p_cur
    na, nb = np.linalg.norm(a), np.linalg.norm(b)
    if na < 1e-9 or nb < 1e-9:
        return np.pi
    return float(np.arccos(np.clip(np.dot(a, b) / (na * nb), -1.0, 1.0)))


def finger_channels(hands, rig, report):
    """hands: side -> (N,21,3) hand world landmarks (NaN allowed).
    Flexion per finger joint from landmark triples; local rotation =
    axis-angle about the rest-calibrated flexion axis (in the
    joint's local frame) by (theta - theta_rest), composed with the
    rig rest local. Returns channel list."""
    rest = rig["rest"]
    channels = []
    for side in ("Left", "Right"):
        lm = hands.get(side)
        if lm is None:
            continue
        hand_j = f"mixamorig:{side}Hand"
        hand_pos = np.array(rest[hand_j]["pos"])
        idx_dir = np.array(rest[f"mixamorig:{side}HandIndex1"]["pos"]) - hand_pos
        pin_dir = np.array(rest[f"mixamorig:{side}HandPinky1"]["pos"]) - hand_pos
        palm_n = pm.normalize(np.cross(idx_dir, pin_dir))
        detected = int(np.sum(~np.isnan(lm[:, 0, 0])))
        report["hands"][side] = {"frames_detected": detected,
                                 "frames_total": int(len(lm))}
        if detected == 0:
            continue
        for finger, chain in HAND_CHAINS.items():
            rig_names = [f"mixamorig:{side}Hand{finger}{k}" for k in (1, 2, 3)]
            # rest angles + axes from rig positions
            pts = [hand_pos] + [np.array(rest[n]["pos"]) for n in rig_names]
            tip = pts[-1] + (pts[-1] - pts[-2])
            pts.append(tip)
            for ji, jname in enumerate(rig_names):
                theta_rest = _angle(pts[ji], pts[ji + 1], pts[ji + 2])
                bone_dir = pm.normalize(pts[ji + 2] - pts[ji + 1])
                a_world = pm.normalize(np.cross(bone_dir, palm_n))
                A_j = pm.matrix_from_quat(np.array(rest[jname]["quat_xyzw"]))
                a_local = A_j.T @ a_world
                parent = rig["hierarchy"]
                par = {h["name"]: h["parent"] for h in parent}[jname]
                A_p = pm.matrix_from_quat(np.array(rest[par]["quat_xyzw"]))
                L_rest = pm.quat_from_matrix(A_p.T @ A_j)
                thetas = []
                mp_chain = [0] + chain  # wrist + finger landmarks
                for f in range(len(lm)):
                    if np.isnan(lm[f, 0, 0]):
                        thetas.append(thetas[-1] if thetas else theta_rest)
                        continue
                    p = lm[f]
                    th = _angle(p[mp_chain[ji]], p[mp_chain[ji + 1]],
                                p[mp_chain[ji + 2]])
                    thetas.append(th)
                quats = []
                for th in thetas:
                    d = th - theta_rest
                    # flexion closes the angle; sign chosen so a
                    # closing angle rotates about +a_local
                    qd = np.array([a_local[0] * np.sin(d / 2),
                                   a_local[1] * np.sin(d / 2),
                                   a_local[2] * np.sin(d / 2),
                                   np.cos(d / 2)])
                    quats.append(pm.quat_mul(qd, L_rest))
                quats = pm.sign_continuity(np.array(quats))
                channels.append({
                    "target": {"kind": "joint", "name": jname,
                               "property": "rotation"},
                    "keys": _rot_keys(quats)})
    return channels


def hips_translation(image_lm, meta, cfg, rest_hips):
    """Weak-perspective unprojection of the image-space hip track.
    Scale: the subject's nose-to-ankle span in frame-height units,
    against cfg subject_height_m. z is held at the rest plane."""
    hip = (image_lm[:, rt.HIP_L] + image_lm[:, rt.HIP_R]) / 2.0
    nose = image_lm[:, rt.NOSE]
    ank = (image_lm[:, rt.ANK_L] + image_lm[:, rt.ANK_R]) / 2.0
    span = np.abs(nose[:, 1] - ank[:, 1])
    valid = ~np.isnan(span) & (span > 0.1)
    span_med = float(np.median(span[valid])) if valid.any() else 0.6
    dist = cfg["subject_height_m"] / span_med  # metres per frame height
    W, H = meta["width"], meta["height"]
    idx = np.arange(len(hip))
    out = np.zeros((len(hip), 3))
    base = None
    for c in range(2):
        col = hip[:, c]
        v = ~np.isnan(col)
        filled = np.interp(idx, idx[v], col[v]) if v.any() else np.zeros(len(hip))
        out[:, c] = filled
    dx = (out[:, 0] - out[0, 0]) * dist * (W / H)
    dy = -(out[:, 1] - out[0, 1]) * dist
    C = np.array(cfg["camera_rotation"], dtype=np.float64)
    disp = np.stack([dx, dy, np.zeros_like(dx)], axis=1) @ C.T
    return np.array(rest_hips, dtype=np.float64)[None, :] + disp


def build_clip(name, solved, trans, face, finger_chans, rig, cfg,
               op_entries):
    channels = []
    for jname in rt.DRIVEN:
        channels.append({"target": {"kind": "joint", "name": jname,
                                    "property": "rotation"},
                         "keys": _rot_keys(solved[jname])})
    channels.append({"target": {"kind": "joint", "name": "mixamorig:Hips",
                                "property": "translation"},
                     "keys": _keys(trans, lambda v: [float(c) for c in v])})
    channels.extend(finger_chans)
    if face is not None:
        names, vals = face
        for nm in FACE_SUBSET:
            if nm in names:
                col = vals[:, names.index(nm)]
                channels.append({
                    "target": {"kind": "parameter",
                               "name": f"mocapface.{nm}",
                               "property": "value"},
                    "keys": _keys(col, lambda v: float(v))})
    n = len(trans)
    doc = {"format": "torogao-motion-clip/2",
           "name": name,
           "rig": rig["hierarchy_sha256"],
           "rate": {"num": cfg["rate_num"], "den": cfg["rate_den"]},
           "range": {"start": 0, "end": n - 1},
           "loop": None,
           "channels": channels,
           "op_log": op_entries}
    return doc
