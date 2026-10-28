//! Canonical RON emission for gear documents: a validated
//! document re-emits through the project's one RON emitter
//! (`torogao_author::ron`), so the bytes a pack stores are
//! byte-stable and `parse(emit(doc))` round-trips.

use torogao_author::ron::{Ron, emit_document};

use crate::document::{
    ApparelBlock, Attachment, Device, DrawOrderEntry, EffectBlock, FixtureBlock, GearDocument,
    Hook, PartRef, Physics, PoseConstraint, Provenance, Sensation, WaveSpec,
};

fn opt<T>(value: &Option<T>, f: impl Fn(&T) -> Ron) -> Ron {
    match value {
        Some(inner) => Ron::Opt(Some(Box::new(f(inner)))),
        None => Ron::Opt(None),
    }
}

fn opt_int(value: &Option<i64>) -> Ron {
    opt(value, |n| Ron::Int(*n))
}

fn str_list(values: &[String]) -> Ron {
    Ron::List(values.iter().map(|s| Ron::Str(s.clone())).collect())
}

fn attachment_ron(a: &Attachment) -> Ron {
    Ron::Struct(
        "Attachment".to_string(),
        vec![
            ("anchor".into(), Ron::Str(a.anchor.clone())),
            ("contact_radius_px".into(), Ron::Int(a.contact_radius_px)),
            ("facing".into(), Ron::Ident(a.facing.name().to_string())),
            (
                "max_pressure_permille".into(),
                Ron::Int(a.max_pressure_permille),
            ),
            (
                "offset".into(),
                Ron::Tuple(vec![Ron::Float(a.offset_x_px), Ron::Float(a.offset_y_px)]),
            ),
            ("stiffness_permille".into(), Ron::Int(a.stiffness_permille)),
        ],
    )
}

fn part_ron(p: &PartRef) -> Ron {
    Ron::Struct(
        "PartRef".to_string(),
        vec![
            ("hash".into(), Ron::Str(p.hash.clone())),
            ("name".into(), Ron::Str(p.name.clone())),
            (
                "texture".into(),
                opt(&p.texture, |(w, h)| {
                    Ron::Tuple(vec![Ron::Int(*w), Ron::Int(*h)])
                }),
            ),
        ],
    )
}

fn draw_ron(d: &DrawOrderEntry) -> Ron {
    Ron::Struct(
        "DrawOrder".to_string(),
        vec![
            ("layer".into(), Ron::Str(d.layer.clone())),
            ("relation".into(), Ron::Ident(d.relation.name().to_string())),
            ("target".into(), Ron::Str(d.target.clone())),
        ],
    )
}

fn hook_ron(h: &Hook) -> Ron {
    Ron::Struct(
        "Hook".to_string(),
        vec![
            ("kind".into(), Ron::Ident(h.kind.name().to_string())),
            ("name".into(), Ron::Str(h.name.clone())),
        ],
    )
}

fn physics_ron(p: &Physics) -> Ron {
    Ron::Struct(
        "Physics".to_string(),
        vec![
            ("displaceable".into(), Ron::Bool(p.displaceable)),
            (
                "displacement_threshold_px".into(),
                Ron::Int(p.displacement_threshold_px),
            ),
        ],
    )
}

fn sensation_ron(s: &Sensation) -> Ron {
    Ron::Struct(
        "Sensation".to_string(),
        vec![
            (
                "arousal_floor_permille".into(),
                opt_int(&s.arousal_floor_permille),
            ),
            ("pressure_permille".into(), opt_int(&s.pressure_permille)),
            ("site".into(), Ron::Str(s.site.clone())),
        ],
    )
}

fn wave_ron(w: &WaveSpec) -> Ron {
    Ron::Struct(
        "WaveSpec".to_string(),
        vec![
            ("amplitude_permille".into(), Ron::Int(w.amplitude_permille)),
            ("duty_permille".into(), opt_int(&w.duty_permille)),
            ("frequency_millihz".into(), Ron::Int(w.frequency_millihz)),
            ("kind".into(), Ron::Ident(w.kind.name().to_string())),
            (
                "pattern".into(),
                opt(&w.pattern, |steps| {
                    Ron::List(steps.iter().map(|s| Ron::Int(*s)).collect())
                }),
            ),
            ("pattern_step_ticks".into(), opt_int(&w.pattern_step_ticks)),
            ("phase_ticks".into(), Ron::Int(w.phase_ticks)),
        ],
    )
}

fn device_ron(d: &Device) -> Ron {
    Ron::Struct(
        "Device".to_string(),
        vec![
            ("contact_site".into(), Ron::Str(d.contact_site.clone())),
            ("mirror_capable".into(), Ron::Bool(d.mirror_capable)),
            ("waveform".into(), wave_ron(&d.waveform)),
        ],
    )
}

fn apparel_ron(a: &ApparelBlock) -> Ron {
    Ron::Struct(
        "Apparel".to_string(),
        vec![
            ("covers".into(), str_list(&a.covers)),
            (
                "transmission_permille".into(),
                Ron::Int(a.transmission_permille),
            ),
        ],
    )
}

fn pose_ron(p: &PoseConstraint) -> Ron {
    Ron::Struct(
        "PoseConstraint".to_string(),
        vec![
            ("max_permille".into(), Ron::Int(p.max_permille)),
            ("min_permille".into(), Ron::Int(p.min_permille)),
            ("param".into(), Ron::Str(p.param.clone())),
        ],
    )
}

fn fixture_ron(f: &FixtureBlock) -> Ron {
    Ron::Struct(
        "Fixture".to_string(),
        vec![
            ("declares_anchors".into(), str_list(&f.declares_anchors)),
            (
                "pose_constraints".into(),
                Ron::List(f.pose_constraints.iter().map(pose_ron).collect()),
            ),
        ],
    )
}

fn effect_ron(e: &EffectBlock) -> Ron {
    Ron::Struct(
        "Effect".to_string(),
        vec![
            ("inputs".into(), str_list(&e.inputs)),
            ("outputs".into(), str_list(&e.outputs)),
        ],
    )
}

fn provenance_ron(p: &Provenance) -> Ron {
    Ron::Struct(
        "Provenance".to_string(),
        vec![
            ("author".into(), Ron::Str(p.author.clone())),
            ("honesty".into(), Ron::Ident(p.honesty.name().to_string())),
            ("source_pack".into(), Ron::Str(p.source_pack.clone())),
        ],
    )
}

/// The canonical RON value of a gear document. Field order is
/// alphabetical at every level (the estate ordering rule), so
/// emission is a pure function of the document.
pub fn to_ron(doc: &GearDocument) -> Ron {
    Ron::Struct(
        "GearDocument".to_string(),
        vec![
            (
                "anchors".into(),
                Ron::List(doc.anchors.iter().map(attachment_ron).collect()),
            ),
            ("apparel".into(), opt(&doc.apparel, apparel_ron)),
            ("class".into(), Ron::Ident(doc.class.name().to_string())),
            ("depends_on".into(), str_list(&doc.depends_on)),
            ("device".into(), opt(&doc.device, device_ron)),
            ("display_name".into(), Ron::Str(doc.display_name.clone())),
            (
                "draw_order".into(),
                Ron::List(doc.draw_order.iter().map(draw_ron).collect()),
            ),
            ("effect".into(), opt(&doc.effect, effect_ron)),
            ("fixture".into(), opt(&doc.fixture, fixture_ron)),
            ("format".into(), Ron::Str(doc.format.clone())),
            (
                "hooks".into(),
                Ron::List(doc.hooks.iter().map(hook_ron).collect()),
            ),
            ("id".into(), Ron::Str(doc.id.clone())),
            ("materials".into(), str_list(&doc.materials)),
            (
                "parts".into(),
                Ron::List(doc.parts.iter().map(part_ron).collect()),
            ),
            ("physics".into(), physics_ron(&doc.physics)),
            ("provenance".into(), provenance_ron(&doc.provenance)),
            ("sensation".into(), opt(&doc.sensation, sensation_ron)),
            (
                "state_variants".into(),
                Ron::List(
                    doc.state_variants
                        .iter()
                        .map(|v| Ron::Ident(v.name().to_string()))
                        .collect(),
                ),
            ),
            ("version".into(), Ron::Int(doc.version)),
        ],
    )
}

/// The canonical document text: the form packs store and
/// authors diff.
pub fn document_text(doc: &GearDocument) -> String {
    emit_document(&to_ron(doc))
}
