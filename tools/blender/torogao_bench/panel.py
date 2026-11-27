"""panel — the Torogao bench panel and its file operators.

The bench is the animator's surface for the round-trip
converter: pick the rig (armature + body), its hierarchy
definition (the rig identity) and its GLB of record (the rest
binding), then convert, freeze, and census without leaving
Blender. Freezing and validation belong to the estate gate
tool; the panel shells to it and reports its verdict, it does
not reimplement it.
"""

import os
import subprocess

import bpy

from . import census, clipdoc, converter


def _binding(context):
    scene = context.scene
    if not scene.torogao_glb_path:
        raise converter.ClipError("set the rig's GLB of record first")
    return converter.RigBinding(
        bpy.path.abspath(scene.torogao_glb_path))


def _rig(context):
    scene = context.scene
    if not scene.torogao_hierarchy_path:
        raise converter.ClipError("set the hierarchy definition first")
    return clipdoc.rig_identity(
        bpy.path.abspath(scene.torogao_hierarchy_path))


def _armature(context):
    arm = context.scene.torogao_armature
    if arm is None or arm.type != "ARMATURE":
        raise converter.ClipError("set the rig armature first")
    return arm


def _body(context):
    body = context.scene.torogao_body
    return body


class _FileOp:
    filepath: bpy.props.StringProperty(subtype="FILE_PATH")


class TOROGAO_OT_action_to_clip(bpy.types.Operator, _FileOp):
    bl_idname = "torogao.action_to_clip"
    bl_label = "Action -> Clip"
    bl_description = "Convert the active action (+ shape key action) to a working torogao-motion-clip/2 document"

    def execute(self, context):
        scene = context.scene
        try:
            doc, notes = converter.action_to_clip(
                _armature(context), _body(context), _binding(context),
                scene.torogao_clip_name, _rig(context),
                scene.torogao_session,
                loop=(scene.frame_start, scene.frame_end)
                if scene.torogao_loop else None)
        except converter.ClipError as error:
            self.report({"ERROR"}, str(error))
            return {"CANCELLED"}
        clipdoc.write_document(doc, bpy.path.abspath(self.filepath))
        for note in notes:
            self.report({"INFO"}, note)
        self.report({"INFO"}, "wrote {} ({} channels)".format(
            self.filepath, len(doc["channels"])))
        return {"FINISHED"}

    def invoke(self, context, event):
        context.window_manager.fileselect_add(self)
        return {"RUNNING_MODAL"}


class TOROGAO_OT_clip_to_action(bpy.types.Operator, _FileOp):
    bl_idname = "torogao.clip_to_action"
    bl_label = "Clip -> Action"
    bl_description = "Import a torogao-motion-clip/2 document as Blender actions on the rig"

    def execute(self, context):
        scene = context.scene
        try:
            doc = clipdoc.load_document(bpy.path.abspath(self.filepath))
            if doc["rig"] != _rig(context):
                raise converter.ClipError(
                    "clip is bound to rig {}, this bench is set "
                    "to rig {}".format(doc["rig"], _rig(context)))
            converter.clip_to_action(
                doc, _armature(context), _body(context),
                _binding(context), scene.torogao_session)
        except converter.ClipError as error:
            self.report({"ERROR"}, str(error))
            return {"CANCELLED"}
        self.report({"INFO"}, "imported clip {!r}".format(doc["name"]))
        return {"FINISHED"}

    def invoke(self, context, event):
        context.window_manager.fileselect_add(self)
        return {"RUNNING_MODAL"}


class TOROGAO_OT_flatten_to_clip(bpy.types.Operator, _FileOp):
    bl_idname = "torogao.flatten_to_clip"
    bl_label = "Flatten NLA -> Clip"
    bl_description = "Sample the evaluated pose (action + NLA stack) at every frame into a clip; the strip layout is recorded in the op log"

    def execute(self, context):
        scene = context.scene
        try:
            doc = converter.flatten_evaluated(
                _armature(context), _body(context), _binding(context),
                scene.torogao_clip_name, _rig(context),
                scene.torogao_session,
                int(scene.frame_start), int(scene.frame_end),
                loop=(int(scene.frame_start), int(scene.frame_end))
                if scene.torogao_loop else None)
        except converter.ClipError as error:
            self.report({"ERROR"}, str(error))
            return {"CANCELLED"}
        clipdoc.write_document(doc, bpy.path.abspath(self.filepath))
        self.report({"INFO"}, "wrote flattened clip ({} channels)"
                    .format(len(doc["channels"])))
        return {"FINISHED"}

    def invoke(self, context, event):
        context.window_manager.fileselect_add(self)
        return {"RUNNING_MODAL"}


class TOROGAO_OT_clip_to_pose(bpy.types.Operator, _FileOp):
    bl_idname = "torogao.clip_to_pose"
    bl_label = "Clip -> Pose Asset"
    bl_description = "Import a one-frame clip as a pose asset on the rig"

    def execute(self, context):
        scene = context.scene
        try:
            doc = clipdoc.load_document(bpy.path.abspath(self.filepath))
            converter.clip_to_pose_asset(
                doc, _armature(context), _body(context),
                _binding(context), scene.torogao_session)
        except converter.ClipError as error:
            self.report({"ERROR"}, str(error))
            return {"CANCELLED"}
        self.report({"INFO"}, "pose asset from {!r}".format(doc["name"]))
        return {"FINISHED"}

    def invoke(self, context, event):
        context.window_manager.fileselect_add(self)
        return {"RUNNING_MODAL"}


class TOROGAO_OT_freeze_clip(bpy.types.Operator):
    bl_idname = "torogao.freeze_clip"
    bl_label = "Freeze Clip (gate tool)"
    bl_description = "Validate and freeze a working clip with the estate motion-clip-check tool"

    filepath: bpy.props.StringProperty(subtype="FILE_PATH")

    def execute(self, context):
        scene = context.scene
        binary = bpy.path.abspath(scene.torogao_freeze_bin)
        if not binary or not os.path.exists(binary):
            self.report({"ERROR"}, "set the motion-clip-check binary path")
            return {"CANCELLED"}
        src = bpy.path.abspath(self.filepath)
        dst = src.replace(".clip.json", ".frozen.clip.json")
        # The gate tool's freeze grammar: --freeze <compiler>
        # <band> <file>, frozen canonical text on stdout. The
        # compiler stamp names the format's freeze
        # implementation of record (FORMATS.md).
        proc = subprocess.run(
            [binary, "--freeze",
             "torogao-format motion_clip h0 (2026-11-27)",
             "tier2-1e-6", src],
            capture_output=True)
        if proc.returncode != 0:
            self.report({"ERROR"}, "gate tool refused: {}".format(
                proc.stderr.decode("utf-8", "replace").strip()[:400]))
            return {"CANCELLED"}
        with open(dst, "wb") as handle:
            handle.write(proc.stdout)
        self.report({"INFO"}, "frozen -> {}".format(dst))
        return {"FINISHED"}

    def invoke(self, context, event):
        context.window_manager.fileselect_add(self)
        return {"RUNNING_MODAL"}


class TOROGAO_OT_export_glb_census(bpy.types.Operator, _FileOp):
    bl_idname = "torogao.export_glb_census"
    bl_label = "Export GLB + Census"
    bl_description = "Export the rig with its active animation as GLB and write the A10.2-shape census beside it"

    def execute(self, context):
        path = bpy.path.abspath(self.filepath)
        bpy.ops.export_scene.gltf(
            filepath=path, export_format="GLB",
            export_animations=True,
            export_animation_mode="ACTIVE_ACTIONS",
            export_frame_step=1)
        result = census.census_glb(path)
        import json
        with open(path + ".census.json", "w") as handle:
            json.dump(result, handle, indent=2)
        anim = result["animations"][0] if result["animations"] else {}
        self.report({"INFO"},
                    "GLB: {} joints, {} morphs, animation {!r} "
                    "{} channels".format(len(result["joints"]),
                                         len(result["morph_targets"]),
                                         anim.get("name"),
                                         anim.get("channels")))
        return {"FINISHED"}

    def invoke(self, context, event):
        context.window_manager.fileselect_add(self)
        return {"RUNNING_MODAL"}


class TOROGAO_PT_bench(bpy.types.Panel):
    bl_idname = "TOROGAO_PT_bench"
    bl_label = "Torogao Bench"
    bl_space_type = "VIEW_3D"
    bl_region_type = "UI"
    bl_category = "Torogao"

    def draw(self, context):
        layout = self.layout
        scene = context.scene
        box = layout.box()
        box.label(text="Rig")
        box.prop(scene, "torogao_armature")
        box.prop(scene, "torogao_body")
        box.prop(scene, "torogao_hierarchy_path")
        box.prop(scene, "torogao_glb_path")
        box = layout.box()
        box.label(text="Clip")
        box.prop(scene, "torogao_clip_name")
        box.prop(scene, "torogao_session")
        box.prop(scene, "torogao_loop")
        row = box.row(align=True)
        row.operator("torogao.action_to_clip")
        row.operator("torogao.clip_to_action")
        row = box.row(align=True)
        row.operator("torogao.flatten_to_clip")
        row.operator("torogao.clip_to_pose")
        box.prop(scene, "torogao_freeze_bin")
        box.operator("torogao.freeze_clip")
        box.operator("torogao.export_glb_census")
        box = layout.box()
        box.label(text="NLA Layers (native data)")
        box.prop(scene, "torogao_nla_action")
        row = box.row(align=True)
        row.operator("torogao.nla_refresh")
        row.operator("torogao.nla_add_strip")
        box.template_list("TOROGAO_UL_nla_strips", "",
                          scene, "torogao_nla_items",
                          scene, "torogao_nla_active_index", rows=4)
        row = box.row(align=True)
        row.operator("torogao.nla_select")
        row.operator("torogao.nla_remove_strip")
        row = box.row(align=True)
        row.operator("torogao.nla_strip_up")
        row.operator("torogao.nla_strip_down")
        row = box.row(align=True)
        row.operator("torogao.nla_track_up")
        row.operator("torogao.nla_track_down")


CLASSES = (
    TOROGAO_OT_action_to_clip,
    TOROGAO_OT_clip_to_action,
    TOROGAO_OT_flatten_to_clip,
    TOROGAO_OT_clip_to_pose,
    TOROGAO_OT_freeze_clip,
    TOROGAO_OT_export_glb_census,
    TOROGAO_PT_bench,
)
