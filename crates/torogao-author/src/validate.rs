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
use crate::limits::{ASSETS_MAX, IMAGE_SIDE_MAX, PROVENANCE_TEXT_BYTES_MAX};
use crate::model::{Asset, DeformerKind, Keyforms, Project};
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
    Ok(rebuilt)
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

/// The game section's ops, in authoring (dependency) order:
/// the section itself, then the clock, stats, tiers, and
/// flags, then characters and scenes, then values and
/// conditions (both may read character stats), then effect
/// sets, then the dialogue chains in (scene, ordinal) order,
/// then triggers. The prose each prose-authoring op carries is
/// read back from the stored sidecar entries; a reference with
/// no entry synthesizes an empty text, which the gate refuses —
/// and `game_findings` names the dangling reference itself.
fn game_ops(project: &Project) -> Vec<Op> {
    let mut ops = Vec::new();
    let Some(game) = &project.game else {
        return ops;
    };
    let prose = |id: &str| {
        project
            .content
            .iter()
            .find(|entry| entry.id == id)
            .map(|entry| entry.text.clone())
            .unwrap_or_default()
    };
    ops.push(Op::Game(GameOp::GameDefine(GameDefineInput {
        id: game.id.clone(),
        start_scene: game.start_scene.clone(),
        save_schema_version: Some(game.save_schema_version),
    })));
    if let Some(clock) = &game.clock {
        ops.push(Op::Game(GameOp::ClockDefine(ClockDefineInput {
            slots: clock.slots.clone(),
            days_per_week: clock.days_per_week,
            day_limit: clock.day_limit,
        })));
    }
    for stat in &game.stats {
        ops.push(Op::Game(GameOp::StatDefine(
            crate::game::apply::stat_input_of(stat),
        )));
    }
    for tier in &game.tiers {
        ops.push(Op::Game(GameOp::StatTierDefine(StatTierDefineInput {
            stat: tier.stat.clone(),
            thresholds: tier.thresholds.clone(),
            names: tier.names.clone(),
        })));
    }
    for flag in &game.flags {
        ops.push(Op::Game(GameOp::FlagDefine(FlagDefineInput {
            id: flag.id.clone(),
            scope: match flag.scope {
                crate::game::model::FlagScope::Globals => FlagScopeInput::Globals,
                crate::game::model::FlagScope::Run => FlagScopeInput::Run,
            },
            default: flag.default,
        })));
    }
    for character in &game.characters {
        ops.push(Op::Game(GameOp::CharacterDefine(CharacterDefineInput {
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
        })));
    }
    for scene in &game.scenes {
        ops.push(Op::Game(GameOp::SceneDefine(SceneDefineInput {
            id: scene.id.clone(),
            presentation: match scene.presentation {
                crate::game::model::Presentation::Event => PresentationInput::Event,
                crate::game::model::Presentation::Room => PresentationInput::Room,
                crate::game::model::Presentation::Vn => PresentationInput::Vn,
            },
        })));
    }
    for value in &game.values {
        ops.push(Op::Game(GameOp::ValueDefine(ValueDefineInput {
            id: value.id.clone(),
            expr: crate::game::expr::expr_to_input(&value.expr),
        })));
    }
    for condition in &game.conditions {
        ops.push(Op::Game(GameOp::ConditionDefine(ConditionDefineInput {
            id: condition.id.clone(),
            expr: crate::game::expr::expr_to_input(&condition.expr),
        })));
    }
    for set in &game.effect_sets {
        ops.push(Op::Game(GameOp::EffectSetDefine(EffectSetDefineInput {
            id: set.id.clone(),
            atoms: set
                .atoms
                .iter()
                .map(crate::game::apply::atom_input_of)
                .collect(),
        })));
    }
    let mut lines: Vec<&crate::game::model::DialogueLine> = game.dialogue.iter().collect();
    lines.sort_by(|a, b| (a.scene.as_str(), a.ordinal).cmp(&(b.scene.as_str(), b.ordinal)));
    for line in lines {
        ops.push(Op::Game(GameOp::DialogueLineDefine(
            DialogueLineDefineInput {
                id: line.id.clone(),
                scene: line.scene.clone(),
                ordinal: line.ordinal,
                speaker: line.speaker.clone(),
                text_ref: line.text_ref.clone(),
                text: prose(&line.text_ref),
                next: line.next.clone(),
                hook_expression: line.hook_expression.clone(),
            },
        )));
    }
    for trigger in &game.triggers {
        ops.push(Op::Game(GameOp::TriggerDefine(TriggerDefineInput {
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
        })));
    }
    ops
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
        findings.extend(chain_cycle_findings(game));
    }
    // Orphan sidecar entries: prose no record references. The
    // sweep cannot rebuild them (no op authors them), so they
    // are reported here and only here.
    let referenced = |id: &str| {
        project.game.as_ref().is_some_and(|game| {
            game.characters.iter().any(|c| c.name_ref == id)
                || game.dialogue.iter().any(|l| l.text_ref == id)
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
/// can never be exited; with choices not yet authored, any
/// `next` cycle is one. One finding per scene with a cycle.
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
