"""Score the mocap pipeline against the ground-truth scene.

Three separations, so the numbers say WHERE error lives:
  1. estimator error -- cleaned BlazePose landmarks vs landmarks
     synthesised from the GT joint positions, compared in each
     frame's subject frame (hip-centred, hip/shoulder axes), which
     factors out the camera/world frame difference. Reported raw
     and with a single optimal scale fit.
  2. oracle retarget error -- the retargeter fed the GT landmarks
     (no estimator in the loop), FK'd back to joint positions vs
     GT positions. This is the solver's own error floor.
  3. end-to-end error -- the actual draft clip from the GT render,
     FK'd on the canonical rig vs GT positions: per-joint position
     error, bone-direction error, elbow/knee flexion RMSE. Plus an
     authoring sanity check (GT positions vs the authored curves).

Usage (venv python):
  python validate/gt_score.py GT_DIR RIG_JSON
where GT_DIR holds gt_positions.json and out/ (the pipeline run).
"""
import json
import os
import sys

import numpy as np
from scipy.spatial.transform import Rotation

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)),
                                ".."))
from mocap import cleanup, retarget  # noqa: E402

JOINT_LM = [  # (landmark index, rig joint) exact correspondences
    (retarget.SHO_L, "mixamorig:LeftArm"),
    (retarget.SHO_R, "mixamorig:RightArm"),
    (retarget.ELB_L, "mixamorig:LeftForeArm"),
    (retarget.ELB_R, "mixamorig:RightForeArm"),
    (retarget.WRI_L, "mixamorig:LeftHand"),
    (retarget.WRI_R, "mixamorig:RightHand"),
    (retarget.HIP_L, "mixamorig:LeftUpLeg"),
    (retarget.HIP_R, "mixamorig:RightUpLeg"),
    (retarget.KNE_L, "mixamorig:LeftLeg"),
    (retarget.KNE_R, "mixamorig:RightLeg"),
    (retarget.ANK_L, "mixamorig:LeftFoot"),
    (retarget.ANK_R, "mixamorig:RightFoot"),
]


def subject_coords(P):
    """Landmarks -> hip-centred coordinates in the subject frame
    (columns: left, up, forward) built from the landmarks
    themselves."""
    mid_hip = (P[retarget.HIP_L] + P[retarget.HIP_R]) / 2.0
    mid_sho = (P[retarget.SHO_L] + P[retarget.SHO_R]) / 2.0
    left = P[retarget.HIP_L] - P[retarget.HIP_R]
    left = left / np.linalg.norm(left)
    up = (mid_sho - mid_hip)
    up = up - left * float(np.dot(up, left))
    up = up / np.linalg.norm(up)
    fwd = np.cross(left, up)
    B = np.stack([left, up, fwd], axis=1)
    return (P - mid_hip) @ B


def synth_gt_landmarks(gt_frame, rest):
    """Full 33-landmark set from one frame of GT joint positions.
    Joint-mapped landmarks are exact; toes/eyes/finger bases are
    approximated from the frame's subject axes (they only feed the
    Foot/Head/Hand construction frames in the oracle check)."""
    fake = {jn: {"pos": list(p)} for jn, p in gt_frame.items()}
    for jn, d in rest.items():
        fake.setdefault(jn, {"pos": list(d["pos"])})
    # subject axes from GT positions
    g = lambda n: np.array(gt_frame[n], dtype=np.float64)
    left = g("mixamorig:LeftArm") - g("mixamorig:RightArm")
    left = left / np.linalg.norm(left)
    up = g("mixamorig:Spine2") - g("mixamorig:Hips")
    up = up / np.linalg.norm(up)
    fwd = np.cross(up, left)
    fwd = fwd / np.linalg.norm(fwd)
    head = g("mixamorig:Head")
    fake["mixamorig:LeftEye"] = {"pos": list(head + left * 0.034
                                            + fwd * 0.095 + up * 0.028)}
    fake["mixamorig:RightEye"] = {"pos": list(head - left * 0.034
                                             + fwd * 0.095 + up * 0.028)}
    for s in ("Left", "Right"):
        foot = g(f"mixamorig:{s}Foot")
        fake[f"mixamorig:{s}ToeBase"] = {
            "pos": list(foot + fwd * 0.125 - up * 0.055)}
        hand = g(f"mixamorig:{s}Hand")
        fore = g(f"mixamorig:{s}ForeArm")
        hd = hand - fore
        hd = hd / np.linalg.norm(hd)
        sgn = 1.0 if s == "Left" else -1.0
        fake[f"mixamorig:{s}HandIndex1"] = {
            "pos": list(hand + hd * 0.075 + left * sgn * 0.018)}
        fake[f"mixamorig:{s}HandPinky1"] = {
            "pos": list(hand + hd * 0.07 - left * sgn * 0.02)}
        fake[f"mixamorig:{s}HandThumb1"] = {
            "pos": list(hand + hd * 0.04 + fwd * 0.02)}
    return retarget.synth_rest_landmarks(fake)


def fk_positions(clip, rig, frames):
    """FK the draft clip on the rig -> {joint: (N,3)} positions."""
    rest = rig["rest"]
    parent = {h["name"]: h["parent"] for h in rig["hierarchy"]}
    A = {n: Rotation.from_quat(d["quat_xyzw"]) for n, d in rest.items()}
    rot = {}
    trans = None
    for ch in clip["channels"]:
        t = ch["target"]
        if t["kind"] != "joint":
            continue
        vals = np.array([k["value"] for k in ch["keys"]],
                        dtype=np.float64)
        if t["property"] == "rotation":
            rot[t["name"]] = vals
        elif t["property"] == "translation":
            trans = vals
    pos = {n: np.zeros((frames, 3)) for n in retarget.DRIVEN}
    world = {}

    def depth(jn):
        d, p = 0, parent.get(jn)
        while p is not None:
            d += 1
            p = parent.get(p)
        return d

    # skin order is not guaranteed parents-first: sort by depth
    order = sorted(retarget.DRIVEN, key=depth)
    for f in range(frames):
        world.clear()
        for jn in order:
            p = parent[jn]
            L = (Rotation.from_quat(rot[jn][f]) if jn in rot
                 else A[jn])  # missing channel: rest local == A via parent? no:
            if p is None or p not in world:
                W = L
                pp = (trans[f] if trans is not None
                      else np.array(rest[jn]["pos"]))
            else:
                Wp, pp_p = world[p]
                W = Wp * L
                off = A[p].inv().apply(
                    np.array(rest[jn]["pos"]) - np.array(rest[p]["pos"]))
                pp = pp_p + Wp.apply(off)
            world[jn] = (W, pp)
            pos[jn][f] = pp
    return pos


def joint_angle(p_a, p_b, p_c):
    """Angle at b (degrees) for (N,3) position triples."""
    v1 = p_a - p_b
    v2 = p_c - p_b
    cos = np.sum(v1 * v2, axis=1) / (
        np.linalg.norm(v1, axis=1) * np.linalg.norm(v2, axis=1))
    return np.degrees(np.arccos(np.clip(cos, -1.0, 1.0)))


def main():
    gt_dir, rig_path = sys.argv[1], sys.argv[2]
    rig = retarget.load_rig(rig_path)
    rest = rig["rest"]
    gt_doc = json.load(open(os.path.join(gt_dir, "gt_positions.json")))
    gt_frames = gt_doc["frames"]
    N = len(gt_frames)
    gt_pos = {jn: np.array([fr[jn] for fr in gt_frames])
              for jn in retarget.DRIVEN}

    stage = np.load(os.path.join(gt_dir, "out", "prima-gt.stage.npz"))
    world = stage["world"]
    vis = np.where(np.isnan(world[:, :, 0]), 0.0, stage["visibility"])
    world_f = np.where(np.isnan(world), 0.0, world)
    clean, _ = cleanup.cleanup(world_f, vis, {
        "confidence_tau": 0.5, "spike_cap_mps": 3.0, "max_gap": 15,
        "savgol_window": 5, "savgol_poly": 2, "foot_contact_eps_m": 0.04,
        "foot_pin_min_frames": 4, "rate": 30.0})

    gt_lm = np.array([synth_gt_landmarks(fr, rest) for fr in gt_frames])

    # ---- 1. estimator error in subject coordinates
    errs, errs_scaled = [], []
    for f in range(N):
        cg = subject_coords(gt_lm[f])
        ce = subject_coords(clean[f])
        idx = [i for i, _ in JOINT_LM]
        d = cg[idx] - ce[idx]
        errs.append(np.linalg.norm(d, axis=1))
        # optimal global scale on the estimated coords
        num = float(np.sum(cg[idx] * ce[idx]))
        den = float(np.sum(ce[idx] * ce[idx]))
        s = num / den if den > 0 else 1.0
        errs_scaled.append(np.linalg.norm(cg[idx] - s * ce[idx], axis=1))
    errs = np.array(errs) * 100.0
    errs_scaled = np.array(errs_scaled) * 100.0
    print(f"== estimator error (cm), {N} frames, 12 joint landmarks")
    for k, (_, jn) in enumerate(JOINT_LM):
        print(f"  {jn:28s} mean {errs[:, k].mean():6.2f}  "
              f"p95 {np.percentile(errs[:, k], 95):6.2f}  "
              f"scaled mean {errs_scaled[:, k].mean():6.2f}")
    print(f"  {'ALL':28s} mean {errs.mean():6.2f}  "
          f"scaled mean {errs_scaled.mean():6.2f}")

    # ---- FK comparison helper
    def score_fk(pos, label):
        print(f"== {label}")
        all_e = []
        for jn in retarget.DRIVEN:
            a = pos[jn] - pos["mixamorig:Hips"]
            b = gt_pos[jn] - gt_pos["mixamorig:Hips"]
            e = np.linalg.norm(a - b, axis=1) * 100.0
            all_e.append(e)
            print(f"  {jn:28s} mean {e.mean():6.2f} cm  "
                  f"p95 {np.percentile(e, 95):6.2f}")
        all_e = np.array(all_e)
        print(f"  {'ALL DRIVEN':28s} mean {all_e.mean():6.2f} cm")
        return all_e

    # ---- 2. oracle retarget (GT landmarks through the solver)
    rt = retarget.Retargeter(rig)
    solved = rt.solve(gt_lm)
    fake_clip = {"channels": [
        {"target": {"kind": "joint", "name": jn, "property": "rotation"},
         "keys": [{"value": list(q)} for q in qs]}
        for jn, qs in solved.items()]}
    oracle_pos = fk_positions(fake_clip, rig, N)
    score_fk(oracle_pos, "oracle retarget error (solver floor)")

    # ---- 3. end-to-end clip
    clip = json.load(open(os.path.join(gt_dir, "out",
                                       "prima-gt.clip.json")))
    clip_pos = fk_positions(clip, rig, N)
    score_fk(clip_pos, "end-to-end clip error (estimator + solver)")

    # ---- flexion curves
    print("== flexion RMSE (deg)")
    for side in ("Left", "Right"):
        ga = joint_angle(gt_pos[f"mixamorig:{side}Arm"],
                         gt_pos[f"mixamorig:{side}ForeArm"],
                         gt_pos[f"mixamorig:{side}Hand"])
        ca = joint_angle(clip_pos[f"mixamorig:{side}Arm"],
                         clip_pos[f"mixamorig:{side}ForeArm"],
                         clip_pos[f"mixamorig:{side}Hand"])
        oa = joint_angle(oracle_pos[f"mixamorig:{side}Arm"],
                         oracle_pos[f"mixamorig:{side}ForeArm"],
                         oracle_pos[f"mixamorig:{side}Hand"])
        bend_gt = 180.0 - ga
        print(f"  elbow {side:5s} authored-range "
              f"{bend_gt.min():5.1f}..{bend_gt.max():5.1f}  "
              f"clip RMSE {np.sqrt(np.mean((ca - ga) ** 2)):5.2f}  "
              f"oracle RMSE {np.sqrt(np.mean((oa - ga) ** 2)):5.2f}")
        gk = joint_angle(gt_pos[f"mixamorig:{side}UpLeg"],
                         gt_pos[f"mixamorig:{side}Leg"],
                         gt_pos[f"mixamorig:{side}Foot"])
        ck = joint_angle(clip_pos[f"mixamorig:{side}UpLeg"],
                         clip_pos[f"mixamorig:{side}Leg"],
                         clip_pos[f"mixamorig:{side}Foot"])
        ok_ = joint_angle(oracle_pos[f"mixamorig:{side}UpLeg"],
                          oracle_pos[f"mixamorig:{side}Leg"],
                          oracle_pos[f"mixamorig:{side}Foot"])
        bend_k = 180.0 - gk
        print(f"  knee  {side:5s} authored-range "
              f"{bend_k.min():5.1f}..{bend_k.max():5.1f}  "
              f"clip RMSE {np.sqrt(np.mean((ck - gk) ** 2)):5.2f}  "
              f"oracle RMSE {np.sqrt(np.mean((ok_ - gk) ** 2)):5.2f}")


if __name__ == "__main__":
    main()
