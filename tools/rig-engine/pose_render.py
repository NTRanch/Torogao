# pose_render.py — pose-test / preview renders for the rig engine.
#
# Opens a gated blend, measures each requested morph's vertex
# displacement from the shape-key data itself (the authored
# delta: max |key - basis| over the Body mesh, times the
# applied weight — the Rule 10 artefact measurement), applies
# the full morph set, and renders a face close-up and a front
# full-body still in the face_stills bench pattern.
#
# Run: blender -b -Y --factory-startup --offline-mode -noaudio
#        -P pose_render.py -- <blend> <spec.json> <morphs.json>
#        <outdir> <prefix>
# morphs.json: [{"name": "Fac_Mth_Smile", "weight": 1.0}, ...]
# Writes <outdir>/result.json + <prefix>_face.png + <prefix>_front.png
import bpy, json, math, os, sys
from mathutils import Vector

argv = sys.argv[sys.argv.index("--") + 1:]
BLEND, SPEC_P, MORPHS_P, OUT, PREFIX = argv
os.makedirs(OUT, exist_ok=True)
spec = json.load(open(SPEC_P))
morphs = json.load(open(MORPHS_P))
ZS = spec.get("z_scale", 1.0)
eye_z = spec["face"]["eye_z"] * ZS

bpy.ops.wm.open_mainfile(filepath=BLEND)
body = next(o for o in bpy.data.objects
            if o.type == 'MESH' and o.name.endswith('_Body'))
keys = body.data.shape_keys.key_blocks
basis = keys['Basis']

measured = []
for m in morphs:
    name, weight = m["name"], float(m["weight"])
    if name not in keys:
        measured.append({"name": name, "weight": weight,
                         "max_displacement_m": 0.0, "missing": True})
        continue
    kb = keys[name]
    maxd = 0.0
    for i, v in enumerate(basis.data):
        d = (kb.data[i].co - v.co).length
        if d > maxd:
            maxd = d
    measured.append({"name": name, "weight": weight,
                     "max_displacement_m": maxd * abs(weight)})

# The measurements are the gate-relevant output: file them
# BEFORE any rendering, so a wedged GPU render cannot eat
# them (the engine reads this file even when the render
# step fails).
with open(os.path.join(OUT, 'result.json'), 'w') as fh:
    json.dump({"measured": measured}, fh)

for kb in keys:
    if kb.name != 'Basis':
        kb.value = 0.0
for m in morphs:
    if m["name"] in keys:
        keys[m["name"]].value = float(m["weight"])

for o in bpy.data.objects:
    if o.type == 'MESH':
        o.hide_render = ('OutfitEnd' in o.name)
        o.hide_viewport = o.hide_render

sc = bpy.context.scene
sc.render.engine = 'BLENDER_EEVEE'
sc.render.resolution_percentage = 100
sc.render.image_settings.file_format = 'PNG'

world = bpy.data.worlds.new("W")
sc.world = world
world.use_nodes = True
world.node_tree.nodes["Background"].inputs[0].default_value = (0.16, 0.15, 0.19, 1)
world.node_tree.nodes["Background"].inputs[1].default_value = 1.0

cam_d = bpy.data.cameras.new('cam')
cam_d.lens = 60
cam = bpy.data.objects.new('cam', cam_d)
sc.collection.objects.link(cam)
sc.camera = cam
for nm, loc, rot, e in (("sun", (1.5, -2.0, 3.0), (math.radians(55), 0, math.radians(-25)), 3.0),
                        ("fill", (-2.0, -1.0, 1.5), (math.radians(70), 0, math.radians(140)), 1.2)):
    ld = bpy.data.lights.new(nm, 'SUN')
    ld.energy = e
    lo = bpy.data.objects.new(nm, ld)
    lo.location = loc
    lo.rotation_euler = rot
    sc.collection.objects.link(lo)


def look_at(obj, target):
    obj.rotation_euler = (Vector(target) - obj.location).to_track_quat('-Z', 'Y').to_euler()


# Face close-up (the face_stills framing).
sc.render.resolution_x, sc.render.resolution_y = 900, 1080
cam.location = (0, -0.55, eye_z + 0.05)
look_at(cam, (0, -0.02, eye_z - 0.03))
sc.render.filepath = os.path.join(OUT, PREFIX + '_face.png')
bpy.ops.render.render(write_still=True)
print('SHOT face')

# Front full-body: frame the evaluated body bounds.
dg = bpy.context.evaluated_depsgraph_get()
ev = body.evaluated_get(dg)
zs = [v.co.z for v in ev.to_mesh().vertices]
ev.to_mesh_clear()
top = max(zs) if zs else 1.7
mid = top / 2.0
sc.render.resolution_x, sc.render.resolution_y = 768, 1152
cam_d.lens = 50
cam.location = (0, -2.4, mid + 0.1)
look_at(cam, (0, 0, mid))
sc.render.filepath = os.path.join(OUT, PREFIX + '_front.png')
bpy.ops.render.render(write_still=True)
print('SHOT front')

print('POSE DONE')
