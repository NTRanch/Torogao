//! Binding validation (SPEC.md section 9, steps 11–13)
//! and the bound clip: a validated clip plus the records it
//! is bound to, ready to evaluate.
//!
//! The SkeletonDef record is defined by its own
//! specification (SPEC.md section 14, item 2), which this
//! format references but does not define. Binding therefore
//! runs against the [`SkeletonDef`] trait — the narrow view
//! this format consumes: the def token, the presented
//! document's bytes (whose SHA-256 the binding verifies,
//! `TMC-204`), the bone list, and rest transforms. A
//! concrete SkeletonDef document parser belongs to the
//! SkeletonDef record's own build; nothing here invents
//! that record's shape. Channel declarations are likewise
//! consumed through the [`ChannelDecls`] trait.

use std::collections::{BTreeMap, BTreeSet};

use crate::error::ClipError;
use crate::model::{BoneTrack, ChannelTrack, Clip, Group, LayerTrack};
use crate::uclip::sha256_hex;

/// One bone of a SkeletonDef: its name and rest transform
/// (local TRS, SPEC.md section 2.2).
#[derive(Debug, Clone, PartialEq)]
pub struct BoneDef {
    /// The bone's name (an id, drawn from the def).
    pub name: String,
    /// Rest translation, in metres.
    pub rest_translation: [f64; 3],
    /// Rest rotation, a unit quaternion `[x, y, z, w]`.
    pub rest_rotation: [f64; 4],
    /// Rest scale.
    pub rest_scale: [f64; 3],
}

/// The view of a SkeletonDef this format consumes.
pub trait SkeletonDef {
    /// The def's versioned token (e.g. `torogao-humanoid/1`).
    fn def_token(&self) -> &str;
    /// The presented def document's bytes. Binding hashes
    /// these bytes and compares against the clip's
    /// `skeleton.sha256` (`TMC-204`).
    fn document_bytes(&self) -> &[u8];
    /// The def's bones, in def order. Evaluation emits
    /// bones in this order.
    fn bones(&self) -> &[BoneDef];
    /// Look a bone up by name.
    fn bone(&self, name: &str) -> Option<&BoneDef> {
        self.bones().iter().find(|bone| bone.name == name)
    }
}

/// One rig channel declaration: the channel's default and
/// its governed range (the range feeds the one sanctioned
/// clamp, SPEC.md section 10.3).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChannelDef {
    /// The value a channel with no track holds. (Section 7's
    /// evaluated-frame set contains only channels that
    /// appear in a track or a layer scope — scope-only
    /// channels take base value 0 — so evaluation never
    /// consults this field; it is part of the declaration
    /// record regardless.)
    pub default: f64,
    /// The declared range minimum.
    pub min: f64,
    /// The declared range maximum.
    pub max: f64,
}

/// The rig's channel declarations.
pub trait ChannelDecls {
    /// Look a channel up by name.
    fn channel(&self, name: &str) -> Option<ChannelDef>;
}

/// A validated clip bound to its SkeletonDef and the rig's
/// channel declarations. Only a bound clip can evaluate
/// (SPEC.md section 2.4).
pub struct BoundClip<'a> {
    /// The validated clip.
    pub(crate) clip: &'a Clip,
    /// The bound SkeletonDef.
    pub(crate) skeleton: &'a dyn SkeletonDef,
    /// The rig's channel declarations.
    pub(crate) channels: &'a dyn ChannelDecls,
    /// Body tracks by bone name (sorted-key map: evaluation
    /// never iterates a hash map, SPEC.md section 8.3).
    pub(crate) body_tracks: BTreeMap<&'a str, &'a BoneTrack>,
    /// Face tracks by channel name.
    pub(crate) face_tracks: BTreeMap<&'a str, &'a ChannelTrack>,
    /// Every channel the evaluated frame carries: channels
    /// in a base track or in any layer's scope, sorted.
    pub(crate) channel_names: BTreeSet<&'a str>,
}

impl<'a> BoundClip<'a> {
    /// Run binding validation (SPEC.md section 9, steps
    /// 11–13) and construct the bound clip.
    ///
    /// Step 11: the presented def must be the def the clip
    /// names, and its document bytes must hash to
    /// `skeleton.sha256` (`TMC-204`). Step 12: every bone in
    /// a base track resolves against the def (`TMC-203`);
    /// every bone in a layer scope resolves (`TMC-602`).
    /// Step 13: the same for channels against the rig's
    /// declarations.
    pub fn bind(
        clip: &'a Clip,
        skeleton: &'a dyn SkeletonDef,
        channels: &'a dyn ChannelDecls,
    ) -> Result<BoundClip<'a>, ClipError> {
        if skeleton.def_token() != clip.skeleton.def {
            return Err(ClipError::SkeletonHashMismatch {
                expected: clip.skeleton.sha256.clone(),
                found: sha256_hex(skeleton.document_bytes()),
            });
        }
        let found = sha256_hex(skeleton.document_bytes());
        if found != clip.skeleton.sha256 {
            return Err(ClipError::SkeletonHashMismatch {
                expected: clip.skeleton.sha256.clone(),
                found,
            });
        }
        let mut body_tracks = BTreeMap::new();
        let mut face_tracks = BTreeMap::new();
        let mut channel_names = BTreeSet::new();
        for group in &clip.groups {
            match group {
                Group::Body { tracks } => {
                    for track in tracks {
                        if skeleton.bone(&track.bone).is_none() {
                            return Err(ClipError::UnresolvedReference {
                                name: track.bone.clone(),
                            });
                        }
                        body_tracks.insert(track.bone.as_str(), track);
                    }
                }
                Group::Face { tracks } => {
                    for track in tracks {
                        if channels.channel(&track.channel).is_none() {
                            return Err(ClipError::UnresolvedReference {
                                name: track.channel.clone(),
                            });
                        }
                        face_tracks.insert(track.channel.as_str(), track);
                        channel_names.insert(track.channel.as_str());
                    }
                }
                Group::Camera { .. } | Group::Hands => {}
            }
        }
        for layer in &clip.layers {
            for bone in &layer.scope.bones {
                if skeleton.bone(bone).is_none() {
                    return Err(ClipError::ScopeUnresolved {
                        layer: layer.id.clone(),
                        name: bone.clone(),
                    });
                }
            }
            for channel in &layer.scope.channels {
                if channels.channel(channel).is_none() {
                    return Err(ClipError::ScopeUnresolved {
                        layer: layer.id.clone(),
                        name: channel.clone(),
                    });
                }
                channel_names.insert(channel.as_str());
            }
        }
        // Layer track targets are inside their scopes
        // (document validation, TMC-601) and the scopes
        // resolved above, so no further reference check is
        // needed for them; the assertion documents the
        // invariant the two stages compose into.
        debug_assert!(clip.layers.iter().all(|layer| {
            layer.tracks.iter().all(|track| match track {
                LayerTrack::Bone(track) => layer.scope.bones.contains(&track.bone),
                LayerTrack::Channel(track) => layer.scope.channels.contains(&track.channel),
            })
        }));
        Ok(BoundClip {
            clip,
            skeleton,
            channels,
            body_tracks,
            face_tracks,
            channel_names,
        })
    }

    /// The clip this bound form evaluates.
    pub fn clip(&self) -> &'a Clip {
        self.clip
    }
}
