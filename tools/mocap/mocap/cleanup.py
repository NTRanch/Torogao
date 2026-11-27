"""Deterministic cleanup stage for captured landmarks.

The estimator is a proposal source (D-1397 hybrid ruling): what
ships is capture + these named, fixed operations + hand finishing.
Every operation is a pure function of (landmarks, config) with the
parameters recorded in the run's config; there are no learned
components and no adaptive state. Operations, in order:

  1. confidence_gate  visibility < tau marks the sample missing.
  2. spike_reject     per-landmark speed above cap (m/s) marks the
                      sample missing.
  3. gap_fill         linear interpolation across gaps of at most
                      max_gap frames; longer gaps hold the last
                      valid sample and are listed in the report.
  4. savgol_smooth    Savitzky-Golay filter (window, poly) per
                      coordinate — a fixed-coefficient linear FIR.
  5. foot_pin         during detected ground contact (ankle height
                      within contact_eps of the clip ground), the
                      ankle's horizontal position is pinned to the
                      contact run's median, reducing foot skate.

The report dict counts what each stage touched; it becomes part of
the clip's loss report.
"""
import numpy as np
from scipy.signal import savgol_filter

from . import retarget as rt


def cleanup(world, visibility, cfg):
    """world: (N,33,3) float64 world landmarks; visibility: (N,33).
    Returns (cleaned (N,33,3), report dict)."""
    report = {"gated": 0, "spikes": 0, "filled": 0, "held_gaps": [],
              "foot_pinned_frames": 0}
    P = world.copy()
    N = len(P)
    missing = np.zeros((N, 33), dtype=bool)

    tau = cfg["confidence_tau"]
    bad = visibility < tau
    report["gated"] = int(bad.sum())
    missing |= bad

    cap = cfg["spike_cap_mps"] / cfg["rate"]  # metres per frame
    if N > 1:
        speed = np.linalg.norm(np.diff(P, axis=0), axis=2)  # (N-1,33)
        # a speed is only meaningful when both endpoint samples are
        # present; pairs touching a gated sample are skipped, so an
        # occlusion boundary cannot masquerade as a spike
        pair_ok = ~missing[1:] & ~missing[:-1]
        spike = np.zeros((N, 33), dtype=bool)
        spike[1:] = (speed > cap) & pair_ok
        report["spikes"] = int(spike.sum())
        missing |= spike

    # gap fill per landmark/coordinate
    idx = np.arange(N)
    for j in range(33):
        m = missing[:, j]
        if m.all():
            continue
        for c in range(3):
            col = P[:, j, c]
            valid = ~m
            if not valid.any():
                continue
            filled = np.interp(idx, idx[valid], col[valid])
            # hold (not interp) across gaps longer than max_gap
            runs = _true_runs(m)
            for (a, b) in runs:  # missing run [a, b)
                if b - a > cfg["max_gap"]:
                    src = a - 1 if a > 0 else b
                    filled[a:b] = col[src] if 0 <= src < N else filled[a:b]
                    report["held_gaps"].append([int(a), int(b), int(j)])
                else:
                    report["filled"] += (b - a)
            P[:, j, c] = filled

    w = cfg["savgol_window"]
    if N >= w and w >= 3:
        for j in range(33):
            for c in range(3):
                P[:, j, c] = savgol_filter(P[:, j, c], w, cfg["savgol_poly"],
                                           mode="interp")

    # foot pin: height axis is -y in MediaPipe world landmarks
    # (higher points carry smaller y; verified against the rig rest
    # calibration in the ground-truth loop).
    heights = -P[:, [rt.ANK_L, rt.ANK_R], 1]  # (N,2), larger = higher
    ground = float(np.percentile(heights, 2))
    eps = cfg["foot_contact_eps_m"]
    for k, ank in enumerate((rt.ANK_L, rt.ANK_R)):
        contact = heights[:, k] < ground + eps
        for (a, b) in _true_runs(contact):
            if b - a >= cfg["foot_pin_min_frames"]:
                med = np.median(P[a:b, ank, [0, 2]], axis=0)
                P[a:b, ank, 0] = med[0]
                P[a:b, ank, 2] = med[1]
                report["foot_pinned_frames"] += int(b - a)
    return P, report


def _true_runs(mask):
    runs = []
    i, n = 0, len(mask)
    while i < n:
        if mask[i]:
            j = i
            while j < n and mask[j]:
                j += 1
            runs.append((i, j))
            i = j
        else:
            i += 1
    return runs
