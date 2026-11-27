//! The Prima arc, assembled: model, clip, def, channels,
//! probe, and scene, loaded from the rung's inputs
//! directory, and the per-frame draw packets the bins hand
//! to the backend. This is the assembly the proof renders
//! from — nothing in here invents data; every input is a
//! staged file with a recorded hash (the bins manifest
//! them).

use std::path::Path;

use torogao_motion_clip::{BoundClip, Clip};

use crate::backend::{DrawFrame, PackedScene, pack_scene};
use crate::error::{RenderError, read_file};
use crate::model::{Model, load_model};
use crate::pose::{ArcPose, PrimaChannels, PrimaSkeleton, mesh_weights, parse_clip};
use crate::setscene::{
    ArcScene, OutfitSelection, PLATE_FRAMES, ProbeData, outfit_for_frame, prima_arc_scene,
};

/// The bust weight the m100 plates were rendered at
/// (RECORD.md: canon frames at m = 1.00 ⇒ `Body_Bust_Size`
/// weight 0.0; the GLB saves 1.0 for the m* = 1.50 canon).
pub const PLATE_BUST_WEIGHT: f32 = 0.0;

/// Everything the arc needs, loaded.
pub struct ArcInputs {
    pub model: Model,
    pub skeleton: PrimaSkeleton,
    pub channels: PrimaChannels,
    pub clip: Clip,
    pub probe: ProbeData,
    pub scene: ArcScene,
    pub pose: ArcPose,
    /// SHA-256 of each input file, by file name.
    pub input_hashes: Vec<(String, String)>,
}

/// Load the arc inputs from a directory holding
/// `prima.glb`, `prima.clip.json`, `prima-skeleton.json`,
/// `prima-channels.json` and `scene-probe.json`.
pub fn load_arc_inputs(dir: &Path) -> Result<ArcInputs, RenderError> {
    let read = |name: &str| -> Result<(Vec<u8>, String), RenderError> {
        let bytes = read_file(&dir.join(name))?;
        let hash = crate::compare::sha256_hex(&bytes);
        Ok((bytes, hash))
    };
    let (glb, h_glb) = read("prima.glb")?;
    let (clip_bytes, h_clip) = read("prima.clip.json")?;
    let (def_bytes, h_def) = read("prima-skeleton.json")?;
    let (channels_bytes, h_channels) = read("prima-channels.json")?;
    let (probe_bytes, h_probe) = read("scene-probe.json")?;

    let model = load_model(&glb)?;
    let skeleton = PrimaSkeleton::from_bytes(def_bytes)?;
    let channels = PrimaChannels::from_bytes(&channels_bytes)?;
    let clip = parse_clip(&clip_bytes)?;
    let probe = ProbeData::from_bytes(&probe_bytes)?;
    let pose = ArcPose::prepare(&skeleton, &model, &clip, PLATE_FRAMES)?;
    let scene = prima_arc_scene(&probe);
    Ok(ArcInputs {
        model,
        skeleton,
        channels,
        clip,
        probe,
        scene,
        pose,
        input_hashes: vec![
            ("prima.glb".to_string(), h_glb),
            ("prima.clip.json".to_string(), h_clip),
            ("prima-skeleton.json".to_string(), h_def),
            ("prima-channels.json".to_string(), h_channels),
            ("scene-probe.json".to_string(), h_probe),
        ],
    })
}

impl ArcInputs {
    /// Bind the clip (borrows the inputs; the returned
    /// BoundClip must not outlive them).
    pub fn bind(&self) -> Result<BoundClip<'_>, RenderError> {
        BoundClip::bind(&self.clip, &self.skeleton, &self.channels)
            .map_err(|e| RenderError::Document(format!("clip bind: {e}")))
    }

    /// The packed scene for the backend boundary.
    pub fn packed_scene(&self) -> PackedScene {
        pack_scene(&self.model, &self.scene)
    }

    /// Joint matrices in f32 (skin order) for a plate
    /// frame.
    pub fn joint_matrices_f32(
        &self,
        bound: &BoundClip<'_>,
        frame: u32,
    ) -> Result<Vec<[f32; 16]>, RenderError> {
        let evaluated = self.pose.evaluate_plate_frame(bound, frame)?;
        Ok(self
            .pose
            .joint_matrices(&self.model, &evaluated)
            .iter()
            .map(|m| m.to_f32_cols())
            .collect())
    }

    /// Per-mesh morph weights for a plate frame, with the
    /// plate bust override applied.
    pub fn frame_weights(
        &self,
        bound: &BoundClip<'_>,
        frame: u32,
    ) -> Result<Vec<Vec<f32>>, RenderError> {
        let evaluated = self.pose.evaluate_plate_frame(bound, frame)?;
        Ok(self
            .model
            .meshes
            .iter()
            .map(|mesh| {
                mesh_weights(
                    &mesh.target_names,
                    &mesh.default_weights,
                    &evaluated.channels,
                    Some(PLATE_BUST_WEIGHT),
                )
            })
            .collect())
    }

    /// Assemble the draw packet for one plate frame.
    pub fn draw_frame(
        &self,
        bound: &BoundClip<'_>,
        frame: u32,
        include_set: bool,
    ) -> Result<DrawFrame, RenderError> {
        Ok(DrawFrame {
            frame,
            joint_matrices: self.joint_matrices_f32(bound, frame)?,
            mesh_weights: self.frame_weights(bound, frame)?,
            visible_meshes: visible_meshes(&self.model, frame),
            include_set,
            toy_model: self.probe.toy_matrices[frame as usize],
        })
    }
}

/// Which character meshes draw on a plate frame: Body and
/// Hair always; the two outfit meshes follow the show's
/// outfit plan.
pub fn visible_meshes(model: &Model, frame: u32) -> Vec<bool> {
    let selection = outfit_for_frame(frame);
    model
        .meshes
        .iter()
        .map(|mesh| {
            if mesh.name.ends_with("_OutfitEnd") {
                selection == OutfitSelection::OutfitEnd
            } else if mesh.name.ends_with("_Outfit") {
                selection == OutfitSelection::Outfit
            } else {
                true
            }
        })
        .collect()
}
