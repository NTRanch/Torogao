"""nla_front — the thin NLA layer front of the Torogao bench
(the Owner's tooling ruling, 2026-11-27 00:20 PT).

A strip list over Blender's *native* NLA data: tracks and
strips stay exactly where Blender keeps them, in the
armature's animation_data.nla_tracks. The front adds no layer
format and no evaluation semantics of its own — Blender's NLA
evaluates; the front lists, and lets the animator add /
select / mute / solo / reorder strips and edit the blend mode
and influence the native strip carries. This replaces any
need for a paid animation-layers add-on: in Blender 5.3 the
NLA is the layering surface, and this is its bench face.
"""

import bpy


def _resolve_strip(context, item):
    arm = context.scene.torogao_armature
    if arm is None or arm.animation_data is None:
        return None, None
    tracks = arm.animation_data.nla_tracks
    if item.track_index >= len(tracks):
        return None, None
    track = tracks[item.track_index]
    if item.strip_index >= len(track.strips):
        return track, None
    return track, track.strips[item.strip_index]


def _write_mute(self, context):
    _track, strip = _resolve_strip(context, self)
    if strip is not None:
        strip.mute = self.mute


def _write_solo(self, context):
    track, _strip = _resolve_strip(context, self)
    if track is not None:
        track.is_solo = self.solo


def _write_blend(self, context):
    _track, strip = _resolve_strip(context, self)
    if strip is not None:
        strip.blend_type = self.blend_type


def _write_influence(self, context):
    _track, strip = _resolve_strip(context, self)
    if strip is not None:
        strip.influence = self.influence


class TorogaoNLAItem(bpy.types.PropertyGroup):
    """One row of the strip list: a mirror of one native strip,
    with write-through on the editable fields."""

    track_index: bpy.props.IntProperty(default=-1)
    strip_index: bpy.props.IntProperty(default=-1)
    track_name: bpy.props.StringProperty()
    action_name: bpy.props.StringProperty()
    frame_label: bpy.props.StringProperty()
    mute: bpy.props.BoolProperty(
        name="Mute", default=False, update=_write_mute)
    solo: bpy.props.BoolProperty(
        name="Solo", default=False, update=_write_solo)
    blend_type: bpy.props.EnumProperty(
        name="Blend",
        items=[("REPLACE", "Replace", ""),
               ("COMBINE", "Combine", ""),
               ("ADD", "Add", ""),
               ("SUBTRACT", "Subtract", "")],
        update=_write_blend)
    influence: bpy.props.FloatProperty(
        name="Influence", default=1.0, min=0.0, max=1.0,
        update=_write_influence)


class TOROGAO_UL_nla_strips(bpy.types.UIList):
    def draw_item(self, context, layout, data, item, icon,
                  active_data, active_propname, index):
        row = layout.row(align=True)
        row.label(text="{}/{}".format(item.track_name, item.name))
        row.label(text=item.action_name)
        row.label(text=item.frame_label)
        row.prop(item, "mute", text="", icon="MUTE_IPO_OFF")
        row.prop(item, "solo", text="", icon="SOLO_ON")
        row.prop(item, "blend_type", text="")
        row.prop(item, "influence", text="", slider=True)


class TOROGAO_OT_nla_refresh(bpy.types.Operator):
    bl_idname = "torogao.nla_refresh"
    bl_label = "Refresh Strip List"
    bl_description = "Rebuild the strip list from the armature's native NLA tracks"

    def execute(self, context):
        scene = context.scene
        scene.torogao_nla_items.clear()
        arm = scene.torogao_armature
        if arm is None or arm.animation_data is None:
            return {"CANCELLED"}
        for ti, track in enumerate(arm.animation_data.nla_tracks):
            for si, strip in enumerate(track.strips):
                item = scene.torogao_nla_items.add()
                item.track_index = ti
                item.strip_index = si
                item.name = strip.name
                item.track_name = track.name
                item.action_name = strip.action.name \
                    if strip.action else "(none)"
                item.frame_label = "{}..{}".format(
                    int(strip.frame_start), int(strip.frame_end))
                item.mute = strip.mute
                item.solo = track.is_solo
                item.blend_type = strip.blend_type
                item.influence = strip.influence
        return {"FINISHED"}


class TOROGAO_OT_nla_add_strip(bpy.types.Operator):
    bl_idname = "torogao.nla_add_strip"
    bl_label = "Add Strip"
    bl_description = "Add the chosen action as a strip on a new native NLA track, at the current frame"

    def execute(self, context):
        scene = context.scene
        arm = scene.torogao_armature
        action = scene.torogao_nla_action
        if arm is None or action is None:
            self.report({"ERROR"}, "pick an armature and an action")
            return {"CANCELLED"}
        adt = arm.animation_data or arm.animation_data_create()
        track = adt.nla_tracks.new()
        track.name = action.name
        start = int(context.scene.frame_current)
        strip = track.strips.new(action.name, start, action)
        strip.influence = 1.0
        bpy.ops.torogao.nla_refresh()
        return {"FINISHED"}


class TOROGAO_OT_nla_select(bpy.types.Operator):
    bl_idname = "torogao.nla_select"
    bl_label = "Select Strip"
    bl_description = "Select the active strip in the list and jump the playhead to its first frame"

    def execute(self, context):
        scene = context.scene
        index = scene.torogao_nla_active_index
        if index < 0 or index >= len(scene.torogao_nla_items):
            return {"CANCELLED"}
        item = scene.torogao_nla_items[index]
        track, strip = _resolve_strip(context, item)
        if strip is None:
            return {"CANCELLED"}
        adt = scene.torogao_armature.animation_data
        for t in adt.nla_tracks:
            for s in t.strips:
                if hasattr(s, "select"):
                    s.select = False
        if hasattr(strip, "select"):
            strip.select = True
        scene.frame_set(int(strip.frame_start))
        return {"FINISHED"}


_STRIP_COPY_FIELDS = (
    "action", "frame_start", "frame_end", "blend_type",
    "influence", "mute", "repeat", "scale", "strip_time",
    "action_frame_start", "action_frame_end", "use_animated_time",
    "use_animated_influence", "use_reverse", "use_auto_blend",
    "blend_in", "blend_out",
)


def _move_strip(context, delta):
    """Reorder: move the active strip to the adjacent track
    (layering order in the NLA is track order). A strip cannot
    be reparented in place, so the front recreates it on the
    target track with its native fields copied, then removes
    the original — the data stays native throughout."""
    scene = context.scene
    index = scene.torogao_nla_active_index
    if index < 0 or index >= len(scene.torogao_nla_items):
        return False
    item = scene.torogao_nla_items[index]
    arm = scene.torogao_armature
    adt = arm.animation_data
    tracks = adt.nla_tracks
    target = item.track_index + delta
    if target < 0 or target >= len(tracks):
        return False
    track, strip = _resolve_strip(context, item)
    saved = {f: getattr(strip, f) for f in _STRIP_COPY_FIELDS
             if hasattr(strip, f)}
    new_strip = tracks[target].strips.new(
        strip.name, int(strip.frame_start), saved["action"])
    for field, value in saved.items():
        if field in ("action", "frame_start"):
            continue
        try:
            setattr(new_strip, field, value)
        except (TypeError, AttributeError, RuntimeError):
            pass
    track.strips.remove(strip)
    return True


class TOROGAO_OT_nla_strip_up(bpy.types.Operator):
    bl_idname = "torogao.nla_strip_up"
    bl_label = "Strip to Track Above"

    def execute(self, context):
        if not _move_strip(context, -1):
            return {"CANCELLED"}
        bpy.ops.torogao.nla_refresh()
        return {"FINISHED"}


class TOROGAO_OT_nla_strip_down(bpy.types.Operator):
    bl_idname = "torogao.nla_strip_down"
    bl_label = "Strip to Track Below"

    def execute(self, context):
        if not _move_strip(context, +1):
            return {"CANCELLED"}
        bpy.ops.torogao.nla_refresh()
        return {"FINISHED"}


def _snapshot_tracks(adt):
    snap = []
    for track in adt.nla_tracks:
        strips = []
        for strip in track.strips:
            strips.append({
                "name": strip.name,
                "fields": {f: getattr(strip, f)
                           for f in _STRIP_COPY_FIELDS
                           if hasattr(strip, f)},
            })
        snap.append({
            "name": track.name, "mute": track.mute,
            "is_solo": track.is_solo, "lock": track.lock,
            "strips": strips,
        })
    return snap


def _rebuild_tracks(adt, snap):
    """Recreate the track stack in the given order. The RNA
    collection offers no move(), so the front reorders by
    rebuilding — every field copied is native strip/track
    data, and evaluation is Blender's own throughout."""
    while adt.nla_tracks:
        adt.nla_tracks.remove(adt.nla_tracks[0])
    for entry in snap:
        track = adt.nla_tracks.new()
        track.name = entry["name"]
        track.mute = entry["mute"]
        track.is_solo = entry["is_solo"]
        track.lock = entry["lock"]
        for sentry in entry["strips"]:
            fields = sentry["fields"]
            strip = track.strips.new(
                sentry["name"], int(fields["frame_start"]),
                fields["action"])
            for field, value in fields.items():
                if field in ("action", "frame_start"):
                    continue
                try:
                    setattr(strip, field, value)
                except (TypeError, AttributeError, RuntimeError):
                    pass


def _move_track(context, delta):
    scene = context.scene
    index = scene.torogao_nla_active_index
    if index < 0 or index >= len(scene.torogao_nla_items):
        return False
    item = scene.torogao_nla_items[index]
    adt = scene.torogao_armature.animation_data
    target = item.track_index + delta
    if target < 0 or target >= len(adt.nla_tracks):
        return False
    snap = _snapshot_tracks(adt)
    snap[item.track_index], snap[target] = \
        snap[target], snap[item.track_index]
    _rebuild_tracks(adt, snap)
    return True


class TOROGAO_OT_nla_track_up(bpy.types.Operator):
    bl_idname = "torogao.nla_track_up"
    bl_label = "Track Up"
    bl_description = "Reorder the active strip's native track one step up"

    def execute(self, context):
        if not _move_track(context, -1):
            return {"CANCELLED"}
        bpy.ops.torogao.nla_refresh()
        return {"FINISHED"}


class TOROGAO_OT_nla_track_down(bpy.types.Operator):
    bl_idname = "torogao.nla_track_down"
    bl_label = "Track Down"

    def execute(self, context):
        if not _move_track(context, +1):
            return {"CANCELLED"}
        bpy.ops.torogao.nla_refresh()
        return {"FINISHED"}


class TOROGAO_OT_nla_remove_strip(bpy.types.Operator):
    bl_idname = "torogao.nla_remove_strip"
    bl_label = "Remove Strip"

    def execute(self, context):
        scene = context.scene
        index = scene.torogao_nla_active_index
        if index < 0 or index >= len(scene.torogao_nla_items):
            return {"CANCELLED"}
        item = scene.torogao_nla_items[index]
        track, strip = _resolve_strip(context, item)
        if strip is not None:
            track.strips.remove(strip)
        bpy.ops.torogao.nla_refresh()
        return {"FINISHED"}


CLASSES = (
    TorogaoNLAItem,
    TOROGAO_UL_nla_strips,
    TOROGAO_OT_nla_refresh,
    TOROGAO_OT_nla_add_strip,
    TOROGAO_OT_nla_select,
    TOROGAO_OT_nla_strip_up,
    TOROGAO_OT_nla_strip_down,
    TOROGAO_OT_nla_track_up,
    TOROGAO_OT_nla_track_down,
    TOROGAO_OT_nla_remove_strip,
)
