"""Torogao Bench — the estate's hand-animation bench for
Blender 5.3: the Blender Action <-> torogao-motion-clip/2
round-trip converter, pose-asset import/export on the same
converter, an evaluated NLA flatten, and the thin NLA layer
front over native NLA data.

Hand animation on this bench authors the D-1397 scene spec
(torogao-motion-clip/2) directly — there is no bypass format.
Freezing and validation belong to the estate gate tool
(motion-clip-check); the bench writes working documents.

Adopted stack (Owner's tooling ruling, 2026-11-27): Blender
5.3 core + the bundled Copy Global Transform + free AnimAll +
free Commotion. The bench itself depends on core only.
"""

import bpy

from . import nla_front, panel

_CLASSES = nla_front.CLASSES + panel.CLASSES


def _register_props():
    scene = bpy.types.Scene
    scene.torogao_armature = bpy.props.PointerProperty(
        name="Armature", type=bpy.types.Object)
    scene.torogao_body = bpy.props.PointerProperty(
        name="Body", type=bpy.types.Object)
    scene.torogao_hierarchy_path = bpy.props.StringProperty(
        name="Hierarchy Def", subtype="FILE_PATH",
        description="The rig's hierarchy definition; its SHA-256 "
                    "is the clip rig identity")
    scene.torogao_glb_path = bpy.props.StringProperty(
        name="Rig GLB", subtype="FILE_PATH",
        description="The rig's GLB of record; the converter "
                    "reads rest translations and the joint set "
                    "from it")
    scene.torogao_clip_name = bpy.props.StringProperty(
        name="Clip Name", default="lana/bench/clip")
    scene.torogao_session = bpy.props.StringProperty(
        name="Session", default="bench")
    scene.torogao_loop = bpy.props.BoolProperty(
        name="Loop over Scene Range", default=False)
    scene.torogao_freeze_bin = bpy.props.StringProperty(
        name="motion-clip-check", subtype="FILE_PATH")
    scene.torogao_nla_action = bpy.props.PointerProperty(
        name="Strip Action", type=bpy.types.Action)
    scene.torogao_nla_items = bpy.props.CollectionProperty(
        type=nla_front.TorogaoNLAItem)
    scene.torogao_nla_active_index = bpy.props.IntProperty(
        name="Active Strip", default=-1)


def _unregister_props():
    scene = bpy.types.Scene
    for name in ("torogao_armature", "torogao_body",
                 "torogao_hierarchy_path", "torogao_glb_path",
                 "torogao_clip_name", "torogao_session",
                 "torogao_loop", "torogao_freeze_bin",
                 "torogao_nla_action", "torogao_nla_items",
                 "torogao_nla_active_index"):
        if hasattr(scene, name):
            delattr(scene, name)


def register():
    for cls in _CLASSES:
        bpy.utils.register_class(cls)
    _register_props()


def unregister():
    _unregister_props()
    for cls in reversed(_CLASSES):
        bpy.utils.unregister_class(cls)
