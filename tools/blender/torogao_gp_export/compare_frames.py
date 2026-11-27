#!/usr/bin/env python3
"""Compare played-back frames against Blender's reference renders.

Usage: compare_frames.py PLAYED_DIR REFS_DIR OUT_JSON

Per frame: pixel count differing in any channel, count differing by
exactly 1 (rounding-class), and the maximum per-channel delta. The
verdict is pixel-exact only when every frame has zero differing
pixels.
"""

import json
import os
import sys

from PIL import Image


def main():
    played_dir, refs_dir, out_json = sys.argv[1], sys.argv[2], sys.argv[3]
    names = sorted(f for f in os.listdir(refs_dir) if f.endswith(".png"))
    frames = []
    total_diff = 0
    for name in names:
        ref = Image.open(os.path.join(refs_dir, name)).convert("RGBA")
        played_path = os.path.join(played_dir, name)
        if not os.path.exists(played_path):
            frames.append({"frame": name, "error": "played frame missing"})
            total_diff += -1
            continue
        played = Image.open(played_path).convert("RGBA")
        if played.size != ref.size:
            frames.append({"frame": name, "error":
                           f"size {played.size} != ref {ref.size}"})
            total_diff += -1
            continue
        ref_px = ref.tobytes()
        played_px = played.tobytes()
        diff_pixels = 0
        off_by_one = 0
        max_delta = 0
        npix = ref.size[0] * ref.size[1]
        for i in range(npix):
            base = i * 4
            pixel_max = 0
            for ch in range(4):
                delta = abs(ref_px[base + ch] - played_px[base + ch])
                if delta > pixel_max:
                    pixel_max = delta
            if pixel_max:
                diff_pixels += 1
                if pixel_max == 1:
                    off_by_one += 1
                if pixel_max > max_delta:
                    max_delta = pixel_max
        total_diff += diff_pixels
        frames.append({
            "frame": name,
            "pixels": npix,
            "differing_pixels": diff_pixels,
            "differing_by_exactly_1": off_by_one,
            "max_channel_delta": max_delta,
        })
    verdict = {
        "frames_compared": len(names),
        "total_differing_pixels": total_diff,
        "pixel_exact": total_diff == 0,
        "frames": frames,
    }
    with open(out_json, "w") as fh:
        json.dump(verdict, fh, indent=2)
    print(json.dumps({k: verdict[k] for k in
                      ("frames_compared", "total_differing_pixels",
                       "pixel_exact")}, indent=2))
    for frame in frames:
        print(frame)


if __name__ == "__main__":
    main()
