//! Canon compilation (DESIGN §1.5): a canon character record —
//! CHARACTERS.md identity, wardrobe as recorded, wardrobe-
//! independent expression register, canon scene fragments —
//! compiles into the slot structure, so "the same character in
//! a new scene" is the same record with one slot's block
//! replaced. The palette rides the identity reference-embed
//! source only; no text block may carry hex codes (colour is
//! the reference's job).

use crate::error::GencondResult;
use crate::error::{CODE_MALFORMED, refuse};
use crate::schema::{
    CombineEmbeds, Conditioning, ControlType, CropPolicy, EmbedScaling, ImageRef, LoraEntry,
    Normalisation, PoseMapRef, PoseOrigin, ReferenceEmbed, SCHEMA_VERSION, SlotText, Source,
    SourceKind, SpatialMap, TextPair, TextSlot, WeightType,
};
use serde::{Deserialize, Serialize};

/// One canon character's generation inputs, independent of any
/// engine. `None` scene means the record carries no scene
/// block yet (the slot is simply unfilled).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CanonRecord {
    /// The identity block (CHARACTERS.md identity, verbatim
    /// class).
    pub identity: String,
    /// The wardrobe block as recorded for this outfit.
    pub wardrobe: String,
    /// The expression register block (wardrobe-independent).
    pub expression: String,
    /// The scene block, when the record carries one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scene: Option<String>,
    /// The negative block.
    pub negative: String,
    /// The canon portrait the identity reference frames.
    pub portrait_path: String,
    /// SHA-256 of the portrait bytes.
    pub portrait_sha256: String,
    /// S-7 name of the identity adapter.
    pub adapter: String,
    /// S-7 name of the vision encoder.
    pub vision_encoder: String,
    /// The rig-derived pose source, when the record has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pose: Option<CanonPose>,
    /// LoRA stack entries, in order.
    #[serde(default)]
    pub loras: Vec<LoraEntry>,
}

/// A canon record's pose source: a rig projection (the pose
/// provenance of record) plus the union control binding it
/// applies through.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CanonPose {
    /// Rig reference.
    pub rig: String,
    /// Pose id in the rig's pose set.
    pub pose: String,
    /// Camera id.
    pub camera: String,
    /// SHA-256 of the rendered map.
    pub map_sha256: String,
    /// S-7 name of the union control model.
    pub control_model: String,
    /// The union control type.
    pub control_type: ControlType,
}

/// The tuneables a canon compile takes: measured quantities
/// from a production regime, passed in explicitly — never
/// compiled-in defaults (DESIGN §1.3's no-invented-fields rule
/// applied to compilation itself).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CanonProfile {
    /// The identity reference-embed weight.
    pub reference_weight: f64,
    /// The pose-source strength, when a pose is present.
    pub control_strength: f64,
}

/// The cured 2026-11-03 production regime (D-802): the
/// configuration under which all ten kink-stylesheet keepers
/// were generated, and the regime the Stage 3 parity runs
/// target. Vivienne calibration cell: seed 20261301, IPA
/// weight 0.35 against a head-and-shoulders square portrait
/// crop, union openpose at strength 0.5. Named because it is
/// measured, not because it is universal: the IPA sweep found
/// identity surviving to 0.15 for all ten heroines and 0.20
/// the uniform safe point — other profiles are other data.
pub const D802_PROFILE: CanonProfile = CanonProfile {
    reference_weight: 0.35,
    control_strength: 0.5,
};

/// Compiles a canon record into a `Conditioning` record under
/// a profile. Source ids are the fixed `identity-ref` and
/// `pose`; the crop is the cured head-and-shoulders framing;
/// the normalisation is the per-reference step under the
/// regime's scaling mode.
pub fn compile_canon(record: &CanonRecord, profile: CanonProfile) -> GencondResult<Conditioning> {
    for (what, text) in [
        ("identity", &record.identity),
        ("wardrobe", &record.wardrobe),
        ("expression", &record.expression),
    ] {
        if text.is_empty() {
            return refuse(CODE_MALFORMED, format!("canon {what} block is empty"));
        }
        refuse_hex_colours(text, what)?;
    }
    if let Some(scene) = &record.scene {
        refuse_hex_colours(scene, "scene")?;
    }
    let mut positive = vec![
        SlotText {
            slot: TextSlot::Identity,
            text: record.identity.clone(),
        },
        SlotText {
            slot: TextSlot::Wardrobe,
            text: record.wardrobe.clone(),
        },
        SlotText {
            slot: TextSlot::Expression,
            text: record.expression.clone(),
        },
    ];
    if let Some(scene) = &record.scene {
        positive.push(SlotText {
            slot: TextSlot::Scene,
            text: scene.clone(),
        });
    }
    let mut sources = vec![Source {
        id: "identity-ref".to_string(),
        kind: SourceKind::ReferenceEmbed(ReferenceEmbed {
            reference: ImageRef {
                path: record.portrait_path.clone(),
                sha256: record.portrait_sha256.clone(),
                crop: CropPolicy::HeadShouldersSquare,
            },
            adapter: record.adapter.clone(),
            vision_encoder: record.vision_encoder.clone(),
            weight: profile.reference_weight,
            weight_type: WeightType::Linear,
            combine: CombineEmbeds::Concat,
            window: None,
            normalisation: Normalisation::PerReference {
                scaling: EmbedScaling::KeyValueCPenalty,
            },
        }),
        enabled: true,
    }];
    if let Some(pose) = &record.pose {
        sources.push(Source {
            id: "pose".to_string(),
            kind: SourceKind::SpatialMap(SpatialMap {
                model: pose.control_model.clone(),
                control_type: pose.control_type,
                map: PoseMapRef {
                    origin: PoseOrigin::RigProjection {
                        rig: pose.rig.clone(),
                        pose: pose.pose.clone(),
                        camera: pose.camera.clone(),
                    },
                    sha256: pose.map_sha256.clone(),
                },
                strength: profile.control_strength,
                window: None,
            }),
            enabled: true,
        });
    }
    let conditioning = Conditioning {
        schema: SCHEMA_VERSION,
        text: TextPair {
            positive,
            negative: record.negative.clone(),
        },
        lora_stack: record.loras.clone(),
        sources,
    };
    conditioning.validate()?;
    Ok(conditioning)
}

/// The palette rule (DESIGN §1.5): `#` hex colour codes in a
/// text block refuse — colour identity rides the reference.
fn refuse_hex_colours(text: &str, what: &str) -> GencondResult<()> {
    let bytes = text.as_bytes();
    let mut index = 0;
    while index + 6 < bytes.len() {
        if bytes[index] == b'#'
            && bytes[index + 1..index + 7]
                .iter()
                .all(|b| b.is_ascii_hexdigit())
        {
            return refuse(
                CODE_MALFORMED,
                format!(
                    "canon {what} block carries a hex colour code; palette rides the \
                     identity reference, never text"
                ),
            );
        }
        index += 1;
    }
    Ok(())
}
