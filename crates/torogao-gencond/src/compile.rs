//! The compiler: `GenerationPlan` + `Conditioning` -> a
//! `torogao-gen-graph/1` graph (DESIGN §1.3).
//!
//! The production shape is the estate's observed working
//! wiring (the D-802 cured chain): checkpoint, LoRA chain,
//! slot-composed CLIP text pair, latent, then per-source
//! branches (reference-embed branches patch the model path;
//! spatial-map branches patch the text pair), sampler, decode,
//! save. Fields the record omits are never invented: the one
//! defaulted field, `window`, is restated explicitly (DESIGN
//! §1.3). A disabled source produces NO node and is listed in
//! the report as omitted — structural off, not a zero.

use crate::error::GencondResult;
use crate::error::{
    CODE_BLANK_MAP, CODE_DANGLING, CODE_HASH_MISMATCH, CODE_MAP_FORMAT, CODE_RANGE, refuse,
};
use crate::graph::{GenGraph, GraphInput, int, link, text, uint};
use crate::limits::ASSET_BYTES_MAX;
use crate::limits::MAP_PIXELS_MAX;
use crate::measure::{EmbedMeasurement, NormalisationRecord, normalize_reference_embed};
use crate::plan::{GenerationPlan, ModelBinding};
use crate::schema::{Conditioning, PoseOrigin, SourceKind, Window};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

/// Asset bytes for the compile, keyed by the SHA-256 the
/// record claims for them. The compiler re-hashes every blob
/// it touches: bytes that do not match their key refuse with
/// `TG-GENCOND-HASH-MISMATCH`.
#[derive(Clone, Debug, Default)]
pub struct AssetBundle {
    /// Blobs by claimed SHA-256 (lowercase hex).
    pub blobs: BTreeMap<String, Vec<u8>>,
}

/// How one model name resolved, recorded for provenance
/// (S-8's binding record).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct BindingRecord {
    /// The S-7 name as recorded.
    pub name: String,
    /// The role the name played (checkpoint, lora, adapter,
    /// vision_encoder, control_net).
    pub role: String,
    /// The resolved file, when the binding carries one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    /// The resolved preset (adapters), when carried.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preset: Option<String>,
    /// The file's recorded hash, when carried.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
}

/// A source the record disabled, listed so absence from the
/// graph is auditable, not silent.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct OmittedSource {
    /// The source's id.
    pub id: String,
    /// The source's kind name.
    pub kind: String,
}

/// A LoRA entry omitted by the spec-level zero rule (S-4
/// §1.3: an entry with both strengths 0.0 is definitionally
/// identical to its absence, so the compiler omits it).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct OmittedLora {
    /// The entry's model name.
    pub model: String,
    /// Why it was omitted.
    pub reason: String,
}

/// The compile's audit trail: what was omitted, what
/// normalisation measured, what the models bound to, and what
/// was flagged. Provenance consumes this record (S-8).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CompileReport {
    /// SHA-256 of the graph's canonical serialization.
    pub graph_sha256: String,
    /// Disabled sources, in record order.
    pub omitted_sources: Vec<OmittedSource>,
    /// Zero-omitted LoRA entries, in record order.
    pub omitted_loras: Vec<OmittedLora>,
    /// One normalisation record per enabled reference-embed
    /// source.
    pub normalisations: Vec<NormalisationRecord>,
    /// Model bindings used, in first-use order.
    pub bindings: Vec<BindingRecord>,
    /// Compile flags (e.g. `hot-reference:<source-id>`).
    pub flags: Vec<String>,
}

/// A successful compile: the graph and its report.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CompileOutput {
    /// The compiled graph.
    pub graph: GenGraph,
    /// The compile report.
    pub report: CompileReport,
}

/// Compiles a plan and conditioning record into a gen-graph.
///
/// Deterministic: emission order is fixed (plan chain, LoRAs
/// in record order, text pair, latent, sources in record
/// order), ids are sequential, and the report is built in the
/// same order — the same inputs compile to byte-identical
/// canonical JSON (DESIGN §3.3.3).
pub fn compile(
    plan: &GenerationPlan,
    conditioning: &Conditioning,
    registry: &BTreeMap<String, ModelBinding>,
    assets: &AssetBundle,
    measurements: &[EmbedMeasurement],
) -> GencondResult<CompileOutput> {
    plan.validate()?;
    conditioning.validate()?;
    let mut builder = Builder {
        graph: GenGraph::default(),
        bindings: Vec::new(),
    };
    let checkpoint = builder.binding(plan.checkpoint.as_str(), "checkpoint", registry)?;
    let checkpoint_file = required_file(&checkpoint, plan.checkpoint.as_str())?;
    let checkpoint_node = builder.graph.push(
        "CheckpointLoaderSimple",
        vec![("ckpt_name", text(&checkpoint_file))],
    )?;
    builder.record_binding(plan.checkpoint.as_str(), "checkpoint", &checkpoint);

    // LoRA chain on (model, clip) from the checkpoint.
    let mut model_out = (checkpoint_node.clone(), 0u32);
    let mut clip_out = (checkpoint_node.clone(), 1u32);
    let mut omitted_loras = Vec::new();
    for entry in &conditioning.lora_stack {
        if entry.strength_model == 0.0 && entry.strength_clip == 0.0 {
            omitted_loras.push(OmittedLora {
                model: entry.model.clone(),
                reason: "both strengths are 0.0; spec-level zero is omission (S-4 §1.3)"
                    .to_string(),
            });
            continue;
        }
        let binding = builder.binding(entry.model.as_str(), "lora", registry)?;
        let file = required_file(&binding, entry.model.as_str())?;
        let node = builder.graph.push(
            "LoraLoader",
            vec![
                ("clip", link(&clip_out.0, clip_out.1)),
                ("lora_name", text(&file)),
                ("model", link(&model_out.0, model_out.1)),
                ("strength_clip", GraphInput::float(entry.strength_clip)),
                ("strength_model", GraphInput::float(entry.strength_model)),
            ],
        )?;
        builder.record_binding(entry.model.as_str(), "lora", &binding);
        model_out = (node.clone(), 0);
        clip_out = (node, 1);
    }

    // The text pair, from the (possibly LoRA-patched) clip.
    let positive_node = builder.graph.push(
        "CLIPTextEncode",
        vec![
            ("clip", link(&clip_out.0, clip_out.1)),
            ("text", text(&conditioning.composed_positive())),
        ],
    )?;
    let negative_node = builder.graph.push(
        "CLIPTextEncode",
        vec![
            ("clip", link(&clip_out.0, clip_out.1)),
            ("text", text(&conditioning.text.negative)),
        ],
    )?;
    let latent_node = builder.graph.push(
        "EmptyLatentImage",
        vec![
            ("batch_size", uint(u64::from(plan.latent.batch_size))),
            ("height", uint(u64::from(plan.latent.height))),
            ("width", uint(u64::from(plan.latent.width))),
        ],
    )?;

    // Sources, in record order. Reference-embed branches patch
    // the model path; spatial branches patch the text pair.
    let mut positive_cur = (positive_node, 0u32);
    let mut negative_cur = (negative_node, 0u32);
    let mut omitted_sources = Vec::new();
    let mut normalisations = Vec::new();
    let mut flags: Vec<String> = Vec::new();
    for source in &conditioning.sources {
        if !source.enabled {
            // Structural off: no node, no asset consulted, no
            // measurement required — the report lists it.
            omitted_sources.push(OmittedSource {
                id: source.id.clone(),
                kind: source.kind.kind_name().to_string(),
            });
            continue;
        }
        match &source.kind {
            SourceKind::ReferenceEmbed(embed) => {
                verified_blob(assets, &embed.reference.sha256, "reference")?;
                let (record, flag) = normalize_reference_embed(&source.id, embed, measurements)?;
                normalisations.push(record);
                if let Some(flag) = flag {
                    flags.push(flag);
                }
                let image_node = builder
                    .graph
                    .push("LoadImage", vec![("image", text(&embed.reference.path))])?;
                let adapter = builder.binding(embed.adapter.as_str(), "adapter", registry)?;
                let preset = adapter.preset.clone().ok_or_else(|| {
                    crate::error::GencondError::new(
                        CODE_DANGLING,
                        format!("adapter '{}' has no preset binding", embed.adapter),
                    )
                })?;
                builder.record_binding(embed.adapter.as_str(), "adapter", &adapter);
                let encoder =
                    builder.binding(embed.vision_encoder.as_str(), "vision_encoder", registry)?;
                builder.record_binding(embed.vision_encoder.as_str(), "vision_encoder", &encoder);
                let unified_node = builder.graph.push(
                    "IPAdapterUnifiedLoader",
                    vec![
                        ("model", link(&model_out.0, model_out.1)),
                        ("preset", text(&preset)),
                    ],
                )?;
                let window = embed.window.unwrap_or(Window::FULL);
                let advanced_node = builder.graph.push(
                    "IPAdapterAdvanced",
                    vec![
                        ("combine_embeds", text(embed.combine.oracle_str())),
                        (
                            "embeds_scaling",
                            text(embed.normalisation.scaling().oracle_str()),
                        ),
                        ("end_at", GraphInput::float(window.end)),
                        ("image", link(&image_node, 0)),
                        ("ipadapter", link(&unified_node, 1)),
                        ("model", link(&unified_node, 0)),
                        ("start_at", GraphInput::float(window.start)),
                        ("weight", GraphInput::float(embed.weight)),
                        ("weight_type", text(embed.weight_type.oracle_str())),
                    ],
                )?;
                model_out = (advanced_node, 0);
            }
            SourceKind::SpatialMap(map) => {
                let blob = verified_blob(assets, &map.map.sha256, "pose map")?;
                refuse_if_uniform_map(&source.id, blob)?;
                let image_path = match &map.map.origin {
                    PoseOrigin::File { path } => path.clone(),
                    PoseOrigin::RigProjection { .. } => {
                        format!("gencond/maps/{}.png", map.map.sha256)
                    }
                };
                let image_node = builder
                    .graph
                    .push("LoadImage", vec![("image", text(&image_path))])?;
                let binding = builder.binding(map.model.as_str(), "control_net", registry)?;
                let file = required_file(&binding, map.model.as_str())?;
                let loader_node = builder
                    .graph
                    .push("ControlNetLoader", vec![("control_net_name", text(&file))])?;
                builder.record_binding(map.model.as_str(), "control_net", &binding);
                let type_node = builder.graph.push(
                    "SetUnionControlNetType",
                    vec![
                        ("control_net", link(&loader_node, 0)),
                        ("type", text(map.control_type.oracle_str())),
                    ],
                )?;
                let window = map.window.unwrap_or(Window::FULL);
                let apply_node = builder.graph.push(
                    "ControlNetApplyAdvanced",
                    vec![
                        ("control_net", link(&type_node, 0)),
                        ("end_percent", GraphInput::float(window.end)),
                        ("image", link(&image_node, 0)),
                        ("negative", link(&negative_cur.0, negative_cur.1)),
                        ("positive", link(&positive_cur.0, positive_cur.1)),
                        ("start_percent", GraphInput::float(window.start)),
                        ("strength", GraphInput::float(map.strength)),
                    ],
                )?;
                positive_cur = (apply_node.clone(), 0);
                negative_cur = (apply_node, 1);
            }
        }
    }

    let sampler_node = builder.graph.push(
        "KSampler",
        vec![
            ("cfg", GraphInput::float(plan.sampler.cfg)),
            ("denoise", GraphInput::float(plan.sampler.denoise)),
            ("latent_image", link(&latent_node, 0)),
            ("model", link(&model_out.0, model_out.1)),
            ("negative", link(&negative_cur.0, negative_cur.1)),
            ("positive", link(&positive_cur.0, positive_cur.1)),
            ("sampler_name", text(&plan.sampler.name)),
            ("scheduler", text(&plan.sampler.scheduler)),
            ("seed", int(plan.seed)),
            ("steps", uint(u64::from(plan.sampler.steps))),
        ],
    )?;
    let decode_node = builder.graph.push(
        "VAEDecode",
        vec![
            ("samples", link(&sampler_node, 0)),
            ("vae", link(&checkpoint_node, 2)),
        ],
    )?;
    builder.graph.push(
        "SaveImage",
        vec![
            ("filename_prefix", text(&plan.filename_prefix)),
            ("images", link(&decode_node, 0)),
        ],
    )?;

    let graph_sha256 = builder.graph.sha256();
    Ok(CompileOutput {
        graph: builder.graph,
        report: CompileReport {
            graph_sha256,
            omitted_sources,
            omitted_loras,
            normalisations,
            bindings: builder.bindings,
            flags,
        },
    })
}

struct Builder {
    graph: GenGraph,
    bindings: Vec<BindingRecord>,
}

impl Builder {
    fn binding(
        &self,
        name: &str,
        role: &str,
        registry: &BTreeMap<String, ModelBinding>,
    ) -> GencondResult<ModelBinding> {
        registry.get(name).cloned().ok_or_else(|| {
            crate::error::GencondError::new(
                CODE_DANGLING,
                format!("{role} '{name}' has no binding in the model registry"),
            )
        })
    }

    fn record_binding(&mut self, name: &str, role: &str, binding: &ModelBinding) {
        let record = BindingRecord {
            name: name.to_string(),
            role: role.to_string(),
            file: binding.file.clone(),
            preset: binding.preset.clone(),
            sha256: binding.sha256.clone(),
        };
        if !self.bindings.contains(&record) {
            self.bindings.push(record);
        }
    }
}

fn required_file(binding: &ModelBinding, name: &str) -> GencondResult<String> {
    binding.file.clone().ok_or_else(|| {
        crate::error::GencondError::new(
            CODE_DANGLING,
            format!("model '{name}' has no file binding"),
        )
    })
}

/// Fetches a blob by its claimed hash and verifies the bytes
/// hash to it. Disabled sources never reach this function.
fn verified_blob<'a>(
    assets: &'a AssetBundle,
    sha256: &str,
    what: &str,
) -> GencondResult<&'a Vec<u8>> {
    let blob = assets.blobs.get(sha256).ok_or_else(|| {
        crate::error::GencondError::new(
            CODE_DANGLING,
            format!("no asset bytes supplied for the {what} hashed {sha256}"),
        )
    })?;
    if blob.len() > ASSET_BYTES_MAX {
        return refuse(CODE_RANGE, format!("{what} blob exceeds the byte bound"));
    }
    if sha256_hex(blob) != sha256 {
        return refuse(
            CODE_HASH_MISMATCH,
            format!("{what} bytes do not hash to the recorded {sha256}"),
        );
    }
    Ok(blob)
}

/// SHA-256 of bytes, lowercase hex.
pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// The false-off refusal (DESIGN §2.1): a pose map that decodes
/// to one uniform colour is not a null — measured at full
/// conditioning scale in Phase 0 — so the compiler refuses it
/// by name instead of emitting a branch that would silently
/// condition on nothing-shaped data.
fn refuse_if_uniform_map(source_id: &str, bytes: &[u8]) -> GencondResult<()> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::EXPAND);
    let mut reader = decoder.read_info().map_err(|error| {
        crate::error::GencondError::new(
            CODE_MAP_FORMAT,
            format!("source '{source_id}' map is not a decodable PNG: {error}"),
        )
    })?;
    let (width, height) = (reader.info().width, reader.info().height);
    if u64::from(width) * u64::from(height) > MAP_PIXELS_MAX {
        return refuse(
            CODE_RANGE,
            format!("source '{source_id}' map exceeds the pixel bound"),
        );
    }
    let buffer_size = reader.output_buffer_size().ok_or_else(|| {
        crate::error::GencondError::new(
            CODE_MAP_FORMAT,
            format!("source '{source_id}' map frame size is not determinable"),
        )
    })?;
    let mut buffer = vec![0_u8; buffer_size];
    let frame = reader.next_frame(&mut buffer).map_err(|error| {
        crate::error::GencondError::new(
            CODE_MAP_FORMAT,
            format!("source '{source_id}' map frame does not decode: {error}"),
        )
    })?;
    let bytes_per_sample = match frame.bit_depth {
        png::BitDepth::Eight => 1_usize,
        png::BitDepth::Sixteen => 2_usize,
        _ => {
            return refuse(
                CODE_MAP_FORMAT,
                format!("source '{source_id}' map bit depth is not 8 or 16 after expansion"),
            );
        }
    };
    let stride = frame.color_type.samples() * bytes_per_sample;
    let data = &buffer[..frame.buffer_size()];
    if stride == 0 || data.len() < stride {
        return refuse(
            CODE_MAP_FORMAT,
            format!("source '{source_id}' map frame is shorter than one pixel"),
        );
    }
    let first = &data[..stride];
    if data.chunks_exact(stride).all(|pixel| pixel == first) {
        return refuse(
            CODE_BLANK_MAP,
            format!(
                "source '{source_id}' pose map is a single uniform colour; a blank map is \
                 not a null (it conditions at full scale) — disable the source instead"
            ),
        );
    }
    Ok(())
}
