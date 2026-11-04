//! The unchecked clip: a fully typed document with defaults
//! resolved, before semantic validation (SPEC.md section 9,
//! steps 4–10) has run.
//!
//! This type exists for two reasons. First, validation must
//! report the first failure in the spec's order, so the
//! checks run over a representation that can hold an invalid
//! document without complaint. Second, the self-hash check
//! (step 6) needs canonical emission before the document is
//! known valid, so emission is defined here, over the
//! unchecked form: a validated [`crate::model::Clip`]
//! converts back into this form losslessly and emits through
//! the same path.
//!
//! Canonical emission (SPEC.md section 1.2): struct field
//! order, 2-space indentation, one trailing newline, f64 in
//! shortest round-trip form (serde_json's Ryu output), the
//! section 1.2 collections sorted (groups by kind, tracks by
//! target, keys by frame, scope lists ascending), layers in
//! authored stack order, defaults emitted at resolved
//! values, an empty layer stack emitted as an absent field.

use serde::Serialize;

use crate::error::ClipError;
use crate::raw::{
    RawClip, RawDeltaQuatKey, RawDeltaScalarKey, RawDeltaVecKey, RawFrameMap, RawFrameRate,
    RawGroup, RawGroupProvenance, RawLayer, RawLayerTrack, RawProvenance, RawQuatKey, RawScalarKey,
    RawScope, RawSkeleton, RawTrack, RawVecKey,
};

/// A whole clip document with defaults resolved.
#[derive(Debug, Clone, Serialize)]
pub struct UClip {
    /// The format token, as authored.
    pub format: String,
    /// The clip's id.
    pub id: String,
    /// The skeleton binding.
    pub skeleton: USkeleton,
    /// Frame count.
    pub frames: u32,
    /// Authored-to-loop flag, resolved.
    #[serde(rename = "loop")]
    pub loop_: bool,
    /// Channel groups, as authored.
    pub groups: Vec<UGroup>,
    /// The layer stack, as authored (never reordered).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub layers: Vec<ULayer>,
    /// The provenance block.
    pub provenance: UProvenance,
    /// The self-hash, as authored.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
}

/// The skeleton binding.
#[derive(Debug, Clone, Serialize)]
pub struct USkeleton {
    /// The SkeletonDef token.
    pub def: String,
    /// The hash of the exact def document.
    pub sha256: String,
}

/// One channel group. `tracks` is `None` when the field was
/// absent; presence is significant for declaration-only
/// groups (SPEC.md section 3.3).
#[derive(Debug, Clone)]
pub struct UGroup {
    /// Group kind token.
    pub kind: String,
    /// Camera `mode`, when the field was present.
    pub mode: Option<String>,
    /// Tracks, when the field was present.
    pub tracks: Option<Vec<UTrack>>,
}

impl Serialize for UGroup {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeMap;
        let mut len = 1;
        if self.mode.is_some() {
            len += 1;
        }
        if self.tracks.is_some() {
            len += 1;
        }
        let mut map = serializer.serialize_map(Some(len))?;
        map.serialize_entry("kind", &self.kind)?;
        if let Some(mode) = &self.mode {
            map.serialize_entry("mode", mode)?;
        }
        if let Some(tracks) = &self.tracks {
            map.serialize_entry("tracks", tracks)?;
        }
        map.end()
    }
}

/// A base track in union shape (bone xor channel, enforced
/// in validation).
#[derive(Debug, Clone)]
pub struct UTrack {
    /// Bone name (bone tracks only).
    pub bone: Option<String>,
    /// Channel id (channel tracks only).
    pub channel: Option<String>,
    /// Rotation quat keys.
    pub rotation: Option<Vec<UQuatKey>>,
    /// Scale vector keys.
    pub scale: Option<Vec<UVecKey>>,
    /// Translation vector keys.
    pub translation: Option<Vec<UVecKey>>,
    /// Scalar keys.
    pub keys: Option<Vec<UScalarKey>>,
}

impl UTrack {
    /// The track's target name for ordering and duplicate
    /// checks: the bone name if present, else the channel.
    pub fn target(&self) -> Option<&str> {
        self.bone.as_deref().or(self.channel.as_deref())
    }
}

impl Serialize for UTrack {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeMap;
        let mut len = 0;
        if self.bone.is_some() {
            len += 1;
        }
        if self.channel.is_some() {
            len += 1;
        }
        if self.rotation.is_some() {
            len += 1;
        }
        if self.scale.is_some() {
            len += 1;
        }
        if self.translation.is_some() {
            len += 1;
        }
        if self.keys.is_some() {
            len += 1;
        }
        let mut map = serializer.serialize_map(Some(len))?;
        if let Some(bone) = &self.bone {
            map.serialize_entry("bone", bone)?;
        }
        if let Some(channel) = &self.channel {
            map.serialize_entry("channel", channel)?;
        }
        if let Some(rotation) = &self.rotation {
            map.serialize_entry("rotation", rotation)?;
        }
        if let Some(scale) = &self.scale {
            map.serialize_entry("scale", scale)?;
        }
        if let Some(translation) = &self.translation {
            map.serialize_entry("translation", translation)?;
        }
        if let Some(keys) = &self.keys {
            map.serialize_entry("keys", keys)?;
        }
        map.end()
    }
}

/// A base scalar key, interpolation resolved.
#[derive(Debug, Clone, Serialize)]
pub struct UScalarKey {
    /// Key frame.
    pub frame: u32,
    /// Scalar value.
    pub value: f64,
    /// Segment interpolation token.
    pub interpolation: String,
}

/// A base vector key, interpolation resolved.
#[derive(Debug, Clone, Serialize)]
pub struct UVecKey {
    /// Key frame.
    pub frame: u32,
    /// Vector value.
    pub value: [f64; 3],
    /// Segment interpolation token.
    pub interpolation: String,
}

/// A base quaternion key, interpolation resolved.
#[derive(Debug, Clone, Serialize)]
pub struct UQuatKey {
    /// Key frame.
    pub frame: u32,
    /// Quaternion value, `[x, y, z, w]`.
    pub value: [f64; 4],
    /// Segment interpolation token.
    pub interpolation: String,
}

/// One additive layer, weight resolved.
#[derive(Debug, Clone, Serialize)]
pub struct ULayer {
    /// Layer id.
    pub id: String,
    /// Optional free-text name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Layer weight, resolved (default 1).
    pub weight: f64,
    /// Declared scope.
    pub scope: UScope,
    /// Layer tracks.
    pub tracks: Vec<ULayerTrack>,
}

/// A layer's declared scope; both lists always emitted at
/// their resolved values (possibly empty).
#[derive(Debug, Clone, Serialize)]
pub struct UScope {
    /// Scoped bone names.
    pub bones: Vec<String>,
    /// Scoped channel ids.
    pub channels: Vec<String>,
}

/// A layer track in union shape, with delta key types
/// distinct from the base key types.
#[derive(Debug, Clone)]
pub struct ULayerTrack {
    /// Bone name (bone tracks only).
    pub bone: Option<String>,
    /// Channel id (channel tracks only).
    pub channel: Option<String>,
    /// Rotation delta keys.
    pub rotation: Option<Vec<UDeltaQuatKey>>,
    /// Scale delta keys.
    pub scale: Option<Vec<UDeltaVecKey>>,
    /// Translation delta keys.
    pub translation: Option<Vec<UDeltaVecKey>>,
    /// Scalar delta keys.
    pub keys: Option<Vec<UDeltaScalarKey>>,
}

impl ULayerTrack {
    /// The track's target name, as in [`UTrack::target`].
    pub fn target(&self) -> Option<&str> {
        self.bone.as_deref().or(self.channel.as_deref())
    }
}

impl Serialize for ULayerTrack {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        // Same field order as UTrack; the key types differ.
        use serde::ser::SerializeMap;
        let mut len = 0;
        if self.bone.is_some() {
            len += 1;
        }
        if self.channel.is_some() {
            len += 1;
        }
        if self.rotation.is_some() {
            len += 1;
        }
        if self.scale.is_some() {
            len += 1;
        }
        if self.translation.is_some() {
            len += 1;
        }
        if self.keys.is_some() {
            len += 1;
        }
        let mut map = serializer.serialize_map(Some(len))?;
        if let Some(bone) = &self.bone {
            map.serialize_entry("bone", bone)?;
        }
        if let Some(channel) = &self.channel {
            map.serialize_entry("channel", channel)?;
        }
        if let Some(rotation) = &self.rotation {
            map.serialize_entry("rotation", rotation)?;
        }
        if let Some(scale) = &self.scale {
            map.serialize_entry("scale", scale)?;
        }
        if let Some(translation) = &self.translation {
            map.serialize_entry("translation", translation)?;
        }
        if let Some(keys) = &self.keys {
            map.serialize_entry("keys", keys)?;
        }
        map.end()
    }
}

/// A scalar delta key, interpolation resolved.
#[derive(Debug, Clone, Serialize)]
pub struct UDeltaScalarKey {
    /// Key frame.
    pub frame: u32,
    /// Scalar delta.
    pub value: f64,
    /// Segment interpolation token.
    pub interpolation: String,
}

/// A vector delta key, interpolation resolved.
#[derive(Debug, Clone, Serialize)]
pub struct UDeltaVecKey {
    /// Key frame.
    pub frame: u32,
    /// Vector delta.
    pub value: [f64; 3],
    /// Segment interpolation token.
    pub interpolation: String,
}

/// A quaternion delta key, interpolation resolved.
#[derive(Debug, Clone, Serialize)]
pub struct UDeltaQuatKey {
    /// Key frame.
    pub frame: u32,
    /// Quaternion delta, `[x, y, z, w]`.
    pub value: [f64; 4],
    /// Segment interpolation token.
    pub interpolation: String,
}

/// The provenance block.
#[derive(Debug, Clone, Serialize)]
pub struct UProvenance {
    /// The provenance format token, as authored.
    pub format: String,
    /// One entry per channel group, as authored.
    pub groups: Vec<UGroupProvenance>,
}

/// One group's provenance entry.
#[derive(Debug, Clone, Serialize)]
pub struct UGroupProvenance {
    /// The group kind this entry describes.
    pub group: String,
    /// Source class token.
    pub source_class: String,
    /// Converter id, or `"authored"`.
    pub converter: String,
    /// Converter build version.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub converter_version: Option<String>,
    /// Producer model weights hash.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_sha256: Option<String>,
    /// Source artifact hash.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_sha256: Option<String>,
    /// Source content region.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub region: Option<Uregion>,
    /// Measurement record id.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub measurement_ref: Option<String>,
    /// Frame map.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frame_map: Option<UFrameMap>,
}

/// A source content region.
#[derive(Debug, Clone, Serialize)]
pub struct Uregion {
    /// Left edge, in source pixels.
    pub x: u32,
    /// Top edge, in source pixels.
    pub y: u32,
    /// Width, in source pixels.
    pub w: u32,
    /// Height, in source pixels.
    pub h: u32,
}

/// A frame map.
#[derive(Debug, Clone, Serialize)]
pub struct UFrameMap {
    /// Number of frames in the source.
    pub source_frame_count: u32,
    /// The source rate.
    pub source_rate: UFrameRate,
    /// One entry per clip frame.
    pub entries: Vec<u32>,
}

/// A frame map's source rate.
#[derive(Debug, Clone, PartialEq)]
pub enum UFrameRate {
    /// A rational rate, `num / den`.
    Rational {
        /// Numerator.
        num: u32,
        /// Denominator.
        den: u32,
    },
    /// A variable-rate source.
    Variable,
}

impl Serialize for UFrameRate {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeMap;
        match self {
            UFrameRate::Variable => serializer.serialize_str("variable"),
            UFrameRate::Rational { num, den } => {
                let mut map = serializer.serialize_map(Some(2))?;
                map.serialize_entry("num", num)?;
                map.serialize_entry("den", den)?;
                map.end()
            }
        }
    }
}

impl UClip {
    /// Convert a raw parse into the unchecked form, resolving
    /// the spec's named defaults (SPEC.md section 10.2). The
    /// only failures are JSON-null stand-ins for array/object
    /// fields, which are type errors (`TMC-103`) — the raw
    /// types use `Option<Option<T>>` exactly where presence
    /// is significant.
    pub fn from_raw(raw: RawClip) -> Result<UClip, ClipError> {
        let layers = match raw.layers {
            None => Vec::new(),
            Some(None) => {
                return Err(ClipError::Schema {
                    message: "`layers` must be an array".to_string(),
                });
            }
            Some(Some(layers)) => layers.into_iter().map(ULayer::from_raw).collect(),
        };
        Ok(UClip {
            format: raw.format,
            id: raw.id,
            skeleton: USkeleton::from_raw(raw.skeleton),
            frames: raw.frames,
            loop_: raw.loop_.unwrap_or(false),
            groups: raw
                .groups
                .into_iter()
                .map(UGroup::from_raw)
                .collect::<Result<Vec<_>, _>>()?,
            layers,
            provenance: UProvenance::from_raw(raw.provenance),
            sha256: raw.sha256,
        })
    }

    /// Sort the collections section 1.2 canonicalizes. Layers
    /// keep stack order; provenance entries keep authored
    /// order (the spec canonicalizes neither).
    fn sort_collections(&mut self) {
        self.groups.sort_by(|a, b| a.kind.cmp(&b.kind));
        for group in &mut self.groups {
            if let Some(tracks) = &mut group.tracks {
                tracks.sort_by(|a, b| a.target().cmp(&b.target()));
                for track in tracks.iter_mut() {
                    sort_base_track_keys(track);
                }
            }
        }
        for layer in &mut self.layers {
            layer.scope.bones.sort();
            layer.scope.channels.sort();
            for track in &mut layer.tracks {
                sort_layer_track_keys(track);
            }
        }
    }
}

fn sort_base_track_keys(track: &mut UTrack) {
    if let Some(keys) = &mut track.rotation {
        keys.sort_by_key(|key| key.frame);
    }
    if let Some(keys) = &mut track.scale {
        keys.sort_by_key(|key| key.frame);
    }
    if let Some(keys) = &mut track.translation {
        keys.sort_by_key(|key| key.frame);
    }
    if let Some(keys) = &mut track.keys {
        keys.sort_by_key(|key| key.frame);
    }
}

fn sort_layer_track_keys(track: &mut ULayerTrack) {
    if let Some(keys) = &mut track.rotation {
        keys.sort_by_key(|key| key.frame);
    }
    if let Some(keys) = &mut track.scale {
        keys.sort_by_key(|key| key.frame);
    }
    if let Some(keys) = &mut track.translation {
        keys.sort_by_key(|key| key.frame);
    }
    if let Some(keys) = &mut track.keys {
        keys.sort_by_key(|key| key.frame);
    }
}

impl USkeleton {
    fn from_raw(raw: RawSkeleton) -> USkeleton {
        USkeleton {
            def: raw.def,
            sha256: raw.sha256,
        }
    }
}

impl UGroup {
    fn from_raw(raw: RawGroup) -> Result<UGroup, ClipError> {
        let tracks = match raw.tracks {
            None => None,
            Some(None) => {
                // A present-but-null `tracks`: for body/face
                // this is a plain type error; for camera/hands
                // the field's presence is what TMC-108 names,
                // so keep an empty list as the presence marker.
                if raw.kind == "camera" || raw.kind == "hands" {
                    Some(Vec::new())
                } else {
                    return Err(ClipError::Schema {
                        message: "`tracks` must be an array".to_string(),
                    });
                }
            }
            Some(Some(tracks)) => {
                Some(tracks.into_iter().map(UTrack::from_raw).collect::<Vec<_>>())
            }
        };
        let mode = match raw.mode {
            None => None,
            Some(None) => Some(String::new()),
            Some(Some(mode)) => Some(mode),
        };
        Ok(UGroup {
            kind: raw.kind,
            mode,
            tracks,
        })
    }
}

impl UTrack {
    fn from_raw(raw: RawTrack) -> UTrack {
        UTrack {
            bone: raw.bone,
            channel: raw.channel,
            rotation: raw
                .rotation
                .map(|keys| keys.into_iter().map(UQuatKey::from_raw).collect()),
            scale: raw
                .scale
                .map(|keys| keys.into_iter().map(UVecKey::from_raw).collect()),
            translation: raw
                .translation
                .map(|keys| keys.into_iter().map(UVecKey::from_raw).collect()),
            keys: raw
                .keys
                .map(|keys| keys.into_iter().map(UScalarKey::from_raw).collect()),
        }
    }
}

impl UScalarKey {
    fn from_raw(raw: RawScalarKey) -> UScalarKey {
        UScalarKey {
            frame: raw.frame,
            value: raw.value,
            interpolation: raw.interpolation.unwrap_or_else(|| "linear".to_string()),
        }
    }
}

impl UVecKey {
    fn from_raw(raw: RawVecKey) -> UVecKey {
        UVecKey {
            frame: raw.frame,
            value: raw.value,
            interpolation: raw.interpolation.unwrap_or_else(|| "linear".to_string()),
        }
    }
}

impl UQuatKey {
    fn from_raw(raw: RawQuatKey) -> UQuatKey {
        UQuatKey {
            frame: raw.frame,
            value: raw.value,
            interpolation: raw.interpolation.unwrap_or_else(|| "linear".to_string()),
        }
    }
}

impl ULayer {
    fn from_raw(raw: RawLayer) -> ULayer {
        ULayer {
            id: raw.id,
            name: raw.name,
            weight: raw.weight.unwrap_or(1.0),
            scope: raw.scope.map(UScope::from_raw).unwrap_or(UScope {
                bones: Vec::new(),
                channels: Vec::new(),
            }),
            tracks: raw.tracks.into_iter().map(ULayerTrack::from_raw).collect(),
        }
    }
}

impl UScope {
    fn from_raw(raw: RawScope) -> UScope {
        UScope {
            bones: raw.bones.unwrap_or_default(),
            channels: raw.channels.unwrap_or_default(),
        }
    }
}

impl ULayerTrack {
    fn from_raw(raw: RawLayerTrack) -> ULayerTrack {
        ULayerTrack {
            bone: raw.bone,
            channel: raw.channel,
            rotation: raw
                .rotation
                .map(|keys| keys.into_iter().map(UDeltaQuatKey::from_raw).collect()),
            scale: raw
                .scale
                .map(|keys| keys.into_iter().map(UDeltaVecKey::from_raw).collect()),
            translation: raw
                .translation
                .map(|keys| keys.into_iter().map(UDeltaVecKey::from_raw).collect()),
            keys: raw
                .keys
                .map(|keys| keys.into_iter().map(UDeltaScalarKey::from_raw).collect()),
        }
    }
}

impl UDeltaScalarKey {
    fn from_raw(raw: RawDeltaScalarKey) -> UDeltaScalarKey {
        UDeltaScalarKey {
            frame: raw.frame,
            value: raw.value,
            interpolation: raw.interpolation.unwrap_or_else(|| "linear".to_string()),
        }
    }
}

impl UDeltaVecKey {
    fn from_raw(raw: RawDeltaVecKey) -> UDeltaVecKey {
        UDeltaVecKey {
            frame: raw.frame,
            value: raw.value,
            interpolation: raw.interpolation.unwrap_or_else(|| "linear".to_string()),
        }
    }
}

impl UDeltaQuatKey {
    fn from_raw(raw: RawDeltaQuatKey) -> UDeltaQuatKey {
        UDeltaQuatKey {
            frame: raw.frame,
            value: raw.value,
            interpolation: raw.interpolation.unwrap_or_else(|| "linear".to_string()),
        }
    }
}

impl UProvenance {
    fn from_raw(raw: RawProvenance) -> UProvenance {
        UProvenance {
            format: raw.format,
            groups: raw
                .groups
                .into_iter()
                .map(UGroupProvenance::from_raw)
                .collect(),
        }
    }
}

impl UGroupProvenance {
    fn from_raw(raw: RawGroupProvenance) -> UGroupProvenance {
        UGroupProvenance {
            group: raw.group,
            source_class: raw.source_class,
            converter: raw.converter,
            converter_version: raw.converter_version,
            model_sha256: raw.model_sha256,
            source_sha256: raw.source_sha256,
            region: raw.region.map(|region| Uregion {
                x: region.x,
                y: region.y,
                w: region.w,
                h: region.h,
            }),
            measurement_ref: raw.measurement_ref,
            frame_map: raw.frame_map.map(UFrameMap::from_raw),
        }
    }
}

impl UFrameMap {
    fn from_raw(raw: RawFrameMap) -> UFrameMap {
        UFrameMap {
            source_frame_count: raw.source_frame_count,
            source_rate: match raw.source_rate {
                RawFrameRate::Rational { num, den } => UFrameRate::Rational { num, den },
                RawFrameRate::Variable => UFrameRate::Variable,
            },
            entries: raw.entries,
        }
    }
}

/// Emit the canonical serialization of an unchecked clip
/// (SPEC.md section 1.2), with the `sha256` field replaced
/// by `sha_override` when one is given (the self-hash form,
/// section 3.4: the field emptied).
pub fn emit_canonical(clip: &UClip, sha_override: Option<&str>) -> String {
    let mut sorted = clip.clone();
    sorted.sort_collections();
    if let Some(sha) = sha_override {
        sorted.sha256 = Some(sha.to_string());
    }
    let mut text = serde_json::to_string_pretty(&sorted)
        .expect("UClip serialization cannot fail: plain data only");
    text.push('\n');
    text
}

/// SHA-256 of a byte string, as 64 lowercase hex characters.
pub fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::Digest;
    let digest = sha2::Sha256::digest(bytes);
    let mut hex = String::with_capacity(64);
    for byte in digest {
        hex.push_str(&format!("{byte:02x}"));
    }
    hex
}
