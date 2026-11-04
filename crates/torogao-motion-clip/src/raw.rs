//! Raw document types: the serde view of a clip document
//! before semantic validation.
//!
//! Every struct is field-strict (`deny_unknown_fields`):
//! SPEC.md section 1.1 makes an unknown field `TMC-104`
//! everywhere, and section 5.4 relies on that strictness —
//! there is no field in which a layer could hide a reference
//! to base data. Serde's derived impls also reject duplicate
//! object keys (`TMC-103`) and wrong JSON types (`TMC-103`)
//! during this parse, which is validation step 3 of SPEC.md
//! section 9.
//!
//! Tokens (group kinds, interpolation, source classes) stay
//! strings here: their checks belong to later validation
//! steps with their own error codes, so the raw parse must
//! not fail on them early. Base keys and delta keys are
//! DISTINCT raw types (section 5.4, mechanism 3) even though
//! their JSON silhouettes match.
//!
//! Fields whose *presence* carries meaning use
//! `Option<Option<T>>`: absent → `None`, JSON null →
//! `Some(None)`, a value → `Some(Some(_))`. That is how a
//! `tracks` field on a declaration-only group is detectable
//! (`TMC-108`) instead of being silently read as absent.

use serde::Deserialize;

/// A whole clip document, as parsed.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawClip {
    /// The format token (already probed; parsed again here).
    pub format: String,
    /// The clip's id.
    pub id: String,
    /// The skeleton binding.
    pub skeleton: RawSkeleton,
    /// Frame count.
    pub frames: u32,
    /// Authored-to-loop flag; absent means false.
    #[serde(rename = "loop")]
    pub loop_: Option<bool>,
    /// Channel groups.
    pub groups: Vec<RawGroup>,
    /// The additive layer stack; absent means an empty stack.
    pub layers: Option<Option<Vec<RawLayer>>>,
    /// The provenance block.
    pub provenance: RawProvenance,
    /// Optional self-hash.
    pub sha256: Option<String>,
}

/// The skeleton binding (SPEC.md section 3.2).
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawSkeleton {
    /// The SkeletonDef token.
    pub def: String,
    /// The hash of the exact def document.
    pub sha256: String,
}

/// One channel group, as a union of the per-kind shapes;
/// per-kind field sets are enforced in validation.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawGroup {
    /// Group kind token.
    pub kind: String,
    /// Camera-only `mode`; presence is significant.
    pub mode: Option<Option<String>>,
    /// Tracks (body/face only); presence is significant.
    pub tracks: Option<Option<Vec<RawTrack>>>,
}

/// A base track: a bone track (bone + component lists) or a
/// scalar channel track (channel + keys), as a union; the
/// exact shape is enforced in validation.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawTrack {
    /// Bone name (bone tracks only).
    pub bone: Option<String>,
    /// Channel id (channel tracks only).
    pub channel: Option<String>,
    /// Rotation quat keys (bone tracks only).
    pub rotation: Option<Vec<RawQuatKey>>,
    /// Scale vector keys (bone tracks only).
    pub scale: Option<Vec<RawVecKey>>,
    /// Translation vector keys (bone tracks only).
    pub translation: Option<Vec<RawVecKey>>,
    /// Scalar keys (channel tracks only).
    pub keys: Option<Vec<RawScalarKey>>,
}

/// A base scalar key (SPEC.md section 4.1).
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawScalarKey {
    /// Key frame.
    pub frame: u32,
    /// Scalar value.
    pub value: f64,
    /// Segment interpolation token; absent means linear.
    pub interpolation: Option<String>,
}

/// A base vector key.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawVecKey {
    /// Key frame.
    pub frame: u32,
    /// Vector value.
    pub value: [f64; 3],
    /// Segment interpolation token; absent means linear.
    pub interpolation: Option<String>,
}

/// A base quaternion key.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawQuatKey {
    /// Key frame.
    pub frame: u32,
    /// Quaternion value, `[x, y, z, w]`.
    pub value: [f64; 4],
    /// Segment interpolation token; absent means linear.
    pub interpolation: Option<String>,
}

/// One additive layer (SPEC.md section 5.1).
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawLayer {
    /// Layer id.
    pub id: String,
    /// Optional free-text name.
    pub name: Option<String>,
    /// Layer weight; absent means 1.
    pub weight: Option<f64>,
    /// Declared scope; absent means an empty scope.
    pub scope: Option<RawScope>,
    /// Layer tracks (delta keys); may be empty.
    pub tracks: Vec<RawLayerTrack>,
}

/// A layer's declared scope (SPEC.md section 5.1).
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawScope {
    /// Scoped bone names; absent means empty.
    pub bones: Option<Vec<String>>,
    /// Scoped channel ids; absent means empty.
    pub channels: Option<Vec<String>>,
}

/// A layer track: same outer shape as a base track, but its
/// keys are delta keys, a different type with a different
/// meaning (SPEC.md section 5.1).
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawLayerTrack {
    /// Bone name (bone tracks only).
    pub bone: Option<String>,
    /// Channel id (channel tracks only).
    pub channel: Option<String>,
    /// Rotation delta keys (bone tracks only).
    pub rotation: Option<Vec<RawDeltaQuatKey>>,
    /// Scale delta keys (bone tracks only).
    pub scale: Option<Vec<RawDeltaVecKey>>,
    /// Translation delta keys (bone tracks only).
    pub translation: Option<Vec<RawDeltaVecKey>>,
    /// Scalar delta keys (channel tracks only).
    pub keys: Option<Vec<RawDeltaScalarKey>>,
}

/// A scalar delta key: an offset from the composed-so-far
/// value (SPEC.md section 5.2). Distinct from
/// [`RawScalarKey`] by design (section 5.4, mechanism 3).
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawDeltaScalarKey {
    /// Key frame.
    pub frame: u32,
    /// Scalar delta.
    pub value: f64,
    /// Segment interpolation token; absent means linear.
    pub interpolation: Option<String>,
}

/// A vector delta key (translation offset or scale factor).
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawDeltaVecKey {
    /// Key frame.
    pub frame: u32,
    /// Vector delta.
    pub value: [f64; 3],
    /// Segment interpolation token; absent means linear.
    pub interpolation: Option<String>,
}

/// A quaternion delta key: a rotation relative to the
/// composed-so-far orientation.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawDeltaQuatKey {
    /// Key frame.
    pub frame: u32,
    /// Quaternion delta, `[x, y, z, w]`.
    pub value: [f64; 4],
    /// Segment interpolation token; absent means linear.
    pub interpolation: Option<String>,
}

/// The provenance block (SPEC.md section 6).
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawProvenance {
    /// The provenance format token.
    pub format: String,
    /// One entry per channel group.
    pub groups: Vec<RawGroupProvenance>,
}

/// One group's provenance entry.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawGroupProvenance {
    /// The group kind this entry describes.
    pub group: String,
    /// Source class token.
    pub source_class: String,
    /// Converter id, or `"authored"`.
    pub converter: String,
    /// Converter build version, when it differs.
    pub converter_version: Option<String>,
    /// Producer model weights hash, when a model was involved.
    pub model_sha256: Option<String>,
    /// Source artifact hash.
    pub source_sha256: Option<String>,
    /// Source content region, in source pixels.
    pub region: Option<RawRegion>,
    /// Measurement record id.
    pub measurement_ref: Option<String>,
    /// Frame map, when the source ran off 60 Hz.
    pub frame_map: Option<RawFrameMap>,
}

/// A source content region (SPEC.md section 6).
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawRegion {
    /// Left edge, in source pixels.
    pub x: u32,
    /// Top edge, in source pixels.
    pub y: u32,
    /// Width, in source pixels.
    pub w: u32,
    /// Height, in source pixels.
    pub h: u32,
}

/// A frame map (SPEC.md section 6.2).
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawFrameMap {
    /// Number of frames in the source.
    pub source_frame_count: u32,
    /// The source rate: a rational, or `"variable"`.
    pub source_rate: RawFrameRate,
    /// One entry per clip frame: the source frame whose
    /// sampled value produced that take frame.
    pub entries: Vec<u32>,
}

/// A frame map's source rate: never a float (SPEC.md §6.2).
#[derive(Debug, Clone, PartialEq)]
pub enum RawFrameRate {
    /// A rational rate, `num / den` frames per second.
    Rational {
        /// Numerator.
        num: u32,
        /// Denominator; must be nonzero (checked in
        /// validation, `TMC-702`).
        den: u32,
    },
    /// A variable-rate source; the map itself is the truth.
    Variable,
}

impl<'de> serde::Deserialize<'de> for RawFrameRate {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct RateVisitor;

        impl<'de> serde::de::Visitor<'de> for RateVisitor {
            type Value = RawFrameRate;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("a source_rate rational object or the string \"variable\"")
            }

            fn visit_str<E>(self, text: &str) -> Result<RawFrameRate, E>
            where
                E: serde::de::Error,
            {
                if text == "variable" {
                    Ok(RawFrameRate::Variable)
                } else {
                    Err(E::custom("source_rate string must be \"variable\""))
                }
            }

            fn visit_map<A>(self, map: A) -> Result<RawFrameRate, A::Error>
            where
                A: serde::de::MapAccess<'de>,
            {
                #[derive(Deserialize)]
                #[serde(deny_unknown_fields)]
                struct Rational {
                    num: u32,
                    den: u32,
                }
                let rational =
                    Rational::deserialize(serde::de::value::MapAccessDeserializer::new(map))?;
                Ok(RawFrameRate::Rational {
                    num: rational.num,
                    den: rational.den,
                })
            }
        }

        deserializer.deserialize_any(RateVisitor)
    }
}
