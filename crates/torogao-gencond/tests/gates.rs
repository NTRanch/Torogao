//! Stage-1 gates for torogao-gencond: half validation (the
//! record, the canon compile, the graph shape, determinism),
//! half adversarial (the false off in its three forms, stale
//! and missing measurements, hash lies, duplicate definitions).

use std::collections::BTreeMap;
use torogao_gencond::canon::{CanonPose, CanonRecord, D802_PROFILE, compile_canon};
use torogao_gencond::compile::{AssetBundle, compile, sha256_hex};
use torogao_gencond::measure::EmbedMeasurement;
use torogao_gencond::plan::{GenerationPlan, LatentSpec, ModelBinding, SamplerSpec};
use torogao_gencond::ron_tree::Ron;
use torogao_gencond::schema::{
    CombineEmbeds, Conditioning, ControlType, CropPolicy, EmbedScaling, ImageRef, LoraEntry,
    Normalisation, PoseMapRef, PoseOrigin, ReferenceEmbed, SlotText, Source, SourceKind,
    SpatialMap, TextPair, TextSlot, WeightType,
};

const ADAPTER: &str = "ip-adapter-plus_sdxl";
const CHECKPOINT: &str = "waiIllustriousSDXL_v170";
const CONTROL: &str = "controlnet-union-sdxl";
const ENCODER: &str = "clip-vit-h";
const LORA: &str = "style-detailer";

fn png_bytes(width: u32, height: u32, pixel: impl Fn(u32, u32) -> [u8; 3]) -> Vec<u8> {
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, width, height);
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();
        let mut data = Vec::new();
        for y in 0..height {
            for x in 0..width {
                data.extend_from_slice(&pixel(x, y));
            }
        }
        writer.write_image_data(&data).unwrap();
    }
    out
}

fn gradient_png() -> Vec<u8> {
    png_bytes(8, 8, |x, y| [(x * 30) as u8, (y * 30) as u8, 128])
}

fn uniform_png() -> Vec<u8> {
    png_bytes(8, 8, |_, _| [0, 0, 0])
}

fn portrait_png() -> Vec<u8> {
    png_bytes(8, 8, |x, _| [(255 - x * 20) as u8, 90, 160])
}

fn plan() -> GenerationPlan {
    GenerationPlan {
        id: "vivienne-keeper".to_string(),
        checkpoint: CHECKPOINT.to_string(),
        latent: LatentSpec {
            width: 1024,
            height: 1280,
            batch_size: 1,
        },
        sampler: SamplerSpec {
            steps: 30,
            cfg: 6.0,
            name: "euler_ancestral".to_string(),
            scheduler: "normal".to_string(),
            denoise: 1.0,
        },
        seed: 20261301,
        filename_prefix: "keeper".to_string(),
    }
}

fn conditioning() -> Conditioning {
    Conditioning {
        schema: 1,
        text: TextPair {
            positive: vec![
                SlotText {
                    slot: TextSlot::Scene,
                    text: "in a candlelit library".to_string(),
                },
                SlotText {
                    slot: TextSlot::Identity,
                    text: "Vivienne, deep violet-black hair".to_string(),
                },
            ],
            negative: "photograph, blur".to_string(),
        },
        lora_stack: vec![LoraEntry {
            model: LORA.to_string(),
            strength_model: 0.4,
            strength_clip: 0.2,
        }],
        sources: vec![
            Source {
                id: "identity-ref".to_string(),
                kind: SourceKind::ReferenceEmbed(ReferenceEmbed {
                    reference: ImageRef {
                        path: "portraits/vivienne_begin.png".to_string(),
                        sha256: sha256_hex(&portrait_png()),
                        crop: CropPolicy::HeadShouldersSquare,
                    },
                    adapter: ADAPTER.to_string(),
                    vision_encoder: ENCODER.to_string(),
                    weight: 0.35,
                    weight_type: WeightType::Linear,
                    combine: CombineEmbeds::Concat,
                    window: None,
                    normalisation: Normalisation::PerReference {
                        scaling: EmbedScaling::KeyValueCPenalty,
                    },
                }),
                enabled: true,
            },
            Source {
                id: "pose".to_string(),
                kind: SourceKind::SpatialMap(SpatialMap {
                    model: CONTROL.to_string(),
                    control_type: ControlType::Openpose,
                    map: PoseMapRef {
                        origin: PoseOrigin::RigProjection {
                            rig: "vivienne-rig".to_string(),
                            pose: "standing".to_string(),
                            camera: "front".to_string(),
                        },
                        sha256: sha256_hex(&gradient_png()),
                    },
                    strength: 0.5,
                    window: None,
                }),
                enabled: true,
            },
        ],
    }
}

fn registry() -> BTreeMap<String, ModelBinding> {
    let mut map = BTreeMap::new();
    map.insert(
        CHECKPOINT.to_string(),
        ModelBinding {
            file: Some("waiIllustriousSDXL_v170.safetensors".to_string()),
            preset: None,
            sha256: None,
        },
    );
    map.insert(
        ADAPTER.to_string(),
        ModelBinding {
            file: Some("ip-adapter-plus_sdxl_vit-h.safetensors".to_string()),
            preset: Some("PLUS (high strength)".to_string()),
            sha256: None,
        },
    );
    map.insert(
        ENCODER.to_string(),
        ModelBinding {
            file: Some("CLIP-ViT-H-14.safetensors".to_string()),
            preset: None,
            sha256: None,
        },
    );
    map.insert(
        CONTROL.to_string(),
        ModelBinding {
            file: Some("controlnet-union-sdxl-promax.safetensors".to_string()),
            preset: None,
            sha256: None,
        },
    );
    map.insert(
        LORA.to_string(),
        ModelBinding {
            file: Some("style-detailer.safetensors".to_string()),
            preset: None,
            sha256: None,
        },
    );
    map
}

fn assets() -> AssetBundle {
    let mut bundle = AssetBundle::default();
    bundle
        .blobs
        .insert(sha256_hex(&portrait_png()), portrait_png());
    bundle
        .blobs
        .insert(sha256_hex(&gradient_png()), gradient_png());
    bundle
}

fn measurements() -> Vec<EmbedMeasurement> {
    vec![EmbedMeasurement {
        reference_sha256: sha256_hex(&portrait_png()),
        vision_encoder: ENCODER.to_string(),
        token_count: 257,
        magnitude_mean: 11.25,
        magnitude_max: 24.5,
    }]
}

fn node_by_class<'a>(value: &'a serde_json::Value, class: &str) -> &'a serde_json::Value {
    value
        .as_object()
        .unwrap()
        .values()
        .find(|node| node["class_type"] == class)
        .unwrap_or_else(|| panic!("no node of class {class}"))
}

fn classes(value: &serde_json::Value) -> Vec<String> {
    value
        .as_object()
        .unwrap()
        .values()
        .map(|node| node["class_type"].as_str().unwrap().to_string())
        .collect()
}

// --- validation -------------------------------------------------

#[test]
fn tree_round_trip_is_a_fixed_point() {
    let record = conditioning();
    let tree = record.to_ron_tree();
    let back = Conditioning::from_ron_tree(tree.clone()).unwrap();
    assert_eq!(back, record);
    assert_eq!(back.to_ron_tree(), tree);
}

#[test]
fn json_round_trip_preserves_the_record() {
    let record = conditioning();
    let text = serde_json::to_string(&record).unwrap();
    let back: Conditioning = serde_json::from_str(&text).unwrap();
    assert_eq!(back, record);
    // The oracle strings ride the JSON form directly.
    assert!(text.contains("\"weight_type\":\"linear\""));
    assert!(text.contains("\"K+V w/ C penalty\""));
    assert!(text.contains("\"openpose\""));
}

#[test]
fn positive_composes_in_slot_order_not_record_order() {
    // The fixture lists Scene before Identity; composition is
    // slot-ordered regardless.
    assert_eq!(
        conditioning().composed_positive(),
        "Vivienne, deep violet-black hair, in a candlelit library"
    );
}

#[test]
fn canon_compiles_to_the_cured_shape() {
    let canon = CanonRecord {
        identity: "Vivienne, deep violet-black hair".to_string(),
        wardrobe: "a black velvet gown".to_string(),
        expression: "calm, half-lidded gaze".to_string(),
        scene: Some("in a candlelit library".to_string()),
        negative: "photograph, blur".to_string(),
        portrait_path: "portraits/vivienne_begin.png".to_string(),
        portrait_sha256: "a".repeat(64),
        adapter: ADAPTER.to_string(),
        vision_encoder: ENCODER.to_string(),
        pose: Some(CanonPose {
            rig: "vivienne-rig".to_string(),
            pose: "standing".to_string(),
            camera: "front".to_string(),
            map_sha256: "b".repeat(64),
            control_model: CONTROL.to_string(),
            control_type: ControlType::Openpose,
        }),
        loras: Vec::new(),
    };
    let compiled = compile_canon(&canon, D802_PROFILE).unwrap();
    assert_eq!(compiled.sources.len(), 2);
    match &compiled.sources[0].kind {
        SourceKind::ReferenceEmbed(embed) => {
            assert_eq!(embed.weight, 0.35);
            assert_eq!(embed.reference.crop, CropPolicy::HeadShouldersSquare);
            assert_eq!(
                embed.normalisation.scaling(),
                EmbedScaling::KeyValueCPenalty
            );
            assert_eq!(embed.weight_type, WeightType::Linear);
            assert_eq!(embed.combine, CombineEmbeds::Concat);
        }
        other => panic!("first source is not a reference embed: {other:?}"),
    }
    match &compiled.sources[1].kind {
        SourceKind::SpatialMap(map) => assert_eq!(map.strength, 0.5),
        other => panic!("second source is not a spatial map: {other:?}"),
    }
    assert!(compiled.composed_positive().starts_with("Vivienne"));
}

#[test]
fn graph_carries_the_production_shape_and_numbers() {
    let output = compile(
        &plan(),
        &conditioning(),
        &registry(),
        &assets(),
        &measurements(),
    )
    .unwrap();
    let value = output.graph.to_value();
    let adapter = node_by_class(&value, "IPAdapterAdvanced");
    assert_eq!(adapter["inputs"]["weight"], serde_json::json!(0.35));
    assert_eq!(adapter["inputs"]["weight_type"], "linear");
    assert_eq!(adapter["inputs"]["combine_embeds"], "concat");
    assert_eq!(adapter["inputs"]["embeds_scaling"], "K+V w/ C penalty");
    // The omitted window is restated explicitly (DESIGN §1.3).
    assert_eq!(adapter["inputs"]["start_at"], serde_json::json!(0.0));
    assert_eq!(adapter["inputs"]["end_at"], serde_json::json!(1.0));
    let apply = node_by_class(&value, "ControlNetApplyAdvanced");
    assert_eq!(apply["inputs"]["strength"], serde_json::json!(0.5));
    let type_node = node_by_class(&value, "SetUnionControlNetType");
    assert_eq!(type_node["inputs"]["type"], "openpose");
    let sampler = node_by_class(&value, "KSampler");
    assert_eq!(sampler["inputs"]["seed"], serde_json::json!(20261301));
    assert_eq!(sampler["inputs"]["steps"], serde_json::json!(30));
    assert_eq!(sampler["inputs"]["cfg"], serde_json::json!(6.0));
    assert_eq!(output.report.graph_sha256, output.graph.sha256());
    assert_eq!(output.report.normalisations.len(), 1);
    assert_eq!(output.report.normalisations[0].magnitude_mean, 11.25);
    assert!(output.report.flags.is_empty());
    // The rig-projection map loads from its derived path.
    let map_load = value.as_object().unwrap().values().find(|node| {
        node["class_type"] == "LoadImage"
            && node["inputs"]["image"]
                .as_str()
                .is_some_and(|path| path.starts_with("gencond/maps/"))
    });
    assert!(map_load.is_some());
}

#[test]
fn compilation_is_byte_deterministic() {
    let first = compile(
        &plan(),
        &conditioning(),
        &registry(),
        &assets(),
        &measurements(),
    )
    .unwrap();
    let second = compile(
        &plan(),
        &conditioning(),
        &registry(),
        &assets(),
        &measurements(),
    )
    .unwrap();
    assert_eq!(first.graph.canonical_text(), second.graph.canonical_text());
    assert_eq!(first.report.graph_sha256, second.report.graph_sha256);
}

#[test]
fn disabled_source_emits_no_nodes_and_is_reported() {
    let mut record = conditioning();
    record.sources[1].enabled = false;
    // The disabled source's asset is not even supplied: off
    // means never consulted.
    let mut bundle = AssetBundle::default();
    bundle
        .blobs
        .insert(sha256_hex(&portrait_png()), portrait_png());
    let output = compile(&plan(), &record, &registry(), &bundle, &measurements()).unwrap();
    let names = classes(&output.graph.to_value());
    assert!(!names.iter().any(|c| c == "ControlNetApplyAdvanced"));
    assert!(!names.iter().any(|c| c == "ControlNetLoader"));
    assert_eq!(output.report.omitted_sources.len(), 1);
    assert_eq!(output.report.omitted_sources[0].id, "pose");
}

#[test]
fn both_zero_lora_is_omitted_single_zero_is_kept() {
    let mut record = conditioning();
    record.lora_stack = vec![
        LoraEntry {
            model: LORA.to_string(),
            strength_model: 0.0,
            strength_clip: 0.0,
        },
        LoraEntry {
            model: "half-lora".to_string(),
            strength_model: 0.0,
            strength_clip: 0.3,
        },
    ];
    let mut reg = registry();
    reg.insert(
        "half-lora".to_string(),
        ModelBinding {
            file: Some("half-lora.safetensors".to_string()),
            preset: None,
            sha256: None,
        },
    );
    let output = compile(&plan(), &record, &reg, &assets(), &measurements()).unwrap();
    assert_eq!(output.report.omitted_loras.len(), 1);
    assert_eq!(output.report.omitted_loras[0].model, LORA);
    let names = classes(&output.graph.to_value());
    assert_eq!(names.iter().filter(|c| *c == "LoraLoader").count(), 1);
}

// --- adversarial ------------------------------------------------

#[test]
fn uniform_map_is_refused_by_name() {
    let mut record = conditioning();
    if let SourceKind::SpatialMap(map) = &mut record.sources[1].kind {
        map.map.sha256 = sha256_hex(&uniform_png());
    }
    let mut bundle = assets();
    bundle
        .blobs
        .insert(sha256_hex(&uniform_png()), uniform_png());
    let error = compile(&plan(), &record, &registry(), &bundle, &measurements()).unwrap_err();
    assert_eq!(error.code, "TG-GENCOND-BLANK-MAP");
    assert!(error.message.contains("pose"));
}

#[test]
fn missing_crop_is_refused_in_tree_and_json() {
    fn strip_crop(tree: &mut Ron) {
        if let Ron::Struct(name, fields) = tree {
            if name == "ImageRef" {
                fields.retain(|(key, _)| key != "crop");
            }
            for (_, value) in fields.iter_mut() {
                strip_crop(value);
            }
        }
        if let Ron::List(items) = tree {
            for item in items.iter_mut() {
                strip_crop(item);
            }
        }
    }
    let mut tree = conditioning().to_ron_tree();
    strip_crop(&mut tree);
    let error = Conditioning::from_ron_tree(tree).unwrap_err();
    assert!(
        error.message.contains("crop"),
        "unexpected refusal: {error}"
    );

    let mut value = serde_json::to_value(conditioning()).unwrap();
    value["sources"][0]["kind"]["reference_embed"]["reference"]
        .as_object_mut()
        .unwrap()
        .remove("crop");
    let json_error = serde_json::from_value::<Conditioning>(value).unwrap_err();
    assert!(json_error.to_string().contains("crop"));
}

#[test]
fn enabled_weight_zero_keeps_its_nodes() {
    // The zero-point distinction (DESIGN §2.1): weight 0.0
    // with enabled=true is a measured operating point, not an
    // off — the branch is present in the graph.
    let mut record = conditioning();
    if let SourceKind::ReferenceEmbed(embed) = &mut record.sources[0].kind {
        embed.weight = 0.0;
    }
    let output = compile(&plan(), &record, &registry(), &assets(), &measurements()).unwrap();
    let names = classes(&output.graph.to_value());
    assert!(names.iter().any(|c| c == "IPAdapterAdvanced"));
    assert!(output.report.omitted_sources.is_empty());
}

#[test]
fn hot_reference_is_flagged_outside_the_evidence_regime() {
    let mut record = conditioning();
    if let SourceKind::ReferenceEmbed(embed) = &mut record.sources[0].kind {
        embed.normalisation = Normalisation::PerReference {
            scaling: EmbedScaling::VOnly,
        };
    }
    let output = compile(&plan(), &record, &registry(), &assets(), &measurements()).unwrap();
    assert_eq!(output.report.flags, vec!["hot-reference:identity-ref"]);
    assert!(output.report.normalisations[0].hot_reference);
    assert_eq!(output.report.normalisations[0].mode, "V only");
}

#[test]
fn unmeasured_reference_is_refused() {
    let error = compile(&plan(), &conditioning(), &registry(), &assets(), &[]).unwrap_err();
    assert_eq!(error.code, "TG-GENCOND-UNMEASURED-REFERENCE");
}

#[test]
fn stale_measurement_is_refused() {
    let mut stale = measurements();
    stale[0].vision_encoder = "some-other-encoder".to_string();
    let error = compile(&plan(), &conditioning(), &registry(), &assets(), &stale).unwrap_err();
    assert_eq!(error.code, "TG-GENCOND-STALE-MEASUREMENT");
}

#[test]
fn hash_mismatch_is_refused() {
    let claimed = sha256_hex(&portrait_png());
    let mut bundle = assets();
    bundle.blobs.insert(claimed.clone(), gradient_png());
    let error = compile(
        &plan(),
        &conditioning(),
        &registry(),
        &bundle,
        &measurements(),
    )
    .unwrap_err();
    assert_eq!(error.code, "TG-GENCOND-HASH-MISMATCH");
}

#[test]
fn unknown_model_is_a_dangling_refusal() {
    let mut reg = registry();
    reg.remove(ADAPTER);
    let error = compile(&plan(), &conditioning(), &reg, &assets(), &measurements()).unwrap_err();
    assert_eq!(error.code, "TG-GENCOND-DANGLING");
}

#[test]
fn duplicate_slot_and_source_id_are_conflicts() {
    let mut record = conditioning();
    record.text.positive.push(SlotText {
        slot: TextSlot::Identity,
        text: "a second identity".to_string(),
    });
    assert_eq!(record.validate().unwrap_err().code, "TG-GENCOND-CONFLICT");

    let mut record = conditioning();
    record.sources.push(record.sources[0].clone());
    assert_eq!(record.validate().unwrap_err().code, "TG-GENCOND-CONFLICT");
}

#[test]
fn canon_hex_colours_are_refused() {
    let canon = CanonRecord {
        identity: "Vivienne, hair #7A1FA2".to_string(),
        wardrobe: "a black velvet gown".to_string(),
        expression: "calm".to_string(),
        scene: None,
        negative: "blur".to_string(),
        portrait_path: "portraits/vivienne_begin.png".to_string(),
        portrait_sha256: "a".repeat(64),
        adapter: ADAPTER.to_string(),
        vision_encoder: ENCODER.to_string(),
        pose: None,
        loras: Vec::new(),
    };
    let error = compile_canon(&canon, D802_PROFILE).unwrap_err();
    assert!(error.message.contains("hex colour"));
}
