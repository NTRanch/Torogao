"""mocap pipeline CLI.

  python -m mocap.cli run    --video V --name N --out DIR [--config C]
  python -m mocap.cli freeze --clip CLIP.json --check-bin BIN

`run` executes estimate -> cleanup -> retarget -> emit and writes
<name>.clip.json (the draft), <name>.loss.json (the loss report)
and <name>.stage.npz (raw estimator output, for re-builds).
`freeze` validates the draft with the estate's motion-clip-check
(H0 bench) and freezes it, proving the draft is bench-clean.
"""
import argparse
import hashlib
import json
import os
import subprocess
import sys

import numpy as np

from . import cleanup, emit, estimate, retarget


def load_config(path):
    with open(path) as f:
        cfg = json.load(f)
    base = os.path.dirname(os.path.abspath(path))
    for k in ("pose_model", "face_model", "hand_model", "rig_file"):
        if k in cfg and not os.path.isabs(cfg[k]):
            cfg[k] = os.path.normpath(os.path.join(base, "..", cfg[k]))
    return cfg


def sha256_file(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def cmd_run(args):
    cfg = load_config(args.config)
    os.makedirs(args.out, exist_ok=True)
    rig = retarget.load_rig(cfg["rig_file"])
    meta, arrays = estimate.estimate(args.video, cfg, args.out)
    stage_path = os.path.join(args.out, f"{args.name}.stage.npz")
    estimate.save_stage(stage_path, meta, arrays)
    world = arrays["world"]
    # frames with no detection arrive as NaN: gate them through the
    # visibility channel (NaN -> visibility 0) before cleanup
    vis = np.where(np.isnan(world[:, :, 0]), 0.0, arrays["visibility"])
    world_filled = np.where(np.isnan(world), 0.0, world)
    clean, creport = cleanup.cleanup(world_filled, vis, {
        "confidence_tau": cfg["confidence_tau"],
        "spike_cap_mps": cfg["spike_cap_mps"], "max_gap": cfg["max_gap"],
        "savgol_window": cfg["savgol_window"], "savgol_poly": cfg["savgol_poly"],
        "foot_contact_eps_m": cfg["foot_contact_eps_m"],
        "foot_pin_min_frames": cfg["foot_pin_min_frames"],
        "rate": cfg["rate_num"] / cfg["rate_den"]})
    solver = retarget.Retargeter(rig)
    solved = solver.solve(clean)
    trans = emit.hips_translation(arrays["image"], meta, cfg,
                                  rig["rest"]["mixamorig:Hips"]["pos"])
    report = {"video": os.path.basename(args.video),
              "video_sha256": sha256_file(args.video),
              "estimator": meta, "cleanup": creport, "hands": {},
              "losses": [
                  "Hips translation z held at the rest plane "
                  "(weak-perspective unprojection; no depth track)",
                  "limb twist rides the two-bone bend-plane normal; "
                  "axial twist is not independently observed",
                  "torso observed as one segment, distributed over "
                  "Spine/Spine1/Spine2 by fixed weights 0.30/0.62/1.00",
                  "ToeBase, eyes, hair, cloth and twist joints are "
                  "not driven by body capture (rest values)",
                  "keys are per-frame linear samples of the cleaned "
                  "curves (sampling, named per DESIGN section 3.4)",
              ]}
    finger_chans = []
    if cfg.get("hands"):
        hands = estimate.estimate_hands(args.video, cfg, arrays["src_idx"])
        finger_chans = emit.finger_channels(hands, rig, report)
        report["losses"].append(
            "finger channels carry flexion only; spread and twist "
            "are not observed; thumb opposition is approximate")
    face = None
    if cfg.get("face"):
        got = estimate.estimate_face(args.video, cfg, arrays["src_idx"])
        if got is not None:
            face = got
            report["face"] = {"blendshapes": len(got[0]),
                              "channels_emitted": len(emit.FACE_SUBSET)}
            report["losses"].append(
                "face blendshapes ride as mocapface.* parameters "
                "awaiting binding to rig morph targets")
    ops = [
        {"seq": 0, "actor": "converter", "session": "mocap-pipeline/0.1.0",
         "op": f"estimate {meta['estimator']} model sha256 "
               f"{meta['model_sha256'][:16]} from video sha256 "
               f"{report['video_sha256'][:16]}"},
        {"seq": 1, "actor": "converter", "session": "mocap-pipeline/0.1.0",
         "op": "cleanup: confidence gate, spike reject, gap fill, "
               "Savitzky-Golay, foot pin (parameters in config)"},
        {"seq": 2, "actor": "converter", "session": "mocap-pipeline/0.1.0",
         "op": "retarget onto canonical162 by analytic frame "
               "construction; DRAFT - hand finishing required "
               "before production freeze"},
    ]
    doc = emit.build_clip(args.name, solved, trans, face, finger_chans,
                          rig, cfg, ops)
    clip_path = os.path.join(args.out, f"{args.name}.clip.json")
    with open(clip_path, "w") as f:
        json.dump(doc, f, sort_keys=True)
    report["channels"] = len(doc["channels"])
    report["frames"] = int(len(trans))
    report["driven_joints"] = len(retarget.DRIVEN)
    report["clip_sha256"] = sha256_file(clip_path)
    with open(os.path.join(args.out, f"{args.name}.loss.json"), "w") as f:
        json.dump(report, f, indent=1, sort_keys=True)
    print(f"draft clip: {clip_path} ({len(doc['channels'])} channels, "
          f"{len(trans)} frames)")
    print(f"clip sha256: {report['clip_sha256']}")
    return 0


def cmd_freeze(args):
    out = subprocess.run([args.check_bin, args.clip],
                         capture_output=True, text=True)
    print(out.stdout.strip())
    if out.returncode != 0:
        print(out.stderr.strip(), file=sys.stderr)
        return out.returncode
    frozen = args.clip.replace(".clip.json", ".frozen.clip.json")
    out = subprocess.run([args.check_bin, "--freeze",
                          "torogao-format motion_clip (mocap draft proof)",
                          "tier2-1e-6", args.clip],
                         capture_output=True, text=True)
    if out.returncode != 0:
        print(out.stderr.strip(), file=sys.stderr)
        return out.returncode
    with open(frozen, "w") as f:
        f.write(out.stdout)
    chk = subprocess.run([args.check_bin, frozen],
                         capture_output=True, text=True)
    print(chk.stdout.strip())
    if chk.returncode != 0:
        if "TG-CLIP-FREEZE-MISMATCH" in chk.stdout:
            # Known bench finding (tools/mocap/FINDING-torogao-format-f64.md):
            # torogao-format's canonical writer is not round-trip
            # stable for arbitrary f64 values, so a frozen document
            # carrying mocap-shaped numbers fails its own hash check
            # on revalidation from disk. The DRAFT validated clean
            # above; the defect is in the bench's float path, not in
            # the draft. Report distinctly; do not mask it.
            print("BENCH FINDING: frozen draft fails revalidation via "
                  "TG-CLIP-FREEZE-MISMATCH — torogao-format canonical "
                  "writer f64 instability (see "
                  "tools/mocap/FINDING-torogao-format-f64.md). The "
                  "draft itself is valid; freeze is blocked on the "
                  "bench fix.", file=sys.stderr)
            return 3
        print(chk.stderr.strip(), file=sys.stderr)
        return chk.returncode
    print(f"frozen draft: {frozen}")
    return 0


def main():
    ap = argparse.ArgumentParser(prog="mocap")
    sub = ap.add_subparsers(dest="cmd", required=True)
    here = os.path.dirname(os.path.abspath(__file__))
    default_cfg = os.path.join(here, "..", "config", "default.json")
    r = sub.add_parser("run")
    r.add_argument("--video", required=True)
    r.add_argument("--name", required=True)
    r.add_argument("--out", required=True)
    r.add_argument("--config", default=default_cfg)
    r.set_defaults(fn=cmd_run)
    f = sub.add_parser("freeze")
    f.add_argument("--clip", required=True)
    f.add_argument("--check-bin", required=True)
    f.set_defaults(fn=cmd_freeze)
    args = ap.parse_args()
    return args.fn(args)


if __name__ == "__main__":
    sys.exit(main())
