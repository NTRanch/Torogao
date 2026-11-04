//! The `Conditioning` record: the authoring form of the
//! conditioning algebra (DESIGN §1, S-4).
//!
//! Every numeric in this record is data. Nothing has a
//! compiled-in default: an absent field is a validation refusal,
//! not a silent fill — the one exception is `window` (DESIGN
//! §1.3), whose omission means the full trajectory and is
//! restated explicitly by the compiler.
//!
//! `enabled` is the only off switch in the schema. There is no
//! `weight: 0.0` idiom and no blank-map value: `PoseMapRef`'s
//! origin has no `Blank` variant, deliberately — the type
//! system refuses the false off, not just the style guide
//! (DESIGN §1.2, §2.1).

use crate::error::GencondResult;
use crate::error::{CODE_CONFLICT, CODE_MALFORMED, CODE_RANGE, CODE_UNSUPPORTED_SCHEMA, refuse};
use crate::limits::{
    ASSET_PATH_BYTES_MAX, LORA_ENTRIES_MAX, MODEL_NAME_BYTES_MAX, SOURCE_ID_BYTES_MAX, SOURCES_MAX,
    TEXT_BLOCK_BYTES_MAX, TEXT_BLOCKS_MAX, WEIGHT_MAX,
};
use serde::{Deserialize, Serialize};

/// The schema version this build reads and writes.
pub const SCHEMA_VERSION: u32 = 1;

/// The conditioning section of a generation record.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Conditioning {
    /// Schema version; must equal [`SCHEMA_VERSION`].
    pub schema: u32,
    /// The mandatory text operand pair (S-4 §1.1).
    pub text: TextPair,
    /// Ordered LoRA stack; order is significant (S-4 §1.3).
    pub lora_stack: Vec<LoraEntry>,
    /// Conditioning sources, applied in record order.
    pub sources: Vec<Source>,
}

/// The (positive, negative) text pair. Both operands are
/// mandatory (S-4 §1.1: the negative is a first-class operand
/// with full leverage; there is no "no negative" value).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TextPair {
    /// Slot-composed positive blocks (replacement, never
    /// stacking, S-4 §1.2).
    pub positive: Vec<SlotText>,
    /// The negative block, verbatim.
    pub negative: String,
}

/// One positive text block in a named slot.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SlotText {
    /// The slot this block fills.
    pub slot: TextSlot,
    /// The block's whole text; changing a slot swaps it
    /// atomically.
    pub text: String,
}

/// The positive-text slots, in composition order.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextSlot {
    /// Who the subject is (canon: CHARACTERS.md line).
    Identity,
    /// Wardrobe and hair arrangement.
    Wardrobe,
    /// Expression register (D-650).
    Expression,
    /// Scene and composition.
    Scene,
}

impl TextSlot {
    /// The tree-form identifier.
    pub fn ident(self) -> &'static str {
        match self {
            TextSlot::Identity => "Identity",
            TextSlot::Wardrobe => "Wardrobe",
            TextSlot::Expression => "Expression",
            TextSlot::Scene => "Scene",
        }
    }

    /// Parses a tree-form identifier.
    pub fn from_ident(name: &str) -> GencondResult<TextSlot> {
        match name {
            "Identity" => Ok(TextSlot::Identity),
            "Wardrobe" => Ok(TextSlot::Wardrobe),
            "Expression" => Ok(TextSlot::Expression),
            "Scene" => Ok(TextSlot::Scene),
            other => refuse(
                crate::error::CODE_UNKNOWN_KIND,
                format!("text slot '{other}' is not known"),
            ),
        }
    }
}

/// One LoRA stack entry: an S-7 model name and its two
/// independent strengths.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LoraEntry {
    /// S-7 model enumeration name.
    pub model: String,
    /// Model-patch strength.
    pub strength_model: f64,
    /// CLIP-patch strength.
    pub strength_clip: f64,
}

/// One conditioning source. `enabled: false` removes the source
/// from the compiled graph entirely (structural off, DESIGN
/// §2.1); the compile report lists it as omitted.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    /// Source id (`[a-z0-9-]`, unique in the record).
    pub id: String,
    /// What the source conditions with.
    pub kind: SourceKind,
    /// The only off switch.
    pub enabled: bool,
}

/// A source's conditioning mechanism.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    /// IP-Adapter-style reference embedding (S-4 Annex A).
    ReferenceEmbed(ReferenceEmbed),
    /// ControlNet-style spatial map (S-4 §1.4).
    SpatialMap(SpatialMap),
}

impl SourceKind {
    /// The kind's stable name, for reports.
    pub fn kind_name(&self) -> &'static str {
        match self {
            SourceKind::ReferenceEmbed(_) => "reference_embed",
            SourceKind::SpatialMap(_) => "spatial_map",
        }
    }
}

/// An image reference: path, content hash, and the declared
/// crop. The crop is mandatory and recorded — the framing the
/// vision encoder sees is conditioning design, never left to
/// the encoder's implicit centre crop (DESIGN §1.4, D-802).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImageRef {
    /// Project-relative path of the reference image.
    pub path: String,
    /// SHA-256 of the image bytes (lowercase hex).
    pub sha256: String,
    /// The declared framing.
    pub crop: CropPolicy,
}

/// A declared reference framing.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CropPolicy {
    /// The whole frame, uncropped, named explicitly.
    FullFrame,
    /// The centre square (what an encoder would do implicitly,
    /// recorded so it is a choice, not an accident).
    CentreSquare,
    /// The head-and-shoulders square from the top of the frame
    /// (the D-802 cured framing for canon portraits).
    HeadShouldersSquare,
}

impl CropPolicy {
    /// The tree-form identifier.
    pub fn ident(self) -> &'static str {
        match self {
            CropPolicy::FullFrame => "FullFrame",
            CropPolicy::CentreSquare => "CentreSquare",
            CropPolicy::HeadShouldersSquare => "HeadShouldersSquare",
        }
    }

    /// Parses a tree-form identifier.
    pub fn from_ident(name: &str) -> GencondResult<CropPolicy> {
        match name {
            "FullFrame" => Ok(CropPolicy::FullFrame),
            "CentreSquare" => Ok(CropPolicy::CentreSquare),
            "HeadShouldersSquare" => Ok(CropPolicy::HeadShouldersSquare),
            other => refuse(
                crate::error::CODE_UNKNOWN_KIND,
                format!("crop policy '{other}' is not known"),
            ),
        }
    }
}

/// IP-Adapter-style reference-embed conditioning. The field set
/// is the observed oracle contract (S-4 Annex A §3): image,
/// adapter, vision encoder, weight, weight type, combine mode,
/// embeds scaling, and the start/end window.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReferenceEmbed {
    /// The reference image.
    pub reference: ImageRef,
    /// S-7 name of the adapter model.
    pub adapter: String,
    /// S-7 name of the vision encoder.
    pub vision_encoder: String,
    /// The branch weight (a true scalar; 0.0 is a measured
    /// zero-point with nodes present, not an off — DESIGN §2.1).
    pub weight: f64,
    /// How the weight is shaped across the trajectory.
    pub weight_type: WeightType,
    /// How multiple embeds combine.
    pub combine: CombineEmbeds,
    /// The trajectory window; omitted means the full window
    /// (DESIGN §1.3).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window: Option<Window>,
    /// Embed normalisation: a named compile step with its
    /// measured statistics recorded (DESIGN §2.2).
    pub normalisation: Normalisation,
}

/// The IP-Adapter weight-shaping modes, under the oracle
/// contract's own names (the gen-graph input strings).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum WeightType {
    /// `linear`
    #[serde(rename = "linear")]
    Linear,
    /// `ease in`
    #[serde(rename = "ease in")]
    EaseIn,
    /// `ease out`
    #[serde(rename = "ease out")]
    EaseOut,
    /// `ease in-out`
    #[serde(rename = "ease in-out")]
    EaseInOut,
    /// `reverse in-out`
    #[serde(rename = "reverse in-out")]
    ReverseInOut,
    /// `weak input`
    #[serde(rename = "weak input")]
    WeakInput,
    /// `weak output`
    #[serde(rename = "weak output")]
    WeakOutput,
    /// `weak middle`
    #[serde(rename = "weak middle")]
    WeakMiddle,
    /// `strong middle`
    #[serde(rename = "strong middle")]
    StrongMiddle,
    /// `style transfer`
    #[serde(rename = "style transfer")]
    StyleTransfer,
    /// `composition`
    #[serde(rename = "composition")]
    Composition,
    /// `strong style transfer`
    #[serde(rename = "strong style transfer")]
    StrongStyleTransfer,
    /// `style and composition`
    #[serde(rename = "style and composition")]
    StyleComposition,
    /// `strong style and composition`
    #[serde(rename = "strong style and composition")]
    StrongStyleComposition,
}

impl WeightType {
    /// The oracle contract string (gen-graph input value).
    pub fn oracle_str(self) -> &'static str {
        match self {
            WeightType::Linear => "linear",
            WeightType::EaseIn => "ease in",
            WeightType::EaseOut => "ease out",
            WeightType::EaseInOut => "ease in-out",
            WeightType::ReverseInOut => "reverse in-out",
            WeightType::WeakInput => "weak input",
            WeightType::WeakOutput => "weak output",
            WeightType::WeakMiddle => "weak middle",
            WeightType::StrongMiddle => "strong middle",
            WeightType::StyleTransfer => "style transfer",
            WeightType::Composition => "composition",
            WeightType::StrongStyleTransfer => "strong style transfer",
            WeightType::StyleComposition => "style and composition",
            WeightType::StrongStyleComposition => "strong style and composition",
        }
    }

    /// The tree-form identifier.
    pub fn ident(self) -> &'static str {
        match self {
            WeightType::Linear => "Linear",
            WeightType::EaseIn => "EaseIn",
            WeightType::EaseOut => "EaseOut",
            WeightType::EaseInOut => "EaseInOut",
            WeightType::ReverseInOut => "ReverseInOut",
            WeightType::WeakInput => "WeakInput",
            WeightType::WeakOutput => "WeakOutput",
            WeightType::WeakMiddle => "WeakMiddle",
            WeightType::StrongMiddle => "StrongMiddle",
            WeightType::StyleTransfer => "StyleTransfer",
            WeightType::Composition => "Composition",
            WeightType::StrongStyleTransfer => "StrongStyleTransfer",
            WeightType::StyleComposition => "StyleComposition",
            WeightType::StrongStyleComposition => "StrongStyleComposition",
        }
    }

    /// Parses a tree-form identifier.
    pub fn from_ident(name: &str) -> GencondResult<WeightType> {
        match name {
            "Linear" => Ok(WeightType::Linear),
            "EaseIn" => Ok(WeightType::EaseIn),
            "EaseOut" => Ok(WeightType::EaseOut),
            "EaseInOut" => Ok(WeightType::EaseInOut),
            "ReverseInOut" => Ok(WeightType::ReverseInOut),
            "WeakInput" => Ok(WeightType::WeakInput),
            "WeakOutput" => Ok(WeightType::WeakOutput),
            "WeakMiddle" => Ok(WeightType::WeakMiddle),
            "StrongMiddle" => Ok(WeightType::StrongMiddle),
            "StyleTransfer" => Ok(WeightType::StyleTransfer),
            "Composition" => Ok(WeightType::Composition),
            "StrongStyleTransfer" => Ok(WeightType::StrongStyleTransfer),
            "StyleComposition" => Ok(WeightType::StyleComposition),
            "StrongStyleComposition" => Ok(WeightType::StrongStyleComposition),
            other => refuse(
                crate::error::CODE_UNKNOWN_KIND,
                format!("weight type '{other}' is not known"),
            ),
        }
    }
}

/// How multiple image embeds combine in the adapter branch.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum CombineEmbeds {
    /// `concat`
    #[serde(rename = "concat")]
    Concat,
    /// `add`
    #[serde(rename = "add")]
    Add,
    /// `subtract`
    #[serde(rename = "subtract")]
    Subtract,
    /// `average`
    #[serde(rename = "average")]
    Average,
    /// `norm average`
    #[serde(rename = "norm average")]
    NormAverage,
}

impl CombineEmbeds {
    /// The oracle contract string (gen-graph input value).
    pub fn oracle_str(self) -> &'static str {
        match self {
            CombineEmbeds::Concat => "concat",
            CombineEmbeds::Add => "add",
            CombineEmbeds::Subtract => "subtract",
            CombineEmbeds::Average => "average",
            CombineEmbeds::NormAverage => "norm average",
        }
    }

    /// The tree-form identifier.
    pub fn ident(self) -> &'static str {
        match self {
            CombineEmbeds::Concat => "Concat",
            CombineEmbeds::Add => "Add",
            CombineEmbeds::Subtract => "Subtract",
            CombineEmbeds::Average => "Average",
            CombineEmbeds::NormAverage => "NormAverage",
        }
    }

    /// Parses a tree-form identifier.
    pub fn from_ident(name: &str) -> GencondResult<CombineEmbeds> {
        match name {
            "Concat" => Ok(CombineEmbeds::Concat),
            "Add" => Ok(CombineEmbeds::Add),
            "Subtract" => Ok(CombineEmbeds::Subtract),
            "Average" => Ok(CombineEmbeds::Average),
            "NormAverage" => Ok(CombineEmbeds::NormAverage),
            other => refuse(
                crate::error::CODE_UNKNOWN_KIND,
                format!("combine mode '{other}' is not known"),
            ),
        }
    }
}

/// How the adapter rescales embed magnitudes (the oracle
/// contract's `embeds_scaling` domain, all four values).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum EmbedScaling {
    /// `V only`
    #[serde(rename = "V only")]
    VOnly,
    /// `K+V`
    #[serde(rename = "K+V")]
    KeyValue,
    /// `K+V w/ C penalty` — the cured production mode (D-802).
    #[serde(rename = "K+V w/ C penalty")]
    KeyValueCPenalty,
    /// `K+V w/o C penalty`
    #[serde(rename = "K+V w/o C penalty")]
    KeyValueNoCPenalty,
}

impl EmbedScaling {
    /// The oracle contract string (gen-graph input value).
    pub fn oracle_str(self) -> &'static str {
        match self {
            EmbedScaling::VOnly => "V only",
            EmbedScaling::KeyValue => "K+V",
            EmbedScaling::KeyValueCPenalty => "K+V w/ C penalty",
            EmbedScaling::KeyValueNoCPenalty => "K+V w/o C penalty",
        }
    }

    /// The tree-form identifier.
    pub fn ident(self) -> &'static str {
        match self {
            EmbedScaling::VOnly => "VOnly",
            EmbedScaling::KeyValue => "KeyValue",
            EmbedScaling::KeyValueCPenalty => "KeyValueCPenalty",
            EmbedScaling::KeyValueNoCPenalty => "KeyValueNoCPenalty",
        }
    }

    /// Parses a tree-form identifier.
    pub fn from_ident(name: &str) -> GencondResult<EmbedScaling> {
        match name {
            "VOnly" => Ok(EmbedScaling::VOnly),
            "KeyValue" => Ok(EmbedScaling::KeyValue),
            "KeyValueCPenalty" => Ok(EmbedScaling::KeyValueCPenalty),
            "KeyValueNoCPenalty" => Ok(EmbedScaling::KeyValueNoCPenalty),
            other => refuse(
                crate::error::CODE_UNKNOWN_KIND,
                format!("embeds scaling '{other}' is not known"),
            ),
        }
    }
}

/// Embed normalisation, as recorded in the record. Only the
/// per-reference form is observed; new variants arrive by
/// schema bump when measurements land (DESIGN §1.2).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Normalisation {
    /// Magnitude statistics are measured per reference and
    /// recorded at compile time (DESIGN §2.2).
    PerReference {
        /// The scaling mode the statistics are taken under.
        scaling: EmbedScaling,
    },
}

impl Normalisation {
    /// The declared scaling mode.
    pub fn scaling(self) -> EmbedScaling {
        match self {
            Normalisation::PerReference { scaling } => scaling,
        }
    }
}

/// ControlNet-style spatial-map conditioning.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpatialMap {
    /// S-7 name of the (union) control model.
    pub model: String,
    /// The union control type (closed enum, S-4 §1.4).
    pub control_type: ControlType,
    /// The control map and its provenance.
    pub map: PoseMapRef,
    /// Application strength.
    pub strength: f64,
    /// The trajectory window; omitted means the full window
    /// (DESIGN §1.3).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window: Option<Window>,
}

/// The union control types, under the oracle contract's own
/// strings (the instance's validation domain, S-4 §1.4).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum ControlType {
    /// `auto`
    #[serde(rename = "auto")]
    Auto,
    /// `openpose`
    #[serde(rename = "openpose")]
    Openpose,
    /// `depth`
    #[serde(rename = "depth")]
    Depth,
    /// `hed/pidi/scribble/ted`
    #[serde(rename = "hed/pidi/scribble/ted")]
    HedPidiScribbleTed,
    /// `canny/lineart/anime_lineart/mlsd`
    #[serde(rename = "canny/lineart/anime_lineart/mlsd")]
    CannyLineartAnimeLineartMlsd,
    /// `normal`
    #[serde(rename = "normal")]
    Normal,
    /// `segment`
    #[serde(rename = "segment")]
    Segment,
    /// `tile`
    #[serde(rename = "tile")]
    Tile,
    /// `repaint`
    #[serde(rename = "repaint")]
    Repaint,
}

impl ControlType {
    /// The oracle contract string (gen-graph input value).
    pub fn oracle_str(self) -> &'static str {
        match self {
            ControlType::Auto => "auto",
            ControlType::Openpose => "openpose",
            ControlType::Depth => "depth",
            ControlType::HedPidiScribbleTed => "hed/pidi/scribble/ted",
            ControlType::CannyLineartAnimeLineartMlsd => "canny/lineart/anime_lineart/mlsd",
            ControlType::Normal => "normal",
            ControlType::Segment => "segment",
            ControlType::Tile => "tile",
            ControlType::Repaint => "repaint",
        }
    }

    /// The tree-form identifier.
    pub fn ident(self) -> &'static str {
        match self {
            ControlType::Auto => "Auto",
            ControlType::Openpose => "Openpose",
            ControlType::Depth => "Depth",
            ControlType::HedPidiScribbleTed => "HedPidiScribbleTed",
            ControlType::CannyLineartAnimeLineartMlsd => "CannyLineartAnimeLineartMlsd",
            ControlType::Normal => "Normal",
            ControlType::Segment => "Segment",
            ControlType::Tile => "Tile",
            ControlType::Repaint => "Repaint",
        }
    }

    /// Parses a tree-form identifier.
    pub fn from_ident(name: &str) -> GencondResult<ControlType> {
        match name {
            "Auto" => Ok(ControlType::Auto),
            "Openpose" => Ok(ControlType::Openpose),
            "Depth" => Ok(ControlType::Depth),
            "HedPidiScribbleTed" => Ok(ControlType::HedPidiScribbleTed),
            "CannyLineartAnimeLineartMlsd" => Ok(ControlType::CannyLineartAnimeLineartMlsd),
            "Normal" => Ok(ControlType::Normal),
            "Segment" => Ok(ControlType::Segment),
            "Tile" => Ok(ControlType::Tile),
            "Repaint" => Ok(ControlType::Repaint),
            other => refuse(
                crate::error::CODE_UNKNOWN_KIND,
                format!("control type '{other}' is not known"),
            ),
        }
    }
}

/// A control map: where it came from and the SHA-256 of the
/// rendered map itself. There is deliberately no `Blank`
/// variant (DESIGN §1.2).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PoseMapRef {
    /// The map's provenance.
    pub origin: PoseOrigin,
    /// SHA-256 of the rendered map's bytes (lowercase hex).
    pub sha256: String,
}

/// Where a control map came from.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PoseOrigin {
    /// Projected from a D-654 rig's evaluated joint positions
    /// through a pose camera into COCO-18 (the pose-map source
    /// of record for rig work, DESIGN §1.5).
    RigProjection {
        /// Rig reference (S-7-style name).
        rig: String,
        /// Pose id in the rig's pose set.
        pose: String,
        /// Camera id the joints were projected through.
        camera: String,
    },
    /// A map file carried outside the rig pipeline (its bytes
    /// still hash-checked at compile time).
    File {
        /// Project-relative path of the map image.
        path: String,
    },
}

/// A trajectory window: fractions of the sampling trajectory
/// the source applies over.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Window {
    /// Start fraction, in [0, 1].
    pub start: f64,
    /// End fraction, in [0, 1], not before `start`.
    pub end: f64,
}

impl Window {
    /// The full trajectory (the DESIGN §1.3 default an omitted
    /// window restates to at compile time).
    pub const FULL: Window = Window {
        start: 0.0,
        end: 1.0,
    };
}

impl Conditioning {
    /// Validates the record against the schema's rules: bounds,
    /// id syntax, slot uniqueness, hash shapes, finite and
    /// in-range scalars. Asset-level checks (hash equality,
    /// blank maps, measurements) belong to the compiler, which
    /// has the bytes.
    pub fn validate(&self) -> GencondResult<()> {
        if self.schema != SCHEMA_VERSION {
            return refuse(
                CODE_UNSUPPORTED_SCHEMA,
                format!(
                    "conditioning schema {} is not supported (this build reads {SCHEMA_VERSION})",
                    self.schema
                ),
            );
        }
        if self.text.positive.len() > TEXT_BLOCKS_MAX {
            return refuse(CODE_RANGE, "too many positive text blocks");
        }
        let mut slots: Vec<TextSlot> = Vec::new();
        for block in &self.text.positive {
            check_text(&block.text, "text block")?;
            if slots.contains(&block.slot) {
                return refuse(
                    CODE_CONFLICT,
                    format!(
                        "text slot {} is defined twice; blocks compose by replacement, \
                         never stacking",
                        block.slot.ident()
                    ),
                );
            }
            slots.push(block.slot);
        }
        check_text(&self.text.negative, "negative block")?;
        if self.lora_stack.len() > LORA_ENTRIES_MAX {
            return refuse(CODE_RANGE, "too many LoRA entries");
        }
        for entry in &self.lora_stack {
            check_model_name(&entry.model)?;
            check_weight(entry.strength_model, "strength_model")?;
            check_weight(entry.strength_clip, "strength_clip")?;
        }
        if self.sources.len() > SOURCES_MAX {
            return refuse(CODE_RANGE, "too many conditioning sources");
        }
        let mut ids: Vec<&str> = Vec::new();
        for source in &self.sources {
            check_source_id(&source.id)?;
            if ids.contains(&source.id.as_str()) {
                return refuse(
                    CODE_CONFLICT,
                    format!("source id '{}' is defined twice", source.id),
                );
            }
            ids.push(source.id.as_str());
            match &source.kind {
                SourceKind::ReferenceEmbed(embed) => validate_embed(embed)?,
                SourceKind::SpatialMap(map) => validate_map(map)?,
            }
        }
        Ok(())
    }

    /// The positive text composed in slot order (Identity,
    /// Wardrobe, Expression, Scene), blocks joined by `, ` —
    /// the estate's observed prompt form. Deterministic: slot
    /// order, not record order, decides.
    pub fn composed_positive(&self) -> String {
        let mut blocks: Vec<&SlotText> = self.text.positive.iter().collect();
        blocks.sort_by_key(|block| block.slot);
        blocks
            .iter()
            .map(|block| block.text.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    }
}

fn validate_embed(embed: &ReferenceEmbed) -> GencondResult<()> {
    check_asset_path(&embed.reference.path)?;
    check_sha256(&embed.reference.sha256, "reference sha256")?;
    check_model_name(&embed.adapter)?;
    check_model_name(&embed.vision_encoder)?;
    check_weight(embed.weight, "weight")?;
    check_window(embed.window)?;
    Ok(())
}

fn validate_map(map: &SpatialMap) -> GencondResult<()> {
    check_model_name(&map.model)?;
    check_sha256(&map.map.sha256, "map sha256")?;
    match &map.map.origin {
        PoseOrigin::RigProjection { rig, pose, camera } => {
            check_model_name(rig)?;
            check_name_text(pose, "pose id")?;
            check_name_text(camera, "camera id")?;
        }
        PoseOrigin::File { path } => check_asset_path(path)?,
    }
    check_weight(map.strength, "strength")?;
    check_window(map.window)?;
    Ok(())
}

fn check_window(window: Option<Window>) -> GencondResult<()> {
    if let Some(window) = window {
        for (field, value) in [("window.start", window.start), ("window.end", window.end)] {
            if !value.is_finite() || !(0.0..=1.0).contains(&value) {
                return refuse(CODE_RANGE, format!("{field} must lie in [0, 1]"));
            }
        }
        if window.start > window.end {
            return refuse(CODE_RANGE, "window.start must not exceed window.end");
        }
    }
    Ok(())
}

fn check_weight(value: f64, field: &str) -> GencondResult<()> {
    if !value.is_finite() || !(0.0..=WEIGHT_MAX).contains(&value) {
        return refuse(
            CODE_RANGE,
            format!("{field} must be finite and lie in [0, {WEIGHT_MAX}]"),
        );
    }
    Ok(())
}

fn check_text(text: &str, what: &str) -> GencondResult<()> {
    if text.is_empty() {
        return refuse(CODE_MALFORMED, format!("{what} is empty"));
    }
    if text.len() > TEXT_BLOCK_BYTES_MAX {
        return refuse(CODE_RANGE, format!("{what} exceeds the byte bound"));
    }
    Ok(())
}

/// Source ids: lowercase ASCII letters, digits, and `-`,
/// starting with a letter or digit.
pub fn check_source_id(id: &str) -> GencondResult<()> {
    let valid = !id.is_empty()
        && id.len() <= SOURCE_ID_BYTES_MAX
        && id
            .bytes()
            .next()
            .is_some_and(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    if !valid {
        return refuse(
            CODE_MALFORMED,
            format!("source id '{id}' is not a valid id ([a-z0-9-], bounded length)"),
        );
    }
    Ok(())
}

/// S-7 model names: non-empty, bounded, printable without
/// control characters or path escapes.
pub fn check_model_name(name: &str) -> GencondResult<()> {
    check_name_text(name, "model name")?;
    if name.len() > MODEL_NAME_BYTES_MAX {
        return refuse(CODE_RANGE, "model name exceeds the byte bound");
    }
    Ok(())
}

fn check_name_text(text: &str, what: &str) -> GencondResult<()> {
    if text.is_empty() || text.chars().any(|ch| ch.is_control()) {
        return refuse(
            CODE_MALFORMED,
            format!("{what} is empty or carries control characters"),
        );
    }
    Ok(())
}

fn check_asset_path(path: &str) -> GencondResult<()> {
    if path.is_empty() || path.len() > ASSET_PATH_BYTES_MAX {
        return refuse(CODE_RANGE, "asset path is empty or exceeds the byte bound");
    }
    if path.starts_with('/') || path.split('/').any(|part| part == "..") {
        return refuse(
            CODE_MALFORMED,
            format!("asset path '{path}' is not a contained relative path"),
        );
    }
    Ok(())
}

/// SHA-256 fields are exactly 64 lowercase hex characters.
pub fn check_sha256(text: &str, what: &str) -> GencondResult<()> {
    let valid = text.len() == 64
        && text
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
    if !valid {
        return refuse(
            CODE_MALFORMED,
            format!("{what} is not a 64-digit lowercase hex SHA-256"),
        );
    }
    Ok(())
}
