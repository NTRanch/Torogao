//! The validated clip model: the public types a caller
//! works with once a document has passed document
//! validation (SPEC.md section 9, steps 1–10).
//!
//! Base keys and delta keys are distinct types with no
//! shared storage and no conversion between them (section
//! 5.4, mechanism 3). The only bridge the format sanctions
//! is an explicit converter that computes deltas from two
//! absolute clips and emits new delta data (section 5.2);
//! this crate deliberately provides no such conversion.

use crate::uclip::{
    UClip, UFrameMap, UFrameRate, UGroupProvenance, ULayer, ULayerTrack, UProvenance, UScope,
    USkeleton, emit_canonical,
};

/// The format token this crate implements.
pub const FORMAT_TOKEN: &str = "torogao-motion-clip/1";
/// The provenance format token carried inline (SPEC.md §6).
pub const PROVENANCE_TOKEN: &str = "torogao-take-provenance/1";

/// A validated `torogao-motion-clip/1` document.
#[derive(Debug, Clone, PartialEq)]
pub struct Clip {
    /// The clip's id.
    pub id: String,
    /// The skeleton binding.
    pub skeleton: SkeletonBinding,
    /// Frame count, `1..=FRAMES_MAX`. Valid frame indices
    /// are `0..frames`; evaluation is at 60 Hz (SPEC.md
    /// section 2.1).
    pub frames: u32,
    /// Whether the clip is authored to loop.
    pub loop_: bool,
    /// Channel groups, in canonical (alphabetical) order.
    pub groups: Vec<Group>,
    /// The additive layer stack, in stack order (index 0
    /// applies first, directly over the base).
    pub layers: Vec<Layer>,
    /// The provenance block.
    pub provenance: Provenance,
    /// The self-hash, when the document carries one
    /// (verified at parse time).
    pub sha256: Option<String>,
}

impl Clip {
    /// The canonical serialization of this clip (SPEC.md
    /// section 1.2). For a clip parsed from canonical bytes,
    /// this reproduces the input byte for byte.
    pub fn to_canonical_json(&self) -> String {
        emit_canonical(&self.to_uclip(), None)
    }

    /// The body group's tracks, if a body group is present.
    pub fn body_tracks(&self) -> Option<&[BoneTrack]> {
        self.groups.iter().find_map(|group| match group {
            Group::Body { tracks } => Some(tracks.as_slice()),
            _ => None,
        })
    }

    /// The face group's tracks, if a face group is present.
    pub fn face_tracks(&self) -> Option<&[ChannelTrack]> {
        self.groups.iter().find_map(|group| match group {
            Group::Face { tracks } => Some(tracks.as_slice()),
            _ => None,
        })
    }

    /// Convert back to the unchecked form for emission.
    /// Lossless: validation has already fixed every default
    /// and order this conversion relies on.
    pub fn to_uclip(&self) -> UClip {
        UClip {
            format: FORMAT_TOKEN.to_string(),
            id: self.id.clone(),
            skeleton: USkeleton {
                def: self.skeleton.def.clone(),
                sha256: self.skeleton.sha256.clone(),
            },
            frames: self.frames,
            loop_: self.loop_,
            groups: self.groups.iter().map(group_to_uclip).collect(),
            layers: self.layers.iter().map(layer_to_uclip).collect(),
            provenance: UProvenance {
                format: PROVENANCE_TOKEN.to_string(),
                groups: self
                    .provenance
                    .groups
                    .iter()
                    .map(group_provenance_to_uclip)
                    .collect(),
            },
            sha256: self.sha256.clone(),
        }
    }
}

/// The skeleton binding (SPEC.md section 3.2).
#[derive(Debug, Clone, PartialEq)]
pub struct SkeletonBinding {
    /// The SkeletonDef token (e.g. `torogao-humanoid/1`).
    pub def: String,
    /// SHA-256 of the exact SkeletonDef document this clip
    /// was authored against.
    pub sha256: String,
}

/// A channel group kind (SPEC.md section 2.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum GroupKind {
    /// Bone tracks, named by the SkeletonDef.
    Body,
    /// Declaration only in v1: the source camera mode.
    Camera,
    /// Scalar channel tracks, named by the rig.
    Face,
    /// Declaration only in v1; content deferred to P4.
    Hands,
}

impl GroupKind {
    /// The group kind's token.
    pub fn token(&self) -> &'static str {
        match self {
            GroupKind::Body => "body",
            GroupKind::Camera => "camera",
            GroupKind::Face => "face",
            GroupKind::Hands => "hands",
        }
    }

    /// Parse a group kind token, if known.
    pub fn from_token(token: &str) -> Option<GroupKind> {
        match token {
            "body" => Some(GroupKind::Body),
            "camera" => Some(GroupKind::Camera),
            "face" => Some(GroupKind::Face),
            "hands" => Some(GroupKind::Hands),
            _ => None,
        }
    }
}

/// One channel group.
#[derive(Debug, Clone, PartialEq)]
pub enum Group {
    /// The body group: bone tracks.
    Body {
        /// Bone tracks, sorted ascending by bone name.
        tracks: Vec<BoneTrack>,
    },
    /// The face group: scalar channel tracks.
    Face {
        /// Channel tracks, sorted ascending by channel id.
        tracks: Vec<ChannelTrack>,
    },
    /// The camera group (declaration only in v1).
    Camera {
        /// Whether the source performance was captured with
        /// a static or a moving camera.
        mode: CameraMode,
    },
    /// The hands group (declaration only in v1).
    Hands,
}

impl Group {
    /// This group's kind.
    pub fn kind(&self) -> GroupKind {
        match self {
            Group::Body { .. } => GroupKind::Body,
            Group::Face { .. } => GroupKind::Face,
            Group::Camera { .. } => GroupKind::Camera,
            Group::Hands => GroupKind::Hands,
        }
    }
}

/// The camera group's `mode` (SPEC.md section 3.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CameraMode {
    /// The source camera moved during capture.
    Moving,
    /// The source camera was static during capture.
    Static,
}

impl CameraMode {
    /// The mode's token.
    pub fn token(&self) -> &'static str {
        match self {
            CameraMode::Moving => "moving",
            CameraMode::Static => "static",
        }
    }

    /// Parse a camera mode token, if known.
    pub fn from_token(token: &str) -> Option<CameraMode> {
        match token {
            "moving" => Some(CameraMode::Moving),
            "static" => Some(CameraMode::Static),
            _ => None,
        }
    }
}

/// A bone track: independent component key lists for one
/// bone (SPEC.md section 4.2). At least one list is present,
/// and every present list is non-empty.
#[derive(Debug, Clone, PartialEq)]
pub struct BoneTrack {
    /// The bone name, drawn from the SkeletonDef.
    pub bone: String,
    /// Rotation keys (unit quaternions), if keyed.
    pub rotation: Option<Vec<QuatKey>>,
    /// Scale keys (all components > 0), if keyed.
    pub scale: Option<Vec<VecKey>>,
    /// Translation keys, if keyed. Root motion is the root
    /// bone's translation track.
    pub translation: Option<Vec<VecKey>>,
}

/// A scalar channel track (SPEC.md section 4.3).
#[derive(Debug, Clone, PartialEq)]
pub struct ChannelTrack {
    /// The channel id, drawn from the rig's declarations.
    pub channel: String,
    /// Scalar keys; non-empty.
    pub keys: Vec<ScalarKey>,
}

/// How the segment FROM a key to the next key evaluates
/// (SPEC.md section 4.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Interpolation {
    /// Linear interpolation (nlerp for quaternions).
    Linear,
    /// Hold the key's value for the whole segment.
    Stepped,
}

impl Interpolation {
    /// The interpolation token.
    pub fn token(&self) -> &'static str {
        match self {
            Interpolation::Linear => "linear",
            Interpolation::Stepped => "stepped",
        }
    }

    /// Parse an interpolation token, if known.
    pub fn from_token(token: &str) -> Option<Interpolation> {
        match token {
            "linear" => Some(Interpolation::Linear),
            "stepped" => Some(Interpolation::Stepped),
            _ => None,
        }
    }
}

/// A base scalar key.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScalarKey {
    /// Key frame.
    pub frame: u32,
    /// Scalar value.
    pub value: f64,
    /// The segment mode from this key to the next.
    pub interpolation: Interpolation,
}

/// A base vector key.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VecKey {
    /// Key frame.
    pub frame: u32,
    /// Vector value.
    pub value: [f64; 3],
    /// The segment mode from this key to the next.
    pub interpolation: Interpolation,
}

/// A base quaternion key.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct QuatKey {
    /// Key frame.
    pub frame: u32,
    /// Unit quaternion, `[x, y, z, w]`.
    pub value: [f64; 4],
    /// The segment mode from this key to the next.
    pub interpolation: Interpolation,
}

/// One additive layer (SPEC.md section 5.1). Stores delta
/// keys only; the grammar has no production that could
/// reference base data.
#[derive(Debug, Clone, PartialEq)]
pub struct Layer {
    /// Layer id; unique within the stack.
    pub id: String,
    /// Optional free-text name.
    pub name: Option<String>,
    /// Authored weight in `[0, 1]`. Evaluation uses it as
    /// authored; there is no runtime weight parameter.
    pub weight: f64,
    /// The declared scope. A layer contributes only on
    /// targets in its scope, and every track target must be
    /// in scope.
    pub scope: Scope,
    /// Layer tracks. May be empty: a zero-track layer is a
    /// named, weighted placeholder and evaluates to exactly
    /// no change.
    pub tracks: Vec<LayerTrack>,
}

/// A layer's declared scope.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Scope {
    /// Scoped bone names, sorted ascending, duplicate-free.
    pub bones: Vec<String>,
    /// Scoped channel ids, sorted ascending, duplicate-free.
    pub channels: Vec<String>,
}

/// A layer track: a bone track or a channel track whose
/// keys are deltas from the composed-so-far value.
#[derive(Debug, Clone, PartialEq)]
pub enum LayerTrack {
    /// A bone layer track (delta keys).
    Bone(LayerBoneTrack),
    /// A channel layer track (delta keys).
    Channel(LayerChannelTrack),
}

impl LayerTrack {
    /// The track's target name.
    pub fn target(&self) -> &str {
        match self {
            LayerTrack::Bone(track) => &track.bone,
            LayerTrack::Channel(track) => &track.channel,
        }
    }
}

/// A bone layer track: independent delta key lists for one
/// bone. At least one list is present; present lists are
/// non-empty.
#[derive(Debug, Clone, PartialEq)]
pub struct LayerBoneTrack {
    /// The bone name; must be in the layer's scope.
    pub bone: String,
    /// Rotation delta keys.
    pub rotation: Option<Vec<DeltaQuatKey>>,
    /// Scale factor delta keys.
    pub scale: Option<Vec<DeltaVecKey>>,
    /// Translation delta keys.
    pub translation: Option<Vec<DeltaVecKey>>,
}

/// A channel layer track.
#[derive(Debug, Clone, PartialEq)]
pub struct LayerChannelTrack {
    /// The channel id; must be in the layer's scope.
    pub channel: String,
    /// Scalar delta keys; non-empty.
    pub keys: Vec<DeltaScalarKey>,
}

/// A scalar delta key: an offset added to the
/// composed-so-far channel value (SPEC.md section 5.2).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DeltaScalarKey {
    /// Key frame.
    pub frame: u32,
    /// Scalar delta; 0 means no change.
    pub value: f64,
    /// The segment mode from this key to the next.
    pub interpolation: Interpolation,
}

/// A vector delta key: a translation offset, or a scale
/// factor (SPEC.md section 5.2).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DeltaVecKey {
    /// Key frame.
    pub frame: u32,
    /// Vector delta; `(0, 0, 0)` (translation) or
    /// `(1, 1, 1)` (scale) means no change.
    pub value: [f64; 3],
    /// The segment mode from this key to the next.
    pub interpolation: Interpolation,
}

/// A quaternion delta key: a rotation relative to the
/// composed-so-far orientation, in the bone's local frame.
/// The identity quaternion means no change.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DeltaQuatKey {
    /// Key frame.
    pub frame: u32,
    /// Quaternion delta, `[x, y, z, w]`.
    pub value: [f64; 4],
    /// The segment mode from this key to the next.
    pub interpolation: Interpolation,
}

/// The provenance block (SPEC.md section 6). Required:
/// every clip states where its data came from.
#[derive(Debug, Clone, PartialEq)]
pub struct Provenance {
    /// One entry per channel group in the clip, exactly
    /// one each.
    pub groups: Vec<GroupProvenance>,
}

/// One group's provenance entry.
#[derive(Debug, Clone, PartialEq)]
pub struct GroupProvenance {
    /// The group this entry describes.
    pub group: GroupKind,
    /// How the group's data was produced.
    pub source_class: SourceClass,
    /// The named converter that produced the group, or
    /// `"authored"`.
    pub converter: String,
    /// The converter's build version, when it differs from
    /// the name's version.
    pub converter_version: Option<String>,
    /// SHA-256 of the producer's model weights, when a
    /// learned model was involved.
    pub model_sha256: Option<String>,
    /// SHA-256 of the source artifact the group derives
    /// from.
    pub source_sha256: Option<String>,
    /// The source content region, when the source needed
    /// region detection.
    pub region: Option<Region>,
    /// The measurement record holding the raw estimator
    /// output, when one exists.
    pub measurement_ref: Option<String>,
    /// The frame map, when the source ran off 60 Hz.
    pub frame_map: Option<FrameMap>,
}

/// A provenance source class (SPEC.md section 6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceClass {
    /// Written by hand or by an authoring tool directly in
    /// this format.
    Authored,
    /// Derived from a recorded performance by an intake
    /// converter.
    Captured,
    /// Translated from another animation format by a named
    /// converter.
    Converted,
    /// Produced by a generative authoring-time method.
    Generated,
}

impl SourceClass {
    /// The source class token.
    pub fn token(&self) -> &'static str {
        match self {
            SourceClass::Authored => "authored",
            SourceClass::Captured => "captured",
            SourceClass::Converted => "converted",
            SourceClass::Generated => "generated",
        }
    }

    /// Parse a source class token, if known.
    pub fn from_token(token: &str) -> Option<SourceClass> {
        match token {
            "authored" => Some(SourceClass::Authored),
            "captured" => Some(SourceClass::Captured),
            "converted" => Some(SourceClass::Converted),
            "generated" => Some(SourceClass::Generated),
            _ => None,
        }
    }
}

/// A source content region, in source pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Region {
    /// Left edge.
    pub x: u32,
    /// Top edge.
    pub y: u32,
    /// Width.
    pub w: u32,
    /// Height.
    pub h: u32,
}

/// A frame map (SPEC.md section 6.2): `entries[f]` is the
/// index of the source frame whose sampled value produced
/// take frame `f`. Entries are non-decreasing, one per clip
/// frame.
#[derive(Debug, Clone, PartialEq)]
pub struct FrameMap {
    /// Number of frames in the source.
    pub source_frame_count: u32,
    /// The source rate.
    pub source_rate: FrameRate,
    /// One entry per clip frame.
    pub entries: Vec<u32>,
}

/// A frame map's source rate: a rational, never a float.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FrameRate {
    /// A rational rate, `num / den` frames per second.
    Rational {
        /// Numerator.
        num: u32,
        /// Denominator (nonzero).
        den: u32,
    },
    /// A variable-rate source; the map is the whole truth.
    Variable,
}

fn group_to_uclip(group: &Group) -> crate::uclip::UGroup {
    use crate::uclip::{UGroup, UTrack};
    match group {
        Group::Body { tracks } => UGroup {
            kind: "body".to_string(),
            mode: None,
            tracks: Some(
                tracks
                    .iter()
                    .map(|track| UTrack {
                        bone: Some(track.bone.clone()),
                        channel: None,
                        rotation: track.rotation.as_ref().map(|keys| {
                            keys.iter()
                                .map(|key| crate::uclip::UQuatKey {
                                    frame: key.frame,
                                    value: key.value,
                                    interpolation: key.interpolation.token().to_string(),
                                })
                                .collect()
                        }),
                        scale: track.scale.as_ref().map(|keys| {
                            keys.iter()
                                .map(|key| crate::uclip::UVecKey {
                                    frame: key.frame,
                                    value: key.value,
                                    interpolation: key.interpolation.token().to_string(),
                                })
                                .collect()
                        }),
                        translation: track.translation.as_ref().map(|keys| {
                            keys.iter()
                                .map(|key| crate::uclip::UVecKey {
                                    frame: key.frame,
                                    value: key.value,
                                    interpolation: key.interpolation.token().to_string(),
                                })
                                .collect()
                        }),
                        keys: None,
                    })
                    .collect(),
            ),
        },
        Group::Face { tracks } => UGroup {
            kind: "face".to_string(),
            mode: None,
            tracks: Some(
                tracks
                    .iter()
                    .map(|track| UTrack {
                        bone: None,
                        channel: Some(track.channel.clone()),
                        rotation: None,
                        scale: None,
                        translation: None,
                        keys: Some(
                            track
                                .keys
                                .iter()
                                .map(|key| crate::uclip::UScalarKey {
                                    frame: key.frame,
                                    value: key.value,
                                    interpolation: key.interpolation.token().to_string(),
                                })
                                .collect(),
                        ),
                    })
                    .collect(),
            ),
        },
        Group::Camera { mode } => UGroup {
            kind: "camera".to_string(),
            mode: Some(mode.token().to_string()),
            tracks: None,
        },
        Group::Hands => UGroup {
            kind: "hands".to_string(),
            mode: None,
            tracks: None,
        },
    }
}

fn layer_to_uclip(layer: &Layer) -> ULayer {
    ULayer {
        id: layer.id.clone(),
        name: layer.name.clone(),
        weight: layer.weight,
        scope: UScope {
            bones: layer.scope.bones.clone(),
            channels: layer.scope.channels.clone(),
        },
        tracks: layer
            .tracks
            .iter()
            .map(|track| match track {
                LayerTrack::Bone(track) => ULayerTrack {
                    bone: Some(track.bone.clone()),
                    channel: None,
                    rotation: track.rotation.as_ref().map(|keys| {
                        keys.iter()
                            .map(|key| crate::uclip::UDeltaQuatKey {
                                frame: key.frame,
                                value: key.value,
                                interpolation: key.interpolation.token().to_string(),
                            })
                            .collect()
                    }),
                    scale: track.scale.as_ref().map(|keys| {
                        keys.iter()
                            .map(|key| crate::uclip::UDeltaVecKey {
                                frame: key.frame,
                                value: key.value,
                                interpolation: key.interpolation.token().to_string(),
                            })
                            .collect()
                    }),
                    translation: track.translation.as_ref().map(|keys| {
                        keys.iter()
                            .map(|key| crate::uclip::UDeltaVecKey {
                                frame: key.frame,
                                value: key.value,
                                interpolation: key.interpolation.token().to_string(),
                            })
                            .collect()
                    }),
                    keys: None,
                },
                LayerTrack::Channel(track) => ULayerTrack {
                    bone: None,
                    channel: Some(track.channel.clone()),
                    rotation: None,
                    scale: None,
                    translation: None,
                    keys: Some(
                        track
                            .keys
                            .iter()
                            .map(|key| crate::uclip::UDeltaScalarKey {
                                frame: key.frame,
                                value: key.value,
                                interpolation: key.interpolation.token().to_string(),
                            })
                            .collect(),
                    ),
                },
            })
            .collect(),
    }
}

fn group_provenance_to_uclip(entry: &GroupProvenance) -> UGroupProvenance {
    UGroupProvenance {
        group: entry.group.token().to_string(),
        source_class: entry.source_class.token().to_string(),
        converter: entry.converter.clone(),
        converter_version: entry.converter_version.clone(),
        model_sha256: entry.model_sha256.clone(),
        source_sha256: entry.source_sha256.clone(),
        region: entry.region.map(|region| crate::uclip::Uregion {
            x: region.x,
            y: region.y,
            w: region.w,
            h: region.h,
        }),
        measurement_ref: entry.measurement_ref.clone(),
        frame_map: entry.frame_map.as_ref().map(|map| UFrameMap {
            source_frame_count: map.source_frame_count,
            source_rate: match map.source_rate {
                FrameRate::Rational { num, den } => UFrameRate::Rational { num, den },
                FrameRate::Variable => UFrameRate::Variable,
            },
            entries: map.entries.clone(),
        }),
    }
}
