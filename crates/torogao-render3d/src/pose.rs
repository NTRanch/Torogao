//! Deterministic pose evaluation for the arc: the
//! sanctioned `torogao-motion-clip` evaluator, bound to
//! Prima's skeleton def and channel declarations. This
//! module writes NO second evaluator — it binds, evaluates,
//! and turns evaluated frames into joint matrices and morph
//! weights.
//!
//! Frame mapping (host policy, per the clip format's
//! section 7.4): the arc plates run at 24 fps over 360
//! frames; the clip runs at the canonical 60 Hz over 900
//! frames. Plate frame `k` sits at clip time `2.5 * k`.
//! Between evaluated clip frames the host policy is slerp
//! for rotations, lerp for translations, scales and channel
//! values. At the comparison sample frames (every 6th plate
//! frame) the clip time is an exact integer key, so the
//! numeric plate comparison carries no interpolation at
//! all.

use std::collections::BTreeMap;

use serde::Deserialize;
use torogao_motion_clip::{
    BoneDef, BoundClip, ChannelDecls, ChannelDef, Clip, EvaluatedFrame, SkeletonDef, parse_document,
};

use crate::error::RenderError;
use crate::math::{Mat4, Quat};
use crate::model::Model;

/// Prima's skeleton def (`prima-skeleton.json`, the document
/// whose SHA-256 the clip binds against) as a `SkeletonDef`.
pub struct PrimaSkeleton {
    bytes: Vec<u8>,
    token: String,
    bones: Vec<BoneDef>,
    /// GLB node name per def bone, in def order.
    pub glb_names: Vec<String>,
}

#[derive(Deserialize)]
struct SkeletonJson {
    def: String,
    bones: Vec<String>,
    rest: BTreeMap<String, RestJson>,
    glb_bone_names: BTreeMap<String, String>,
}

#[derive(Deserialize)]
struct RestJson {
    translation: [f64; 3],
    rotation: [f64; 4],
    scale: [f64; 3],
}

impl PrimaSkeleton {
    /// Parse the def document; the raw bytes are retained
    /// verbatim because binding hashes them.
    pub fn from_bytes(bytes: Vec<u8>) -> Result<PrimaSkeleton, RenderError> {
        let parsed: SkeletonJson = serde_json::from_slice(&bytes)
            .map_err(|e| RenderError::Document(format!("skeleton def: {e}")))?;
        let mut bones = Vec::with_capacity(parsed.bones.len());
        let mut glb_names = Vec::with_capacity(parsed.bones.len());
        for name in &parsed.bones {
            let rest = parsed
                .rest
                .get(name)
                .ok_or_else(|| RenderError::Document(format!("def rest missing bone {name}")))?;
            let glb = parsed
                .glb_bone_names
                .get(name)
                .ok_or_else(|| RenderError::Document(format!("def glb name missing {name}")))?;
            bones.push(BoneDef {
                name: name.clone(),
                rest_translation: rest.translation,
                rest_rotation: rest.rotation,
                rest_scale: rest.scale,
            });
            glb_names.push(glb.clone());
        }
        Ok(PrimaSkeleton {
            bytes,
            token: parsed.def,
            bones,
            glb_names,
        })
    }
}

impl SkeletonDef for PrimaSkeleton {
    fn def_token(&self) -> &str {
        &self.token
    }

    fn document_bytes(&self) -> &[u8] {
        &self.bytes
    }

    fn bones(&self) -> &[BoneDef] {
        &self.bones
    }
}

/// Channel declarations for the arc clip: the channels the
/// show drives (`prima-channels.json`), each declared
/// `default 0, range [0, 1]` (the estate's scalar-channel
/// convention). The replay-hash test arbitrates this choice:
/// if the clip of record carried a value outside the range
/// its clamping would change the hash.
pub struct PrimaChannels {
    decls: BTreeMap<String, ChannelDef>,
}

impl PrimaChannels {
    /// The channels document is a flat array of channel
    /// names (the channels the show drives).
    pub fn from_bytes(bytes: &[u8]) -> Result<PrimaChannels, RenderError> {
        let parsed: Vec<String> = serde_json::from_slice(bytes)
            .map_err(|e| RenderError::Document(format!("channels: {e}")))?;
        let mut decls = BTreeMap::new();
        for name in parsed {
            decls.insert(
                name,
                ChannelDef {
                    default: 0.0,
                    min: 0.0,
                    max: 1.0,
                },
            );
        }
        Ok(PrimaChannels { decls })
    }

    pub fn count(&self) -> usize {
        self.decls.len()
    }
}

impl ChannelDecls for PrimaChannels {
    fn channel(&self, name: &str) -> Option<ChannelDef> {
        self.decls.get(name).copied()
    }
}

/// A bound, ready-to-evaluate arc.
pub struct ArcPose {
    /// Plate frames in the arc (360).
    pub plate_frames: u32,
    clip_frames: u32,
    /// Def bone index -> GLB node index, per def bone.
    bone_nodes: Vec<usize>,
    /// Node indices in evaluation walk order (parents first).
    walk_order: Vec<usize>,
    /// Per def bone: whether the clip's body group keys a
    /// base translation track for the bone. See
    /// `joint_matrices` for the binding rule this feeds.
    translation_keyed: Vec<bool>,
}

/// Parse a clip document (the caller's `Clip` must outlive
/// the `BoundClip` built from it).
pub fn parse_clip(clip_bytes: &[u8]) -> Result<Clip, RenderError> {
    parse_document(clip_bytes).map_err(|e| RenderError::Document(format!("clip parse: {e}")))
}

impl ArcPose {
    /// Resolve every def bone to its GLB node and precompute
    /// the parents-first node walk. Any missing node is a
    /// hard error here, not a surprise at render time.
    pub fn prepare(
        skeleton: &PrimaSkeleton,
        model: &Model,
        clip: &torogao_motion_clip::Clip,
        plate_frames: u32,
    ) -> Result<ArcPose, RenderError> {
        let clip_frames = clip.frames;
        let mut bone_nodes = Vec::with_capacity(skeleton.glb_names.len());
        for glb_name in &skeleton.glb_names {
            let node = model
                .node_named(glb_name)
                .ok_or_else(|| RenderError::Document(format!("GLB has no node {glb_name}")))?;
            bone_nodes.push(node);
        }
        let mut translation_keyed = vec![false; skeleton.glb_names.len()];
        for group in &clip.groups {
            if let torogao_motion_clip::Group::Body { tracks } = group {
                for track in tracks {
                    if track.translation.is_some() {
                        if let Some(idx) = skeleton
                            .bones()
                            .iter()
                            .position(|bone| bone.name == track.bone)
                        {
                            translation_keyed[idx] = true;
                        }
                    }
                }
            }
        }
        // Parents-first walk of the node forest.
        let mut walk_order = Vec::with_capacity(model.nodes.len());
        let mut state = vec![0_u8; model.nodes.len()];
        fn visit(
            node: usize,
            nodes: &[crate::model::Node],
            state: &mut [u8],
            out: &mut Vec<usize>,
        ) {
            if state[node] != 0 {
                return;
            }
            state[node] = 1;
            if let Some(parent) = nodes[node].parent {
                visit(parent, nodes, state, out);
            }
            out.push(node);
        }
        for node in 0..model.nodes.len() {
            visit(node, &model.nodes, &mut state, &mut walk_order);
        }
        Ok(ArcPose {
            plate_frames,
            clip_frames,
            bone_nodes,
            walk_order,
            translation_keyed,
        })
    }

    /// Evaluate the clip at a plate frame, applying the
    /// host interpolation policy between clip frames.
    pub fn evaluate_plate_frame(
        &self,
        bound: &BoundClip<'_>,
        plate_frame: u32,
    ) -> Result<EvaluatedFrame, RenderError> {
        if plate_frame >= self.plate_frames {
            return Err(RenderError::FrameOutOfRange {
                frame: plate_frame,
                frames: self.plate_frames,
            });
        }
        // Clip time = plate_frame * clip_frames / plate_frames
        // (900/360 = 2.5 exactly for this arc).
        let t = f64::from(plate_frame) * f64::from(self.clip_frames) / f64::from(self.plate_frames);
        let f0 = t.floor() as u32;
        let alpha = t - f64::from(f0);
        let frame0 = bound
            .evaluate_frame(f0)
            .map_err(|e| RenderError::Document(format!("evaluate {f0}: {e:?}")))?;
        if alpha == 0.0 || f0 + 1 >= self.clip_frames {
            return Ok(frame0);
        }
        let frame1 = bound
            .evaluate_frame(f0 + 1)
            .map_err(|e| RenderError::Document(format!("evaluate {}: {e:?}", f0 + 1)))?;
        Ok(blend_frames(&frame0, &frame1, alpha))
    }

    /// Joint matrices (world * inverse-bind), in skin order,
    /// for an evaluated frame. f64 throughout.
    ///
    /// Binding rule for translations (measured, rung 1):
    /// the arc clip's one base translation track (Hips)
    /// carries POSE-BONE OFFSETS, not absolute locals —
    /// `gen_clip.py` copied the show's `hips_loc` verbatim,
    /// and the plate renderer (`anim.py`) assigns those
    /// values to the pose bone's `location`, which Blender
    /// defines as an offset from the bone's rest position.
    /// Read as an absolute local, the hips lands exactly
    /// one rest-translation (0.9134 m) low on every frame
    /// (witnessed against Blender's evaluated vertices).
    /// So: a bone WITH a base translation track composes
    /// `node_local + evaluated`; a bone without one takes
    /// the evaluated TRS as-is (the evaluator already
    /// yields the rest translation there). Rotations and
    /// scales take the evaluated values as-is in both
    /// cases. This is a binding-layer rule of this
    /// renderer, NOT a change to the shared evaluator; a
    /// format-level ruling (bake absolute values in
    /// gen_clip, or spec the offset reading) is carried as
    /// a REVIEW for the format owners.
    pub fn joint_matrices(&self, model: &Model, frame: &EvaluatedFrame) -> Vec<Mat4> {
        // Local transforms: GLB rest everywhere, evaluated
        // TRS on the joint nodes.
        let mut locals: Vec<Mat4> = model.nodes.iter().map(|n| n.local).collect();
        for (bone_idx, (_, pose)) in frame.bones.iter().enumerate() {
            let node = self.bone_nodes[bone_idx];
            let translation = if self.translation_keyed[bone_idx] {
                let base = model.nodes[node].local.translation();
                [
                    base.x + pose.translation[0],
                    base.y + pose.translation[1],
                    base.z + pose.translation[2],
                ]
            } else {
                pose.translation
            };
            locals[node] = Mat4::from_trs(translation, pose.rotation, pose.scale);
        }
        let mut worlds = vec![Mat4::IDENTITY; model.nodes.len()];
        for &node in &self.walk_order {
            worlds[node] = match model.nodes[node].parent {
                Some(parent) => worlds[parent].multiply(locals[node]),
                None => locals[node],
            };
        }
        model
            .skin
            .joints
            .iter()
            .enumerate()
            .map(|(joint, &node)| worlds[node].multiply(model.skin.inverse_bind[joint]))
            .collect()
    }
}

/// Blend two evaluated frames (host interpolation policy):
/// slerp rotations, lerp everything else. Both frames carry
/// the same bone/channel sets by construction of the clip.
pub fn blend_frames(a: &EvaluatedFrame, b: &EvaluatedFrame, alpha: f64) -> EvaluatedFrame {
    assert_eq!(a.bones.len(), b.bones.len(), "bone sets must match");
    let bones = a
        .bones
        .iter()
        .zip(b.bones.iter())
        .map(|((name, pa), (_, pb))| {
            let rotation = Quat::slerp(
                Quat::from_array(pa.rotation),
                Quat::from_array(pb.rotation),
                alpha,
            );
            (
                name.clone(),
                torogao_motion_clip::BonePose {
                    translation: lerp3(pa.translation, pb.translation, alpha),
                    rotation: [rotation.x, rotation.y, rotation.z, rotation.w],
                    scale: lerp3(pa.scale, pb.scale, alpha),
                },
            )
        })
        .collect();
    let channels = a
        .channels
        .iter()
        .zip(b.channels.iter())
        .map(|((name, va), (_, vb))| (name.clone(), va + (vb - va) * alpha))
        .collect();
    EvaluatedFrame { bones, channels }
}

fn lerp3(a: [f64; 3], b: [f64; 3], t: f64) -> [f64; 3] {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
    ]
}

/// Per-mesh morph weights for an evaluated frame: channel
/// values by name; `Body_Bust_Size` is overridden by the
/// caller (the m100 plates were rendered at bust weight
/// 0.0, while the GLB saves 1.0 — RECORD.md, D-1152);
/// channels the frame does not carry read as 0.
pub fn mesh_weights(
    target_names: &[String],
    default_weights: &[f32],
    channels: &[(String, f64)],
    bust_override: Option<f32>,
) -> Vec<f32> {
    target_names
        .iter()
        .enumerate()
        .map(|(i, name)| {
            if name == "Body_Bust_Size" {
                if let Some(override_value) = bust_override {
                    return override_value;
                }
            }
            if let Some((_, value)) = channels.iter().find(|(n, _)| n == name) {
                return *value as f32;
            }
            default_weights.get(i).copied().unwrap_or(0.0)
        })
        .collect()
}
