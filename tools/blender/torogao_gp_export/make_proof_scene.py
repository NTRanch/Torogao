#!/usr/bin/env python3
"""Author the Grease Pencil proof scene for the drawn-clip/1 adaptation.

Builds, programmatically and deterministically (no randomness), a small
held-base character sketch with an expression substitution:

  timeline  Blender frames 1..6 at 12 fps (clip ticks 0..5)
  Body      one drawing held all 6 frames (head, hair, torso, arms)
  Eyes      A (open) frames 1-3, B (closed) frames 4-5, A again frame 6
            -> exposures A held 3, B held 2, A held 1. The frame-6 drawing
            is a *separate* GP drawing with pixel-identical content, so the
            importer's content-identity dedupe is exercised, not assumed.
  Sparkle   no keyframe until frame 5 -> a leading blank exposure, then a
            held accent drawing for frames 5-6 (independent exposure).

Variants (for the exporter's refusal/warning gates):
  good       the scene as described
  bad-blend  the Eyes layer's blend_mode is ADD (must refuse: TG-GP-BLEND)
  modifier   a Noise modifier on the GP object (must warn + flatten:
             TG-GP-WARN-MODIFIER)

Run: blender --background --factory-startup --python make_proof_scene.py \
         -- --out proof.blend [--variant good|bad-blend|modifier]
"""

import math
import sys

import bpy


# ---------------------------------------------------------------- helpers

def circle_pts(cx, cy, rx, ry, n, phase=0.0):
    return [
        (cx + rx * math.cos(phase + 2.0 * math.pi * i / n),
         cy + ry * math.sin(phase + 2.0 * math.pi * i / n))
        for i in range(n)
    ]


def arc_pts(cx, cy, rx, ry, a0, a1, n):
    return [
        (cx + rx * math.cos(a0 + (a1 - a0) * i / (n - 1)),
         cy + ry * math.sin(a0 + (a1 - a0) * i / (n - 1)))
        for i in range(n)
    ]


def add_stroke(drawing, pts, material_index, radius, cyclic=False,
               fill=None):
    drawing.add_strokes([len(pts)])
    stroke = drawing.strokes[-1]
    stroke.material_index = material_index
    stroke.cyclic = cyclic
    if fill is not None:
        # GPv3 fill gate (probed on Blender 5.3.0): a stroke fills only
        # when fill_id != 0 -- add_strokes leaves it 0, i.e. "no fill",
        # whatever the material says. The fill colour lives on the
        # stroke, not the material. -1 = ungrouped fill.
        stroke.fill_id = -1
        stroke.fill_color = (fill[0], fill[1], fill[2], 1.0)
        stroke.fill_opacity = fill[3]
    for point, (x, y) in zip(stroke.points, pts):
        point.position = (x, y, 0.0)
        point.radius = radius
        point.opacity = 1.0
    drawing.tag_positions_changed()
    return stroke


def gp_material(name, stroke_rgba, fill_rgba):
    mat = bpy.data.materials.new(name)
    bpy.data.materials.create_gpencil_data(mat)
    style = mat.grease_pencil
    if stroke_rgba is None:
        style.show_stroke = False
    else:
        style.show_stroke = True
        style.color = stroke_rgba
    if fill_rgba is None:
        style.show_fill = False
    else:
        style.show_fill = True
        style.fill_color = fill_rgba
    return mat


# ------------------------------------------------------------- the drawings

INK = 0
SKIN = 1
HAIR = 2
TUNIC = 3
INK_FILL = 4
GOLD = 5
WHITE = 6


def draw_body(drawing):
    # Torso: a filled, slightly tapered shape (drawn first = underneath).
    torso = [(-0.30, 0.20), (-0.36, -0.20), (-0.40, -0.72),
             (0.40, -0.72), (0.36, -0.20), (0.30, 0.20)]
    add_stroke(drawing, torso, TUNIC, 0.007, cyclic=True,
               fill=(0.27, 0.47, 0.63, 1.0))
    # Arms: thick ink lines over the torso sides.
    add_stroke(drawing, [(-0.33, 0.02), (-0.44, -0.24), (-0.52, -0.46)],
               INK, 0.017)
    add_stroke(drawing, [(0.33, 0.02), (0.44, -0.24), (0.52, -0.46)],
               INK, 0.017)
    # Neck.
    neck = [(-0.07, 0.34), (0.07, 0.34), (0.07, 0.16), (-0.07, 0.16)]
    add_stroke(drawing, neck, SKIN, 0.006, cyclic=True,
               fill=(0.98, 0.79, 0.64, 1.0))
    # Head.
    add_stroke(drawing, circle_pts(0.0, 0.52, 0.36, 0.36, 48),
               SKIN, 0.007, cyclic=True, fill=(0.98, 0.79, 0.64, 1.0))
    # Hair cap: arc over the top of the head, closed by its chord.
    hair = arc_pts(0.0, 0.52, 0.375, 0.375, math.radians(18),
                   math.radians(162), 25)
    hair += [(0.10, 0.665), (-0.10, 0.665)]
    add_stroke(drawing, hair, HAIR, 0.007, cyclic=True,
               fill=(0.45, 0.26, 0.13, 1.0))
    # Mouth: a small calm arc.
    add_stroke(drawing, arc_pts(0.0, 0.415, 0.05, 0.03, math.radians(200),
                                math.radians(340), 9), INK, 0.005)


def draw_brows(drawing):
    for side in (-1.0, 1.0):
        pts = arc_pts(side * 0.13, 0.685, 0.052, 0.018,
                      math.radians(15), math.radians(165), 9)
        add_stroke(drawing, pts, INK, 0.006)


def draw_eyes_open(drawing):
    draw_brows(drawing)
    for side in (-1.0, 1.0):
        cx = side * 0.13
        add_stroke(drawing, circle_pts(cx, 0.565, 0.042, 0.068, 24),
                   INK_FILL, 0.004, cyclic=True,
                   fill=(0.10, 0.08, 0.07, 1.0))
        add_stroke(drawing, circle_pts(cx - 0.014, 0.587, 0.011, 0.011, 12),
                   WHITE, 0.003, cyclic=True, fill=(1.0, 1.0, 1.0, 1.0))


def draw_eyes_closed(drawing):
    draw_brows(drawing)
    for side in (-1.0, 1.0):
        cx = side * 0.13
        pts = arc_pts(cx, 0.575, 0.045, 0.022, math.radians(180),
                      math.radians(360), 11)
        add_stroke(drawing, pts, INK, 0.008)


def draw_sparkle(drawing):
    cx, cy = 0.56, 0.92
    star = []
    for i in range(8):
        ang = math.pi / 2.0 + i * math.pi / 4.0
        r = 0.075 if i % 2 == 0 else 0.028
        star.append((cx + r * math.cos(ang), cy + r * math.sin(ang)))
    add_stroke(drawing, star, GOLD, 0.003, cyclic=True,
               fill=(1.0, 0.76, 0.22, 1.0))
    for dx, dy in ((0.44, 0.78), (-0.52, 0.98)):
        diamond = [(dx, dy + 0.02), (dx + 0.02, dy), (dx, dy - 0.02),
                   (dx - 0.02, dy)]
        add_stroke(drawing, diamond, GOLD, 0.003, cyclic=True,
                   fill=(1.0, 0.76, 0.22, 1.0))


# ------------------------------------------------------------------- main

def main():
    argv = sys.argv[sys.argv.index("--") + 1:]
    out = argv[argv.index("--out") + 1]
    variant = "good"
    if "--variant" in argv:
        variant = argv[argv.index("--variant") + 1]

    bpy.ops.wm.read_factory_settings(use_empty=True)
    scene = bpy.context.scene
    scene.render.resolution_x = 640
    scene.render.resolution_y = 800
    scene.render.resolution_percentage = 100
    scene.render.fps = 12
    scene.render.fps_base = 1.0
    scene.frame_start = 1
    scene.frame_end = 6

    cam_data = bpy.data.cameras.new("Cam")
    cam_data.type = "ORTHO"
    cam_data.ortho_scale = 2.4
    cam = bpy.data.objects.new("Cam", cam_data)
    scene.collection.objects.link(cam)
    cam.location = (0.0, 0.0, 5.0)
    scene.camera = cam

    bpy.ops.object.grease_pencil_add(type="EMPTY")
    obj = bpy.context.active_object
    obj.name = "HeroGP"
    data = obj.data
    # The add operator seeds a default layer (with an empty frame);
    # the proof scene carries exactly the three authored layers.
    while len(data.layers) > 0:
        data.layers.remove(data.layers[0])

    ink = (0.10, 0.08, 0.07, 1.0)
    data.materials.append(gp_material("InkLine", ink, None))
    data.materials.append(gp_material(
        "Skin", (0.35, 0.22, 0.16, 1.0), (0.98, 0.79, 0.64, 1.0)))
    data.materials.append(gp_material(
        "Hair", (0.16, 0.09, 0.05, 1.0), (0.45, 0.26, 0.13, 1.0)))
    data.materials.append(gp_material(
        "Tunic", ink, (0.27, 0.47, 0.63, 1.0)))
    data.materials.append(gp_material("InkFill", None, ink))
    data.materials.append(gp_material("Gold", None, (1.0, 0.76, 0.22, 1.0)))
    data.materials.append(gp_material("White", None, (1.0, 1.0, 1.0, 1.0)))

    body = data.layers.new("Body")
    draw_body(body.frames.new(1).drawing)

    eyes = data.layers.new("Eyes")
    draw_eyes_open(eyes.frames.new(1).drawing)
    draw_eyes_closed(eyes.frames.new(4).drawing)
    draw_eyes_open(eyes.frames.new(6).drawing)

    sparkle = data.layers.new("Sparkle")
    draw_sparkle(sparkle.frames.new(5).drawing)

    # Flat authoring: lit layers would take their colour from scene
    # lighting (and shade black under Workbench with no lights), which
    # is not the drawn register. The exporter warns on lit layers.
    for layer in data.layers:
        layer.use_lights = False

    if variant == "bad-blend":
        eyes.blend_mode = "ADD"
    elif variant == "modifier":
        try:
            obj.modifiers.new("Wiggle", "GREASE_PENCIL_NOISE")
        except Exception as exc:  # type name differs across versions
            print("modifier variant failed:", exc)
            raise
    elif variant != "good":
        raise SystemExit(f"unknown variant {variant!r}")

    bpy.ops.wm.save_as_mainfile(filepath=out)
    print(f"PROOF-SCENE saved {out} variant={variant}")


if __name__ == "__main__":
    main()
