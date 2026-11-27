"""Retarget BlazePose world landmarks onto the canonical 162-joint
hierarchy (mixamorig core) by analytic frame construction.

Method (deterministic, no learned components):
  For every driven joint j we build a construction rotation C_j(t)
  from landmark geometry rigidly attached to that bone (segment
  directions and bend-plane normals). The same construction applied
  to a synthetic landmark set derived from the rig's own rest pose
  gives C_j(rest). The world-space delta is
      dA_j = C_j(t) @ C_j(rest)^-1
  and the clip-local rotation follows from the rig's rest world
  rotations A_j (read from the GLB hierarchy definition):
      L_j = A_parent^-1 @ dA_parent^-1 @ dA_j @ A_j
  with L = dA @ A for the root (Hips). At rest this reduces to the
  rig's rest locals exactly, by construction.

Unobserved degrees of freedom are resolved by fixed rules, named
here and in the loss report: limb twist rides the bend-plane normal
of the two-bone chain; the torso is a single observed segment
(hip-mid to shoulder-mid) distributed over Spine/Spine1/Spine2 by
fixed weights; Neck takes a fixed share of the chest-to-head delta;
ToeBase, clavicle-independent finger spread, hair, cloth and twist
joints are not driven by body capture.
"""
import json
import numpy as np

from . import pose_math as pm

# BlazePose landmark indices
NOSE, L_EYE, R_EYE, L_EAR, R_EAR = 0, 2, 5, 7, 8
MOUTH_L, MOUTH_R = 9, 10
SHO_L, SHO_R, ELB_L, ELB_R, WRI_L, WRI_R = 11, 12, 13, 14, 15, 16
PIN_L, PIN_R, IDX_L, IDX_R = 17, 18, 19, 20
HIP_L, HIP_R, KNE_L, KNE_R, ANK_L, ANK_R = 23, 24, 25, 26, 27, 28
HEE_L, HEE_R, TOE_L, TOE_R = 29, 30, 31, 32


def synth_rest_landmarks(rest):
    """BlazePose-shaped (33,3) landmark set synthesised from the rig's
    rest joint positions, so the construction's rest value is measured
    through the identical code path as live landmarks."""
    P = np.zeros((33, 3))
    g = lambda name: np.array(rest[name]["pos"], dtype=np.float64)
    head = g("mixamorig:Head")
    eye_l, eye_r = g("mixamorig:LeftEye"), g("mixamorig:RightEye")
    left = pm.normalize(g("mixamorig:LeftArm") - g("mixamorig:RightArm"))
    up = pm.normalize(g("mixamorig:Spine2") - g("mixamorig:Hips"))
    fwd = pm.normalize(np.cross(up, left))  # face side, verified vs eye offset
    if float(np.dot((eye_l + eye_r) / 2 - head, fwd)) < 0:
        fwd = -fwd
    P[NOSE] = head + fwd * 0.105 + up * 0.01
    P[L_EYE], P[R_EYE] = eye_l, eye_r
    P[L_EAR] = head + left * 0.085 - fwd * 0.005
    P[R_EAR] = head - left * 0.085 - fwd * 0.005
    P[MOUTH_L] = head + fwd * 0.09 - up * 0.05 + left * 0.028
    P[MOUTH_R] = head + fwd * 0.09 - up * 0.05 - left * 0.028
    for idx, jn in [
        (SHO_L, "mixamorig:LeftArm"), (SHO_R, "mixamorig:RightArm"),
        (ELB_L, "mixamorig:LeftForeArm"), (ELB_R, "mixamorig:RightForeArm"),
        (WRI_L, "mixamorig:LeftHand"), (WRI_R, "mixamorig:RightHand"),
        (PIN_L, "mixamorig:LeftHandPinky1"), (PIN_R, "mixamorig:RightHandPinky1"),
        (IDX_L, "mixamorig:LeftHandIndex1"), (IDX_R, "mixamorig:RightHandIndex1"),
        (HIP_L, "mixamorig:LeftUpLeg"), (HIP_R, "mixamorig:RightUpLeg"),
        (KNE_L, "mixamorig:LeftLeg"), (KNE_R, "mixamorig:RightLeg"),
        (ANK_L, "mixamorig:LeftFoot"), (ANK_R, "mixamorig:RightFoot"),
        (TOE_L, "mixamorig:LeftToeBase"), (TOE_R, "mixamorig:RightToeBase"),
    ]:
        P[idx] = g(jn)
    P[HEE_L] = g("mixamorig:LeftFoot") - fwd * 0.055 - up * 0.005
    P[HEE_R] = g("mixamorig:RightFoot") - fwd * 0.055 - up * 0.005
    # thumb points: unused by the body solver; set to the hand joints
    P[21], P[22] = g("mixamorig:LeftHandThumb1"), g("mixamorig:RightHandThumb1")
    P[1], P[3], P[4], P[6] = eye_l, eye_l, eye_r, eye_r
    return P


def _bend_normal(d1, d2, fallback):
    n = np.cross(d1, d2)
    if np.linalg.norm(n) < 1e-6:
        return fallback
    return pm.normalize(n)


def construction_frames(P):
    """Per-joint construction rotations C_j for one frame of landmarks.
    Returns dict joint-name -> 3x3 matrix, plus the torso delta quats
    needed for the spine/neck distribution."""
    mid_hip = (P[HIP_L] + P[HIP_R]) / 2.0
    mid_sho = (P[SHO_L] + P[SHO_R]) / 2.0
    left_axis = P[HIP_L] - P[HIP_R]
    torso_dir = mid_sho - mid_hip
    hips = pm.frame_y_along(torso_dir, left_axis)
    torso_fwd = hips[:, 2]
    # chest frame: neck direction (shoulder-mid -> ear-mid), shoulder axis as ref
    ear_mid = (P[L_EAR] + P[R_EAR]) / 2.0
    chest = pm.frame_y_along(ear_mid - mid_sho, P[SHO_L] - P[SHO_R])
    # head frame: up from mouth-mid -> eye-mid, forward from ear-mid -> nose
    eye_mid = (P[L_EYE] + P[R_EYE]) / 2.0
    mouth_mid = (P[MOUTH_L] + P[MOUTH_R]) / 2.0
    head = pm.frame_y_along(eye_mid - mouth_mid, P[NOSE] - ear_mid)
    out = {"mixamorig:Hips": hips, "__chest__": chest, "mixamorig:Head": head}
    for side, (sho, elb, wri, pin, idx, hip, kne, ank, toe) in {
        "Left": (SHO_L, ELB_L, WRI_L, PIN_L, IDX_L, HIP_L, KNE_L, ANK_L, TOE_L),
        "Right": (SHO_R, ELB_R, WRI_R, PIN_R, IDX_R, HIP_R, KNE_R, ANK_R, TOE_R),
    }.items():
        ua = P[elb] - P[sho]
        fa = P[wri] - P[elb]
        bn = _bend_normal(ua, fa, -torso_fwd)
        out[f"mixamorig:{side}Shoulder"] = pm.frame_y_along(ua, torso_fwd)
        out[f"mixamorig:{side}Arm"] = pm.frame_y_along(ua, bn)
        out[f"mixamorig:{side}ForeArm"] = pm.frame_y_along(fa, bn)
        hand_dir = (P[idx] + P[pin]) / 2.0 - P[wri]
        palm_ref = np.cross(hand_dir, P[idx] - P[pin])
        out[f"mixamorig:{side}Hand"] = pm.frame_y_along(hand_dir, palm_ref)
        th = P[kne] - P[hip]
        sh = P[ank] - P[kne]
        bn_leg = _bend_normal(th, sh, torso_fwd)
        out[f"mixamorig:{side}UpLeg"] = pm.frame_y_along(th, bn_leg)
        out[f"mixamorig:{side}Leg"] = pm.frame_y_along(sh, bn_leg)
        out[f"mixamorig:{side}Foot"] = pm.frame_y_along(P[toe] - P[ank], hips[:, 1])
    return out


# spine distribution weights: cumulative share of the hips->chest delta
SPINE_STOPS = [("mixamorig:Spine", 0.30), ("mixamorig:Spine1", 0.62),
               ("mixamorig:Spine2", 1.00)]
NECK_SHARE = 0.45  # neck takes this share of the chest->head delta

DRIVEN = (["mixamorig:Hips", "mixamorig:Spine", "mixamorig:Spine1",
           "mixamorig:Spine2", "mixamorig:Neck", "mixamorig:Head"] +
          [f"mixamorig:{s}{j}" for s in ("Left", "Right")
           for j in ("Shoulder", "Arm", "ForeArm", "Hand",
                     "UpLeg", "Leg", "Foot")])


class Retargeter:
    def __init__(self, rig):
        self.rig = rig
        self.rest = rig["rest"]
        self.parent = {h["name"]: h["parent"] for h in rig["hierarchy"]}
        rest_lm = synth_rest_landmarks(self.rest)
        self.rest_frames = construction_frames(rest_lm)
        self.A = {name: pm.matrix_from_quat(np.array(d["quat_xyzw"]))
                  for name, d in self.rest.items()}
        # rest deltas for the distribution joints
        self.rest_chest_delta = self._delta("__chest__", self.rest_frames)

    def _delta(self, name, frames):
        C = frames[name]
        Cr = self.rest_frames[name]
        return C @ Cr.T

    def solve_frame(self, P):
        """Landmarks (33,3) -> {joint: local quaternion (x,y,z,w)}."""
        F = construction_frames(P)
        dA = {}
        for name in ("mixamorig:Hips", "mixamorig:Head",
                     "mixamorig:LeftShoulder", "mixamorig:RightShoulder",
                     "mixamorig:LeftArm", "mixamorig:RightArm",
                     "mixamorig:LeftForeArm", "mixamorig:RightForeArm",
                     "mixamorig:LeftHand", "mixamorig:RightHand",
                     "mixamorig:LeftUpLeg", "mixamorig:RightUpLeg",
                     "mixamorig:LeftLeg", "mixamorig:RightLeg",
                     "mixamorig:LeftFoot", "mixamorig:RightFoot"):
            dA[name] = self._delta(name, F)
        d_hips = dA["mixamorig:Hips"]
        d_chest = self._delta("__chest__", F)
        d_head = dA["mixamorig:Head"]
        q_hips = pm.quat_from_matrix(d_hips)
        q_chest = pm.quat_from_matrix(d_chest)
        q_head = pm.quat_from_matrix(d_head)
        prev = q_hips
        for name, stop in SPINE_STOPS:
            q = pm.quat_slerp(q_hips, q_chest, stop)
            dA[name] = pm.matrix_from_quat(q)
            prev = q
        q_neck = pm.quat_slerp(q_chest, q_head, NECK_SHARE)
        dA["mixamorig:Neck"] = pm.matrix_from_quat(q_neck)
        out = {}
        for name in DRIVEN:
            A_j = self.A[name]
            if name == "mixamorig:Hips":
                L = dA[name] @ A_j
            else:
                p = self.parent[name]
                L = self.A[p].T @ dA[p].T @ dA[name] @ A_j
            out[name] = pm.quat_from_matrix(L)
        return out

    def solve(self, landmarks):
        """(N,33,3) -> {joint: (N,4) quaternions} with sign continuity."""
        per = {name: [] for name in DRIVEN}
        for P in landmarks:
            q = self.solve_frame(P)
            for name in DRIVEN:
                per[name].append(q[name])
        return {name: pm.sign_continuity(np.array(v)) for name, v in per.items()}


def load_rig(path):
    with open(path) as f:
        return json.load(f)
