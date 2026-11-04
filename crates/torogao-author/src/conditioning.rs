//! The generation-conditioning family (D-822): two verbs over
//! the project's generation record, delivered by the
//! `torogao-gencond` crate behind this crate's
//! `gen-conditioning` feature.
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

/// The conditioning manifest: two verbs, alphabetical. The
/// sweep verb is Stage 2 of the seam and is not here.
pub const CONDITIONING_MANIFEST: [ConditioningVerbSpec; 2] = [
    verb("conditioning_compile", "generation conditioning", false),
    verb("conditioning_set", "generation conditioning", true),
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

/// The conditioning family's mutating ops.
#[derive(Clone, Debug, PartialEq)]
pub enum ConditioningOp {
    /// `conditioning_set`.
    Set(ConditioningSetInput),
}

impl ConditioningOp {
    /// The canonical verb name.
    pub fn verb(&self) -> &'static str {
        match self {
            ConditioningOp::Set(_) => "conditioning_set",
        }
    }

    /// The normalized inputs as JSON (what the op log records).
    pub fn inputs_json(&self) -> Result<Value, Refusal> {
        let result = match self {
            ConditioningOp::Set(inputs) => serde_json::to_value(inputs),
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
        other => refuse(
            RefusalClass::UnknownKind,
            format!("verb '{other}' is read-only or not a conditioning verb"),
        ),
    }
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
    }
}

/// The apply step for conditioning ops.
pub fn apply_conditioning(project: &mut Project, op: &ConditioningOp) -> Result<(), Refusal> {
    match op {
        ConditioningOp::Set(input) => {
            project.generation = Some(record_of(input)?);
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
