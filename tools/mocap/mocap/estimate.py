"""Estimation stage: video file -> per-frame landmarks.

Primary estimator: MediaPipe PoseLandmarker (BlazePose GHUM),
Tasks API, IMAGE running mode — each frame is estimated statelessly,
so the estimator is a pure function of (frame, pinned model). The
VIDEO running mode's temporal tracking/smoothing is deliberately
not used: temporal behaviour belongs to the pipeline's own named
cleanup stage, where it is deterministic and inspectable.

Verified on primo (2026-11-27): two fresh landmarker instances on
the same frame produce byte-identical world landmarks (SHA-256 of
the float64 array). The model file is pinned by SHA-256 in config.

Optional same-stack auxiliaries: FaceLandmarker blendshapes and
HandLandmarker world landmarks, emitted as sidecar arrays for the
emit stage's parameter/finger channels.
"""
import hashlib
import json
import os

import cv2
import numpy as np


def _sha256(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def check_model(path, expected_sha):
    got = _sha256(path)
    if expected_sha and got != expected_sha:
        raise SystemExit(f"model pin mismatch: {path} sha256 {got} "
                         f"!= config {expected_sha}")
    return got


def frame_indices(src_fps, rate_num, rate_den, n_src):
    """Deterministic resampling: clip frame k takes the source frame
    nearest to k * src_fps / rate."""
    rate = rate_num / rate_den
    n_out = int(n_src * rate / src_fps)
    return [min(n_src - 1, int(round(k * src_fps / rate)))
            for k in range(n_out)]


def estimate(video_path, cfg, work_dir):
    """Run pose estimation. Returns (meta dict, arrays dict)."""
    import mediapipe as mp
    from mediapipe.tasks import python as mpt
    from mediapipe.tasks.python import vision

    model = cfg["pose_model"]
    sha = check_model(model, cfg.get("pose_model_sha256"))
    cap = cv2.VideoCapture(video_path)
    if not cap.isOpened():
        raise SystemExit(f"cannot open video: {video_path}")
    src_fps = cap.get(cv2.CAP_PROP_FPS)
    n_src = int(cap.get(cv2.CAP_PROP_FRAME_COUNT))
    W = int(cap.get(cv2.CAP_PROP_FRAME_WIDTH))
    H = int(cap.get(cv2.CAP_PROP_FRAME_HEIGHT))
    want = set(frame_indices(src_fps, cfg["rate_num"], cfg["rate_den"], n_src))
    opts = vision.PoseLandmarkerOptions(
        base_options=mpt.BaseOptions(model_asset_path=model),
        running_mode=vision.RunningMode.IMAGE, num_poses=1,
        min_pose_detection_confidence=cfg["min_detection_confidence"],
        min_pose_presence_confidence=cfg["min_presence_confidence"])
    lmk = vision.PoseLandmarker.create_from_options(opts)
    world, image, vis = [], [], []
    src_idx = []
    i = -1
    while True:
        ok, frame = cap.read()
        if not ok:
            break
        i += 1
        if i not in want:
            continue
        rgb = cv2.cvtColor(frame, cv2.COLOR_BGR2RGB)
        mp_img = mp.Image(image_format=mp.ImageFormat.SRGB, data=rgb)
        r = lmk.detect(mp_img)
        if r.pose_world_landmarks:
            wl = r.pose_world_landmarks[0]
            il = r.pose_landmarks[0]
            world.append([[p.x, p.y, p.z] for p in wl])
            image.append([[p.x, p.y] for p in il])
            vis.append([p.visibility for p in il])
        else:
            world.append(np.full((33, 3), np.nan))
            image.append(np.full((33, 2), np.nan))
            vis.append(np.zeros(33))
        src_idx.append(i)
    cap.release()
    lmk.close()
    world = np.array(world, dtype=np.float64)
    # World-frame conversion (named, deterministic): MediaPipe
    # world landmarks live in a y-DOWN frame (up is negative y --
    # measured on primo: a standing subject's nose sits at
    # y ~ -0.43 relative to the hip origin). The retarget solver
    # builds construction frames with cross products and assumes a
    # right-handed y-up frame; feeding y-down points mirrors every
    # constructed frame and produced a constant whole-body frame
    # error in the ground-truth loop (end-to-end 68 cm mean joint
    # error, collapsing to 22 cm with this conversion -- the
    # estimator's own noise level). Negating y converts the
    # left-handed y-down set into the right-handed y-up set the
    # rest of the pipeline works in.
    world[:, :, 1] *= -1.0
    meta = {"estimator": "mediapipe PoseLandmarker (BlazePose GHUM)",
            "model_file": os.path.basename(model), "model_sha256": sha,
            "running_mode": "IMAGE (stateless per frame)",
            "world_frame": "MediaPipe world landmarks converted "
                           "y-down -> y-up (y negated) at ingest",
            "src_fps": src_fps, "src_frames": n_src, "width": W,
            "height": H, "frames": len(world)}
    arrays = {"world": world,
              "image": np.array(image, dtype=np.float64),
              "visibility": np.array(vis, dtype=np.float64),
              "src_idx": np.array(src_idx, dtype=np.int64)}
    return meta, arrays


def estimate_face(video_path, cfg, src_indices):
    """FaceLandmarker blendshapes at the given source frame indices.
    Returns (N, K) float64 + the ordered blendshape names, or None."""
    import mediapipe as mp
    from mediapipe.tasks import python as mpt
    from mediapipe.tasks.python import vision

    model = cfg["face_model"]
    check_model(model, cfg.get("face_model_sha256"))
    opts = vision.FaceLandmarkerOptions(
        base_options=mpt.BaseOptions(model_asset_path=model),
        running_mode=vision.RunningMode.IMAGE, num_faces=1,
        output_face_blendshapes=True)
    lmk = vision.FaceLandmarker.create_from_options(opts)
    cap = cv2.VideoCapture(video_path)
    want = set(int(i) for i in src_indices)
    names, rows = None, []
    i = -1
    while True:
        ok, frame = cap.read()
        if not ok:
            break
        i += 1
        if i not in want:
            continue
        rgb = cv2.cvtColor(frame, cv2.COLOR_BGR2RGB)
        r = lmk.detect(mp.Image(image_format=mp.ImageFormat.SRGB, data=rgb))
        if r.face_blendshapes:
            cats = r.face_blendshapes[0]
            if names is None:
                names = [c.category_name for c in cats]
            rows.append([c.score for c in cats])
        else:
            rows.append(None)
    cap.release()
    lmk.close()
    if names is None:
        return None
    K = len(names)
    out = np.zeros((len(rows), K))
    for r_i, row in enumerate(rows):
        if row is not None:
            out[r_i] = row
        elif r_i > 0:
            out[r_i] = out[r_i - 1]
    return names, out


def estimate_hands(video_path, cfg, src_indices):
    """HandLandmarker world landmarks at the given source indices.
    Returns dict side -> (N,21,3) float64 (NaN where undetected)."""
    import mediapipe as mp
    from mediapipe.tasks import python as mpt
    from mediapipe.tasks.python import vision

    model = cfg["hand_model"]
    check_model(model, cfg.get("hand_model_sha256"))
    opts = vision.HandLandmarkerOptions(
        base_options=mpt.BaseOptions(model_asset_path=model),
        running_mode=vision.RunningMode.IMAGE, num_hands=2)
    lmk = vision.HandLandmarker.create_from_options(opts)
    cap = cv2.VideoCapture(video_path)
    want = {int(i): k for k, i in enumerate(src_indices)}
    res = {"Left": np.full((len(src_indices), 21, 3), np.nan),
           "Right": np.full((len(src_indices), 21, 3), np.nan)}
    i = -1
    while True:
        ok, frame = cap.read()
        if not ok:
            break
        i += 1
        if i not in want:
            continue
        rgb = cv2.cvtColor(frame, cv2.COLOR_BGR2RGB)
        r = lmk.detect(mp.Image(image_format=mp.ImageFormat.SRGB, data=rgb))
        for h_i, wl in enumerate(r.hand_world_landmarks):
            side = r.handedness[h_i][0].category_name
            # same y-down -> y-up world conversion as the pose stage
            res[side][want[i]] = [[p.x, -p.y, p.z] for p in wl]
    cap.release()
    lmk.close()
    return res


def save_stage(path, meta, arrays):
    np.savez_compressed(path, **arrays)
    with open(path + ".meta.json", "w") as f:
        json.dump(meta, f, indent=1, sort_keys=True)
