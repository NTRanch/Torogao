#!/usr/bin/env python3
"""Grease Pencil -> torogao-drawn-seq/1 exporter (drawn-clip/1 adaptation).

Takes the active scene of the .blend file Blender was opened with and
emits, into --out:

  drawn-seq.json      a torogao-drawn-seq/1 manifest: one track per
                      Grease Pencil layer (bottom first), each exposure
                      a solo render of that layer at the segment's first
                      frame, held for the segment's length in ticks.
  <track>__sNN.png    the per-segment layer renders the manifest names.
  blank.png           one fully transparent canvas, shared by every
                      leading/blank exposure (content identity dedupes).
  gp-provenance.json  sidecar: pins applied, warnings, segment tables.
  export-result.json  machine-readable verdict: ok | refused + codes.

Optionally --refs DIR also receives Blender's own full-scene renders,
one per tick (frameNNNN.png, tick-indexed), as playback ground truth.

Discipline (the drawn-clip law): anything Grease Pencil can express
that drawn-clip/1 cannot is a typed TG-GP-* refusal or a named
TG-GP-WARN-* flattening -- never a silent approximation. Render
settings that decide pixels are PINNED by this exporter (and every
override reported), so the colour-management / premultiplication /
scale trap family cannot open silently.

Modes: default is LAYERED (one track per GP layer). --flatten emits
a single "composite" track of full-scene renders at the union of all
layers' keyframe boundaries -- Blender's own composite, pixel-exact
by construction, at the price of per-track structure. Layered
playback reproduces Blender's composite everywhere except where
semi-transparent layers overlap: Blender composites in scene-linear
float and sRGB-encodes on save, while the format composites the
encoded bytes, so overlap interiors differ by a bounded, measured
amount (see ADAPTATION.md). Blend modes and layer masks refuse in
layered mode and flatten-with-warning in --flatten mode.

Run: blender --background scene.blend --python export_gp_drawn.py \
         -- --out DIR [--name NAME] [--refs DIR] [--no-loop] [--flatten]
Exit status is carried by export-result.json (Blender swallows
script exit codes in background mode); this script also prints the
verdict lines to stdout.
"""

import hashlib
import json
import os
import re
import sys

import bpy

RENDERABLE_FOREIGN = {
    "MESH", "CURVE", "SURFACE", "META", "FONT", "VOLUME",
    "POINT_CLOUD", "HAIR", "CURVES",
}


# ------------------------------------------------------------------ utils

def slugify(raw, fallback):
    slug = re.sub(r"[^a-z0-9]+", "-", raw.lower()).strip("-")
    return slug or fallback


def sanitize_clip_name(raw):
    name = re.sub(r"[^a-z0-9_-]+", "-", raw.lower()).strip("-")
    return (name or "gp-clip")[:64]


class Report:
    def __init__(self):
        self.refusals = []   # (code, message)
        self.warnings = []   # (code, message)
        self.pins = []       # "setting: old -> new"

    def refuse(self, code, message):
        self.refusals.append((code, message))

    def warn(self, code, message):
        self.warnings.append((code, message))

    def pin(self, setting, old, new):
        if old != new:
            self.pins.append(f"{setting}: {old!r} -> {new!r}")


# ------------------------------------------------------- settings pinning

def pin_render_settings(scene, report):
    """Force every pixel-deciding render setting; report each change."""
    r = scene.render
    saved = {
        "engine": r.engine,
        "resolution_percentage": r.resolution_percentage,
        "film_transparent": r.film_transparent,
        "file_format": r.image_settings.file_format,
        "color_mode": r.image_settings.color_mode,
        "color_depth": r.image_settings.color_depth,
        "compression": r.image_settings.compression,
        "use_compositing": r.use_compositing,
        "use_sequencer": r.use_sequencer,
        "dither_intensity": r.dither_intensity,
        "filepath": r.filepath,
        "view_transform": scene.view_settings.view_transform,
        "look": scene.view_settings.look,
        "exposure": scene.view_settings.exposure,
        "gamma": scene.view_settings.gamma,
        "display_device": scene.display_settings.display_device,
    }
    report.pin("render.engine", r.engine, "BLENDER_WORKBENCH")
    r.engine = "BLENDER_WORKBENCH"
    report.pin("render.resolution_percentage", r.resolution_percentage, 100)
    r.resolution_percentage = 100
    report.pin("render.film_transparent", r.film_transparent, True)
    r.film_transparent = True
    report.pin("render.image_settings.file_format",
               r.image_settings.file_format, "PNG")
    r.image_settings.file_format = "PNG"
    report.pin("render.image_settings.color_mode",
               r.image_settings.color_mode, "RGBA")
    r.image_settings.color_mode = "RGBA"
    report.pin("render.image_settings.color_depth",
               r.image_settings.color_depth, "8")
    r.image_settings.color_depth = "8"
    report.pin("render.use_compositing", r.use_compositing, False)
    r.use_compositing = False
    report.pin("render.use_sequencer", r.use_sequencer, False)
    r.use_sequencer = False
    report.pin("render.dither_intensity", r.dither_intensity, 0.0)
    r.dither_intensity = 0.0
    vs = scene.view_settings
    report.pin("view_settings.view_transform", vs.view_transform, "Standard")
    vs.view_transform = "Standard"
    report.pin("view_settings.look", vs.look, "None")
    vs.look = "None"
    report.pin("view_settings.exposure", vs.exposure, 0.0)
    vs.exposure = 0.0
    report.pin("view_settings.gamma", vs.gamma, 1.0)
    vs.gamma = 1.0
    report.pin("display_settings.display_device",
               scene.display_settings.display_device, "sRGB")
    scene.display_settings.display_device = "sRGB"
    return saved


def restore_render_settings(scene, saved):
    r = scene.render
    r.engine = saved["engine"]
    r.resolution_percentage = saved["resolution_percentage"]
    r.film_transparent = saved["film_transparent"]
    r.image_settings.file_format = saved["file_format"]
    r.image_settings.color_mode = saved["color_mode"]
    r.image_settings.color_depth = saved["color_depth"]
    r.image_settings.compression = saved["compression"]
    r.use_compositing = saved["use_compositing"]
    r.use_sequencer = saved["use_sequencer"]
    r.dither_intensity = saved["dither_intensity"]
    r.filepath = saved["filepath"]
    scene.view_settings.view_transform = saved["view_transform"]
    scene.view_settings.look = saved["look"]
    scene.view_settings.exposure = saved["exposure"]
    scene.view_settings.gamma = saved["gamma"]
    scene.display_settings.display_device = saved["display_device"]


# -------------------------------------------------------------- segments

def layer_segments(layer, start, end):
    """Maximal runs of scene frames sharing one in-force GP frame.

    Returns [(frame_object | None, first_scene_frame, hold), ...]
    covering [start, end] exactly. None = no keyframe yet (blank).
    """
    frames = sorted(layer.frames, key=lambda f: f.frame_number)
    segments = []
    for scene_frame in range(start, end + 1):
        in_force = None
        for candidate in frames:
            if candidate.frame_number <= scene_frame:
                in_force = candidate
            else:
                break
        key = id(in_force) if in_force is not None else None
        if segments and segments[-1][0] == key:
            segments[-1][3] += 1
        else:
            segments.append([key, in_force, scene_frame, 1])
    return [(seg[1], seg[2], seg[3]) for seg in segments]


def canonicalize_png(path):
    """Zero the RGB of fully transparent pixels in a rendered PNG.

    Blender's premultiplied->straight PNG conversion leaves colour
    residue under zero alpha; torogao-paint's composite normalises
    transparent pixels to (0,0,0,0). Canonicalising at export puts
    both sides in the same normal form. The step cannot change any
    visible pixel (alpha 0 is invisible under every composite), and
    the 8-bit -> float -> 8-bit round trip through Blender's image
    save is exact (proven on the witness frame: only residue pixels
    changed). Returns the number of pixels zeroed.
    """
    image = bpy.data.images.load(path)
    try:
        pixels = list(image.pixels)
        zeroed = 0
        for i in range(0, len(pixels), 4):
            if pixels[i + 3] == 0.0 and (
                    pixels[i] != 0.0 or pixels[i + 1] != 0.0
                    or pixels[i + 2] != 0.0):
                pixels[i] = pixels[i + 1] = pixels[i + 2] = 0.0
                zeroed += 1
        if zeroed:
            image.pixels = pixels
            image.filepath_raw = path
            image.file_format = "PNG"
            image.save()
        return zeroed
    finally:
        bpy.data.images.remove(image)


def union_segments(frame_numbers_per_layer, start, end):
    """Maximal runs of [start, end] where no layer's in-force
    drawing changes: the boundary set is every keyframe number in
    (start, end] across all layers."""
    bounds = {start, end + 1}
    for frame_numbers in frame_numbers_per_layer:
        for number in frame_numbers:
            if start < number <= end:
                bounds.add(number)
    points = sorted(bounds)
    return [(points[i], points[i + 1] - points[i])
            for i in range(len(points) - 1)]


def png_pixels_equal(path_a, path_b):
    """Exact decoded-pixel equality of two PNGs, via Blender."""
    img_a = bpy.data.images.load(path_a)
    try:
        img_b = bpy.data.images.load(path_b)
        try:
            return (img_a.size[0] == img_b.size[0]
                    and img_a.size[1] == img_b.size[1]
                    and list(img_a.pixels) == list(img_b.pixels))
        finally:
            bpy.data.images.remove(img_b)
    finally:
        bpy.data.images.remove(img_a)


# ------------------------------------------------------------------ main

def main():
    argv = sys.argv[sys.argv.index("--") + 1:]

    def arg_value(flag):
        return argv[argv.index(flag) + 1] if flag in argv else None

    out_dir = arg_value("--out")
    if not out_dir:
        print("TG-GP-REFUSAL TG-GP-USAGE: --out DIR is required")
        return
    os.makedirs(out_dir, exist_ok=True)
    refs_dir = arg_value("--refs")
    if refs_dir:
        os.makedirs(refs_dir, exist_ok=True)
    loop = "--no-loop" not in argv
    flatten = "--flatten" in argv

    report = Report()
    scene = bpy.context.scene
    view_objects = list(bpy.context.view_layer.objects)

    # ---- scene facts -------------------------------------------------
    if scene.camera is None:
        report.refuse("TG-GP-NO-CAMERA",
                      "the active scene has no camera; Blender cannot "
                      "render the stack")
    fps_eff = scene.render.fps / scene.render.fps_base
    fps = int(round(fps_eff))
    if abs(fps_eff - fps) > 1e-6 or not (1 <= fps <= 240):
        report.refuse("TG-GP-FPS",
                      f"effective fps {fps_eff} is not an integer in "
                      "1..=240; drawn-clip fps is integral and a "
                      "fractional rate would silently retime the clip")
    start, end = scene.frame_start, scene.frame_end
    ticks = end - start + 1
    if ticks < 1 or ticks > 100000:
        report.refuse("TG-GP-RANGE",
                      f"frame range {start}..{end} spans {ticks} ticks; "
                      "drawn-clip ranges span 1..=100000")
    width, height = scene.render.resolution_x, scene.render.resolution_y
    if not (1 <= width <= 8192 and 1 <= height <= 8192):
        report.refuse("TG-GP-CANVAS",
                      f"canvas {width}x{height} is outside 1..=8192")

    gp_objects = [o for o in view_objects if o.type == "GREASEPENCIL"]
    if not gp_objects:
        report.refuse("TG-GP-NO-GP",
                      "the active scene contains no Grease Pencil object")
    foreign = [o.name for o in view_objects
               if o.type in RENDERABLE_FOREIGN and not o.hide_render]
    if foreign:
        report.refuse("TG-GP-FOREIGN",
                      "visible non-Grease-Pencil renderable objects "
                      f"{foreign} would appear in Blender's composite "
                      "but in no drawn-clip track")
    if len(gp_objects) > 1:
        report.warn("TG-GP-WARN-OBJECT-ORDER",
                    "multiple Grease Pencil objects: cross-object "
                    "stacking follows view-layer object order at the "
                    "exporter's track order; depth-separated objects "
                    "are outside the track model")

    # ---- per-layer audit ----------------------------------------------
    # Track plan entries: dicts with object, layer, track_name, segments.
    plan = []
    used_names = set()
    for obj in gp_objects:
        if obj.hide_render:
            report.warn("TG-GP-WARN-HIDDEN-OBJECT",
                        f"Grease Pencil object {obj.name!r} has "
                        "hide_render set; Blender excludes it from the "
                        "composite, so the exporter emits no tracks "
                        "for it")
            continue
        if len(obj.modifiers) > 0:
            names = [m.name for m in obj.modifiers]
            report.warn("TG-GP-WARN-MODIFIER",
                        f"object {obj.name!r} carries modifiers "
                        f"{names}; they are flattened into the exported "
                        "pixels and have no drawn-clip representation")
        prefix = f"{obj.name}/" if len(gp_objects) > 1 else ""
        # Blender's GP layer list is already bottom-first (the
        # first-created layer is the stack bottom -- proven on the
        # witness scene, where reversing the list put the opaque
        # face fill over the eyes in playback). drawn-clip tracks
        # are bottom-first too, so the plan walks the list as-is.
        for layer in list(obj.data.layers):
            track_name = prefix + layer.name
            if track_name in used_names:
                report.refuse("TG-GP-TRACK-NAME",
                              f"duplicate track name {track_name!r}")
            used_names.add(track_name)
            if layer.blend_mode != "REGULAR":
                if flatten:
                    report.warn("TG-GP-WARN-BLEND-FLATTENED",
                                f"layer {track_name!r} blends with "
                                f"{layer.blend_mode}; in --flatten mode "
                                "Blender's own composite carries it, "
                                "with no per-track representation")
                else:
                    report.refuse("TG-GP-BLEND",
                                  f"layer {track_name!r} blends with "
                                  f"{layer.blend_mode}; drawn-clip/1 "
                                  "carries normal blending only")
            if layer.use_masks and len(layer.mask_layers) > 0:
                if flatten:
                    report.warn("TG-GP-WARN-MASK-FLATTENED",
                                f"layer {track_name!r} is masked by "
                                f"{[m.name for m in layer.mask_layers]}; "
                                "in --flatten mode Blender's own "
                                "composite carries it, with no "
                                "per-track representation")
                else:
                    report.refuse("TG-GP-MASK",
                                  f"layer {track_name!r} is masked by "
                                  f"{[m.name for m in layer.mask_layers]}; "
                                  "layer masks have no drawn-clip "
                                  "mapping")
            if layer.tint_factor != 0.0:
                report.warn("TG-GP-WARN-TINT",
                            f"layer {track_name!r} has tint_factor "
                            f"{layer.tint_factor}; the tint is baked "
                            "into the exported pixels")
            if layer.use_lights:
                report.warn("TG-GP-WARN-LIGHTS",
                            f"layer {track_name!r} uses scene lights; "
                            "the scene's lighting state is baked into "
                            "the exported pixels (with no light objects "
                            "present, Workbench shades lit strokes "
                            "black -- the exporter captures the scene "
                            "exactly as authored either way)")
            late = [f.frame_number for f in layer.frames
                    if f.frame_number > end]
            if late:
                report.warn("TG-GP-WARN-FRAME-CLIP",
                            f"layer {track_name!r} has keyframes past "
                            f"the range end ({late}); they are outside "
                            "the exported range and ignored")
            plan.append({
                "object": obj,
                "layer": layer,
                "name": track_name,
                "authored_hide": layer.hide,
                "segments": layer_segments(layer, start, end),
            })

    result = {
        "tool": "export_gp_drawn.py (torogao grease-pencil adaptation)",
        "blender": ".".join(str(v) for v in bpy.app.version),
        "blend_file": bpy.data.filepath,
        "scene": scene.name,
        "fps": fps,
        "range": {"from": 0, "to": ticks - 1},
        "canvas": {"width": width, "height": height},
    }
    if bpy.data.filepath and os.path.exists(bpy.data.filepath):
        with open(bpy.data.filepath, "rb") as handle:
            result["blend_sha256"] = hashlib.sha256(handle.read()).hexdigest()

    if report.refusals:
        result["status"] = "refused"
        result["refusals"] = [
            {"code": c, "message": m} for c, m in report.refusals]
        result["warnings"] = [
            {"code": c, "message": m} for c, m in report.warnings]
        with open(os.path.join(out_dir, "export-result.json"), "w") as fh:
            json.dump(result, fh, indent=2)
        for code, message in report.refusals:
            print(f"TG-GP-REFUSAL {code}: {message}")
        return

    # ---- render --------------------------------------------------------
    saved_settings = pin_render_settings(scene, report)
    # Visibility is keyed by (object name, layer name): GP layer
    # wrappers are not identity-stable across collection accesses,
    # so `is` matching silently never matches.
    saved_visibility = {
        (obj.name, layer.name): layer.hide
        for obj in gp_objects for layer in obj.data.layers
    }

    def restore_visibility():
        for obj in gp_objects:
            for layer in obj.data.layers:
                layer.hide = saved_visibility[(obj.name, layer.name)]
    saved_frame = scene.frame_current
    tracks_json = []
    provenance_tracks = []
    used_slugs = {}
    blank_name = "blank.png"
    blank_rendered = False
    try:
        def set_solo(target_obj_name, target_layer_name):
            for obj in gp_objects:
                for layer in obj.data.layers:
                    layer.hide = not (
                        obj.name == target_obj_name
                        and layer.name == target_layer_name)

        def set_all_hidden():
            for obj in gp_objects:
                for layer in obj.data.layers:
                    layer.hide = True

        canon_counts = {}

        def render_to(path):
            scene.render.filepath = path
            bpy.ops.render.render(write_still=True)
            canon_counts[os.path.basename(path)] = canonicalize_png(path)

        if flatten:
            restore_visibility()
            frame_numbers = [
                [f.frame_number for f in entry["layer"].frames]
                for entry in plan
            ]
            risky = any(
                len(entry["object"].modifiers) > 0
                or (entry["object"].animation_data
                    and entry["object"].animation_data.action)
                for entry in plan)
            exposures = []
            prov_segments = []
            for index, (first_frame, hold) in enumerate(
                    union_segments(frame_numbers, start, end)):
                file_name = f"composite__s{index:02d}.png"
                scene.frame_set(first_frame)
                render_to(os.path.join(out_dir, file_name))
                if hold >= 2 and risky:
                    probe_path = os.path.join(
                        out_dir, f".stability-composite-{index:02d}.png")
                    scene.frame_set(first_frame + hold - 1)
                    render_to(probe_path)
                    stable = png_pixels_equal(
                        os.path.join(out_dir, file_name), probe_path)
                    os.remove(probe_path)
                    if not stable:
                        report.refuse(
                            "TG-GP-TIME-VARYING",
                            f"composite segment {index} (scene frames "
                            f"{first_frame}..{first_frame + hold - 1}) "
                            "changes inside its hold under modifiers/"
                            "animation; a held exposure cannot carry "
                            "per-frame variation")
                exposures.append({"file": file_name, "hold": hold})
                prov_segments.append({
                    "file": file_name,
                    "first_scene_frame": first_frame,
                    "hold": hold,
                    "gp_frame_number": first_frame,
                })
            tracks_json.append({
                "name": "composite",
                "exposures": exposures,
            })
            provenance_tracks.append({
                "name": "composite",
                "object": None,
                "layer": None,
                "segments": prov_segments,
            })

        if not flatten:
            for entry in plan:
                layer = entry["layer"]
                slug = slugify(entry["name"], "track")
                if slug in used_slugs:
                    used_slugs[slug] += 1
                    slug = f"{slug}-{used_slugs[slug]}"
                else:
                    used_slugs[slug] = 1
                exposures = []
                prov_segments = []
                for index, (frame_obj, first_frame, hold) in enumerate(
                        entry["segments"]):
                    if frame_obj is None:
                        if not blank_rendered:
                            set_all_hidden()
                            scene.frame_set(start)
                            render_to(os.path.join(out_dir, blank_name))
                            blank_rendered = True
                        file_name = blank_name
                    else:
                        file_name = f"{slug}__s{index:02d}.png"
                        set_solo(entry["object"].name, layer.name)
                        scene.frame_set(first_frame)
                        render_to(os.path.join(out_dir, file_name))
                    if (hold >= 2 and frame_obj is not None
                            and (len(entry["object"].modifiers) > 0
                                 or (entry["object"].animation_data
                                     and entry["object"].animation_data.action))):
                        # Modifiers / object animation can vary a drawing
                        # *inside* a hold, which the exposure model cannot
                        # carry. Prove stability or refuse by name.
                        probe_path = os.path.join(
                            out_dir, f".stability-{slug}-{index:02d}.png")
                        scene.frame_set(first_frame + hold - 1)
                        render_to(probe_path)
                        stable = png_pixels_equal(
                            os.path.join(out_dir, file_name), probe_path)
                        os.remove(probe_path)
                        if not stable:
                            report.refuse(
                                "TG-GP-TIME-VARYING",
                                f"track {entry['name']!r} segment {index} "
                                f"(scene frames {first_frame}.."
                                f"{first_frame + hold - 1}) changes inside "
                                "its hold under modifiers/animation; a "
                                "held exposure cannot carry per-frame "
                                "variation")
                    exposures.append({"file": file_name, "hold": hold})
                    prov_segments.append({
                        "file": file_name,
                        "first_scene_frame": first_frame,
                        "hold": hold,
                        "gp_frame_number": (
                            frame_obj.frame_number if frame_obj else None),
                    })
                opacity = int(round(layer.opacity * 255.0))
                if abs(layer.opacity * 255.0 - opacity) > 1e-9:
                    report.warn("TG-GP-WARN-OPACITY",
                                f"layer {entry['name']!r} opacity "
                                f"{layer.opacity} quantises to {opacity}/255 "
                                "in the manifest")
                track = {"name": entry["name"]}
                if opacity != 255:
                    track["opacity"] = opacity
                if entry["authored_hide"]:
                    track["visible"] = False
                track["exposures"] = exposures
                tracks_json.append(track)
                provenance_tracks.append({
                    "name": entry["name"],
                    "object": entry["object"].name,
                    "layer": layer.name,
                    "segments": prov_segments,
                })

        if refs_dir:
            restore_visibility()
            for tick in range(ticks):
                scene.frame_set(start + tick)
                render_to(os.path.join(refs_dir, f"frame{tick:04d}.png"))
    finally:
        restore_visibility()
        scene.frame_set(saved_frame)
        restore_render_settings(scene, saved_settings)

    if report.refusals:
        result["status"] = "refused"
        result["refusals"] = [
            {"code": c, "message": m} for c, m in report.refusals]
        result["warnings"] = [
            {"code": c, "message": m} for c, m in report.warnings]
        with open(os.path.join(out_dir, "export-result.json"), "w") as fh:
            json.dump(result, fh, indent=2)
        for code, message in report.refusals:
            print(f"TG-GP-REFUSAL {code}: {message}")
        return

    # ---- emit ------------------------------------------------------------
    blend_stem = os.path.splitext(os.path.basename(bpy.data.filepath))[0]
    clip_name = sanitize_clip_name(arg_value("--name") or blend_stem)
    manifest = {
        "format": "torogao-drawn-seq/1",
        "name": clip_name,
        "fps": fps,
        "loop": loop,
        "range": {"from": 0, "to": ticks - 1},
        "tracks": tracks_json,
    }
    manifest_path = os.path.join(out_dir, "drawn-seq.json")
    with open(manifest_path, "w") as fh:
        json.dump(manifest, fh, indent=2)

    result["status"] = "ok"
    result["mode"] = "flattened" if flatten else "layered"
    result["transparent_pixels_zeroed"] = canon_counts
    result["name"] = clip_name
    result["manifest"] = manifest_path
    result["pins"] = report.pins
    result["warnings"] = [
        {"code": c, "message": m} for c, m in report.warnings]
    result["tracks"] = provenance_tracks
    with open(os.path.join(out_dir, "gp-provenance.json"), "w") as fh:
        json.dump(result, fh, indent=2)
    with open(os.path.join(out_dir, "export-result.json"), "w") as fh:
        json.dump(result, fh, indent=2)
    for pin in report.pins:
        print(f"TG-GP-PIN {pin}")
    for code, message in report.warnings:
        print(f"TG-GP-WARN {code}: {message}")
    print(f"TG-GP-OK exported {clip_name}: {len(tracks_json)} tracks, "
          f"{ticks} ticks @ {fps} fps -> {manifest_path}")


if __name__ == "__main__":
    main()
