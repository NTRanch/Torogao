//! The generation-conditioning family (D-822): verbs over
//! the project's generation record, delivered by the
//! `torogao-gencond` crate behind this crate's
//! `gen-conditioning` feature. The LIFT layer (D-838) rides
//! the same family: `scene_record_set` stores a session
//! record, `freeze_track` / `freeze_palette` land frozen
//! artifacts into one, and `conditioning_compile` checks a
//! regeneration proposal against the named scene's custody
//! map before it compiles.
//!
//! The family has its own manifest, like the AUTH-G game
//! family: the rig manifest's census (49 verbs) is untouched
//! in every build. In a build without the feature this module
//! does not exist, the verbs refuse by family name through
//! `op::refuse_unimplemented`, and a saved conditioning record
//! is inert data — the codec carries it as an opaque RON
//! subtree that loads, round-trips, and re-emits untouched
//! (the removal drill is rehearsed in the seam's stage
//! report).
//!
//! `conditioning_set` is the family's one mutating verb: a
//! typed setter for the whole generation record. Its decide()
//! is schema-level (the gencond validator); asset-level checks
//! (hash equality, blank maps, embed measurements) belong to
//! the compiler, which has the bytes. `conditioning_compile`
//! is read-only: it assembles the compile inputs from the
//! project directory (asset files resolved against the project
//! root, bindings and measurements supplied as verb inputs)
//! and runs the gencond compiler, whose typed refusals
//! (`TG-GENCOND-*`) surface in the answer document unchanged.

use crate::model::{GenerationLatent, GenerationRecord, GenerationSampler, Project};
use crate::refusal::{Refusal, RefusalClass, refuse};
use crate::session::{Answer, Status};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::Path;

/// One conditioning-manifest row.
#[derive(Clone, Copy, Debug)]
pub struct ConditioningVerbSpec {
    /// Canonical verb name.
    pub name: &'static str,
    /// Manifest group.
    pub group: &'static str,
    /// True when the verb mutates the project.
    pub mutating: bool,
}

const fn verb(name: &'static str, group: &'static str, mutating: bool) -> ConditioningVerbSpec {
    ConditioningVerbSpec {
        name,
        group,
        mutating,
    }
}

/// The conditioning manifest: five verbs, alphabetical. The
/// sweep verb is Stage 2 of the seam and is not here; the
/// three lift verbs (D-838) are the session-record surface.
pub const CONDITIONING_MANIFEST: [ConditioningVerbSpec; 5] = [
    verb("conditioning_compile", "generation conditioning", false),
    verb("conditioning_set", "generation conditioning", true),
    verb("freeze_palette", "media lift", true),
    verb("freeze_track", "media lift", true),
    verb("scene_record_set", "media lift", true),
];

/// The conditioning-manifest row for a verb name.
pub fn conditioning_verb_spec(name: &str) -> Option<&'static ConditioningVerbSpec> {
    CONDITIONING_MANIFEST.iter().find(|spec| spec.name == name)
}

/// Inputs of `conditioning_set`: the whole generation record,
/// replaced atomically.
#[derive(Clone, Debug, PartialEq, Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct ConditioningSetInput {
    /// Generation record id.
    pub id: String,
    /// The base plan (checkpoint, latent, sampler, seed).
    pub base: BaseInput,
    /// The conditioning section, in its JSON wire form;
    /// `null` stores a plan-only record (compilation then
    /// refuses by name until a section is set).
    pub conditioning: Option<torogao_gencond::Conditioning>,
}

/// The base plan in wire form.
#[derive(Clone, Debug, PartialEq, Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct BaseInput {
    /// S-7 name of the base checkpoint.
    pub checkpoint: String,
    /// The latent canvas.
    pub latent: LatentInput,
    /// The sampler configuration.
    pub sampler: SamplerInput,
    /// The base seed.
    pub seed: i64,
    /// Save-image filename prefix.
    pub filename_prefix: String,
}

/// The latent canvas in wire form.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct LatentInput {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Batch size.
    pub batch_size: u32,
}

/// The sampler configuration in wire form.
#[derive(Clone, Debug, PartialEq, Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct SamplerInput {
    /// Steps.
    pub steps: u32,
    /// CFG scale.
    pub cfg: f64,
    /// Sampler name.
    pub name: String,
    /// Scheduler name.
    pub scheduler: String,
    /// Denoise strength.
    pub denoise: f64,
}

/// Inputs of `scene_record_set`: the whole session record,
/// replaced atomically per scene id (the LIFT layer, D-838).
#[derive(Clone, Debug, PartialEq, Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct SceneRecordSetInput {
    /// The session record, in its JSON wire form.
    pub record: torogao_gencond::SceneRecord,
}

/// Inputs of `freeze_track`: a dense parameter track to
/// freeze into the named scene record. `epsilon` is the
/// deviation contract; omitted, it is derived from the
/// track's own sample noise (`track_noise_epsilon`) and the
/// provenance records which origin supplied it.
#[derive(Clone, Debug, PartialEq, Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct FreezeTrackInput {
    /// The scene record to freeze into.
    pub scene_id: String,
    /// The dense track.
    pub track: torogao_gencond::ParameterTrack,
    /// The deviation contract value, if the caller states
    /// one.
    pub epsilon: Option<f64>,
}

/// Inputs of `freeze_palette`: a colour set to snap onto a
/// locked palette inside the named scene record. `tolerance`
/// is the caller's contract value (Chebyshev distance in
/// sRGB); omitted, it is 0 — exact membership only, stated
/// here rather than assumed.
#[derive(Clone, Debug, PartialEq, Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct FreezePaletteInput {
    /// The scene record to freeze into.
    pub scene_id: String,
    /// The colours to snap.
    pub colors: Vec<String>,
    /// The locked palette.
    pub palette: Vec<String>,
    /// The tolerance contract value, if the caller states
    /// one.
    pub tolerance: Option<u32>,
}

/// The conditioning family's mutating ops.
#[derive(Clone, Debug, PartialEq)]
pub enum ConditioningOp {
    /// `conditioning_set`.
    Set(ConditioningSetInput),
    /// `scene_record_set` (D-838).
    SceneSet(SceneRecordSetInput),
    /// `freeze_track` (D-838).
    FreezeTrack(FreezeTrackInput),
    /// `freeze_palette` (D-838).
    FreezePalette(FreezePaletteInput),
}

impl ConditioningOp {
    /// The canonical verb name.
    pub fn verb(&self) -> &'static str {
        match self {
            ConditioningOp::Set(_) => "conditioning_set",
            ConditioningOp::SceneSet(_) => "scene_record_set",
            ConditioningOp::FreezeTrack(_) => "freeze_track",
            ConditioningOp::FreezePalette(_) => "freeze_palette",
        }
    }

    /// The normalized inputs as JSON (what the op log records).
    pub fn inputs_json(&self) -> Result<Value, Refusal> {
        let result = match self {
            ConditioningOp::Set(inputs) => serde_json::to_value(inputs),
            ConditioningOp::SceneSet(inputs) => serde_json::to_value(inputs),
            ConditioningOp::FreezeTrack(inputs) => serde_json::to_value(inputs),
            ConditioningOp::FreezePalette(inputs) => serde_json::to_value(inputs),
        };
        result.or_else(|err| {
            refuse(
                RefusalClass::Malformed,
                format!("inputs for '{}' cannot be recorded: {err}", self.verb()),
            )
        })
    }
}

/// Parses a conditioning verb into the trunk `Op`. Called
/// from `Op::parse` for names the conditioning manifest
/// carries; the read-only verb refuses here (it routes
/// through the session query path, never the pipeline).
pub fn parse_conditioning_op(verb: &str, inputs: &Value) -> Result<crate::op::Op, Refusal> {
    match verb {
        "conditioning_set" => {
            let input: ConditioningSetInput =
                serde_json::from_value(inputs.clone()).or_else(|err| {
                    refuse(
                        RefusalClass::Malformed,
                        format!("inputs for '{verb}' do not match its schema: {err}"),
                    )
                })?;
            // Building the stored record here runs the
            // gencond validators before the pipeline admits
            // the op; decide() rebuilds and compares it.
            record_of(&input)?;
            Ok(crate::op::Op::Conditioning(ConditioningOp::Set(input)))
        }
        "scene_record_set" => {
            let input: SceneRecordSetInput =
                serde_json::from_value(inputs.clone()).or_else(|err| {
                    refuse(
                        RefusalClass::Malformed,
                        format!("inputs for '{verb}' do not match its schema: {err}"),
                    )
                })?;
            // Validating the typed record here runs the
            // gencond rules before the pipeline admits the
            // op, the setter's same discipline.
            scene_record_of(&input)?;
            Ok(crate::op::Op::Conditioning(ConditioningOp::SceneSet(input)))
        }
        "freeze_track" => {
            let input: FreezeTrackInput =
                serde_json::from_value(inputs.clone()).or_else(|err| {
                    refuse(
                        RefusalClass::Malformed,
                        format!("inputs for '{verb}' do not match its schema: {err}"),
                    )
                })?;
            // Running the freeze here surfaces its typed
            // refusals before the pipeline admits the op;
            // decide()/apply() re-run it (it is pure).
            freeze_track_outcome(&input)?;
            Ok(crate::op::Op::Conditioning(ConditioningOp::FreezeTrack(
                input,
            )))
        }
        "freeze_palette" => {
            let input: FreezePaletteInput =
                serde_json::from_value(inputs.clone()).or_else(|err| {
                    refuse(
                        RefusalClass::Malformed,
                        format!("inputs for '{verb}' do not match its schema: {err}"),
                    )
                })?;
            freeze_palette_outcome(&input)?;
            Ok(crate::op::Op::Conditioning(ConditioningOp::FreezePalette(
                input,
            )))
        }
        other => refuse(
            RefusalClass::UnknownKind,
            format!("verb '{other}' is read-only or not a conditioning verb"),
        ),
    }
}

/// Builds the stored scene record from set inputs, running
/// the gencond validator: the record is stored as an opaque
/// tree, but only a valid record is admitted.
pub fn scene_record_of(input: &SceneRecordSetInput) -> Result<crate::model::SceneRecord, Refusal> {
    input.record.validate().map_err(map_gencond)?;
    Ok(crate::model::SceneRecord {
        id: input.record.id.clone(),
        record: gencond_tree_to_ron(&input.record.to_ron_tree()),
    })
}

/// Parses a stored scene record back to its typed form,
/// refusing when the project carries no such scene, when
/// the subtree does not parse, or when the record's own id
/// disagrees with the id it is filed under.
pub fn typed_scene(
    project: &Project,
    scene_id: &str,
) -> Result<(usize, torogao_gencond::SceneRecord), Refusal> {
    let Some(index) = project.scenes.iter().position(|scene| scene.id == scene_id) else {
        return refuse(
            RefusalClass::UnknownTarget,
            format!("the project carries no scene record '{scene_id}'"),
        );
    };
    let typed = torogao_gencond::SceneRecord::from_ron_tree(ron_to_gencond_tree(
        &project.scenes[index].record,
    ))
    .map_err(map_gencond)?;
    if typed.id != scene_id {
        return refuse(
            RefusalClass::Conflict,
            format!(
                "scene record is filed under id '{scene_id}' but its record names '{}'",
                typed.id
            ),
        );
    }
    Ok((index, typed))
}

/// Stores a typed scene record back into the project at
/// `index`, re-emitting its opaque tree.
fn store_typed_scene(project: &mut Project, index: usize, typed: &torogao_gencond::SceneRecord) {
    project.scenes[index].record = gencond_tree_to_ron(&typed.to_ron_tree());
}

/// The outcome of a `freeze_track` input: the frozen track
/// and its provenance entry. The epsilon is the caller's
/// contract value when stated, else derived from the
/// track's own sample noise; the provenance records which.
pub fn freeze_track_outcome(
    input: &FreezeTrackInput,
) -> Result<
    (
        torogao_gencond::FrozenTrack,
        torogao_gencond::FreezeProvenance,
    ),
    Refusal,
> {
    let (epsilon, origin) = match input.epsilon {
        Some(value) => (value, torogao_gencond::EpsilonOrigin::Caller),
        None => (
            torogao_gencond::track_noise_epsilon(&input.track),
            torogao_gencond::EpsilonOrigin::TrackNoise,
        ),
    };
    let frozen = torogao_gencond::freeze_track(&input.track, epsilon).map_err(map_gencond)?;
    let provenance = torogao_gencond::FreezeProvenance {
        op: torogao_gencond::FreezeOp::FreezeTrack,
        source_sha256: torogao_gencond::freeze::canonical_sha256(&input.track)
            .map_err(map_gencond)?,
        frozen_sha256: torogao_gencond::freeze::canonical_sha256(&frozen).map_err(map_gencond)?,
        epsilon: Some(epsilon),
        epsilon_origin: Some(origin),
        tolerance: None,
    };
    Ok((frozen, provenance))
}

/// The outcome of a `freeze_palette` input: the snap and its
/// provenance entry. The tolerance is the caller's contract
/// value when stated, else 0 (exact membership only).
pub fn freeze_palette_outcome(
    input: &FreezePaletteInput,
) -> Result<
    (
        torogao_gencond::PaletteSnap,
        torogao_gencond::FreezeProvenance,
    ),
    Refusal,
> {
    let tolerance = input.tolerance.unwrap_or(0);
    let snap = torogao_gencond::freeze_palette(&input.colors, &input.palette, tolerance)
        .map_err(map_gencond)?;
    let source = json!({
        "colors": input.colors,
        "palette": input.palette,
        "tolerance": tolerance,
    });
    let provenance = torogao_gencond::FreezeProvenance {
        op: torogao_gencond::FreezeOp::FreezePalette,
        source_sha256: torogao_gencond::freeze::canonical_sha256(&source).map_err(map_gencond)?,
        frozen_sha256: torogao_gencond::freeze::canonical_sha256(&snap).map_err(map_gencond)?,
        epsilon: None,
        epsilon_origin: None,
        tolerance: Some(tolerance),
    };
    Ok((snap, provenance))
}

/// Applies a freeze outcome to a typed scene record: the
/// artifact lands (replacing any earlier freeze of the same
/// parameter/palette) and provenance appends — unless the
/// record already carries this exact freeze, in which case
/// it is returned unchanged, so a repeated freeze is a
/// no-op, the setter discipline.
fn land_freeze(
    record: &torogao_gencond::SceneRecord,
    frozen: Option<&torogao_gencond::FrozenTrack>,
    snap: Option<&torogao_gencond::PaletteSnap>,
    provenance: &torogao_gencond::FreezeProvenance,
) -> torogao_gencond::SceneRecord {
    let mut next = record.clone();
    if let Some(track) = frozen {
        next.land_frozen_track(track.clone());
    }
    if let Some(snap) = snap {
        next.land_palette_snap(snap.clone());
    }
    if !next.provenance.contains(provenance) {
        next.record_freeze(provenance.clone());
    }
    next
}

/// Builds the stored generation record from set inputs,
/// running the gencond validators: the plan's rules and, when
/// a conditioning section is present, the record schema's.
/// Gencond refusals map to `TG-AUTH-MALFORMED` with the
/// gencond code named in the message.
pub fn record_of(input: &ConditioningSetInput) -> Result<GenerationRecord, Refusal> {
    check_record_id(&input.id)?;
    let plan = plan_of(input);
    plan.validate().map_err(map_gencond)?;
    if let Some(conditioning) = &input.conditioning {
        conditioning.validate().map_err(map_gencond)?;
    }
    Ok(GenerationRecord {
        id: input.id.clone(),
        checkpoint: input.base.checkpoint.clone(),
        latent: GenerationLatent {
            width: input.base.latent.width,
            height: input.base.latent.height,
            batch_size: input.base.latent.batch_size,
        },
        sampler: GenerationSampler {
            steps: input.base.sampler.steps,
            cfg: input.base.sampler.cfg,
            name: input.base.sampler.name.clone(),
            scheduler: input.base.sampler.scheduler.clone(),
            denoise: input.base.sampler.denoise,
        },
        seed: input.base.seed,
        filename_prefix: input.base.filename_prefix.clone(),
        conditioning: input
            .conditioning
            .as_ref()
            .map(|record| gencond_tree_to_ron(&record.to_ron_tree())),
    })
}

/// The gencond plan for a record's base.
pub fn plan_of(input: &ConditioningSetInput) -> torogao_gencond::GenerationPlan {
    torogao_gencond::GenerationPlan {
        id: input.id.clone(),
        checkpoint: input.base.checkpoint.clone(),
        latent: torogao_gencond::LatentSpec {
            width: input.base.latent.width,
            height: input.base.latent.height,
            batch_size: input.base.latent.batch_size,
        },
        sampler: torogao_gencond::SamplerSpec {
            steps: input.base.sampler.steps,
            cfg: input.base.sampler.cfg,
            name: input.base.sampler.name.clone(),
            scheduler: input.base.sampler.scheduler.clone(),
            denoise: input.base.sampler.denoise,
        },
        seed: input.base.seed,
        filename_prefix: input.base.filename_prefix.clone(),
    }
}

/// The gencond plan for a stored generation record.
pub fn plan_of_record(record: &GenerationRecord) -> torogao_gencond::GenerationPlan {
    torogao_gencond::GenerationPlan {
        id: record.id.clone(),
        checkpoint: record.checkpoint.clone(),
        latent: torogao_gencond::LatentSpec {
            width: record.latent.width,
            height: record.latent.height,
            batch_size: record.latent.batch_size,
        },
        sampler: torogao_gencond::SamplerSpec {
            steps: record.sampler.steps,
            cfg: record.sampler.cfg,
            name: record.sampler.name.clone(),
            scheduler: record.sampler.scheduler.clone(),
            denoise: record.sampler.denoise,
        },
        seed: record.seed,
        filename_prefix: record.filename_prefix.clone(),
    }
}

fn check_record_id(id: &str) -> Result<(), Refusal> {
    let valid = !id.is_empty()
        && id.len() <= crate::limits::ID_BYTES_MAX
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_');
    if !valid {
        return refuse(
            RefusalClass::Malformed,
            format!("generation record id '{id}' is not a valid id"),
        );
    }
    Ok(())
}

/// Maps a gencond refusal onto the author taxonomy for the
/// pipeline surface, keeping the gencond code in the message.
pub fn map_gencond(error: torogao_gencond::GencondError) -> Refusal {
    Refusal::new(
        RefusalClass::Malformed,
        format!("{}: {}", error.code, error.message),
    )
}

/// Validates a stored generation record end to end (the
/// validation sweep's conditioning check): the base plan's
/// rules plus the conditioning section's schema, parsed from
/// its stored tree.
pub fn check_stored_record(record: &GenerationRecord) -> Result<(), Refusal> {
    plan_of_record(record).validate().map_err(map_gencond)?;
    if let Some(tree) = &record.conditioning {
        // from_ron_tree validates the parsed record.
        torogao_gencond::Conditioning::from_ron_tree(ron_to_gencond_tree(tree))
            .map_err(map_gencond)?;
    }
    Ok(())
}

/// Validates a stored scene record end to end (the
/// validation sweep's scene check): the typed record parses
/// and validates, and its own id matches the id it is filed
/// under.
pub fn check_stored_scene(scene: &crate::model::SceneRecord) -> Result<(), Refusal> {
    // typed_scene's checks, against a one-scene view.
    let typed = torogao_gencond::SceneRecord::from_ron_tree(ron_to_gencond_tree(&scene.record))
        .map_err(map_gencond)?;
    if typed.id != scene.id {
        return refuse(
            RefusalClass::Conflict,
            format!(
                "scene record is filed under id '{}' but its record names '{}'",
                scene.id, typed.id
            ),
        );
    }
    Ok(())
}

/// The decide step for conditioning ops: replacement is a
/// no-op when the stored record already equals the input's.
pub fn decide_conditioning(
    project: &Project,
    op: &ConditioningOp,
) -> Result<crate::decide::Verdict, Refusal> {
    match op {
        ConditioningOp::Set(input) => {
            let record = record_of(input)?;
            if project.generation.as_ref() == Some(&record) {
                Ok(crate::decide::Verdict::NoOp)
            } else {
                Ok(crate::decide::Verdict::Apply)
            }
        }
        ConditioningOp::SceneSet(input) => {
            let record = scene_record_of(input)?;
            if project.scenes.contains(&record) {
                Ok(crate::decide::Verdict::NoOp)
            } else {
                Ok(crate::decide::Verdict::Apply)
            }
        }
        ConditioningOp::FreezeTrack(input) => {
            let (_, stored) = typed_scene(project, &input.scene_id)?;
            let (frozen, provenance) = freeze_track_outcome(input)?;
            if land_freeze(&stored, Some(&frozen), None, &provenance) == stored {
                Ok(crate::decide::Verdict::NoOp)
            } else {
                Ok(crate::decide::Verdict::Apply)
            }
        }
        ConditioningOp::FreezePalette(input) => {
            let (_, stored) = typed_scene(project, &input.scene_id)?;
            let (snap, provenance) = freeze_palette_outcome(input)?;
            if land_freeze(&stored, None, Some(&snap), &provenance) == stored {
                Ok(crate::decide::Verdict::NoOp)
            } else {
                Ok(crate::decide::Verdict::Apply)
            }
        }
    }
}

/// The apply step for conditioning ops.
pub fn apply_conditioning(project: &mut Project, op: &ConditioningOp) -> Result<(), Refusal> {
    match op {
        ConditioningOp::Set(input) => {
            project.generation = Some(record_of(input)?);
            Ok(())
        }
        ConditioningOp::SceneSet(input) => {
            let record = scene_record_of(input)?;
            match project
                .scenes
                .iter()
                .position(|scene| scene.id == record.id)
            {
                Some(index) => project.scenes[index] = record,
                None => {
                    project.scenes.push(record);
                    project.scenes.sort_by(|a, b| a.id.cmp(&b.id));
                }
            }
            Ok(())
        }
        ConditioningOp::FreezeTrack(input) => {
            let (index, stored) = typed_scene(project, &input.scene_id)?;
            let (frozen, provenance) = freeze_track_outcome(input)?;
            let next = land_freeze(&stored, Some(&frozen), None, &provenance);
            store_typed_scene(project, index, &next);
            Ok(())
        }
        ConditioningOp::FreezePalette(input) => {
            let (index, stored) = typed_scene(project, &input.scene_id)?;
            let (snap, provenance) = freeze_palette_outcome(input)?;
            let next = land_freeze(&stored, None, Some(&snap), &provenance);
            store_typed_scene(project, index, &next);
            Ok(())
        }
    }
}

/// Inputs of `conditioning_compile`: the S-7 bindings, the
/// embed measurements, and file resolutions for rig-projection
/// maps (whose bytes live outside the record by design).
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompileInputs {
    /// Model name -> binding.
    #[serde(default)]
    pub bindings: BTreeMap<String, torogao_gencond::ModelBinding>,
    /// Embed measurements from the executing harness.
    #[serde(default)]
    pub measurements: Vec<torogao_gencond::EmbedMeasurement>,
    /// Map SHA-256 -> project-relative file, for
    /// rig-projection maps.
    #[serde(default)]
    pub map_files: BTreeMap<String, String>,
    /// The scene this compile regenerates, if the proposal
    /// names one (D-838): the proposal's `targets` are
    /// checked against that scene record's custody map
    /// before compilation, and a custody-frozen target
    /// refuses the whole proposal.
    #[serde(default)]
    pub scene_id: Option<String>,
    /// The scene nodes this regeneration proposal targets.
    /// Meaningful only with `scene_id`; empty names no
    /// nodes, and no node is custody-checked.
    #[serde(default)]
    pub targets: Vec<String>,
}

/// Runs `conditioning_compile` against the project at `root`.
/// The compiler's typed refusals keep their `TG-GENCOND-*`
/// codes in the answer document; author-side failures (no
/// project, no record, unreadable asset) refuse through the
/// author taxonomy.
pub fn run_compile_query(root: &Path, inputs: &Value) -> Answer {
    let verb = "conditioning_compile";
    let parsed: CompileInputs = match serde_json::from_value(inputs.clone()) {
        Ok(parsed) => parsed,
        Err(error) => {
            return crate::session::refused_for(
                verb,
                &Refusal::new(
                    RefusalClass::Malformed,
                    format!("inputs for '{verb}' do not match its schema: {error}"),
                ),
            );
        }
    };
    let loaded = match crate::store::load(root) {
        Ok(Some(loaded)) => loaded,
        Ok(None) => {
            return crate::session::unloadable_for(
                verb,
                &Refusal::new(
                    RefusalClass::UnknownTarget,
                    format!("no project exists at '{}'", root.display()),
                ),
            );
        }
        Err(refusal) => return crate::session::unloadable_for(verb, &refusal),
    };
    let Some(generation) = &loaded.project.generation else {
        return crate::session::refused_for(
            verb,
            &Refusal::new(
                RefusalClass::UnknownTarget,
                "the project carries no generation record",
            ),
        );
    };
    let Some(tree) = &generation.conditioning else {
        return crate::session::refused_for(
            verb,
            &Refusal::new(
                RefusalClass::UnknownTarget,
                "the generation record carries no conditioning section",
            ),
        );
    };
    let conditioning = match torogao_gencond::Conditioning::from_ron_tree(ron_to_gencond_tree(tree))
    {
        Ok(conditioning) => conditioning,
        Err(error) => return gencond_refused(verb, &error),
    };
    // Custody gate (D-838): a proposal that names a scene is
    // checked against that scene record's custody map BEFORE
    // compilation. A custody-frozen target refuses the whole
    // proposal with the gencond code; nothing compiles, no
    // partial application. A proposal naming no scene is a
    // Stage-1 compile, unchanged.
    if let Some(scene_id) = &parsed.scene_id {
        let Some(scene) = loaded
            .project
            .scenes
            .iter()
            .find(|scene| scene.id == *scene_id)
        else {
            return crate::session::refused_for(
                verb,
                &Refusal::new(
                    RefusalClass::UnknownTarget,
                    format!("the project carries no scene record '{scene_id}'"),
                ),
            );
        };
        let typed =
            match torogao_gencond::SceneRecord::from_ron_tree(ron_to_gencond_tree(&scene.record)) {
                Ok(typed) => typed,
                Err(error) => return gencond_refused(verb, &error),
            };
        if let Err(error) = typed.custody.check_targets(&parsed.targets) {
            return gencond_refused(verb, &error);
        }
    }
    let plan = plan_of_record(generation);
    let mut assets = torogao_gencond::AssetBundle::default();
    for source in &conditioning.sources {
        if !source.enabled {
            continue;
        }
        let (claimed, path) = match &source.kind {
            torogao_gencond::schema::SourceKind::ReferenceEmbed(embed) => {
                (embed.reference.sha256.clone(), embed.reference.path.clone())
            }
            torogao_gencond::schema::SourceKind::SpatialMap(map) => match &map.map.origin {
                torogao_gencond::schema::PoseOrigin::File { path } => {
                    (map.map.sha256.clone(), path.clone())
                }
                torogao_gencond::schema::PoseOrigin::RigProjection { .. } => {
                    match parsed.map_files.get(&map.map.sha256) {
                        Some(path) => (map.map.sha256.clone(), path.clone()),
                        None => {
                            return crate::session::refused_for(
                                verb,
                                &Refusal::new(
                                    RefusalClass::Dangling,
                                    format!(
                                        "no map file supplied for the rig-projection map {}",
                                        map.map.sha256
                                    ),
                                ),
                            );
                        }
                    }
                }
            },
        };
        let bytes = match crate::intake::read_bounded(
            &root.join(&path),
            torogao_gencond::limits::ASSET_BYTES_MAX as u64,
        ) {
            Ok(bytes) => bytes,
            Err(refusal) => return crate::session::refused_for(verb, &refusal),
        };
        assets.blobs.insert(claimed, bytes);
    }
    match torogao_gencond::compile(
        &plan,
        &conditioning,
        &parsed.bindings,
        &assets,
        &parsed.measurements,
    ) {
        Ok(output) => {
            let report = serde_json::to_value(&output.report).unwrap_or(Value::Null);
            Answer {
                status: Status::Ok,
                doc: json!({
                    "graph": output.graph.to_value(),
                    "graph_sha256": output.report.graph_sha256,
                    "report": report,
                    "result": "ok",
                    "verb": verb,
                }),
            }
        }
        Err(error) => gencond_refused(verb, &error),
    }
}

fn gencond_refused(verb: &str, error: &torogao_gencond::GencondError) -> Answer {
    Answer {
        status: Status::Refused,
        doc: json!({
            "code": error.code,
            "message": error.message,
            "result": "refused",
            "verb": verb,
        }),
    }
}

/// Converts the author's RON tree into the gencond tree by a
/// structural identity walk (the variant sets are identical
/// by design; see `torogao_gencond::ron_tree`).
pub fn ron_to_gencond_tree(tree: &crate::ron::Ron) -> torogao_gencond::Ron {
    match tree {
        crate::ron::Ron::Bool(flag) => torogao_gencond::Ron::Bool(*flag),
        crate::ron::Ron::Float(number) => torogao_gencond::Ron::Float(*number),
        crate::ron::Ron::Ident(name) => torogao_gencond::Ron::Ident(name.clone()),
        crate::ron::Ron::Int(number) => torogao_gencond::Ron::Int(*number),
        crate::ron::Ron::List(items) => {
            torogao_gencond::Ron::List(items.iter().map(ron_to_gencond_tree).collect())
        }
        crate::ron::Ron::Opt(inner) => torogao_gencond::Ron::Opt(
            inner
                .as_ref()
                .map(|value| Box::new(ron_to_gencond_tree(value))),
        ),
        crate::ron::Ron::Str(text) => torogao_gencond::Ron::Str(text.clone()),
        crate::ron::Ron::Struct(name, fields) => torogao_gencond::Ron::Struct(
            name.clone(),
            fields
                .iter()
                .map(|(key, value)| (key.clone(), ron_to_gencond_tree(value)))
                .collect(),
        ),
        crate::ron::Ron::Tuple(items) => {
            torogao_gencond::Ron::Tuple(items.iter().map(ron_to_gencond_tree).collect())
        }
    }
}

/// Converts a gencond RON tree into the author's tree (the
/// inverse walk), for storage in the project model.
pub fn gencond_tree_to_ron(tree: &torogao_gencond::Ron) -> crate::ron::Ron {
    match tree {
        torogao_gencond::Ron::Bool(flag) => crate::ron::Ron::Bool(*flag),
        torogao_gencond::Ron::Float(number) => crate::ron::Ron::Float(*number),
        torogao_gencond::Ron::Ident(name) => crate::ron::Ron::Ident(name.clone()),
        torogao_gencond::Ron::Int(number) => crate::ron::Ron::Int(*number),
        torogao_gencond::Ron::List(items) => {
            crate::ron::Ron::List(items.iter().map(gencond_tree_to_ron).collect())
        }
        torogao_gencond::Ron::Opt(inner) => crate::ron::Ron::Opt(
            inner
                .as_ref()
                .map(|value| Box::new(gencond_tree_to_ron(value))),
        ),
        torogao_gencond::Ron::Str(text) => crate::ron::Ron::Str(text.clone()),
        torogao_gencond::Ron::Struct(name, fields) => crate::ron::Ron::Struct(
            name.clone(),
            fields
                .iter()
                .map(|(key, value)| (key.clone(), gencond_tree_to_ron(value)))
                .collect(),
        ),
        torogao_gencond::Ron::Tuple(items) => {
            crate::ron::Ron::Tuple(items.iter().map(gencond_tree_to_ron).collect())
        }
    }
}
