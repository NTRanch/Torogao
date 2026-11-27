"""Laplace reference check: does the capture see what the
measurement saw?

The Laplace hybrid study (~/workspace/laplace-hybrid-20261127/
STUDY.md) measured the corpus clips with pixel-motion analysis:
per-clip action-cycle periods, and in Ayaka a lower band lagging
the core band by ~167 ms (r = 0.76). This check runs the mocap
pipeline's pose stage over a segment of the same clips, builds
motion signals from the cleaned landmarks (core band = hips /
shoulders / elbows / wrists; lower band = ankles / toes), and
compares:
  * dominant autocorrelation period of the core signal
    (study: Ayaka 0.567 s, Serval 0.433 s; tolerance +/-15%)
  * cross-correlation lag of the lower band behind the core
    (study, Ayaka: ~+167 ms at r = 0.76; tolerance +/-80 ms)

Stated scope difference: the study's signals are whole-frame
pixel motion over TWO bodies; the pipeline tracks ONE body.
Period agreement is the claim being tested -- the partnered
bodies share the cycle. Amplitude ratios are not compared.

Usage (venv python):
  python validate/laplace_check.py VIDEO SS DUR NAME EXPECT_PERIOD_S [EXPECT_LAG_MS]
"""
import json
import os
import subprocess
import sys

import numpy as np

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)),
                                ".."))
from mocap import cleanup, estimate  # noqa: E402

CORE = [11, 12, 13, 14, 15, 16, 23, 24]
LOWER = [27, 28, 31, 32]
FPS = 30.0


def motion_signal(P, idx):
    d = np.linalg.norm(np.diff(P[:, idx, :], axis=0), axis=2)
    return d.mean(axis=1)


def autocorr_period(x, fps, lo=0.25, hi=0.95):
    x = x - x.mean()
    ac = np.correlate(x, x, "full")[len(x) - 1:]
    ac = ac / ac[0]
    a, b = int(lo * fps), int(hi * fps)
    k = a + int(np.argmax(ac[a:b]))
    return k / fps, float(ac[k])


def xcorr_lag(a, b, fps, max_lag=0.5):
    """Lag of b behind a (seconds) at peak normalised correlation."""
    a = a - a.mean()
    b = b - b.mean()
    n = min(len(a), len(b))
    a, b = a[:n], b[:n]
    best = (0.0, -2.0)
    m = int(max_lag * fps)
    for k in range(-m, m + 1):
        if k >= 0:
            x, y = a[:n - k or None], b[k:]
        else:
            x, y = a[-k:], b[:n + k or None]
        if len(x) < 10:
            continue
        r = float(np.dot(x, y) / (np.linalg.norm(x) * np.linalg.norm(y)))
        if r > best[1]:
            best = (k / fps, r)
    return best


def main():
    video, ss, dur, name = sys.argv[1], float(sys.argv[2]), float(
        sys.argv[3]), sys.argv[4]
    expect_p = float(sys.argv[5])
    expect_lag = float(sys.argv[6]) if len(sys.argv) > 6 else None
    seg = f"/tmp/laplace-{name}.mp4"
    subprocess.run(["ffmpeg", "-y", "-v", "error", "-ss", str(ss),
                    "-t", str(dur), "-i", video, "-c:v", "libx264",
                    "-pix_fmt", "yuv420p", seg], check=True)
    here = os.path.dirname(os.path.abspath(__file__))
    cfg = json.load(open(os.path.join(here, "..", "config",
                                      "default.json")))
    base = os.path.normpath(os.path.join(here, ".."))
    for k in ("pose_model", "face_model", "hand_model", "rig_file"):
        if k in cfg and not os.path.isabs(cfg[k]):
            cfg[k] = os.path.normpath(os.path.join(base, cfg[k]))
    meta, arrays = estimate.estimate(seg, cfg, "/tmp")
    world = arrays["world"]
    det = ~np.isnan(world[:, 0, 0])
    vis = np.where(np.isnan(world[:, :, 0]), 0.0, arrays["visibility"])
    wf = np.where(np.isnan(world), 0.0, world)
    clean, _ = cleanup.cleanup(wf, vis, {
        "confidence_tau": cfg["confidence_tau"],
        "spike_cap_mps": cfg["spike_cap_mps"], "max_gap": cfg["max_gap"],
        "savgol_window": cfg["savgol_window"],
        "savgol_poly": cfg["savgol_poly"],
        "foot_contact_eps_m": cfg["foot_contact_eps_m"],
        "foot_pin_min_frames": cfg["foot_pin_min_frames"],
        "rate": FPS})
    print(f"{name}: frames {meta['frames']}, detected "
          f"{int(det.sum())}/{len(det)} ({100.0 * det.mean():.0f}%)")
    core = motion_signal(clean, CORE)
    lower = motion_signal(clean, LOWER)
    period, r = autocorr_period(core, FPS)
    ok_p = abs(period - expect_p) / expect_p <= 0.15
    print(f"{name}: core cycle {period:.3f} s (autocorr r {r:.2f}) "
          f"vs study {expect_p:.3f} s -> "
          f"{'AGREE' if ok_p else 'DISAGREE'} "
          f"({100.0 * (period - expect_p) / expect_p:+.1f}%)")
    lag, rl = xcorr_lag(core, lower, FPS)
    print(f"{name}: lower-band lag {lag * 1000:+.0f} ms "
          f"(xcorr r {rl:.2f})"
          + (f" vs study {expect_lag:+.0f} ms -> "
             f"{'AGREE' if abs(lag * 1000 - expect_lag) <= 80 else 'DISAGREE'}"
             if expect_lag is not None else ""))


if __name__ == "__main__":
    main()
