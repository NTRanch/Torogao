"""h2_render — the twin-render spot check for the H2 witness
GLB: import the exported GLB fresh, pose it at frame 24 (the
battery clip's deep pose), and render the same frame twice.
The two renders must agree under the estate twin criterion:
byte-identical after stripping the PNG Date/RenderTime
metadata chunks (the measured GPU noise floor discipline of
D-1397's refinement). Run:

  blender -b -Y --offline-mode -noaudio -P h2_render.py -- \
      <witness.glb> <out_dir>
"""

import os
import struct
import sys

import bpy
from mathutils import Vector


def point_camera(cam, target):
    direction = Vector(target) - cam.location
    cam.rotation_euler = direction.to_track_quat("-Z", "Y").to_euler()


def strip_png_chunks(data):
    """Remove tEXt/zTXt/iTXt/tIME chunks (Date, RenderTime and
    friends) from PNG bytes; keep everything else verbatim."""
    out = [data[:8]]
    pos = 8
    while pos < len(data):
        length = struct.unpack(">I", data[pos:pos + 4])[0]
        ctype = data[pos + 4:pos + 8]
        if ctype not in (b"tEXt", b"zTXt", b"iTXt", b"tIME"):
            out.append(data[pos:pos + 12 + length])
        pos += 12 + length
    return b"".join(out)


def main():
    argv = sys.argv[sys.argv.index("--") + 1:]
    glb_path, out_dir = argv
    os.makedirs(out_dir, exist_ok=True)
    bpy.ops.wm.read_factory_settings(use_empty=True)
    bpy.ops.import_scene.gltf(filepath=glb_path)
    scene = bpy.context.scene
    scene.render.engine = "BLENDER_EEVEE"
    scene.render.resolution_x = 320
    scene.render.resolution_y = 320
    scene.render.film_transparent = False
    world = bpy.data.worlds.new("W")
    world.use_nodes = True
    world.node_tree.nodes["Background"].inputs[0].default_value = \
        (0.25, 0.25, 0.28, 1.0)
    scene.world = world
    sun_data = bpy.data.lights.new("Sun", "SUN")
    sun_data.energy = 3.0
    sun = bpy.data.objects.new("Sun", sun_data)
    scene.collection.objects.link(sun)
    sun.rotation_euler = (0.9, 0.2, 0.4)
    cam_data = bpy.data.cameras.new("Cam")
    cam = bpy.data.objects.new("Cam", cam_data)
    scene.collection.objects.link(cam)
    cam.location = (0.4, -2.3, 1.15)
    point_camera(cam, (0.0, 0.0, 0.92))
    scene.camera = cam
    scene.frame_set(24)
    paths = []
    for i in (1, 2):
        path = os.path.join(out_dir, "spot_f24_{}.png".format(i))
        scene.render.filepath = path
        bpy.ops.render.render(write_still=True)
        paths.append(path)
    raw = [open(p, "rb").read() for p in paths]
    stripped = [strip_png_chunks(r) for r in raw]
    import hashlib
    result = {
        "raw_md5": [hashlib.md5(r).hexdigest() for r in raw],
        "stripped_md5": [hashlib.md5(s).hexdigest() for s in stripped],
        "raw_equal": raw[0] == raw[1],
        "stripped_equal": stripped[0] == stripped[1],
        "sizes": [len(r) for r in raw],
    }
    import json
    with open(os.path.join(out_dir, "twin_render.json"), "w") as fh:
        json.dump(result, fh, indent=2)
    print("TWIN RENDER:", json.dumps(result))
    sys.exit(0 if result["stripped_equal"] else 1)


if __name__ == "__main__":
    main()
