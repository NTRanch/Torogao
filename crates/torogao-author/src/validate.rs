//! `project_validate`: the authored-project analogue of
//! `rig-check`.
//!
//! Four checks, each reported as typed findings:
//! 1. Round trip: the file must re-emit byte-for-byte
//!    (`TG-AUTH-NONCANONICAL`).
//! 2. The decide() sweep: the project is rebuilt from nothing by
//!    synthesizing the op for every stored fact and passing each
//!    through `step`; the first refusal is a finding, and a rebuilt
//!    project that differs from the stored one (rows out of
//!    canonical order, a pose entry equal to its default) is
//!    `TG-AUTH-NONCANONICAL`. Validation therefore has no rules of
//!    its own: whatever the gate would refuse, validation reports.
//! 3. Lattice completeness: every keyed target carries a form at
//!    every key intersection, with at least two keys per axis
//!    (`TG-AUTH-INCOMPLETE`).
//! 4. Asset integrity: every asset file exists under its content
//!    hash (`TG-AUTH-DANGLING` / `TG-AUTH-PROVENANCE`).

use crate::apply::step;
use crate::codec::emit_project;
use crate::decide::{Observed, check_id};
use crate::game::op::{
    CharacterDefineInput, ClockDefineInput, ConditionDefineInput, DialogueLineDefineInput,
    EffectSetDefineInput, FlagDefineInput, FlagScopeInput, GameDefineInput, GameOp,
    PresentationInput, RepetitionInput, SceneDefineInput, StatInitialInput, StatTierDefineInput,
    TriggerDefineInput, ValueDefineInput,
};
use crate::intake::sha256_hex;
use crate::limits::{
    ASSETS_MAX, AUDIO_BINDINGS_MAX, HYPNOSIS_GAIN_OFFSET_DB_MAX, HYPNOSIS_LAYERS_MAX,
    IMAGE_SIDE_MAX, PROVENANCE_TEXT_BYTES_MAX, VISUAL_BINDINGS_MAX, VISUAL_LOOP_SECONDS_MAX,
};
use crate::model::{
    Asset, AudioBinding, AudioBindingKind, DeformerKind, Keyforms, LoopSync, Project,
    VisualBinding, VisualBindingKind,
};
use crate::op::{
    ArtBindInput, BlendInput, ClipInput, Coordinate, DeformerParentInput, DrawInput,
    ExpressionDefineInput, FormInput, GlueInput, GridInput, IdInput, InitInput, KeyformSetInput,
    MeshSetInput, Op, ParamDefineInput, PartParentInput, PoseInput, RectInput, RotationCreateInput,
    VisibilityInput, VisibilityRow, WarpCreateInput,
};
use crate::refusal::{Refusal, RefusalClass};
use std::path::Path;
use torogao_format::MODEL_FORMAT_TOKEN;

/// Validates a loaded project; `text` is the file as read and
/// `root` the project directory (for asset integrity).
pub fn validate(project: &Project, text: &str, root: &Path) -> Vec<Refusal> {
    let mut findings = Vec::new();
    if emit_project(project) != text {
        findings.push(Refusal::new(
            RefusalClass::NonCanonical,
            "project.ron does not re-emit byte-for-byte; the next op re-canonicalizes it",
        ));
    }
    if project.header.model_format != MODEL_FORMAT_TOKEN {
        findings.push(Refusal::new(
            RefusalClass::UnknownKind,
            format!(
                "header model_format '{}' is not '{MODEL_FORMAT_TOKEN}'",
                project.header.model_format
            ),
        ));
    }
    match sweep(project) {
        Err(refusal) => findings.push(refusal),
        Ok(rebuilt) if rebuilt != *project => findings.push(Refusal::new(
            RefusalClass::NonCanonical,
            "stored records differ from what their ops produce (row order or redundant pose)",
        )),
        Ok(_) => {}
    }
    findings.extend(lattice_findings(project));
    findings.extend(asset_findings(project, root));
    findings.extend(game_findings(project));
    findings
}

fn run(state: Project, op: Op) -> Result<Project, Refusal> {
    let observed = Observed::Nothing;
    step(Some(state), &op, &observed).map(|stepped| stepped.project)
}

/// Rebuilds the project through the gate.
pub fn sweep(project: &Project) -> Result<Project, Refusal> {
    let header = &project.header;
    let init = Op::ProjectInit(InitInput {
        name: header.name.clone(),
        stamp: header.stamp.clone(),
        consent: header.consent.clone(),
    });
    let mut rebuilt = step(None, &init, &Observed::Nothing)?.project;
    rebuilt.header.model_format = header.model_format.clone();
    for asset in &project.assets {
        check_asset(asset)?;
    }
    if project.assets.len() > ASSETS_MAX {
        return Err(Refusal::new(RefusalClass::Range, "too many assets"));
    }
    rebuilt.assets = project.assets.clone();
    for op in structure_ops(project) {
        rebuilt = run(rebuilt, op)?;
    }
    for part in &project.parts {
        let Ok(index) = rebuilt.parts.binary_search_by(|p| p.id.cmp(&part.id)) else {
            unreachable!("the sweep created every part");
        };
        rebuilt.parts[index].completion = part.completion;
    }
    for op in content_ops(project) {
        rebuilt = run(rebuilt, op)?;
    }
    for op in rigx_ops(project) {
        rebuilt = run(rebuilt, op)?;
    }
    for op in game_ops(project) {
        rebuilt = run(rebuilt, op)?;
    }
    // Sidecar and version state the ops do not author: the
    // game format stamp is migration state (an untouched older
    // section is valid, not non-canonical), the sidecar locale
    // is a property of the sidecar file, and content entries no
    // record references are game_findings' business, not the
    // sweep's. Each is copied across so the sweep judges exactly
    // the op-authored records.
    if let (Some(rebuilt_game), Some(stored_game)) = (&mut rebuilt.game, &project.game) {
        rebuilt_game.format_version = stored_game.format_version;
    }
    rebuilt.content_locale = project.content_locale.clone();
    for entry in &project.content {
        let referenced = project.game.as_ref().is_some_and(|game| {
            game.characters.iter().any(|c| c.name_ref == entry.id)
                || game.dialogue.iter().any(|l| l.text_ref == entry.id)
                || game.choices.iter().any(|choice| {
                    choice.prompt_line.as_deref() == Some(entry.id.as_str())
                        || choice
                            .options
                            .iter()
                            .any(|option| option.label_ref == entry.id)
                })
                || game.threads.iter().any(|thread| {
                    thread
                        .messages
                        .iter()
                        .any(|message| message.line == entry.id)
                })
        });
        if !referenced && rebuilt.content.iter().all(|e| e.id != entry.id) {
            let position = rebuilt
                .content
                .iter()
                .position(|e| e.id > entry.id)
                .unwrap_or(rebuilt.content.len());
            rebuilt.content.insert(position, entry.clone());
        }
    }
    for set in &project.variant_sets {
        check_id(&set.id, "variant set id")?;
        let all_parts = set.variants.iter().all(|id| project.part(id).is_some());
        if set.variants.len() < 2 || !all_parts {
            return Err(Refusal::new(
                RefusalClass::Dangling,
                format!("variant set '{}' needs two or more existing parts", set.id),
            ));
        }
    }
    rebuilt.variant_sets = project.variant_sets.clone();
    // Audio bindings are authored by `audio_binding_set`
    // since Q1 (D-634); the sweep checks each stored row and
    // carries the section across, as it does for assets and
    // variant sets. Lock-state admission is NOT checked here:
    // unadmitted bindings are legal while authoring; the
    // shippable rule is `crate::voice::validate_shippable`.
    for binding in &project.audio_bindings {
        check_audio_binding(binding)?;
    }
    if project.audio_bindings.len() > AUDIO_BINDINGS_MAX {
        return Err(Refusal::new(RefusalClass::Range, "too many audio bindings"));
    }
    rebuilt.audio_bindings = project.audio_bindings.clone();
    for binding in &project.visual_bindings {
        check_visual_binding(binding)?;
    }
    if project.visual_bindings.len() > VISUAL_BINDINGS_MAX {
        return Err(Refusal::new(
            RefusalClass::Range,
            "too many visual bindings",
        ));
    }
    rebuilt.visual_bindings = project.visual_bindings.clone();
    // The generation record (D-822) is authored by
    // `conditioning_set` in feature builds; the sweep carries
    // the section across like assets and variant sets. The
    // base record's id is checked in every build; the full
    // gencond validation runs where the feature exists (in a
    // build without it the conditioning subtree is inert data
    // by design, and there is nothing here to judge it with).
    if let Some(generation) = &project.generation {
        check_id(&generation.id, "generation record id")?;
        #[cfg(feature = "gen-conditioning")]
        crate::conditioning::check_stored_record(generation)?;
    }
    rebuilt.generation = project.generation.clone();
    // Session records (D-838) ride the sweep the same way:
    // ids checked in every build, the typed record parsed
    // and judged where the feature exists; without it each
    // record is inert data and there is nothing to judge
    // it with.
    for scene in &project.scenes {
        check_id(&scene.id, "scene record id")?;
        #[cfg(feature = "gen-conditioning")]
        crate::conditioning::check_stored_scene(scene)?;
    }
    rebuilt.scenes = project.scenes.clone();
    Ok(rebuilt)
}

/// A reference string's facts in a stored row: present and
/// within the provenance text bound (namespace strings, never
/// id-checked here).
fn check_ref_field(row: &str, what: &str, value: &str) -> Result<(), Refusal> {
    if value.is_empty() || value.len() > PROVENANCE_TEXT_BYTES_MAX {
        return Err(Refusal::new(
            RefusalClass::Malformed,
            format!("binding '{row}' {what} reference is empty or over the bound"),
        ));
    }
    Ok(())
}

/// The facts a stored audio-binding row must carry: the row id
/// and its character in id syntax; the torori delivery
/// references present and within the provenance text bound
/// (they are torori-namespace strings, never id-checked here).
/// A hypnosis-track row additionally carries a bounded,
/// kind-unique layer stack with in-range gain offsets.
fn check_audio_binding(binding: &AudioBinding) -> Result<(), Refusal> {
    check_id(&binding.id, "audio binding id")?;
    match &binding.kind {
        AudioBindingKind::Voice(voice) => {
            check_id(&voice.character, "character id")?;
            for (what, value) in [
                ("identity", &voice.delivery.identity),
                ("record", &voice.delivery.record),
            ] {
                check_ref_field(&binding.id, &format!("delivery {what}"), value)?;
            }
        }
        AudioBindingKind::HypnosisTrack(track) => {
            check_ref_field(&binding.id, "session", &track.session)?;
            if track.layers.is_empty() || track.layers.len() > HYPNOSIS_LAYERS_MAX {
                return Err(Refusal::new(
                    RefusalClass::Range,
                    format!(
                        "audio binding '{}' layer count {} outside [1, {}]",
                        binding.id,
                        track.layers.len(),
                        HYPNOSIS_LAYERS_MAX
                    ),
                ));
            }
            let mut kinds = std::collections::BTreeSet::new();
            for layer in &track.layers {
                if !kinds.insert(layer.kind.label()) {
                    return Err(Refusal::new(
                        RefusalClass::Malformed,
                        format!(
                            "audio binding '{}' names layer kind '{}' twice",
                            binding.id,
                            layer.kind.label()
                        ),
                    ));
                }
                check_ref_field(&binding.id, "layer stem", &layer.stem)?;
                if !layer.gain_offset_db.is_finite()
                    || layer.gain_offset_db.abs() > HYPNOSIS_GAIN_OFFSET_DB_MAX
                {
                    return Err(Refusal::new(
                        RefusalClass::Range,
                        format!(
                            "audio binding '{}' layer gain offset {} dB outside the bound",
                            binding.id, layer.gain_offset_db
                        ),
                    ));
                }
                if let Some(envelope) = &layer.envelope {
                    check_ref_field(&binding.id, "layer envelope", envelope)?;
                }
                for edition in &layer.editions {
                    check_id(edition, "edition id")?;
                }
            }
        }
    }
    Ok(())
}

/// The facts a stored visual-binding row must carry.
fn check_visual_binding(binding: &VisualBinding) -> Result<(), Refusal> {
    check_id(&binding.id, "visual binding id")?;
    match &binding.kind {
        VisualBindingKind::VisualLoop(loop_binding) => {
            check_ref_field(&binding.id, "loop asset", &loop_binding.asset)?;
            check_ref_field(&binding.id, "session", &loop_binding.session)?;
            if let LoopSync::Count(count) = loop_binding.sync {
                if count == 0 {
                    return Err(Refusal::new(
                        RefusalClass::Range,
                        format!("visual binding '{}' loop count is zero", binding.id),
                    ));
                }
            }
            if !loop_binding.duration_seconds.is_finite()
                || loop_binding.duration_seconds <= 0.0
                || loop_binding.duration_seconds > VISUAL_LOOP_SECONDS_MAX
            {
                return Err(Refusal::new(
                    RefusalClass::Range,
                    format!(
                        "visual binding '{}' loop duration {} s outside the bound",
                        binding.id, loop_binding.duration_seconds
                    ),
                ));
            }
            if loop_binding.frame_rate == 0 || loop_binding.frame_rate > 240 {
                return Err(Refusal::new(
                    RefusalClass::Range,
                    format!(
                        "visual binding '{}' frame rate {} outside [1, 240]",
                        binding.id, loop_binding.frame_rate
                    ),
                ));
            }
        }
        VisualBindingKind::OverlayHook(hook) => {
            check_ref_field(&binding.id, "overlay hook", &hook.hook)?;
            check_ref_field(&binding.id, "session", &hook.session)?;
            check_ref_field(&binding.id, "opacity envelope", &hook.opacity_envelope)?;
            if !hook.rotation_rate.is_finite() || hook.rotation_rate.abs() > 100.0 {
                return Err(Refusal::new(
                    RefusalClass::Range,
                    format!(
                        "visual binding '{}' rotation rate {} rad/s outside the bound",
                        binding.id, hook.rotation_rate
                    ),
                ));
            }
        }
    }
    Ok(())
}

fn check_text(text: &str, what: &str) -> Result<(), Refusal> {
    if text.trim().is_empty() || text.len() > PROVENANCE_TEXT_BYTES_MAX {
        return Err(Refusal::new(
            RefusalClass::Provenance,
            format!("asset {what} is missing or over the bound"),
        ));
    }
    Ok(())
}

fn is_sha256(text: &str) -> bool {
    text.len() == 64
        && text
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// The facts an intake op establishes for one asset record.
fn check_asset(asset: &Asset) -> Result<(), Refusal> {
    check_id(&asset.id, "asset id")?;
    let provenance = &asset.provenance;
    check_text(&provenance.license, "license")?;
    check_text(&provenance.origin, "origin")?;
    check_text(&provenance.source_name, "source name")?;
    if !is_sha256(&asset.sha256) || !is_sha256(&provenance.source_sha256) {
        return Err(Refusal::new(
            RefusalClass::Provenance,
            format!("asset '{}' carries a malformed sha256", asset.id),
        ));
    }
    if asset.file != format!("assets/{}.png", asset.sha256) {
        return Err(Refusal::new(
            RefusalClass::Malformed,
            format!("asset '{}' file is not its content-hashed name", asset.id),
        ));
    }
    let sides_ok =
        (1..=IMAGE_SIDE_MAX).contains(&asset.width) && (1..=IMAGE_SIDE_MAX).contains(&asset.height);
    let content_ok = asset.content.is_none_or(|rect| {
        rect.width > 0
            && rect.height > 0
            && rect
                .x
                .checked_add(rect.width)
                .is_some_and(|end| end <= asset.width)
            && rect
                .y
                .checked_add(rect.height)
                .is_some_and(|end| end <= asset.height)
    });
    if !sides_ok || !content_ok {
        return Err(Refusal::new(
            RefusalClass::Range,
            format!(
                "asset '{}' dimensions or content bounds are out of range",
                asset.id
            ),
        ));
    }
    Ok(())
}

/// The G-10 sections: physics groups, motions and their
/// curves, audio tracks and their binding rows rebuild through
/// their define/set verbs. They run after `content_ops` in the
/// sweep: they depend on parameters, parts, and deformers
/// (rebuilt by `structure_ops`) and on expressions (rebuilt by
/// `content_ops` — audio binds reference them).
fn rigx_ops(project: &Project) -> Vec<Op> {
    let mut ops = Vec::new();
    for group in &project.physics_groups {
        ops.push(Op::PhysicsGroupDefine(crate::physics::group_inputs(group)));
    }
    for motion in &project.motions {
        ops.push(Op::MotionCreate(crate::motion::motion_inputs(motion)));
        for curve in &motion.curves {
            match crate::motion::curve_rebuild(&motion.id, curve) {
                crate::motion::CurveRebuild::Set(inputs) => ops.push(Op::CurveSet(inputs)),
                crate::motion::CurveRebuild::FromSamples(inputs) => {
                    ops.push(Op::CurveFromSamples(inputs))
                }
            }
        }
    }
    for track in &project.audio_tracks {
        ops.push(Op::AudioTrackDefine(crate::audio::track_inputs(track)));
    }
    for bind in &project.audio_binds {
        ops.push(Op::AudioBindSet(crate::audio::bind_inputs(bind)));
    }
    ops
}

/// Parameters, nodes, and the two trees.
fn structure_ops(project: &Project) -> Vec<Op> {
    let mut ops = Vec::new();
    for parameter in &project.parameters {
        ops.push(Op::ParamDefine(ParamDefineInput {
            id: parameter.id.clone(),
            class: parameter.class.clone(),
            min: parameter.min,
            default: parameter.default,
            max: parameter.max,
        }));
    }
    for part in &project.parts {
        ops.push(Op::PartCreate(IdInput {
            id: part.id.clone(),
        }));
    }
    for deformer in &project.deformers {
        ops.push(match &deformer.kind {
            DeformerKind::Rotation(rotation) => Op::DeformerRotationCreate(RotationCreateInput {
                id: deformer.id.clone(),
                origin: rotation.origin,
                base_angle_degrees: rotation.base_angle_degrees,
            }),
            DeformerKind::Warp(warp) => Op::DeformerWarpCreate(WarpCreateInput {
                id: deformer.id.clone(),
                columns: warp.columns,
                rows: warp.rows,
                bounds: warp.bounds,
            }),
        });
    }
    for part in project.parts.iter().filter(|p| p.parent.is_some()) {
        ops.push(Op::PartParentSet(PartParentInput {
            part: part.id.clone(),
            parent: part.parent.clone(),
        }));
    }
    for deformer in project.deformers.iter().filter(|d| d.parent.is_some()) {
        ops.push(Op::DeformerParentSet(DeformerParentInput {
            target: deformer.id.clone(),
            parent: deformer.parent.clone(),
        }));
    }
    ops
}

/// Per-part content, keyforms, glues, pose, and expressions.
fn content_ops(project: &Project) -> Vec<Op> {
    let mut ops = Vec::new();
    for part in &project.parts {
        let id = || part.id.clone();
        if part.deformer.is_some() {
            ops.push(Op::DeformerParentSet(DeformerParentInput {
                target: id(),
                parent: part.deformer.clone(),
            }));
        }
        if let Some(art) = &part.art {
            let region = art.region;
            ops.push(Op::PartArtBind(ArtBindInput {
                part: id(),
                asset: art.asset.clone(),
                region: RectInput {
                    x: region.x,
                    y: region.y,
                    width: region.width,
                    height: region.height,
                },
            }));
        }
        if let Some(mesh) = &part.mesh {
            ops.push(Op::MeshSet(MeshSetInput {
                part: id(),
                vertices: mesh.vertices.clone(),
                uvs: mesh.uvs.clone(),
                indices: mesh.indices.clone(),
            }));
        }
        if let Some(draw) = &part.draw {
            ops.push(Op::PartDrawOrderSet(DrawInput {
                part: id(),
                band: draw.band,
                index: draw.index,
                group: draw.group.clone(),
            }));
        }
        if !part.visible {
            ops.push(Op::PartVisibilitySet(VisibilityInput {
                part: id(),
                visible: false,
            }));
        }
    }
    for part in project.parts.iter().filter(|p| p.clip.is_some()) {
        if let Some(clip) = &part.clip {
            ops.push(Op::PartClipSet(ClipInput {
                part: part.id.clone(),
                source: Some(clip.source.clone()),
                inverted: clip.inverted,
            }));
        }
    }
    keyform_ops(project, &mut ops);
    for glue in &project.glues {
        ops.push(Op::DeformerGlueSet(GlueInput {
            id: glue.id.clone(),
            a: glue.a.clone(),
            b: glue.b.clone(),
            pairs: glue.pairs.clone(),
            weight: glue.weight,
        }));
    }
    for entry in &project.pose {
        ops.push(Op::ParamPoseDefaultSet(PoseInput {
            parameter: entry.parameter.clone(),
            value: entry.value,
        }));
    }
    for expression in &project.expressions {
        ops.push(Op::ExpressionDefine(ExpressionDefineInput {
            id: expression.id.clone(),
            blend: match expression.blend {
                crate::model::Blend::Additive => BlendInput::Additive,
                crate::model::Blend::Override => BlendInput::Override,
            },
            values: expression
                .values
                .iter()
                .map(|row| Coordinate {
                    parameter: row.parameter.clone(),
                    value: row.value,
                })
                .collect(),
            visibility: expression
                .visibility
                .iter()
                .map(|row| VisibilityRow {
                    part: row.part.clone(),
                    visible: row.visible,
                })
                .collect(),
        }));
    }
    ops
}

fn lattice_ops<F>(
    target: &str,
    keyforms: &Keyforms<F>,
    ops: &mut Vec<Op>,
    payload: impl Fn(&F) -> FormInput,
) {
    if let [first, second] = &keyforms.axes[..] {
        ops.push(Op::ParamGridDefine(GridInput {
            target: target.to_string(),
            axes: [first.clone(), second.clone()],
        }));
    }
    for form in &keyforms.forms {
        ops.push(Op::KeyformSet(KeyformSetInput {
            target: target.to_string(),
            at: keyforms
                .axes
                .iter()
                .zip(&form.at)
                .map(|(parameter, value)| Coordinate {
                    parameter: parameter.clone(),
                    value: *value,
                })
                .collect(),
            form: payload(&form.form),
        }));
    }
}

fn keyform_ops(project: &Project, ops: &mut Vec<Op>) {
    for part in &project.parts {
        if let Some(keyforms) = &part.keyforms {
            lattice_ops(&part.id, keyforms, ops, |form| FormInput::Mesh {
                vertices: form.vertices.clone(),
            });
        }
    }
    for deformer in &project.deformers {
        match &deformer.kind {
            DeformerKind::Rotation(rotation) => {
                if let Some(keyforms) = &rotation.keyforms {
                    lattice_ops(&deformer.id, keyforms, ops, |form| FormInput::Rotation {
                        angle_degrees: form.angle_degrees,
                        scale: form.scale,
                    });
                }
            }
            DeformerKind::Warp(warp) => {
                if let Some(keyforms) = &warp.keyforms {
                    lattice_ops(&deformer.id, keyforms, ops, |form| FormInput::Warp {
                        positions: form.positions.clone(),
                    });
                }
            }
        }
    }
}

/// The distinct keys per axis of a lattice, ascending.
pub fn lattice_keys<F>(keyforms: &Keyforms<F>) -> Vec<Vec<f64>> {
    (0..keyforms.axes.len())
        .map(|axis| {
            let mut keys: Vec<f64> = keyforms
                .forms
                .iter()
                .filter_map(|f| f.at.get(axis).copied())
                .collect();
            keys.sort_by(f64::total_cmp);
            keys.dedup();
            keys
        })
        .collect()
}

fn lattice_finding<F>(owner: &str, keyforms: &Keyforms<F>) -> Option<Refusal> {
    let keys = lattice_keys(keyforms);
    let expected: usize = keys.iter().map(Vec::len).product();
    let thin_axis = keys.iter().any(|axis| axis.len() < 2);
    if thin_axis || keyforms.forms.len() != expected {
        return Some(Refusal::new(
            RefusalClass::Incomplete,
            format!(
                "'{owner}' keyforms on {:?}: {} of {expected} intersections authored, \
                 key counts {:?} (each axis needs >= 2)",
                keyforms.axes,
                keyforms.forms.len(),
                keys.iter().map(Vec::len).collect::<Vec<_>>()
            ),
        ));
    }
    None
}

fn lattice_findings(project: &Project) -> Vec<Refusal> {
    let mut findings = Vec::new();
    for part in &project.parts {
        if let Some(finding) = part
            .keyforms
            .as_ref()
            .and_then(|k| lattice_finding(&part.id, k))
        {
            findings.push(finding);
        }
    }
    for deformer in &project.deformers {
        let finding = match &deformer.kind {
            DeformerKind::Rotation(r) => r
                .keyforms
                .as_ref()
                .and_then(|k| lattice_finding(&deformer.id, k)),
            DeformerKind::Warp(w) => w
                .keyforms
                .as_ref()
                .and_then(|k| lattice_finding(&deformer.id, k)),
        };
        findings.extend(finding);
    }
    findings
}

/// The game section's ops, in authoring (dependency) order —
/// computed topologically from the records' own references,
/// since the section's one genuine cycle (a table names its
/// outcomes' effect sets; an effect set may roll that table)
/// has no fixed order. The prose each prose-authoring op
/// carries is read back from the stored sidecar entries; a
/// reference with no entry synthesizes an empty text, which
/// the gate refuses — and `game_findings` names the dangling
/// reference itself.
fn game_ops(project: &Project) -> Vec<Op> {
    let Some(game) = &project.game else {
        return Vec::new();
    };
    let prose = |id: &str| {
        project
            .content
            .iter()
            .find(|entry| entry.id == id)
            .map(|entry| entry.text.clone())
            .unwrap_or_default()
    };
    // The ops are collected with their record dependencies and
    // emitted in a stable topological order (earliest canonical
    // position first). A fixed order cannot express the game
    // section's one genuine cycle — a table names its outcomes'
    // effect sets, and an effect set may roll that table — so
    // the sweep orders records the way an author could have:
    // every record after the records it names.
    let mut items: Vec<SweepOp> = Vec::new();
    let mut push = |key: String, deps: Vec<String>, op: Op| {
        items.push(SweepOp { key, deps, op });
    };
    push(
        "game".to_string(),
        Vec::new(),
        Op::Game(GameOp::GameDefine(GameDefineInput {
            id: game.id.clone(),
            start_scene: game.start_scene.clone(),
            save_schema_version: Some(game.save_schema_version),
            alter_ego_veto: game.alter_ego_veto.clone(),
        })),
    );
    if let Some(clock) = &game.clock {
        push(
            "clock".to_string(),
            vec!["game".to_string()],
            Op::Game(GameOp::ClockDefine(ClockDefineInput {
                slots: clock.slots.clone(),
                days_per_week: clock.days_per_week,
                day_limit: clock.day_limit,
            })),
        );
    }
    for stream in &game.streams {
        push(
            format!("stream:{}", stream.id),
            vec!["game".to_string()],
            Op::Game(GameOp::RngStreamDefine(
                crate::game::op::RngStreamDefineInput {
                    id: stream.id.clone(),
                    algorithm: stream.algorithm.clone(),
                },
            )),
        );
    }
    for stat in &game.stats {
        push(
            format!("stat:{}", stat.id),
            vec!["game".to_string()],
            Op::Game(GameOp::StatDefine(crate::game::apply::stat_input_of(stat))),
        );
    }
    for tier in &game.tiers {
        push(
            format!("tier:{}", tier.stat),
            vec![format!("stat:{}", tier.stat)],
            Op::Game(GameOp::StatTierDefine(StatTierDefineInput {
                stat: tier.stat.clone(),
                thresholds: tier.thresholds.clone(),
                names: tier.names.clone(),
            })),
        );
    }
    for flag in &game.flags {
        push(
            format!("flag:{}", flag.id),
            vec!["game".to_string()],
            Op::Game(GameOp::FlagDefine(FlagDefineInput {
                id: flag.id.clone(),
                scope: match flag.scope {
                    crate::game::model::FlagScope::Globals => FlagScopeInput::Globals,
                    crate::game::model::FlagScope::Run => FlagScopeInput::Run,
                },
                default: flag.default,
            })),
        );
    }
    for character in &game.characters {
        let mut deps = vec!["game".to_string()];
        for row in &character.initials {
            deps.push(format!("stat:{}", row.stat));
        }
        push(
            format!("character:{}", character.id),
            deps,
            Op::Game(GameOp::CharacterDefine(CharacterDefineInput {
                id: character.id.clone(),
                name_ref: character.name_ref.clone(),
                name_text: prose(&character.name_ref),
                initials: character
                    .initials
                    .iter()
                    .map(|row| StatInitialInput {
                        stat: row.stat.clone(),
                        value: row.value,
                    })
                    .collect(),
            })),
        );
    }
    for appearance in &game.appearances {
        push(
            format!("appearance:{}", appearance.character),
            vec![
                "game".to_string(),
                format!("character:{}", appearance.character),
            ],
            Op::Game(GameOp::AppearanceDefine(
                crate::game::apply::appearance_input_of(appearance),
            )),
        );
    }
    for state in &game.visual_states {
        push(
            format!("vstate:{}:{}", state.character, state.state),
            vec![
                "game".to_string(),
                format!("appearance:{}", state.character),
            ],
            Op::Game(GameOp::VisualStateDefine(
                crate::game::apply::visual_state_input_of(state),
            )),
        );
    }
    for scene in &game.scenes {
        let mut deps = vec!["game".to_string()];
        for participant in &scene.participants {
            deps.push(format!("character:{participant}"));
        }
        if let Some(condition) = &scene.entry_condition {
            deps.push(format!("condition:{condition}"));
        }
        if let Some(set) = &scene.on_enter {
            deps.push(format!("set:{set}"));
        }
        if let Some(set) = &scene.on_exit {
            deps.push(format!("set:{set}"));
        }
        if let Some(interlude) = &scene.thread_interlude {
            deps.push(format!("thread:{}", interlude.thread));
        }
        push(
            format!("scene:{}", scene.id),
            deps,
            Op::Game(GameOp::SceneDefine(SceneDefineInput {
                id: scene.id.clone(),
                presentation: match scene.presentation {
                    crate::game::model::Presentation::Event => PresentationInput::Event,
                    crate::game::model::Presentation::Room => PresentationInput::Room,
                    crate::game::model::Presentation::Vn => PresentationInput::Vn,
                },
                participants: scene.participants.clone(),
                backdrop: scene.backdrop.clone(),
                entry_condition: scene.entry_condition.clone(),
                on_enter: scene.on_enter.clone(),
                on_exit: scene.on_exit.clone(),
                thread_interlude: scene.thread_interlude.as_ref().map(|interlude| {
                    crate::game::op::ThreadInterludeInput {
                        thread: interlude.thread.clone(),
                        reply_delay_ms: interlude.reply_delay_ms,
                    }
                }),
            })),
        );
    }
    for thread in &game.threads {
        let mut deps = vec!["game".to_string()];
        for participant in &thread.participants {
            deps.push(format!("character:{participant}"));
        }
        push(
            format!("thread:{}", thread.id),
            deps,
            Op::Game(GameOp::ThreadCreate(
                crate::game::apply::thread_create_input_of(thread),
            )),
        );
        for message in &thread.messages {
            let mut deps = vec![
                "game".to_string(),
                format!("thread:{}", thread.id),
                format!("character:{}", message.speaker),
            ];
            // The message's prose lives in the sidecar, authored
            // by whichever character or dialogue op carries that
            // content id (C-7: decide() requires the entry to
            // exist), so the post rebuilds after them.
            if let Some(character) = game.characters.iter().find(|c| c.name_ref == message.line) {
                deps.push(format!("character:{}", character.id));
            }
            if let Some(line) = game.dialogue.iter().find(|l| l.text_ref == message.line) {
                deps.push(format!("line:{}", line.id));
            }
            push(
                format!("threadmsg:{}:{}", thread.id, message.id),
                deps,
                Op::Game(GameOp::ThreadPost(
                    crate::game::apply::thread_post_input_of(&thread.id, message),
                )),
            );
        }
    }
    for value in &game.values {
        let mut deps = vec!["game".to_string()];
        expr_deps(&value.expr, &mut deps);
        push(
            format!("value:{}", value.id),
            deps,
            Op::Game(GameOp::ValueDefine(ValueDefineInput {
                id: value.id.clone(),
                expr: crate::game::expr::expr_to_input(&value.expr),
            })),
        );
    }
    for condition in &game.conditions {
        let mut deps = vec!["game".to_string()];
        expr_deps(&condition.expr, &mut deps);
        push(
            format!("condition:{}", condition.id),
            deps,
            Op::Game(GameOp::ConditionDefine(ConditionDefineInput {
                id: condition.id.clone(),
                expr: crate::game::expr::expr_to_input(&condition.expr),
            })),
        );
    }
    for set in &game.effect_sets {
        let mut deps = vec!["game".to_string()];
        for atom in &set.atoms {
            atom_deps(atom, &mut deps);
        }
        push(
            format!("set:{}", set.id),
            deps,
            Op::Game(GameOp::EffectSetDefine(EffectSetDefineInput {
                id: set.id.clone(),
                atoms: set
                    .atoms
                    .iter()
                    .map(crate::game::apply::atom_input_of)
                    .collect(),
            })),
        );
    }
    for table in &game.tables {
        let mut deps = vec!["game".to_string(), format!("stream:{}", table.stream)];
        for outcome in &table.outcomes {
            deps.push(format!("set:{}", outcome.effect_set));
            if let Some(gate) = &outcome.gate {
                deps.push(format!("condition:{gate}"));
            }
        }
        push(
            format!("table:{}", table.id),
            deps,
            Op::Game(GameOp::TableDefine(crate::game::apply::table_input_of(
                table,
            ))),
        );
    }
    let mut lines: Vec<&crate::game::model::DialogueLine> = game.dialogue.iter().collect();
    lines.sort_by(|a, b| (a.scene.as_str(), a.ordinal).cmp(&(b.scene.as_str(), b.ordinal)));
    for line in lines {
        let mut deps = vec!["game".to_string(), format!("scene:{}", line.scene)];
        if line.speaker != "narrator" {
            deps.push(format!("character:{}", line.speaker));
        }
        push(
            format!("line:{}", line.id),
            deps,
            Op::Game(GameOp::DialogueLineDefine(DialogueLineDefineInput {
                id: line.id.clone(),
                scene: line.scene.clone(),
                ordinal: line.ordinal,
                speaker: line.speaker.clone(),
                text_ref: line.text_ref.clone(),
                text: prose(&line.text_ref),
                next: line.next.clone(),
                hook_expression: line.hook_expression.clone(),
            })),
        );
    }
    for trigger in &game.triggers {
        let mut deps = vec!["game".to_string(), format!("set:{}", trigger.effect_set)];
        if let Some(gate) = &trigger.gate {
            deps.push(format!("condition:{gate}"));
        }
        match &trigger.kind {
            crate::game::model::TriggerKind::OnEnterScene { scene } => {
                deps.push(format!("scene:{scene}"));
            }
            crate::game::model::TriggerKind::OnFlag { flag, .. } => {
                deps.push(format!("flag:{flag}"));
            }
            crate::game::model::TriggerKind::OnThreshold {
                character, stat, ..
            } => {
                deps.push(format!("stat:{stat}"));
                if let Some(who) = character {
                    deps.push(format!("character:{who}"));
                }
            }
            crate::game::model::TriggerKind::OnTick { .. } => {
                deps.push("clock".to_string());
            }
            crate::game::model::TriggerKind::OnEvent { .. } => {}
        }
        push(
            format!("trigger:{}", trigger.id),
            deps,
            Op::Game(GameOp::TriggerDefine(TriggerDefineInput {
                id: trigger.id.clone(),
                kind: crate::game::apply::trigger_kind_input_of(&trigger.kind),
                effect_set: trigger.effect_set.clone(),
                gate: trigger.gate.clone(),
                priority: trigger.priority,
                repetition: match trigger.repetition {
                    crate::game::model::Repetition::Once => RepetitionInput::Once,
                    crate::game::model::Repetition::Repeat => RepetitionInput::Repeat,
                },
                cooldown_ticks: trigger.cooldown_ticks,
                offer: trigger
                    .offer
                    .as_ref()
                    .map(|offer| crate::game::op::OfferInput {
                        expires_days: offer.expires_days,
                    }),
            })),
        );
    }
    if let Some(schema) = &game.save_schema {
        push(
            "schema".to_string(),
            vec!["game".to_string()],
            Op::Game(GameOp::GameSaveSchemaDefine(
                crate::game::apply::save_schema_input_of(schema),
            )),
        );
    }
    for exit in &game.scene_exits {
        let mut deps = vec![
            "game".to_string(),
            format!("scene:{}", exit.scene),
            format!("scene:{}", exit.target),
        ];
        if let Some(condition) = &exit.condition {
            deps.push(format!("condition:{condition}"));
        }
        if let Some(set) = &exit.effect_set {
            deps.push(format!("set:{set}"));
        }
        push(
            format!("exit:{}:{}", exit.scene, exit.id),
            deps,
            Op::Game(GameOp::SceneExitDefine(
                crate::game::apply::scene_exit_input_of(exit),
            )),
        );
    }
    for choice in &game.choices {
        let mut deps = vec!["game".to_string(), format!("scene:{}", choice.scene)];
        // The choice's label/prompt prose lives in the
        // sidecar, authored by whichever character or dialogue
        // op carries that content id (C-7: decide() requires
        // the entry to exist), so the choice rebuilds after
        // them.
        let mut prose_keys: Vec<&str> = choice
            .options
            .iter()
            .map(|option| option.label_ref.as_str())
            .collect();
        if let Some(prompt) = &choice.prompt_line {
            prose_keys.push(prompt);
        }
        for key in prose_keys {
            if let Some(character) = game.characters.iter().find(|c| c.name_ref == key) {
                deps.push(format!("character:{}", character.id));
            }
            if let Some(line) = game.dialogue.iter().find(|l| l.text_ref == key) {
                deps.push(format!("line:{}", line.id));
            }
        }
        for option in &choice.options {
            if let Some(condition) = &option.visible_if {
                deps.push(format!("condition:{condition}"));
            }
            if let Some(condition) = &option.enabled_if {
                deps.push(format!("condition:{condition}"));
            }
            if let Some(set) = &option.effect_set {
                deps.push(format!("set:{set}"));
            }
            if let Some(sort) = &option.sort {
                expr_deps(sort, &mut deps);
            }
            match &option.target {
                crate::game::model::ChoiceTarget::EndingCheck => {}
                crate::game::model::ChoiceTarget::Exit(id) => {
                    deps.push(format!("exit:{}:{id}", choice.scene));
                }
                crate::game::model::ChoiceTarget::Line(id) => {
                    deps.push(format!("line:{id}"));
                }
                crate::game::model::ChoiceTarget::Scene(id) => {
                    deps.push(format!("scene:{id}"));
                }
            }
        }
        push(
            format!("choice:{}", choice.id),
            deps,
            Op::Game(GameOp::ChoiceDefine(crate::game::apply::choice_input_of(
                choice,
            ))),
        );
    }
    for ending in &game.endings {
        push(
            format!("ending:{}", ending.id),
            vec![
                "game".to_string(),
                format!("condition:{}", ending.condition),
                format!("scene:{}", ending.epilogue_scene),
            ],
            Op::Game(GameOp::EndingDefine(crate::game::apply::ending_input_of(
                ending,
            ))),
        );
    }
    for entry in &game.gallery_entries {
        let mut deps = vec![
            "game".to_string(),
            format!("scene:{}", entry.replay_scene),
            format!("line:{}", entry.replay_line),
        ];
        if let crate::game::model::GalleryUnlock::Condition(id) = &entry.unlock {
            deps.push(format!("condition:{id}"));
        }
        push(
            format!("gallery:{}", entry.id),
            deps,
            Op::Game(GameOp::GalleryEntryDefine(
                crate::game::apply::gallery_input_of(entry),
            )),
        );
    }
    for schedule in &game.schedules {
        let mut deps = vec![
            "game".to_string(),
            "clock".to_string(),
            format!("character:{}", schedule.character),
        ];
        for row in &schedule.slots {
            deps.push(format!("scene:{}", row.scene));
        }
        for row in &schedule.overrides {
            deps.push(format!("condition:{}", row.condition));
            deps.push(format!("scene:{}", row.scene));
        }
        push(
            format!("schedule:{}", schedule.character),
            deps,
            Op::Game(GameOp::ScheduleDefine(
                crate::game::apply::schedule_input_of(schedule),
            )),
        );
    }
    for binding in &game.scene_bindings {
        push(
            format!("binding:{}:{}", binding.scene, binding.participant),
            vec!["game".to_string(), format!("scene:{}", binding.scene)],
            Op::Game(GameOp::SceneBindRig(
                crate::game::apply::scene_binding_input_of(binding),
            )),
        );
    }
    topo_order(items)
}

/// One sweep op with the record keys it depends on.
struct SweepOp {
    key: String,
    deps: Vec<String>,
    op: Op,
}

/// Stable topological order: repeatedly emit the earliest op
/// whose dependencies are all emitted. When no op can proceed
/// (a dependency cycle or a reference to a record the section
/// does not carry), the rest are emitted in canonical order —
/// the sweep's own `run` then surfaces the refusal that names
/// the problem, as it did before the ordering existed.
fn topo_order(mut items: Vec<SweepOp>) -> Vec<Op> {
    let mut emitted: Vec<Op> = Vec::new();
    let mut done: Vec<String> = Vec::new();
    while !items.is_empty() {
        let Some(pos) = items
            .iter()
            .position(|item| item.deps.iter().all(|dep| done.contains(dep)))
        else {
            emitted.extend(items.into_iter().map(|item| item.op));
            return emitted;
        };
        let item = items.remove(pos);
        done.push(item.key);
        emitted.push(item.op);
    }
    emitted
}

/// The record keys an expression reads.
fn expr_deps(expr: &crate::game::model::Expr, deps: &mut Vec<String>) {
    use crate::game::model::Expr;
    match expr {
        Expr::Add(left, right)
        | Expr::Max(left, right)
        | Expr::Min(left, right)
        | Expr::Mul(left, right)
        | Expr::Sub(left, right) => {
            expr_deps(left, deps);
            expr_deps(right, deps);
        }
        Expr::And(args) | Expr::Or(args) => {
            for arg in args {
                expr_deps(arg, deps);
            }
        }
        Expr::Clamp(value, lo, hi) => {
            expr_deps(value, deps);
            expr_deps(lo, deps);
            expr_deps(hi, deps);
        }
        Expr::Cmp(_, left, right) => {
            expr_deps(left, deps);
            expr_deps(right, deps);
        }
        Expr::Flag(flag) => deps.push(format!("flag:{flag}")),
        Expr::Neg(inner) | Expr::Not(inner) => expr_deps(inner, deps),
        Expr::Stat { character, stat } | Expr::TierOf { character, stat } => {
            deps.push(format!("stat:{stat}"));
            if let Some(who) = character {
                deps.push(format!("character:{who}"));
            }
        }
        Expr::Value(id) => deps.push(format!("value:{id}")),
        Expr::BoolLit(_) | Expr::Clock(_) | Expr::Lit(_) => {}
    }
}

/// The record keys an effect atom touches.
fn atom_deps(atom: &crate::game::model::Atom, deps: &mut Vec<String>) {
    use crate::game::model::Atom;
    match atom {
        Atom::EmitEvent { .. } => {}
        Atom::EnterScene { scene } => deps.push(format!("scene:{scene}")),
        Atom::FlagClear { flag } | Atom::FlagSet { flag } => {
            deps.push(format!("flag:{flag}"));
        }
        Atom::RollTable { table } => deps.push(format!("table:{table}")),
        Atom::StatAdd {
            character,
            stat,
            amount,
        } => {
            deps.push(format!("stat:{stat}"));
            if let Some(who) = character {
                deps.push(format!("character:{who}"));
            }
            expr_deps(amount, deps);
        }
        Atom::StatSet {
            character,
            stat,
            value,
        } => {
            deps.push(format!("stat:{stat}"));
            if let Some(who) = character {
                deps.push(format!("character:{who}"));
            }
            expr_deps(value, deps);
        }
    }
}

/// The game reference sweeps `decide()` cannot see at define
/// time (validation-time references, the sidecar, and the
/// dialogue chain graph).
fn game_findings(project: &Project) -> Vec<Refusal> {
    let mut findings = Vec::new();
    let has_entry = |id: &str| project.content.iter().any(|entry| entry.id == id);
    if let Some(game) = &project.game {
        if game.format_version < crate::game::model::GAME_FORMAT_VERSION_FIRST
            || game.format_version > crate::game::model::GAME_FORMAT_VERSION
        {
            findings.push(Refusal::new(
                RefusalClass::UnknownKind,
                format!(
                    "game format_version {} is outside the supported range {}..={}",
                    game.format_version,
                    crate::game::model::GAME_FORMAT_VERSION_FIRST,
                    crate::game::model::GAME_FORMAT_VERSION
                ),
            ));
        }
        if let Some(start) = &game.start_scene
            && game.scene(start).is_none()
        {
            findings.push(Refusal::new(
                RefusalClass::Dangling,
                format!("start scene '{start}' does not exist"),
            ));
        }
        for character in &game.characters {
            if !has_entry(&character.name_ref) {
                findings.push(Refusal::new(
                    RefusalClass::Dangling,
                    format!(
                        "character '{}' name reference '{}' has no content.ron entry",
                        character.id, character.name_ref
                    ),
                ));
            }
        }
        for line in &game.dialogue {
            if game.scene(&line.scene).is_none() {
                findings.push(Refusal::new(
                    RefusalClass::Dangling,
                    format!(
                        "dialogue line '{}' names missing scene '{}'",
                        line.id, line.scene
                    ),
                ));
            }
            if !has_entry(&line.text_ref) {
                findings.push(Refusal::new(
                    RefusalClass::Dangling,
                    format!(
                        "dialogue line '{}' text reference '{}' has no content.ron entry",
                        line.id, line.text_ref
                    ),
                ));
            }
            if let Some(next) = &line.next {
                match game.dialogue_line(next) {
                    None => findings.push(Refusal::new(
                        RefusalClass::Dangling,
                        format!(
                            "dialogue line '{}' chains to missing line '{next}'",
                            line.id
                        ),
                    )),
                    Some(target) if target.scene != line.scene => findings.push(Refusal::new(
                        RefusalClass::GameGraph,
                        format!(
                            "dialogue line '{}' chains across scenes to '{next}'",
                            line.id
                        ),
                    )),
                    _ => {}
                }
            }
            if let Some(hook) = &line.hook_expression
                && project.expression(hook).is_none()
            {
                findings.push(Refusal::new(
                    RefusalClass::Dangling,
                    format!(
                        "dialogue line '{}' hook expression '{hook}' does not exist",
                        line.id
                    ),
                ));
            }
        }
        // The AUTH-G flow families' forward references: the
        // same resolutions the decide gate performs, restated
        // for stored projects (a hand-edited file can carry
        // what no op would admit).
        let dangling = |findings: &mut Vec<Refusal>, message: String| {
            findings.push(Refusal::new(RefusalClass::Dangling, message));
        };
        if let Some(veto) = &game.alter_ego_veto
            && game.effect_set(veto).is_none()
        {
            dangling(
                &mut findings,
                format!("alter-ego veto effect set '{veto}' does not exist"),
            );
        }
        for scene in &game.scenes {
            for participant in &scene.participants {
                if game.character(participant).is_none() {
                    dangling(
                        &mut findings,
                        format!(
                            "scene '{}' participant '{participant}' does not exist",
                            scene.id
                        ),
                    );
                }
            }
            if let Some(condition) = &scene.entry_condition
                && game.condition(condition).is_none()
            {
                dangling(
                    &mut findings,
                    format!(
                        "scene '{}' entry condition '{condition}' does not exist",
                        scene.id
                    ),
                );
            }
            for (what, set) in [("on_enter", &scene.on_enter), ("on_exit", &scene.on_exit)] {
                if let Some(set) = set
                    && game.effect_set(set).is_none()
                {
                    dangling(
                        &mut findings,
                        format!(
                            "scene '{}' {what} effect set '{set}' does not exist",
                            scene.id
                        ),
                    );
                }
            }
            if let Some(interlude) = &scene.thread_interlude
                && game.thread(&interlude.thread).is_none()
            {
                dangling(
                    &mut findings,
                    format!(
                        "scene '{}' thread interlude thread '{}' does not exist",
                        scene.id, interlude.thread
                    ),
                );
            }
        }
        for thread in &game.threads {
            for participant in &thread.participants {
                if game.character(participant).is_none() {
                    dangling(
                        &mut findings,
                        format!(
                            "thread '{}' participant '{participant}' does not exist",
                            thread.id
                        ),
                    );
                }
            }
            for (index, message) in thread.messages.iter().enumerate() {
                if !thread
                    .participants
                    .iter()
                    .any(|row| row == &message.speaker)
                {
                    findings.push(Refusal::new(
                        RefusalClass::GameGraph,
                        format!(
                            "thread '{}' message '{}' speaker '{}' is not a participant",
                            thread.id, message.id, message.speaker
                        ),
                    ));
                }
                if !has_entry(&message.line) {
                    dangling(
                        &mut findings,
                        format!(
                            "thread '{}' message '{}' line reference '{}' has no content.ron entry",
                            thread.id, message.id, message.line
                        ),
                    );
                }
                if usize::try_from(message.at).unwrap_or(usize::MAX) != index {
                    findings.push(Refusal::new(
                        RefusalClass::GameGraph,
                        format!(
                            "thread '{}' message '{}' sits at position {} but is authored at {}",
                            thread.id, message.id, message.at, index
                        ),
                    ));
                }
            }
        }
        for choice in &game.choices {
            if game.scene(&choice.scene).is_none() {
                dangling(
                    &mut findings,
                    format!(
                        "choice '{}' names missing scene '{}'",
                        choice.id, choice.scene
                    ),
                );
            }
            if let Some(prompt) = &choice.prompt_line
                && !has_entry(prompt)
            {
                dangling(
                    &mut findings,
                    format!(
                        "choice '{}' prompt reference '{prompt}' has no content.ron entry",
                        choice.id
                    ),
                );
            }
            for option in &choice.options {
                if !has_entry(&option.label_ref) {
                    dangling(
                        &mut findings,
                        format!(
                            "choice '{}' option '{}' label reference '{}' has no content.ron entry",
                            choice.id, option.id, option.label_ref
                        ),
                    );
                }
                for (what, condition) in [
                    ("visible_if", &option.visible_if),
                    ("enabled_if", &option.enabled_if),
                ] {
                    if let Some(condition) = condition
                        && game.condition(condition).is_none()
                    {
                        dangling(
                            &mut findings,
                            format!(
                                "choice '{}' option '{}' {what} condition '{condition}' does not exist",
                                choice.id, option.id
                            ),
                        );
                    }
                }
                if let Some(set) = &option.effect_set
                    && game.effect_set(set).is_none()
                {
                    dangling(
                        &mut findings,
                        format!(
                            "choice '{}' option '{}' effect set '{set}' does not exist",
                            choice.id, option.id
                        ),
                    );
                }
                match &option.target {
                    crate::game::model::ChoiceTarget::EndingCheck => {}
                    crate::game::model::ChoiceTarget::Exit(id) => {
                        if game.scene_exit(&choice.scene, id).is_none() {
                            dangling(
                                &mut findings,
                                format!(
                                    "choice '{}' option '{}' targets missing exit '{id}' in scene '{}'",
                                    choice.id, option.id, choice.scene
                                ),
                            );
                        }
                    }
                    crate::game::model::ChoiceTarget::Line(id) => match game.dialogue_line(id) {
                        None => dangling(
                            &mut findings,
                            format!(
                                "choice '{}' option '{}' targets missing line '{id}'",
                                choice.id, option.id
                            ),
                        ),
                        Some(line) if line.scene != choice.scene => {
                            findings.push(Refusal::new(
                                RefusalClass::GameGraph,
                                format!(
                                    "choice '{}' option '{}' chains across scenes to line '{id}'",
                                    choice.id, option.id
                                ),
                            ));
                        }
                        _ => {}
                    },
                    crate::game::model::ChoiceTarget::Scene(id) => {
                        if game.scene(id).is_none() {
                            dangling(
                                &mut findings,
                                format!(
                                    "choice '{}' option '{}' targets missing scene '{id}'",
                                    choice.id, option.id
                                ),
                            );
                        }
                    }
                }
            }
        }
        let mut priorities: Vec<i64> = Vec::new();
        for ending in &game.endings {
            if game.condition(&ending.condition).is_none() {
                dangling(
                    &mut findings,
                    format!(
                        "ending '{}' condition '{}' does not exist",
                        ending.id, ending.condition
                    ),
                );
            }
            if game.scene(&ending.epilogue_scene).is_none() {
                dangling(
                    &mut findings,
                    format!(
                        "ending '{}' epilogue scene '{}' does not exist",
                        ending.id, ending.epilogue_scene
                    ),
                );
            }
            if priorities.contains(&ending.priority) {
                findings.push(Refusal::new(
                    RefusalClass::GameGraph,
                    format!(
                        "ending priority {} is taken by more than one ending",
                        ending.priority
                    ),
                ));
            }
            priorities.push(ending.priority);
        }
        for entry in &game.gallery_entries {
            if let crate::game::model::GalleryUnlock::Condition(id) = &entry.unlock
                && game.condition(id).is_none()
            {
                dangling(
                    &mut findings,
                    format!(
                        "gallery entry '{}' unlock condition '{id}' does not exist",
                        entry.id
                    ),
                );
            }
            if game.scene(&entry.replay_scene).is_none() {
                dangling(
                    &mut findings,
                    format!(
                        "gallery entry '{}' replay scene '{}' does not exist",
                        entry.id, entry.replay_scene
                    ),
                );
            }
            if game.dialogue_line(&entry.replay_line).is_none() {
                dangling(
                    &mut findings,
                    format!(
                        "gallery entry '{}' replay line '{}' does not exist",
                        entry.id, entry.replay_line
                    ),
                );
            }
        }
        for schedule in &game.schedules {
            if game.character(&schedule.character).is_none() {
                dangling(
                    &mut findings,
                    format!("schedule character '{}' does not exist", schedule.character),
                );
            }
            if let Some(clock) = &game.clock {
                for slot in &clock.slots {
                    if !schedule.slots.iter().any(|row| &row.slot == slot) {
                        findings.push(Refusal::new(
                            RefusalClass::GameGraph,
                            format!(
                                "schedule for '{}' leaves clock slot '{slot}' unassigned",
                                schedule.character
                            ),
                        ));
                    }
                }
                for row in &schedule.slots {
                    if !clock.slots.contains(&row.slot) {
                        findings.push(Refusal::new(
                            RefusalClass::GameGraph,
                            format!(
                                "schedule for '{}' names unknown clock slot '{}'",
                                schedule.character, row.slot
                            ),
                        ));
                    }
                }
            }
            for row in &schedule.slots {
                if game.scene(&row.scene).is_none() {
                    dangling(
                        &mut findings,
                        format!(
                            "schedule for '{}' slot '{}' names missing scene '{}'",
                            schedule.character, row.slot, row.scene
                        ),
                    );
                }
            }
            for row in &schedule.overrides {
                if game.condition(&row.condition).is_none() {
                    dangling(
                        &mut findings,
                        format!(
                            "schedule for '{}' override condition '{}' does not exist",
                            schedule.character, row.condition
                        ),
                    );
                }
                if game.scene(&row.scene).is_none() {
                    dangling(
                        &mut findings,
                        format!(
                            "schedule for '{}' override names missing scene '{}'",
                            schedule.character, row.scene
                        ),
                    );
                }
            }
        }
        for binding in &game.scene_bindings {
            match game.scene(&binding.scene) {
                None => dangling(
                    &mut findings,
                    format!("scene binding names missing scene '{}'", binding.scene),
                ),
                Some(scene) if !scene.participants.iter().any(|p| p == &binding.participant) => {
                    findings.push(Refusal::new(
                        RefusalClass::GameGraph,
                        format!(
                            "binding participant '{}' is not a participant of scene '{}'",
                            binding.participant, binding.scene
                        ),
                    ));
                }
                _ => {}
            }
            if project.expression(&binding.entry_expression).is_none() {
                dangling(
                    &mut findings,
                    format!(
                        "binding for scene '{}' entry expression '{}' does not exist",
                        binding.scene, binding.entry_expression
                    ),
                );
            }
            if let Some(motion) = &binding.entry_motion
                && project.motion(motion).is_none()
            {
                dangling(
                    &mut findings,
                    format!(
                        "binding for scene '{}' entry motion '{motion}' does not exist",
                        binding.scene
                    ),
                );
            }
        }
        for exit in &game.scene_exits {
            if game.scene(&exit.scene).is_none() {
                dangling(
                    &mut findings,
                    format!(
                        "scene exit '{}' names missing scene '{}'",
                        exit.id, exit.scene
                    ),
                );
            }
            if game.scene(&exit.target).is_none() {
                dangling(
                    &mut findings,
                    format!(
                        "scene exit '{}' target scene '{}' does not exist",
                        exit.id, exit.target
                    ),
                );
            }
            if let Some(condition) = &exit.condition
                && game.condition(condition).is_none()
            {
                dangling(
                    &mut findings,
                    format!(
                        "scene exit '{}' condition '{condition}' does not exist",
                        exit.id
                    ),
                );
            }
            if let Some(set) = &exit.effect_set
                && game.effect_set(set).is_none()
            {
                dangling(
                    &mut findings,
                    format!("scene exit '{}' effect set '{set}' does not exist", exit.id),
                );
            }
        }
        findings.extend(chain_cycle_findings(game));
    }
    // Orphan sidecar entries: prose no record references. The
    // sweep cannot rebuild them (no op authors them), so they
    // are reported here and only here.
    let referenced = |id: &str| {
        project.game.as_ref().is_some_and(|game| {
            game.characters.iter().any(|c| c.name_ref == id)
                || game.dialogue.iter().any(|l| l.text_ref == id)
                || game.choices.iter().any(|choice| {
                    choice.prompt_line.as_deref() == Some(id)
                        || choice.options.iter().any(|option| option.label_ref == id)
                })
        })
    };
    for entry in &project.content {
        if !referenced(&entry.id) {
            findings.push(Refusal::new(
                RefusalClass::Dangling,
                format!(
                    "content entry '{}' is not referenced by any game record",
                    entry.id
                ),
            ));
        }
    }
    findings
}

/// A dialogue chain that loops without passing through a choice
/// can never be exited; choice records are separate from the
/// chain, so any `next` cycle is one. One finding per scene
/// with a cycle.
fn chain_cycle_findings(game: &crate::game::model::Game) -> Vec<Refusal> {
    let mut findings = Vec::new();
    for scene in &game.scenes {
        let mut cyclic = false;
        for start in game.dialogue.iter().filter(|line| line.scene == scene.id) {
            let mut seen: Vec<&str> = Vec::new();
            let mut current = Some(start);
            while let Some(line) = current {
                if seen.contains(&line.id.as_str()) {
                    cyclic = true;
                    break;
                }
                seen.push(&line.id);
                // The walk stays inside the scene: a chain
                // leaving the scene is the cross-scene finding's
                // business, not this scene's cycle.
                current = line.next.as_deref().and_then(|id| {
                    game.dialogue_line(id)
                        .filter(|target| target.scene == scene.id)
                });
            }
            if cyclic {
                break;
            }
        }
        if cyclic {
            findings.push(Refusal::new(
                RefusalClass::GameGraph,
                format!(
                    "scene '{}' dialogue chain cycles without passing through a choice",
                    scene.id
                ),
            ));
        }
    }
    findings
}

fn asset_findings(project: &Project, root: &Path) -> Vec<Refusal> {
    let mut findings = Vec::new();
    for asset in &project.assets {
        let path = root.join(&asset.file);
        match crate::intake::read_bounded(&path, crate::limits::IMAGE_SOURCE_BYTES_MAX) {
            Err(_) => findings.push(Refusal::new(
                RefusalClass::Dangling,
                format!("asset '{}' file '{}' is missing", asset.id, asset.file),
            )),
            Ok(bytes) if sha256_hex(&bytes) != asset.sha256 => findings.push(Refusal::new(
                RefusalClass::Provenance,
                format!(
                    "asset '{}' file does not hash to its recorded sha256",
                    asset.id
                ),
            )),
            Ok(_) => {}
        }
    }
    findings
}
