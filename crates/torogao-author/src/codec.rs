//! Project ↔ RON mapping: the one serializer for `project.ron`.
//!
//! Field order inside every record is `id` first, then
//! alphabetical (rectangles keep their natural `x, y, width,
//! height` order). Loading accepts fields in any order and
//! re-sorts collections by id; whether the file was canonical is
//! decided by re-emitting and comparing bytes, never by the
//! loader. Unknown record or variant names are
//! `TG-AUTH-UNKNOWN-KIND`; missing, unknown, or ill-typed fields
//! are `TG-AUTH-MALFORMED`.

use crate::model::{
    AnchorKind, ArtBind, Asset, AssetKind, AudioBind, AudioBindTarget, AudioBinding,
    AudioBindingKind, AudioMarker, AudioTrack,
    Blend, Clip, Completion, Consent, Deformer, DeformerKind, DeliveryRef, Draw, Expression, ExpressionValue,
    ExpressionVisibility, Glue, Header, Keyform, Keyforms, Mesh, MeshForm, Motion, MotionCurve,
    MotionKeyframe, PROJECT_FORMAT, PROJECT_FORMAT_VERSION, Parameter, Part, PhysicsAnchor,
    PhysicsGroup, PhysicsInput, PhysicsOutput, PhysicsParticle, PhysicsStrand, Point, PoseEntry,
    Project, Provenance, Rect, Rotation, RotationForm, Stamp, VariantSet, VoiceBinding, Warp,
    WarpForm,
};
use crate::refusal::{Refusal, RefusalClass, refuse};
use crate::ron::{Ron, emit_document, parse_document};

/// Emits a project as canonical `project.ron` text.
pub fn emit_project(project: &Project) -> String {
    emit_document(&project_to_ron(project))
}

/// Parses `project.ron` text into a project (sorted collections).
/// Does not judge canonicality or run `decide()`.
pub fn parse_project(text: &str) -> Result<Project, Refusal> {
    project_from_ron(parse_document(text)?)
}

// ---- emission helpers -------------------------------------------

pub(crate) fn text(value: &str) -> Ron {
    Ron::Str(value.to_string())
}

fn point(value: &Point) -> Ron {
    Ron::Tuple(vec![Ron::Float(value[0]), Ron::Float(value[1])])
}

fn points(values: &[Point]) -> Ron {
    Ron::List(values.iter().map(point).collect())
}

fn floats(values: &[f64]) -> Ron {
    Ron::List(values.iter().map(|value| Ron::Float(*value)).collect())
}

pub(crate) fn texts(values: &[String]) -> Ron {
    Ron::List(values.iter().map(|value| text(value)).collect())
}

pub(crate) fn opt<T>(value: Option<&T>, convert: impl Fn(&T) -> Ron) -> Ron {
    Ron::Opt(value.map(|inner| Box::new(convert(inner))))
}

pub(crate) fn record(name: &str, fields: Vec<(&str, Ron)>) -> Ron {
    Ron::Struct(
        name.to_string(),
        fields
            .into_iter()
            .map(|(key, value)| (key.to_string(), value))
            .collect(),
    )
}

fn rect_to_ron(rect: &Rect) -> Ron {
    record(
        "Rect",
        vec![
            ("x", Ron::Int(i64::from(rect.x))),
            ("y", Ron::Int(i64::from(rect.y))),
            ("width", Ron::Int(i64::from(rect.width))),
            ("height", Ron::Int(i64::from(rect.height))),
        ],
    )
}

fn project_to_ron(project: &Project) -> Ron {
    // Sections sit in alphabetical order after the header. The
    // optional sections — the game section and the G-10
    // audio/motion/physics sections — are emitted only when
    // present (non-empty), so a project that predates them
    // serializes byte-identically to before they existed.
    let mut fields: Vec<(&str, Ron)> = vec![
        ("header", header_to_ron(&project.header)),
        (
            "assets",
            Ron::List(project.assets.iter().map(asset_to_ron).collect()),
        ),
    ];
    if !project.audio_bindings.is_empty() {
        fields.push((
            "audio_bindings",
            Ron::List(
                project
                    .audio_bindings
                    .iter()
                    .map(audio_binding_to_ron)
                    .collect(),
            ),
        ));
    }
    if !project.audio_binds.is_empty() {
        fields.push((
            "audio_binds",
            Ron::List(project.audio_binds.iter().map(audio_bind_to_ron).collect()),
        ));
    }
    if !project.audio_tracks.is_empty() {
        fields.push((
            "audio_tracks",
            Ron::List(
                project
                    .audio_tracks
                    .iter()
                    .map(audio_track_to_ron)
                    .collect(),
            ),
        ));
    }
    fields.push((
        "deformers",
        Ron::List(project.deformers.iter().map(deformer_to_ron).collect()),
    ));
    fields.push((
        "expressions",
        Ron::List(project.expressions.iter().map(expression_to_ron).collect()),
    ));
    // The game section sits between `expressions` and `glues`
    // (the §G4 layout order); it is emitted only when present,
    // so a game-less project serializes byte-identically to
    // before the game layer existed.
    if let Some(game) = &project.game {
        fields.push((
            "game",
            Ron::Opt(Some(Box::new(crate::game::codec::game_to_ron(game)))),
        ));
    }
    fields.push((
        "glues",
        Ron::List(project.glues.iter().map(glue_to_ron).collect()),
    ));
    if !project.motions.is_empty() {
        fields.push((
            "motions",
            Ron::List(project.motions.iter().map(motion_to_ron).collect()),
        ));
    }
    fields.push((
        "parameters",
        Ron::List(project.parameters.iter().map(parameter_to_ron).collect()),
    ));
    fields.push((
        "parts",
        Ron::List(project.parts.iter().map(part_to_ron).collect()),
    ));
    if !project.physics_groups.is_empty() {
        fields.push((
            "physics_groups",
            Ron::List(
                project
                    .physics_groups
                    .iter()
                    .map(physics_group_to_ron)
                    .collect(),
            ),
        ));
    }
    fields.push((
        "pose",
        Ron::List(project.pose.iter().map(pose_to_ron).collect()),
    ));
    fields.push((
        "variant_sets",
        Ron::List(
            project
                .variant_sets
                .iter()
                .map(variant_set_to_ron)
                .collect(),
        ),
    ));
    record("Project", fields)
}

fn header_to_ron(header: &Header) -> Ron {
    let stamp = &header.stamp;
    let consent = &header.consent;
    record(
        "Header",
        vec![
            (
                "consent",
                record(
                    "Consent",
                    vec![
                        ("adult", Ron::Bool(consent.adult)),
                        ("schema", text(&consent.schema)),
                        (
                            "wants_selected_scenes",
                            Ron::Bool(consent.wants_selected_scenes),
                        ),
                    ],
                ),
            ),
            ("format", text(PROJECT_FORMAT)),
            ("format_version", Ron::Int(PROJECT_FORMAT_VERSION)),
            ("model_format", text(&header.model_format)),
            ("name", text(&header.name)),
            (
                "stamp",
                record(
                    "Stamp",
                    vec![
                        ("basis", text(&stamp.basis)),
                        ("lane", text(&stamp.lane)),
                        ("pack", text(&stamp.pack)),
                        ("rating", text(&stamp.rating)),
                    ],
                ),
            ),
        ],
    )
}

fn asset_to_ron(asset: &Asset) -> Ron {
    let provenance = &asset.provenance;
    record(
        "Asset",
        vec![
            ("id", text(&asset.id)),
            (
                "canvas_origin",
                opt(asset.canvas_origin.as_ref(), |origin| {
                    Ron::Tuple(vec![Ron::Int(origin[0]), Ron::Int(origin[1])])
                }),
            ),
            ("content", opt(asset.content.as_ref(), rect_to_ron)),
            ("file", text(&asset.file)),
            ("height", Ron::Int(i64::from(asset.height))),
            (
                "kind",
                Ron::Ident(
                    match asset.kind {
                        AssetKind::Png => "Png",
                        AssetKind::PsdLayer => "PsdLayer",
                    }
                    .to_string(),
                ),
            ),
            (
                "provenance",
                record(
                    "Provenance",
                    vec![
                        ("layer", opt(provenance.layer.as_ref(), |layer| text(layer))),
                        ("license", text(&provenance.license)),
                        ("origin", text(&provenance.origin)),
                        ("source_name", text(&provenance.source_name)),
                        ("source_sha256", text(&provenance.source_sha256)),
                    ],
                ),
            ),
            ("sha256", text(&asset.sha256)),
            ("width", Ron::Int(i64::from(asset.width))),
        ],
    )
}

fn keyforms_to_ron<F>(keyforms: &Keyforms<F>, form: impl Fn(&Keyform<F>) -> Ron) -> Ron {
    record(
        "Keyforms",
        vec![
            ("axes", texts(&keyforms.axes)),
            (
                "forms",
                Ron::List(keyforms.forms.iter().map(form).collect()),
            ),
        ],
    )
}

fn mesh_form_to_ron(keyform: &Keyform<MeshForm>) -> Ron {
    record(
        "MeshForm",
        vec![
            ("at", floats(&keyform.at)),
            ("vertices", points(&keyform.form.vertices)),
        ],
    )
}

fn rotation_form_to_ron(keyform: &Keyform<RotationForm>) -> Ron {
    record(
        "RotationForm",
        vec![
            ("angle_degrees", Ron::Float(keyform.form.angle_degrees)),
            ("at", floats(&keyform.at)),
            ("scale", point(&keyform.form.scale)),
        ],
    )
}

fn warp_form_to_ron(keyform: &Keyform<WarpForm>) -> Ron {
    record(
        "WarpForm",
        vec![
            ("at", floats(&keyform.at)),
            ("positions", points(&keyform.form.positions)),
        ],
    )
}

fn part_to_ron(part: &Part) -> Ron {
    record(
        "Part",
        vec![
            ("id", text(&part.id)),
            (
                "art",
                opt(part.art.as_ref(), |art| {
                    record(
                        "ArtBind",
                        vec![
                            ("asset", text(&art.asset)),
                            ("region", rect_to_ron(&art.region)),
                        ],
                    )
                }),
            ),
            (
                "clip",
                opt(part.clip.as_ref(), |clip| {
                    record(
                        "Clip",
                        vec![
                            ("inverted", Ron::Bool(clip.inverted)),
                            ("source", text(&clip.source)),
                        ],
                    )
                }),
            ),
            (
                "completion",
                opt(part.completion.as_ref(), |completion| {
                    Ron::Ident(
                        match completion {
                            Completion::NotNeeded => "NotNeeded",
                            Completion::Painted => "Painted",
                        }
                        .to_string(),
                    )
                }),
            ),
            ("deformer", opt(part.deformer.as_ref(), |id| text(id))),
            (
                "draw",
                opt(part.draw.as_ref(), |draw| {
                    record(
                        "Draw",
                        vec![
                            ("band", Ron::Int(i64::from(draw.band))),
                            ("group", opt(draw.group.as_ref(), |group| text(group))),
                            ("index", Ron::Int(i64::from(draw.index))),
                        ],
                    )
                }),
            ),
            (
                "keyforms",
                opt(part.keyforms.as_ref(), |keyforms| {
                    keyforms_to_ron(keyforms, mesh_form_to_ron)
                }),
            ),
            (
                "mesh",
                opt(part.mesh.as_ref(), |mesh| {
                    record(
                        "Mesh",
                        vec![
                            (
                                "indices",
                                Ron::List(
                                    mesh.indices
                                        .iter()
                                        .map(|index| Ron::Int(i64::from(*index)))
                                        .collect(),
                                ),
                            ),
                            ("uvs", points(&mesh.uvs)),
                            ("vertices", points(&mesh.vertices)),
                        ],
                    )
                }),
            ),
            ("parent", opt(part.parent.as_ref(), |id| text(id))),
            ("visible", Ron::Bool(part.visible)),
        ],
    )
}

fn deformer_to_ron(deformer: &Deformer) -> Ron {
    let kind = match &deformer.kind {
        DeformerKind::Rotation(rotation) => record(
            "Rotation",
            vec![
                (
                    "base_angle_degrees",
                    Ron::Float(rotation.base_angle_degrees),
                ),
                (
                    "keyforms",
                    opt(rotation.keyforms.as_ref(), |keyforms| {
                        keyforms_to_ron(keyforms, rotation_form_to_ron)
                    }),
                ),
                ("origin", point(&rotation.origin)),
            ],
        ),
        DeformerKind::Warp(warp) => record(
            "Warp",
            vec![
                (
                    "bounds",
                    Ron::Tuple(warp.bounds.iter().map(|value| Ron::Float(*value)).collect()),
                ),
                ("columns", Ron::Int(i64::from(warp.columns))),
                (
                    "keyforms",
                    opt(warp.keyforms.as_ref(), |keyforms| {
                        keyforms_to_ron(keyforms, warp_form_to_ron)
                    }),
                ),
                ("rows", Ron::Int(i64::from(warp.rows))),
            ],
        ),
    };
    record(
        "Deformer",
        vec![
            ("id", text(&deformer.id)),
            ("kind", kind),
            ("parent", opt(deformer.parent.as_ref(), |id| text(id))),
        ],
    )
}

fn glue_to_ron(glue: &Glue) -> Ron {
    record(
        "Glue",
        vec![
            ("id", text(&glue.id)),
            ("a", text(&glue.a)),
            ("b", text(&glue.b)),
            (
                "pairs",
                Ron::List(
                    glue.pairs
                        .iter()
                        .map(|pair| {
                            Ron::Tuple(vec![
                                Ron::Int(i64::from(pair[0])),
                                Ron::Int(i64::from(pair[1])),
                            ])
                        })
                        .collect(),
                ),
            ),
            ("weight", Ron::Float(glue.weight)),
        ],
    )
}

fn parameter_to_ron(parameter: &Parameter) -> Ron {
    record(
        "Parameter",
        vec![
            ("id", text(&parameter.id)),
            ("class", text(&parameter.class)),
            ("default", Ron::Float(parameter.default)),
            ("max", Ron::Float(parameter.max)),
            ("min", Ron::Float(parameter.min)),
        ],
    )
}

fn pose_to_ron(entry: &PoseEntry) -> Ron {
    record(
        "PoseEntry",
        vec![
            ("parameter", text(&entry.parameter)),
            ("value", Ron::Float(entry.value)),
        ],
    )
}

fn expression_to_ron(expression: &Expression) -> Ron {
    record(
        "Expression",
        vec![
            ("id", text(&expression.id)),
            (
                "blend",
                Ron::Ident(
                    match expression.blend {
                        Blend::Additive => "Additive",
                        Blend::Override => "Override",
                    }
                    .to_string(),
                ),
            ),
            (
                "values",
                Ron::List(
                    expression
                        .values
                        .iter()
                        .map(|row| {
                            record(
                                "Value",
                                vec![
                                    ("parameter", text(&row.parameter)),
                                    ("value", Ron::Float(row.value)),
                                ],
                            )
                        })
                        .collect(),
                ),
            ),
            (
                "visibility",
                Ron::List(
                    expression
                        .visibility
                        .iter()
                        .map(|row| {
                            record(
                                "Visibility",
                                vec![
                                    ("part", text(&row.part)),
                                    ("visible", Ron::Bool(row.visible)),
                                ],
                            )
                        })
                        .collect(),
                ),
            ),
        ],
    )
}

fn variant_set_to_ron(set: &VariantSet) -> Ron {
    record(
        "VariantSet",
        vec![("id", text(&set.id)), ("variants", texts(&set.variants))],
    )
}

fn physics_group_to_ron(group: &PhysicsGroup) -> Ron {
    record(
        "PhysicsGroup",
        vec![
            ("id", text(&group.id)),
            ("gravity", point(&group.gravity)),
            (
                "inputs",
                Ron::List(group.inputs.iter().map(physics_input_to_ron).collect()),
            ),
            (
                "strands",
                Ron::List(group.strands.iter().map(physics_strand_to_ron).collect()),
            ),
            (
                "turbulence_amplitude",
                Ron::Float(group.turbulence_amplitude),
            ),
            ("wind", point(&group.wind)),
        ],
    )
}

fn physics_input_to_ron(input: &PhysicsInput) -> Ron {
    record(
        "PhysicsInput",
        vec![
            ("parameter", text(&input.parameter)),
            ("scale_degrees", Ron::Float(input.scale_degrees)),
        ],
    )
}

fn physics_strand_to_ron(strand: &PhysicsStrand) -> Ron {
    record(
        "PhysicsStrand",
        vec![
            ("id", text(&strand.id)),
            ("anchor", physics_anchor_to_ron(&strand.anchor)),
            ("output", opt(strand.output.as_ref(), physics_output_to_ron)),
            (
                "particles",
                Ron::List(
                    strand
                        .particles
                        .iter()
                        .map(physics_particle_to_ron)
                        .collect(),
                ),
            ),
        ],
    )
}

fn physics_anchor_to_ron(anchor: &PhysicsAnchor) -> Ron {
    record(
        "PhysicsAnchor",
        vec![
            ("id", text(&anchor.id)),
            (
                "kind",
                Ron::Ident(
                    match anchor.kind {
                        AnchorKind::Deformer => "Deformer",
                        AnchorKind::Mesh => "Mesh",
                    }
                    .to_string(),
                ),
            ),
            ("offset", point(&anchor.offset)),
        ],
    )
}

fn physics_particle_to_ron(particle: &PhysicsParticle) -> Ron {
    record(
        "PhysicsParticle",
        vec![
            ("damping", Ron::Float(particle.damping)),
            ("delay", Ron::Float(particle.delay)),
            ("length", Ron::Float(particle.length)),
            ("mobility", Ron::Float(particle.mobility)),
            ("radius", Ron::Float(particle.radius)),
        ],
    )
}

fn physics_output_to_ron(output: &PhysicsOutput) -> Ron {
    record(
        "PhysicsOutput",
        vec![
            ("offset", Ron::Float(output.offset)),
            ("parameter", text(&output.parameter)),
            ("scale", Ron::Float(output.scale)),
        ],
    )
}

fn motion_to_ron(motion: &Motion) -> Ron {
    record(
        "Motion",
        vec![
            ("id", text(&motion.id)),
            (
                "curves",
                Ron::List(motion.curves.iter().map(motion_curve_to_ron).collect()),
            ),
            ("length_ticks", Ron::Int(i64::from(motion.length_ticks))),
            ("looped", Ron::Bool(motion.looped)),
        ],
    )
}

fn motion_curve_to_ron(curve: &MotionCurve) -> Ron {
    record(
        "MotionCurve",
        vec![
            (
                "keyframes",
                Ron::List(curve.keyframes.iter().map(motion_keyframe_to_ron).collect()),
            ),
            ("parameter", text(&curve.parameter)),
            ("source", opt(curve.source.as_ref(), |source| text(source))),
        ],
    )
}

fn motion_keyframe_to_ron(keyframe: &MotionKeyframe) -> Ron {
    record(
        "MotionKeyframe",
        vec![
            ("tick", Ron::Int(i64::from(keyframe.tick))),
            ("value", Ron::Float(keyframe.value)),
        ],
    )
}

fn audio_track_to_ron(track: &AudioTrack) -> Ron {
    record(
        "AudioTrack",
        vec![
            ("id", text(&track.id)),
            (
                "markers",
                Ron::List(track.markers.iter().map(audio_marker_to_ron).collect()),
            ),
        ],
    )
}

fn audio_marker_to_ron(marker: &AudioMarker) -> Ron {
    record(
        "AudioMarker",
        vec![
            ("id", text(&marker.id)),
            ("tick", Ron::Int(i64::from(marker.tick))),
        ],
    )
}

fn audio_binding_to_ron(binding: &AudioBinding) -> Ron {
    match &binding.kind {
        AudioBindingKind::Voice(voice) => record(
            "AudioBinding",
            vec![
                ("id", text(&binding.id)),
                ("character", text(&voice.character)),
                (
                    "delivery",
                    record(
                        "DeliveryRef",
                        vec![
                            ("identity", text(&voice.delivery.identity)),
                            ("record", text(&voice.delivery.record)),
                        ],
                    ),
                ),
                ("kind", Ron::Ident("Voice".to_string())),
            ],
        ),
    }
}

fn audio_bind_to_ron(bind: &AudioBind) -> Ron {
    record(
        "AudioBind",
        vec![
            ("marker", text(&bind.marker)),
            ("peak_tick", Ron::Int(i64::from(bind.peak_tick))),
            ("sync_offset_ticks", Ron::Int(bind.sync_offset_ticks)),
            ("target", audio_bind_target_to_ron(&bind.target)),
            ("track", text(&bind.track)),
            ("trail_ticks", Ron::Int(i64::from(bind.trail_ticks))),
        ],
    )
}

fn audio_bind_target_to_ron(target: &AudioBindTarget) -> Ron {
    match target {
        AudioBindTarget::ParameterOffset { parameter, offset } => record(
            "AudioBindTarget",
            vec![
                ("kind", Ron::Ident("ParameterOffset".to_string())),
                ("offset", Ron::Float(*offset)),
                ("parameter", text(parameter)),
            ],
        ),
        AudioBindTarget::ExpressionTrigger { expression } => record(
            "AudioBindTarget",
            vec![
                ("expression", text(expression)),
                ("kind", Ron::Ident("ExpressionTrigger".to_string())),
            ],
        ),
    }
}

// ---- parsing helpers --------------------------------------------

/// A struct record being consumed field by field.
pub(crate) struct Fields {
    kind: String,
    items: Vec<(String, Ron)>,
}

pub(crate) fn open(value: Ron, expected: &str) -> Result<Fields, Refusal> {
    match value {
        Ron::Struct(name, items) if name == expected => Ok(Fields { kind: name, items }),
        Ron::Struct(name, _) => refuse(
            RefusalClass::UnknownKind,
            format!("record kind '{name}' is not known where '{expected}' is expected"),
        ),
        _ => refuse(
            RefusalClass::Malformed,
            format!("expected a {expected} record"),
        ),
    }
}

impl Fields {
    pub(crate) fn take(&mut self, key: &str) -> Result<Ron, Refusal> {
        match self.items.iter().position(|(name, _)| name == key) {
            Some(index) => Ok(self.items.remove(index).1),
            None => refuse(
                RefusalClass::Malformed,
                format!("{} record lacks field '{key}'", self.kind),
            ),
        }
    }

    pub(crate) fn done(self) -> Result<(), Refusal> {
        match self.items.first() {
            Some((name, _)) => refuse(
                RefusalClass::Malformed,
                format!("{} record has unknown field '{name}'", self.kind),
            ),
            None => Ok(()),
        }
    }

    pub(crate) fn string(&mut self, key: &str) -> Result<String, Refusal> {
        as_string(self.take(key)?, key)
    }

    pub(crate) fn float(&mut self, key: &str) -> Result<f64, Refusal> {
        as_float(self.take(key)?, key)
    }

    pub(crate) fn uint(&mut self, key: &str) -> Result<u32, Refusal> {
        as_u32(self.take(key)?, key)
    }

    pub(crate) fn boolean(&mut self, key: &str) -> Result<bool, Refusal> {
        match self.take(key)? {
            Ron::Bool(flag) => Ok(flag),
            _ => wrong_type(key, "a boolean"),
        }
    }

    pub(crate) fn option(&mut self, key: &str) -> Result<Option<Ron>, Refusal> {
        match self.take(key)? {
            Ron::Opt(inner) => Ok(inner.map(|boxed| *boxed)),
            _ => wrong_type(key, "None or Some(...)"),
        }
    }

    pub(crate) fn list(&mut self, key: &str) -> Result<Vec<Ron>, Refusal> {
        match self.take(key)? {
            Ron::List(items) => Ok(items),
            _ => wrong_type(key, "a list"),
        }
    }

    pub(crate) fn ident(&mut self, key: &str) -> Result<String, Refusal> {
        match self.take(key)? {
            Ron::Ident(name) => Ok(name),
            _ => wrong_type(key, "a variant name"),
        }
    }

    /// Takes a field when present; `None` when absent (for
    /// sections a file may legitimately lack entirely).
    pub(crate) fn try_take(&mut self, key: &str) -> Option<Ron> {
        self.items
            .iter()
            .position(|(name, _)| name == key)
            .map(|index| self.items.remove(index).1)
    }

    /// Takes an integer field.
    pub(crate) fn int(&mut self, key: &str) -> Result<i64, Refusal> {
        as_i64(self.take(key)?, key)
    }

    /// Takes an optional string field.
    pub(crate) fn opt_string(&mut self, key: &str) -> Result<Option<String>, Refusal> {
        match self.option(key)? {
            None => Ok(None),
            Some(value) => as_string(value, key).map(Some),
        }
    }
}

/// Opens a struct record of any kind, returning its kind name
/// with its fields (for sum-typed records dispatched by name).
pub(crate) fn open_any(value: Ron) -> Result<(String, Fields), Refusal> {
    match value {
        Ron::Struct(name, items) => Ok((name.clone(), Fields { kind: name, items })),
        _ => refuse(RefusalClass::Malformed, "expected a named record"),
    }
}

/// An integer field value.
pub(crate) fn as_i64(value: Ron, key: &str) -> Result<i64, Refusal> {
    match value {
        Ron::Int(number) => Ok(number),
        _ => wrong_type(key, "an integer"),
    }
}

pub(crate) fn wrong_type<T>(key: &str, expected: &str) -> Result<T, Refusal> {
    refuse(
        RefusalClass::Malformed,
        format!("field '{key}' must be {expected}"),
    )
}

pub(crate) fn as_string(value: Ron, key: &str) -> Result<String, Refusal> {
    match value {
        Ron::Str(text) => Ok(text),
        _ => wrong_type(key, "a string"),
    }
}

/// Largest integer magnitude an f64 represents exactly (2^53).
const EXACT_INT_MAX: i64 = 9_007_199_254_740_992;

fn as_float(value: Ron, key: &str) -> Result<f64, Refusal> {
    match value {
        Ron::Float(number) => Ok(number),
        Ron::Int(number) if number.abs() <= EXACT_INT_MAX => Ok(number as f64),
        _ => wrong_type(key, "a number"),
    }
}

pub(crate) fn as_u32(value: Ron, key: &str) -> Result<u32, Refusal> {
    match value {
        Ron::Int(number) => u32::try_from(number).or_else(|_| wrong_type(key, "a u32")),
        _ => wrong_type(key, "an integer"),
    }
}

fn as_point(value: Ron, key: &str) -> Result<Point, Refusal> {
    match value {
        Ron::Tuple(items) if items.len() == 2 => {
            let mut items = items.into_iter();
            let (Some(x), Some(y)) = (items.next(), items.next()) else {
                return wrong_type(key, "a pair");
            };
            Ok([as_float(x, key)?, as_float(y, key)?])
        }
        _ => wrong_type(key, "a pair (x, y)"),
    }
}

fn as_points(items: Vec<Ron>, key: &str) -> Result<Vec<Point>, Refusal> {
    items.into_iter().map(|item| as_point(item, key)).collect()
}

pub(crate) fn as_strings(items: Vec<Ron>, key: &str) -> Result<Vec<String>, Refusal> {
    items.into_iter().map(|item| as_string(item, key)).collect()
}

fn as_rect(value: Ron) -> Result<Rect, Refusal> {
    let mut fields = open(value, "Rect")?;
    let rect = Rect {
        x: fields.uint("x")?,
        y: fields.uint("y")?,
        width: fields.uint("width")?,
        height: fields.uint("height")?,
    };
    fields.done()?;
    Ok(rect)
}

pub(crate) fn unknown_variant<T>(key: &str, name: &str) -> Result<T, Refusal> {
    refuse(
        RefusalClass::UnknownKind,
        format!("field '{key}' names unknown kind '{name}'"),
    )
}

/// Sorts a collection by id and refuses duplicate ids.
pub(crate) fn sorted_unique<T>(
    mut items: Vec<T>,
    collection: &str,
    key: impl Fn(&T) -> &str,
) -> Result<Vec<T>, Refusal> {
    items.sort_by(|left, right| key(left).cmp(key(right)));
    for pair in items.windows(2) {
        if key(&pair[0]) == key(&pair[1]) {
            return refuse(
                RefusalClass::Malformed,
                format!("{collection} carry duplicate id '{}'", key(&pair[0])),
            );
        }
    }
    Ok(items)
}

// ---- parsing ----------------------------------------------------

/// Reads a list section that older projects may not carry at
/// all (the G-10 sections): absent maps to empty.
fn list_section<T>(
    fields: &mut Fields,
    key: &str,
    convert: impl Fn(Ron) -> Result<T, Refusal>,
) -> Result<Vec<T>, Refusal> {
    match fields.try_take(key) {
        None => Ok(Vec::new()),
        Some(Ron::List(items)) => collect(items, convert),
        Some(_) => wrong_type(key, "a list"),
    }
}

fn project_from_ron(value: Ron) -> Result<Project, Refusal> {
    let mut fields = open(value, "Project")?;
    let header = header_from_ron(fields.take("header")?)?;
    let assets = collect(fields.list("assets")?, asset_from_ron)?;
    let audio_bindings = list_section(&mut fields, "audio_bindings", audio_binding_from_ron)?;
    let audio_binds = list_section(&mut fields, "audio_binds", audio_bind_from_ron)?;
    let audio_tracks = list_section(&mut fields, "audio_tracks", audio_track_from_ron)?;
    let deformers = collect(fields.list("deformers")?, deformer_from_ron)?;
    let expressions = collect(fields.list("expressions")?, expression_from_ron)?;
    let glues = collect(fields.list("glues")?, glue_from_ron)?;
    let motions = list_section(&mut fields, "motions", motion_from_ron)?;
    let parameters = collect(fields.list("parameters")?, parameter_from_ron)?;
    let parts = collect(fields.list("parts")?, part_from_ron)?;
    let physics_groups = list_section(&mut fields, "physics_groups", physics_group_from_ron)?;
    let pose = collect(fields.list("pose")?, pose_from_ron)?;
    let variant_sets = collect(fields.list("variant_sets")?, variant_set_from_ron)?;
    // The game section is the one optional section: a file
    // without it is a rig-only project. `game: None` written out
    // longhand parses to the same absence and is non-canonical
    // (the emitter omits the field), which validation reports.
    let game = match fields.try_take("game") {
        None => None,
        Some(Ron::Opt(None)) => None,
        Some(Ron::Opt(Some(inner))) => Some(crate::game::codec::game_from_ron(*inner)?),
        Some(_) => return wrong_type("game", "None or Some(Game(...))"),
    };
    fields.done()?;
    // Audio binds sort by the (track, marker) pair, which the
    // &str-keyed sorted_unique cannot express; sort and check
    // duplicates here.
    let mut audio_binds = audio_binds;
    audio_binds.sort_by(|a, b| a.track.cmp(&b.track).then_with(|| a.marker.cmp(&b.marker)));
    for pair in audio_binds.windows(2) {
        if pair[0].track == pair[1].track && pair[0].marker == pair[1].marker {
            return refuse(
                RefusalClass::Malformed,
                format!(
                    "audio binds carry duplicate row '{}'/'{}'",
                    pair[0].track, pair[0].marker
                ),
            );
        }
    }
    Ok(Project {
        header,
        assets: sorted_unique(assets, "assets", |item| &item.id)?,
        audio_bindings: sorted_unique(audio_bindings, "audio bindings", |item| &item.id)?,
        audio_binds,
        audio_tracks: sorted_unique(audio_tracks, "audio tracks", |item| &item.id)?,
        deformers: sorted_unique(deformers, "deformers", |item| &item.id)?,
        expressions: sorted_unique(expressions, "expressions", |item| &item.id)?,
        glues: sorted_unique(glues, "glues", |item| &item.id)?,
        motions: sorted_unique(motions, "motions", |item| &item.id)?,
        parameters: sorted_unique(parameters, "parameters", |item| &item.id)?,
        parts: sorted_unique(parts, "parts", |item| &item.id)?,
        physics_groups: sorted_unique(physics_groups, "physics groups", |item| &item.id)?,
        pose: sorted_unique(pose, "pose entries", |item| &item.parameter)?,
        variant_sets: sorted_unique(variant_sets, "variant sets", |item| &item.id)?,
        game,
        content: Vec::new(),
        content_locale: None,
    })
}

fn physics_group_from_ron(value: Ron) -> Result<PhysicsGroup, Refusal> {
    let mut fields = open(value, "PhysicsGroup")?;
    let group = PhysicsGroup {
        gravity: as_point(fields.take("gravity")?, "gravity")?,
        id: fields.string("id")?,
        inputs: collect(fields.list("inputs")?, physics_input_from_ron)?,
        strands: collect(fields.list("strands")?, physics_strand_from_ron)?,
        turbulence_amplitude: fields.float("turbulence_amplitude")?,
        wind: as_point(fields.take("wind")?, "wind")?,
    };
    fields.done()?;
    Ok(group)
}

fn physics_input_from_ron(value: Ron) -> Result<PhysicsInput, Refusal> {
    let mut fields = open(value, "PhysicsInput")?;
    let input = PhysicsInput {
        parameter: fields.string("parameter")?,
        scale_degrees: fields.float("scale_degrees")?,
    };
    fields.done()?;
    Ok(input)
}

fn physics_strand_from_ron(value: Ron) -> Result<PhysicsStrand, Refusal> {
    let mut fields = open(value, "PhysicsStrand")?;
    let anchor = physics_anchor_from_ron(fields.take("anchor")?)?;
    let id = fields.string("id")?;
    let output = match fields.option("output")? {
        Some(inner) => Some(physics_output_from_ron(inner)?),
        None => None,
    };
    let particles = collect(fields.list("particles")?, physics_particle_from_ron)?;
    fields.done()?;
    Ok(PhysicsStrand {
        id,
        anchor,
        particles,
        output,
    })
}

fn physics_anchor_from_ron(value: Ron) -> Result<PhysicsAnchor, Refusal> {
    let mut fields = open(value, "PhysicsAnchor")?;
    let id = fields.string("id")?;
    let kind = match fields.ident("kind")?.as_str() {
        "Deformer" => AnchorKind::Deformer,
        "Mesh" => AnchorKind::Mesh,
        other => return unknown_variant("kind", other),
    };
    let offset = as_point(fields.take("offset")?, "offset")?;
    fields.done()?;
    Ok(PhysicsAnchor { kind, id, offset })
}

fn physics_particle_from_ron(value: Ron) -> Result<PhysicsParticle, Refusal> {
    let mut fields = open(value, "PhysicsParticle")?;
    let particle = PhysicsParticle {
        damping: fields.float("damping")?,
        delay: fields.float("delay")?,
        length: fields.float("length")?,
        mobility: fields.float("mobility")?,
        radius: fields.float("radius")?,
    };
    fields.done()?;
    Ok(particle)
}

fn physics_output_from_ron(value: Ron) -> Result<PhysicsOutput, Refusal> {
    let mut fields = open(value, "PhysicsOutput")?;
    let output = PhysicsOutput {
        offset: fields.float("offset")?,
        parameter: fields.string("parameter")?,
        scale: fields.float("scale")?,
    };
    fields.done()?;
    Ok(output)
}

fn motion_from_ron(value: Ron) -> Result<Motion, Refusal> {
    let mut fields = open(value, "Motion")?;
    let mut curves = collect(fields.list("curves")?, motion_curve_from_ron)?;
    curves.sort_by(|a, b| a.parameter.cmp(&b.parameter));
    let motion = Motion {
        id: fields.string("id")?,
        length_ticks: fields.uint("length_ticks")?,
        looped: fields.boolean("looped")?,
        curves,
    };
    fields.done()?;
    Ok(motion)
}

fn motion_curve_from_ron(value: Ron) -> Result<MotionCurve, Refusal> {
    let mut fields = open(value, "MotionCurve")?;
    let curve = MotionCurve {
        keyframes: collect(fields.list("keyframes")?, motion_keyframe_from_ron)?,
        parameter: fields.string("parameter")?,
        source: fields.opt_string("source")?,
    };
    fields.done()?;
    Ok(curve)
}

fn motion_keyframe_from_ron(value: Ron) -> Result<MotionKeyframe, Refusal> {
    let mut fields = open(value, "MotionKeyframe")?;
    let keyframe = MotionKeyframe {
        tick: fields.uint("tick")?,
        value: fields.float("value")?,
    };
    fields.done()?;
    Ok(keyframe)
}

fn audio_track_from_ron(value: Ron) -> Result<AudioTrack, Refusal> {
    let mut fields = open(value, "AudioTrack")?;
    let track = AudioTrack {
        id: fields.string("id")?,
        markers: collect(fields.list("markers")?, audio_marker_from_ron)?,
    };
    fields.done()?;
    Ok(track)
}

fn audio_marker_from_ron(value: Ron) -> Result<AudioMarker, Refusal> {
    let mut fields = open(value, "AudioMarker")?;
    let marker = AudioMarker {
        id: fields.string("id")?,
        tick: fields.uint("tick")?,
    };
    fields.done()?;
    Ok(marker)
}

fn audio_binding_from_ron(value: Ron) -> Result<AudioBinding, Refusal> {
    let mut fields = open(value, "AudioBinding")?;
    let id = fields.string("id")?;
    let kind = match fields.ident("kind")?.as_str() {
        "Voice" => {
            let character = fields.string("character")?;
            let mut delivery_fields = open(fields.take("delivery")?, "DeliveryRef")?;
            let delivery = DeliveryRef {
                identity: delivery_fields.string("identity")?,
                record: delivery_fields.string("record")?,
            };
            delivery_fields.done()?;
            AudioBindingKind::Voice(VoiceBinding {
                character,
                delivery,
            })
        }
        other => return unknown_variant("kind", other),
    };
    fields.done()?;
    Ok(AudioBinding { id, kind })
}

fn audio_bind_from_ron(value: Ron) -> Result<AudioBind, Refusal> {
    let mut fields = open(value, "AudioBind")?;
    let marker = fields.string("marker")?;
    let peak_tick = fields.uint("peak_tick")?;
    let sync_offset_ticks = match fields.take("sync_offset_ticks")? {
        Ron::Int(number) => number,
        _ => return wrong_type("sync_offset_ticks", "an integer"),
    };
    let target = audio_bind_target_from_ron(fields.take("target")?)?;
    let track = fields.string("track")?;
    let trail_ticks = fields.uint("trail_ticks")?;
    fields.done()?;
    Ok(AudioBind {
        track,
        marker,
        target,
        sync_offset_ticks,
        peak_tick,
        trail_ticks,
    })
}

fn audio_bind_target_from_ron(value: Ron) -> Result<AudioBindTarget, Refusal> {
    let mut fields = open(value, "AudioBindTarget")?;
    let kind = fields.ident("kind")?;
    let target = match kind.as_str() {
        "ParameterOffset" => AudioBindTarget::ParameterOffset {
            offset: fields.float("offset")?,
            parameter: fields.string("parameter")?,
        },
        "ExpressionTrigger" => AudioBindTarget::ExpressionTrigger {
            expression: fields.string("expression")?,
        },
        other => return unknown_variant("kind", other),
    };
    fields.done()?;
    Ok(target)
}

pub(crate) fn collect<T>(
    items: Vec<Ron>,
    convert: impl Fn(Ron) -> Result<T, Refusal>,
) -> Result<Vec<T>, Refusal> {
    items.into_iter().map(convert).collect()
}

fn header_from_ron(value: Ron) -> Result<Header, Refusal> {
    let mut fields = open(value, "Header")?;
    let format = fields.string("format")?;
    if format != PROJECT_FORMAT {
        return refuse(
            RefusalClass::UnknownKind,
            format!("project format '{format}' is not '{PROJECT_FORMAT}'"),
        );
    }
    match fields.take("format_version")? {
        Ron::Int(PROJECT_FORMAT_VERSION) => {}
        _ => {
            return refuse(
                RefusalClass::UnknownKind,
                format!("project format_version is not {PROJECT_FORMAT_VERSION}"),
            );
        }
    }
    let mut consent_fields = open(fields.take("consent")?, "Consent")?;
    let consent = Consent {
        adult: consent_fields.boolean("adult")?,
        schema: consent_fields.string("schema")?,
        wants_selected_scenes: consent_fields.boolean("wants_selected_scenes")?,
    };
    consent_fields.done()?;
    let mut stamp_fields = open(fields.take("stamp")?, "Stamp")?;
    let stamp = Stamp {
        basis: stamp_fields.string("basis")?,
        lane: stamp_fields.string("lane")?,
        pack: stamp_fields.string("pack")?,
        rating: stamp_fields.string("rating")?,
    };
    stamp_fields.done()?;
    let header = Header {
        name: fields.string("name")?,
        model_format: fields.string("model_format")?,
        stamp,
        consent,
    };
    fields.done()?;
    Ok(header)
}

fn asset_from_ron(value: Ron) -> Result<Asset, Refusal> {
    let mut fields = open(value, "Asset")?;
    let canvas_origin = match fields.option("canvas_origin")? {
        None => None,
        Some(Ron::Tuple(items)) if items.len() == 2 => match (&items[0], &items[1]) {
            (Ron::Int(x), Ron::Int(y)) => Some([*x, *y]),
            _ => return wrong_type("canvas_origin", "an integer pair"),
        },
        Some(_) => return wrong_type("canvas_origin", "an integer pair"),
    };
    let content = fields.option("content")?.map(as_rect).transpose()?;
    let kind = match fields.ident("kind")?.as_str() {
        "Png" => AssetKind::Png,
        "PsdLayer" => AssetKind::PsdLayer,
        other => return unknown_variant("kind", other),
    };
    let mut provenance_fields = open(fields.take("provenance")?, "Provenance")?;
    let provenance = Provenance {
        layer: provenance_fields
            .option("layer")?
            .map(|layer| as_string(layer, "layer"))
            .transpose()?,
        license: provenance_fields.string("license")?,
        origin: provenance_fields.string("origin")?,
        source_name: provenance_fields.string("source_name")?,
        source_sha256: provenance_fields.string("source_sha256")?,
    };
    provenance_fields.done()?;
    let asset = Asset {
        id: fields.string("id")?,
        kind,
        file: fields.string("file")?,
        sha256: fields.string("sha256")?,
        width: fields.uint("width")?,
        height: fields.uint("height")?,
        content,
        canvas_origin,
        provenance,
    };
    fields.done()?;
    Ok(asset)
}

fn keyforms_from_ron<F>(
    value: Ron,
    form_name: &str,
    form: impl Fn(&mut Fields) -> Result<F, Refusal>,
) -> Result<Keyforms<F>, Refusal> {
    let mut fields = open(value, "Keyforms")?;
    let axes = as_strings(fields.list("axes")?, "axes")?;
    let mut forms = Vec::new();
    for item in fields.list("forms")? {
        let mut form_fields = open(item, form_name)?;
        let at = form_fields
            .list("at")?
            .into_iter()
            .map(|value| as_float(value, "at"))
            .collect::<Result<Vec<f64>, Refusal>>()?;
        let payload = form(&mut form_fields)?;
        form_fields.done()?;
        forms.push(Keyform { at, form: payload });
    }
    fields.done()?;
    Ok(Keyforms { axes, forms })
}

fn mesh_form(fields: &mut Fields) -> Result<MeshForm, Refusal> {
    Ok(MeshForm {
        vertices: as_points(fields.list("vertices")?, "vertices")?,
    })
}

fn rotation_form(fields: &mut Fields) -> Result<RotationForm, Refusal> {
    Ok(RotationForm {
        angle_degrees: fields.float("angle_degrees")?,
        scale: as_point(fields.take("scale")?, "scale")?,
    })
}

fn warp_form(fields: &mut Fields) -> Result<WarpForm, Refusal> {
    Ok(WarpForm {
        positions: as_points(fields.list("positions")?, "positions")?,
    })
}

fn part_from_ron(value: Ron) -> Result<Part, Refusal> {
    let mut fields = open(value, "Part")?;
    let art = match fields.option("art")? {
        None => None,
        Some(value) => {
            let mut art_fields = open(value, "ArtBind")?;
            let art = ArtBind {
                asset: art_fields.string("asset")?,
                region: as_rect(art_fields.take("region")?)?,
            };
            art_fields.done()?;
            Some(art)
        }
    };
    let clip = match fields.option("clip")? {
        None => None,
        Some(value) => {
            let mut clip_fields = open(value, "Clip")?;
            let clip = Clip {
                inverted: clip_fields.boolean("inverted")?,
                source: clip_fields.string("source")?,
            };
            clip_fields.done()?;
            Some(clip)
        }
    };
    let completion = match fields.option("completion")? {
        None => None,
        Some(Ron::Ident(name)) => match name.as_str() {
            "NotNeeded" => Some(Completion::NotNeeded),
            "Painted" => Some(Completion::Painted),
            other => return unknown_variant("completion", other),
        },
        Some(_) => return wrong_type("completion", "a variant name"),
    };
    let draw = fields.option("draw")?.map(draw_from_ron).transpose()?;
    let keyforms = fields
        .option("keyforms")?
        .map(|value| keyforms_from_ron(value, "MeshForm", mesh_form))
        .transpose()?;
    let mesh = fields.option("mesh")?.map(mesh_from_ron).transpose()?;
    let part = Part {
        id: fields.string("id")?,
        parent: optional_string(&mut fields, "parent")?,
        deformer: optional_string(&mut fields, "deformer")?,
        art,
        mesh,
        clip,
        draw,
        visible: fields.boolean("visible")?,
        completion,
        keyforms,
    };
    fields.done()?;
    Ok(part)
}

fn optional_string(fields: &mut Fields, key: &str) -> Result<Option<String>, Refusal> {
    fields
        .option(key)?
        .map(|value| as_string(value, key))
        .transpose()
}

fn draw_from_ron(value: Ron) -> Result<Draw, Refusal> {
    let mut fields = open(value, "Draw")?;
    let draw = Draw {
        band: fields.uint("band")?,
        index: fields.uint("index")?,
        group: optional_string(&mut fields, "group")?,
    };
    fields.done()?;
    Ok(draw)
}

fn mesh_from_ron(value: Ron) -> Result<Mesh, Refusal> {
    let mut fields = open(value, "Mesh")?;
    let indices = fields
        .list("indices")?
        .into_iter()
        .map(|value| as_u32(value, "indices"))
        .collect::<Result<Vec<u32>, Refusal>>()?;
    let mesh = Mesh {
        vertices: as_points(fields.list("vertices")?, "vertices")?,
        uvs: as_points(fields.list("uvs")?, "uvs")?,
        indices,
    };
    fields.done()?;
    Ok(mesh)
}

fn deformer_from_ron(value: Ron) -> Result<Deformer, Refusal> {
    let mut fields = open(value, "Deformer")?;
    let kind = match fields.take("kind")? {
        Ron::Struct(name, items) => deformer_kind_from_ron(name, items)?,
        _ => return wrong_type("kind", "a Rotation(...) or Warp(...) record"),
    };
    let deformer = Deformer {
        id: fields.string("id")?,
        parent: optional_string(&mut fields, "parent")?,
        kind,
    };
    fields.done()?;
    Ok(deformer)
}

fn deformer_kind_from_ron(
    name: String,
    items: Vec<(String, Ron)>,
) -> Result<DeformerKind, Refusal> {
    let mut fields = Fields {
        kind: name.clone(),
        items,
    };
    let kind = match name.as_str() {
        "Rotation" => DeformerKind::Rotation(Rotation {
            base_angle_degrees: fields.float("base_angle_degrees")?,
            keyforms: fields
                .option("keyforms")?
                .map(|value| keyforms_from_ron(value, "RotationForm", rotation_form))
                .transpose()?,
            origin: as_point(fields.take("origin")?, "origin")?,
        }),
        "Warp" => {
            let bounds = match fields.take("bounds")? {
                Ron::Tuple(items) if items.len() == 4 => {
                    let mut out = [0.0; 4];
                    for (slot, item) in out.iter_mut().zip(items) {
                        *slot = as_float(item, "bounds")?;
                    }
                    out
                }
                _ => return wrong_type("bounds", "a 4-tuple (x0, y0, x1, y1)"),
            };
            DeformerKind::Warp(Warp {
                bounds,
                columns: fields.uint("columns")?,
                keyforms: fields
                    .option("keyforms")?
                    .map(|value| keyforms_from_ron(value, "WarpForm", warp_form))
                    .transpose()?,
                rows: fields.uint("rows")?,
            })
        }
        other => return unknown_variant("kind", other),
    };
    fields.done()?;
    Ok(kind)
}

fn glue_from_ron(value: Ron) -> Result<Glue, Refusal> {
    let mut fields = open(value, "Glue")?;
    let mut pairs = Vec::new();
    for item in fields.list("pairs")? {
        match item {
            Ron::Tuple(values) if values.len() == 2 => {
                let mut values = values.into_iter();
                let (Some(left), Some(right)) = (values.next(), values.next()) else {
                    return wrong_type("pairs", "index pairs");
                };
                pairs.push([as_u32(left, "pairs")?, as_u32(right, "pairs")?]);
            }
            _ => return wrong_type("pairs", "index pairs"),
        }
    }
    let glue = Glue {
        id: fields.string("id")?,
        a: fields.string("a")?,
        b: fields.string("b")?,
        pairs,
        weight: fields.float("weight")?,
    };
    fields.done()?;
    Ok(glue)
}

fn parameter_from_ron(value: Ron) -> Result<Parameter, Refusal> {
    let mut fields = open(value, "Parameter")?;
    let parameter = Parameter {
        id: fields.string("id")?,
        class: fields.string("class")?,
        min: fields.float("min")?,
        default: fields.float("default")?,
        max: fields.float("max")?,
    };
    fields.done()?;
    Ok(parameter)
}

fn pose_from_ron(value: Ron) -> Result<PoseEntry, Refusal> {
    let mut fields = open(value, "PoseEntry")?;
    let entry = PoseEntry {
        parameter: fields.string("parameter")?,
        value: fields.float("value")?,
    };
    fields.done()?;
    Ok(entry)
}

fn expression_from_ron(value: Ron) -> Result<Expression, Refusal> {
    let mut fields = open(value, "Expression")?;
    let blend = match fields.ident("blend")?.as_str() {
        "Additive" => Blend::Additive,
        "Override" => Blend::Override,
        other => return unknown_variant("blend", other),
    };
    let mut values = Vec::new();
    for item in fields.list("values")? {
        let mut row = open(item, "Value")?;
        values.push(ExpressionValue {
            parameter: row.string("parameter")?,
            value: row.float("value")?,
        });
        row.done()?;
    }
    let mut visibility = Vec::new();
    for item in fields.list("visibility")? {
        let mut row = open(item, "Visibility")?;
        visibility.push(ExpressionVisibility {
            part: row.string("part")?,
            visible: row.boolean("visible")?,
        });
        row.done()?;
    }
    let expression = Expression {
        id: fields.string("id")?,
        blend,
        values,
        visibility,
    };
    fields.done()?;
    Ok(expression)
}

fn variant_set_from_ron(value: Ron) -> Result<VariantSet, Refusal> {
    let mut fields = open(value, "VariantSet")?;
    let set = VariantSet {
        id: fields.string("id")?,
        variants: as_strings(fields.list("variants")?, "variants")?,
    };
    fields.done()?;
    Ok(set)
}
