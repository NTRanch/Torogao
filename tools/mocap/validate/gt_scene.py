"""Ground-truth scene for the mocap pipeline (run inside Blender).

Imports the canonical-base Prima GLB, authors a 90-frame action
with KNOWN joint curves (axis-angle deltas in glTF space, composed
through the hierarchy in numpy-free mathutils), exports per-frame
joint world positions as ground truth, and renders the video the
pipeline must recover the motion from.

Usage:
  blender -b --python gt_scene.py -- probe|full OUTDIR
"""
import json
import math
import os
import sys

import bpy
from mathutils import Matrix, Quaternion, Vector

GLB = ("/home/phaedrus/.local/share/pax/prima-canonical-base-20261124/"
       "base-template-v2/export/prima-rung6.glb")
RIG_JSON = ("/home/phaedrus/.local/share/pax/mocap-pipeline-20261127/"
            "wt/tools/mocap/retarget/canonical162.json")
FRAMES = 90
FPS = 30

# glTF -> Blender coordinate conversion candidates; the script
# verifies against the rig's rest positions and keeps the match.
M_A = Matrix(((1, 0, 0, 0), (0, 0, -1, 0), (0, 1, 0, 0), (0, 0, 0, 1)))
M_B = Matrix(((1, 0, 0, 0), (0, 0, 1, 0), (0, -1, 0, 0), (0, 0, 0, 1)))


def authored_angles(f):
    """Joint deltas (degrees) at frame f. Axes are glTF-space:
    X subject-left, Y up, Z forward. Returns {joint: (axis, deg)}."""
    t = f / FPS
    s1 = math.sin(2 * math.pi * 0.5 * t)
    s2 = math.sin(2 * math.pi * 0.5 * t + math.pi)
    s3 = math.sin(2 * math.pi * 0.5 * t + math.pi / 2)
    sq = math.sin(2 * math.pi * 0.25 * t)
    return {
        "mixamorig:Spine": ((1, 0, 0), 2.0 * s1),
        "mixamorig:Spine1": ((1, 0, 0), 2.0 * s1),
        "mixamorig:Spine2": ((1, 0, 0), 2.0 * s1),
        "mixamorig:Head": ((0, 1, 0), 18.0 * sq),
        "mixamorig:LeftArm": ((0, 0, 1), 10.0 * s1),
        "mixamorig:LeftForeArm": ((0, 0, 1), 45.0 + 35.0 * s1),
        "mixamorig:RightForeArm": ((0, 0, 1), 45.0 + 35.0 * s2),
        "mixamorig:LeftLeg": ((1, 0, 0), 22.0 + 16.0 * s1),
        "mixamorig:RightLeg": ((1, 0, 0), 22.0 + 16.0 * s3),
    }


ANIMATED = list(authored_angles(0).keys())
DRIVEN = (["mixamorig:Hips", "mixamorig:Spine", "mixamorig:Spine1",
           "mixamorig:Spine2", "mixamorig:Neck", "mixamorig:Head"] +
          [f"mixamorig:{s}{j}" for s in ("Left", "Right")
           for j in ("Shoulder", "Arm", "ForeArm", "Hand",
                     "UpLeg", "Leg", "Foot")])


def main():
    mode = sys.argv[sys.argv.index("--") + 1]
    outdir = sys.argv[sys.argv.index("--") + 2]
    os.makedirs(outdir, exist_ok=True)
    bpy.ops.wm.read_factory_settings(use_empty=True)
    bpy.ops.import_scene.gltf(filepath=GLB)
    rig = json.load(open(RIG_JSON))
    rest = rig["rest"]
    arm = next(o for o in bpy.context.scene.objects if o.type == "ARMATURE")
    # verify the coordinate conversion against rest positions
    M = None
    for cand, nm in ((M_A, "A"), (M_B, "B")):
        ok = True
        for jn in ("mixamorig:Hips", "mixamorig:Head", "mixamorig:LeftFoot"):
            want = cand @ Vector(rest[jn]["pos"])
            got = arm.data.bones[jn].head_local
            if (want - got).length > 2e-3:
                ok = False
        if ok:
            M, mname = cand, nm
            break
    assert M is not None, "no coordinate conversion matched the rig rest pose"
    print(f"coordinate conversion: candidate {mname}")
    M3 = M.to_3x3()
    M3i = M3.inverted()

    bones = arm.data.bones
    parent = {h["name"]: h["parent"] for h in rig["hierarchy"]}
    rest_world = {jn: bones[jn].matrix_local.to_3x3() for jn in bones.keys()
                  if jn in rest}
    rest_local = {}
    for jn, R in rest_world.items():
        p = parent.get(jn)
        rest_local[jn] = (rest_world[p].inverted() @ R) if p in rest_world else R

    # Each animated joint's delta is defined in rig (glTF) space
    # against its rest world orientation. Blender composes the
    # hierarchy itself; this script only computes the local pose
    # deltas, via an explicit FK walk (frame_worlds) that mirrors
    # the composition so the exported ground truth is exact.
    pose = arm.pose
    for pb in pose.bones:
        pb.rotation_mode = "QUATERNION"
    scene = bpy.context.scene
    scene.frame_start, scene.frame_end = 0, FRAMES - 1
    scene.render.fps = FPS

    # world rotation of every bone under the authored pose, computed
    # by an explicit FK walk in hierarchy order (parents first).
    order = [h["name"] for h in rig["hierarchy"]]

    def frame_worlds(f):
        W = {}
        for jn in order:
            if jn not in rest_world:
                continue
            p = parent.get(jn)
            Wp = W[p] if p in W else Matrix.Identity(3)
            ang = authored_angles(f).get(jn)
            if ang is None:
                W[jn] = Wp @ rest_local[jn]
            else:
                axis_g, deg = ang
                q_g = Quaternion(Vector(axis_g), math.radians(deg))
                R_b = M3 @ q_g.to_matrix() @ M3i
                W_rest = rest_world[jn]
                # delta in rig space about the joint: posed world =
                # parent_pose @ (parent_rest^-1 @ delta @ parent_rest)
                # @ rest_local -- for the shallow animated chains in
                # this scene the parent-rest conjugation is applied
                # exactly here:
                Wp_rest = rest_world[p] if p in rest_world else Matrix.Identity(3)
                delta_local = Wp_rest.inverted() @ R_b @ Wp_rest
                W[jn] = Wp @ delta_local @ rest_local[jn]
        return W

    gt = {"frames": [], "conversion": mname}
    frames_to_do = [0] if mode == "probe" else list(range(FRAMES))
    for f in frames_to_do:
        W = frame_worlds(f)
        for jn, Wj in W.items():
            p = parent.get(jn)
            Wp = W[p] if p in W else Matrix.Identity(3)
            L = Wp.inverted() @ Wj
            q_delta = (rest_local[jn].inverted() @ L).to_quaternion()
            pb = pose.bones[jn]
            pb.rotation_quaternion = (q_delta.w, q_delta.x, q_delta.y,
                                      q_delta.z)
            if mode == "full":
                pb.keyframe_insert("rotation_quaternion", frame=f)
        scene.frame_set(f)
        bpy.context.view_layer.update()
        if mode == "full":
            row = {}
            for jn in DRIVEN:
                pb = pose.bones[jn]
                pos_b = pb.matrix.translation
                pos_g = M3i @ pos_b
                row[jn] = [pos_g.x, pos_g.y, pos_g.z]
            gt["frames"].append(row)

    if mode == "probe":
        setup_render(scene, arm, M3i)
        scene.render.filepath = os.path.join(outdir, "probe.png")
        scene.frame_set(10)
        bpy.ops.render.render(write_still=True)
        print("probe rendered")
        return

    setup_render(scene, arm, M3i)
    scene.render.filepath = os.path.join(outdir, "f_")
    bpy.ops.render.render(animation=True)
    with open(os.path.join(outdir, "gt_positions.json"), "w") as fh:
        json.dump(gt, fh)
    print("GT scene done:", len(gt["frames"]), "frames")


def mannequin_materials():
    """GT instrument pass: the estate model's real materials are
    near-white and textureless in this export, and BlazePose's
    person detector (trained on photographs) does not fire on
    them at all (measured: 0/30 detections on the unmodified
    render). Like a mocap suit, the GT render therefore recolours
    the meshes by class -- skin tone / dark hair / deep garment --
    so the estimator can see the figure. The geometry, rig and
    motion are untouched."""
    def mat(name, rgb):
        m = bpy.data.materials.new(name)
        m.diffuse_color = (*rgb, 1.0)
        m.use_nodes = True
        bsdf = m.node_tree.nodes["Principled BSDF"]
        bsdf.inputs["Base Color"].default_value = (*rgb, 1.0)
        bsdf.inputs["Roughness"].default_value = 0.85
        return m
    skin = mat("gt_skin", (0.82, 0.58, 0.44))
    garment = mat("gt_garment", (0.10, 0.13, 0.38))
    hair = mat("gt_hair", (0.15, 0.09, 0.07))
    other = mat("gt_other", (0.45, 0.42, 0.40))
    hide_skirt = os.environ.get("GT_HIDE_SKIRT") == "1"
    for obj in bpy.context.scene.objects:
        if obj.type != "MESH":
            continue
        if hide_skirt and obj.name == "Prima_Garment_Skirt":
            # body-visible GT variant: same rig, same authored
            # motion, same exported ground truth -- only the
            # occluding garment is hidden so the estimator can see
            # the legs it is being scored on.
            obj.hide_render = True
            continue
        if obj.name.startswith("Prima_Body") or "Face" in obj.name:
            m = skin
        elif "Garment" in obj.name:
            m = garment
        elif "Hair" in obj.name:
            m = hair
        else:
            m = other
        obj.data.materials.clear()
        obj.data.materials.append(m)


def setup_render(scene, arm, M3i):
    mannequin_materials()
    scene.render.engine = "BLENDER_EEVEE"
    scene.render.resolution_x = 640
    scene.render.resolution_y = 960
    scene.render.resolution_percentage = 100
    scene.render.image_settings.file_format = "PNG"
    scene.eevee.taa_render_samples = 16
    world = bpy.data.worlds.get("World") or bpy.data.worlds.new("World")
    scene.world = world
    world.use_nodes = True
    bg = world.node_tree.nodes["Background"]
    bg.inputs[0].default_value = (0.32, 0.34, 0.40, 1.0)
    bg.inputs[1].default_value = 0.55
    # ground plane (Blender z = 0)
    bpy.ops.mesh.primitive_plane_add(size=30, location=(0, 0, 0))
    plane = bpy.context.active_object
    mat = bpy.data.materials.new("ground")
    mat.diffuse_color = (0.35, 0.35, 0.38, 1.0)
    plane.data.materials.append(mat)
    # sun
    bpy.ops.object.light_add(type="SUN", location=(2.5, -2.0, 4.0))
    sun = bpy.context.active_object
    sun.data.energy = 3.0
    sun.rotation_euler = (math.radians(50), 0, math.radians(30))
    # camera side via GT_CAM_SIDE (front = -Y, back = +Y); the
    # probe pass settles which way the imported figure faces.
    side = -1.0 if os.environ.get("GT_CAM_SIDE", "front") == "front" else 1.0
    bpy.ops.object.camera_add(location=(0.55, side * 2.95, 1.12))
    cam = bpy.context.active_object
    cam.data.lens = 40
    direction = Vector((0.0, 0.0, 0.92)) - cam.location
    cam.rotation_euler = direction.to_track_quat("-Z", "Y").to_euler()
    scene.camera = cam


if __name__ == "__main__":
    main()
