//! Typed record <-> RON tree conversion.
//!
//! The tree shapes are the record types' own names (the
//! author's codec rule): `Conditioning(...)`, `Slot(...)`,
//! `Lora(...)`, `Source(...)`, `ReferenceEmbed(...)`,
//! `SpatialMap(...)`, `ImageRef(...)`, `PoseMapRef(...)`,
//! `Window(...)`, `PerReference(...)`, `RigProjection(...)`,
//! `File(...)`, `BlockText(...)`. The DESIGN §1.2 sketch's
//! anonymous tuples take these names because the estate's tree
//! has no anonymous-struct shape.
//!
//! `window` is the only omissible field (DESIGN §1.3): absent
//! in the tree, `None` in the type. Everything else absent is
//! a refusal — most pointedly `crop`: a reference without a
//! recorded crop does not parse (DESIGN §1.4).

use crate::error::GencondResult;
use crate::ron_tree::{Ron, open, record};
use crate::schema::{
    CombineEmbeds, Conditioning, ControlType, CropPolicy, EmbedScaling, ImageRef, LoraEntry,
    Normalisation, PoseMapRef, PoseOrigin, ReferenceEmbed, SlotText, Source, SourceKind,
    SpatialMap, TextPair, TextSlot, WeightType, Window,
};

impl Conditioning {
    /// The record as a RON tree (emission order = schema
    /// order, alphabetical within each record).
    pub fn to_ron_tree(&self) -> Ron {
        record(
            "Conditioning",
            vec![
                (
                    "lora_stack",
                    Ron::List(self.lora_stack.iter().map(LoraEntry::to_ron_tree).collect()),
                ),
                ("schema", Ron::Int(i64::from(self.schema))),
                (
                    "sources",
                    Ron::List(self.sources.iter().map(Source::to_ron_tree).collect()),
                ),
                ("text", self.text.to_ron_tree()),
            ],
        )
    }

    /// Parses a record from a RON tree, then validates it.
    pub fn from_ron_tree(tree: Ron) -> GencondResult<Conditioning> {
        let mut fields = open(tree, "Conditioning")?;
        let lora_values = fields.list("lora_stack")?;
        let schema = fields.int("schema")?;
        let source_values = fields.list("sources")?;
        let text = TextPair::from_ron_tree(fields.take("text")?)?;
        fields.done()?;
        let conditioning = Conditioning {
            schema: u32::try_from(schema).map_err(|_| {
                crate::error::GencondError::new(
                    crate::error::CODE_MALFORMED,
                    "schema version is not a non-negative integer",
                )
            })?,
            text,
            lora_stack: lora_values
                .into_iter()
                .map(LoraEntry::from_ron_tree)
                .collect::<GencondResult<Vec<_>>>()?,
            sources: source_values
                .into_iter()
                .map(Source::from_ron_tree)
                .collect::<GencondResult<Vec<_>>>()?,
        };
        conditioning.validate()?;
        Ok(conditioning)
    }
}

impl TextPair {
    fn to_ron_tree(&self) -> Ron {
        record(
            "TextPair",
            vec![
                (
                    "negative",
                    record("BlockText", vec![("text", Ron::Str(self.negative.clone()))]),
                ),
                (
                    "positive",
                    Ron::List(self.positive.iter().map(SlotText::to_ron_tree).collect()),
                ),
            ],
        )
    }

    fn from_ron_tree(tree: Ron) -> GencondResult<TextPair> {
        let mut fields = open(tree, "TextPair")?;
        let negative_tree = fields.take("negative")?;
        let positive_values = fields.list("positive")?;
        fields.done()?;
        let mut negative_fields = open(negative_tree, "BlockText")?;
        let negative = negative_fields.string("text")?;
        negative_fields.done()?;
        Ok(TextPair {
            positive: positive_values
                .into_iter()
                .map(SlotText::from_ron_tree)
                .collect::<GencondResult<Vec<_>>>()?,
            negative,
        })
    }
}

impl SlotText {
    fn to_ron_tree(&self) -> Ron {
        record(
            "Slot",
            vec![
                ("slot", Ron::Ident(self.slot.ident().to_string())),
                ("text", Ron::Str(self.text.clone())),
            ],
        )
    }

    fn from_ron_tree(tree: Ron) -> GencondResult<SlotText> {
        let mut fields = open(tree, "Slot")?;
        let slot = TextSlot::from_ident(&fields.ident("slot")?)?;
        let text = fields.string("text")?;
        fields.done()?;
        Ok(SlotText { slot, text })
    }
}

impl LoraEntry {
    fn to_ron_tree(&self) -> Ron {
        record(
            "Lora",
            vec![
                ("model", Ron::Str(self.model.clone())),
                ("strength_clip", Ron::Float(self.strength_clip)),
                ("strength_model", Ron::Float(self.strength_model)),
            ],
        )
    }

    fn from_ron_tree(tree: Ron) -> GencondResult<LoraEntry> {
        let mut fields = open(tree, "Lora")?;
        let model = fields.string("model")?;
        let strength_clip = fields.float("strength_clip")?;
        let strength_model = fields.float("strength_model")?;
        fields.done()?;
        Ok(LoraEntry {
            model,
            strength_model,
            strength_clip,
        })
    }
}

impl Source {
    fn to_ron_tree(&self) -> Ron {
        let kind = match &self.kind {
            SourceKind::ReferenceEmbed(embed) => embed.to_ron_tree(),
            SourceKind::SpatialMap(map) => map.to_ron_tree(),
        };
        record(
            "Source",
            vec![
                ("id", Ron::Str(self.id.clone())),
                ("enabled", Ron::Bool(self.enabled)),
                ("kind", kind),
            ],
        )
    }

    fn from_ron_tree(tree: Ron) -> GencondResult<Source> {
        let mut fields = open(tree, "Source")?;
        let enabled = fields.boolean("enabled")?;
        let id = fields.string("id")?;
        let kind_tree = fields.take("kind")?;
        fields.done()?;
        let kind = match &kind_tree {
            Ron::Struct(name, _) if name == "ReferenceEmbed" => {
                SourceKind::ReferenceEmbed(ReferenceEmbed::from_ron_tree(kind_tree)?)
            }
            Ron::Struct(name, _) if name == "SpatialMap" => {
                SourceKind::SpatialMap(SpatialMap::from_ron_tree(kind_tree)?)
            }
            Ron::Struct(name, _) => {
                return crate::error::refuse(
                    crate::error::CODE_UNKNOWN_KIND,
                    format!("source kind '{name}' is not known"),
                );
            }
            _ => {
                return crate::error::refuse(
                    crate::error::CODE_MALFORMED,
                    "source kind is not a record",
                );
            }
        };
        Ok(Source { id, kind, enabled })
    }
}

impl ReferenceEmbed {
    fn to_ron_tree(&self) -> Ron {
        let mut fields = vec![
            ("adapter", Ron::Str(self.adapter.clone())),
            ("combine", Ron::Ident(self.combine.ident().to_string())),
            (
                "normalisation",
                record(
                    "PerReference",
                    vec![(
                        "scaling",
                        Ron::Ident(self.normalisation.scaling().ident().to_string()),
                    )],
                ),
            ),
            ("reference", self.reference.to_ron_tree()),
            ("vision_encoder", Ron::Str(self.vision_encoder.clone())),
            ("weight", Ron::Float(self.weight)),
            (
                "weight_type",
                Ron::Ident(self.weight_type.ident().to_string()),
            ),
        ];
        if let Some(window) = self.window {
            fields.push(("window", window.to_ron_tree()));
        }
        record("ReferenceEmbed", fields)
    }

    fn from_ron_tree(tree: Ron) -> GencondResult<ReferenceEmbed> {
        let mut fields = open(tree, "ReferenceEmbed")?;
        let adapter = fields.string("adapter")?;
        let combine = CombineEmbeds::from_ident(&fields.ident("combine")?)?;
        let normalisation_tree = fields.take("normalisation")?;
        let reference = ImageRef::from_ron_tree(fields.take("reference")?)?;
        let vision_encoder = fields.string("vision_encoder")?;
        let weight = fields.float("weight")?;
        let weight_type = WeightType::from_ident(&fields.ident("weight_type")?)?;
        let window = match fields.try_take("window") {
            Some(tree) => Some(Window::from_ron_tree(tree)?),
            None => None,
        };
        fields.done()?;
        let mut normalisation_fields = open(normalisation_tree, "PerReference")?;
        let scaling = EmbedScaling::from_ident(&normalisation_fields.ident("scaling")?)?;
        normalisation_fields.done()?;
        Ok(ReferenceEmbed {
            reference,
            adapter,
            vision_encoder,
            weight,
            weight_type,
            combine,
            window,
            normalisation: Normalisation::PerReference { scaling },
        })
    }
}

impl ImageRef {
    fn to_ron_tree(&self) -> Ron {
        record(
            "ImageRef",
            vec![
                ("crop", Ron::Ident(self.crop.ident().to_string())),
                ("path", Ron::Str(self.path.clone())),
                ("sha256", Ron::Str(self.sha256.clone())),
            ],
        )
    }

    fn from_ron_tree(tree: Ron) -> GencondResult<ImageRef> {
        let mut fields = open(tree, "ImageRef")?;
        // Crop is mandatory: `take` refuses when it is absent
        // (DESIGN §1.4 — the framing is recorded, never left
        // implicit).
        let crop = CropPolicy::from_ident(&fields.ident("crop")?)?;
        let path = fields.string("path")?;
        let sha256 = fields.string("sha256")?;
        fields.done()?;
        Ok(ImageRef { path, sha256, crop })
    }
}

impl SpatialMap {
    fn to_ron_tree(&self) -> Ron {
        let mut fields = vec![
            (
                "control_type",
                Ron::Ident(self.control_type.ident().to_string()),
            ),
            ("map", self.map.to_ron_tree()),
            ("model", Ron::Str(self.model.clone())),
            ("strength", Ron::Float(self.strength)),
        ];
        if let Some(window) = self.window {
            fields.push(("window", window.to_ron_tree()));
        }
        record("SpatialMap", fields)
    }

    fn from_ron_tree(tree: Ron) -> GencondResult<SpatialMap> {
        let mut fields = open(tree, "SpatialMap")?;
        let control_type = ControlType::from_ident(&fields.ident("control_type")?)?;
        let map = PoseMapRef::from_ron_tree(fields.take("map")?)?;
        let model = fields.string("model")?;
        let strength = fields.float("strength")?;
        let window = match fields.try_take("window") {
            Some(tree) => Some(Window::from_ron_tree(tree)?),
            None => None,
        };
        fields.done()?;
        Ok(SpatialMap {
            model,
            control_type,
            map,
            strength,
            window,
        })
    }
}

impl PoseMapRef {
    fn to_ron_tree(&self) -> Ron {
        record(
            "PoseMapRef",
            vec![
                ("origin", self.origin.to_ron_tree()),
                ("sha256", Ron::Str(self.sha256.clone())),
            ],
        )
    }

    fn from_ron_tree(tree: Ron) -> GencondResult<PoseMapRef> {
        let mut fields = open(tree, "PoseMapRef")?;
        let origin = PoseOrigin::from_ron_tree(fields.take("origin")?)?;
        let sha256 = fields.string("sha256")?;
        fields.done()?;
        Ok(PoseMapRef { origin, sha256 })
    }
}

impl PoseOrigin {
    fn to_ron_tree(&self) -> Ron {
        match self {
            PoseOrigin::RigProjection { rig, pose, camera } => record(
                "RigProjection",
                vec![
                    ("camera", Ron::Str(camera.clone())),
                    ("pose", Ron::Str(pose.clone())),
                    ("rig", Ron::Str(rig.clone())),
                ],
            ),
            PoseOrigin::File { path } => record("File", vec![("path", Ron::Str(path.clone()))]),
        }
    }

    fn from_ron_tree(tree: Ron) -> GencondResult<PoseOrigin> {
        match &tree {
            Ron::Struct(name, _) if name == "RigProjection" => {
                let mut fields = open(tree, "RigProjection")?;
                let camera = fields.string("camera")?;
                let pose = fields.string("pose")?;
                let rig = fields.string("rig")?;
                fields.done()?;
                Ok(PoseOrigin::RigProjection { rig, pose, camera })
            }
            Ron::Struct(name, _) if name == "File" => {
                let mut fields = open(tree, "File")?;
                let path = fields.string("path")?;
                fields.done()?;
                Ok(PoseOrigin::File { path })
            }
            Ron::Struct(name, _) => crate::error::refuse(
                crate::error::CODE_UNKNOWN_KIND,
                format!("pose-map origin '{name}' is not known"),
            ),
            _ => crate::error::refuse(
                crate::error::CODE_MALFORMED,
                "pose-map origin is not a record",
            ),
        }
    }
}

impl Window {
    fn to_ron_tree(self) -> Ron {
        record(
            "Window",
            vec![
                ("end", Ron::Float(self.end)),
                ("start", Ron::Float(self.start)),
            ],
        )
    }

    fn from_ron_tree(tree: Ron) -> GencondResult<Window> {
        let mut fields = open(tree, "Window")?;
        let end = fields.float("end")?;
        let start = fields.float("start")?;
        fields.done()?;
        Ok(Window { start, end })
    }
}
