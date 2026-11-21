//! Cast-generic character inputs (rung 2, M1): any cast
//! girl's GLB + clip + def + channels loaded by slug, posed
//! by her own clip, and drawn character-only under the plate
//! camera and light rig.
//!
//! Contract. Inputs are untrusted files in one directory:
//! `<slug>.glb`, `<slug>.clip.json`, `<slug>-skeleton.json`,
//! `<slug>-channels.json` and `plate-camera.json`. Every
//! rejection is a typed [`RenderError`]; nothing here panics
//! on input. The Prima plate-arc path ([`crate::arc`]) is not
//! routed through this module and is unchanged by it.
//!
//! Two render-time choices are data, never defaults hidden in
//! code, and both are echoed into the bins' manifests: the
//! outfit plan (which outfit mesh draws from which plate
//! frame — each girl's show carries her own) and the bust
//! weight (`None` = the weight her GLB saves).

use std::path::Path;

use serde::Deserialize;
use torogao_motion_clip::{BoundClip, Clip};

use crate::backend::{DrawFrame, PackedScene, pack_scene};
use crate::error::{RenderError, read_file};
use crate::math::Mat4;
use crate::model::{Model, load_model};
use crate::pose::{ArcPose, PrimaChannels, PrimaSkeleton, mesh_weights, parse_clip};
use crate::setscene::{ArcScene, PLATE_FRAMES, convert_matrix, plate_rig_scene};

/// Longest accepted character slug, in bytes.
pub const SLUG_BYTES_MAX: usize = 32;
/// Most entries an outfit plan may carry.
pub const OUTFIT_PLAN_ENTRIES_MAX: usize = 8;
/// The morph target the bust weight addresses.
pub const BUST_TARGET_NAME: &str = "Body_Bust_Size";
/// Clip frames per plate frame (60 Hz clip, 24 fps plates).
const CLIP_FRAMES_PER_PLATE_FRAME_X2: u32 = 5;

/// Which outfit mesh a plan entry selects. The numbering is
/// the show's `outfit_plan` index.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutfitMesh {
    /// Plan index 1: the `_Outfit` mesh.
    Outfit,
    /// Plan index 2: the `_OutfitEnd` mesh.
    OutfitEnd,
}

impl OutfitMesh {
    fn suffix(self) -> &'static str {
        match self {
            OutfitMesh::Outfit => "_Outfit",
            OutfitMesh::OutfitEnd => "_OutfitEnd",
        }
    }
}

/// A validated outfit plan: entries sorted by start frame,
/// the first starting at plate frame 0.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutfitPlan {
    entries: Vec<(u32, OutfitMesh)>,
}

impl OutfitPlan {
    /// Parse `START:INDEX[,START:INDEX...]` (e.g. `0:1,90:2`).
    /// Rejected: empty plan, more than
    /// [`OUTFIT_PLAN_ENTRIES_MAX`] entries, a first start other
    /// than 0, starts not strictly increasing, a start outside
    /// the arc, an index other than 1 or 2.
    pub fn parse(text: &str) -> Result<OutfitPlan, RenderError> {
        let bad = |why: &str| RenderError::Document(format!("outfit plan {text:?}: {why}"));
        let mut entries: Vec<(u32, OutfitMesh)> = Vec::new();
        for part in text.split(',') {
            if entries.len() == OUTFIT_PLAN_ENTRIES_MAX {
                return Err(bad("too many entries"));
            }
            let (start_text, index_text) = part.split_once(':').ok_or_else(|| bad("no ':'"))?;
            let start: u32 = start_text.parse().map_err(|_| bad("start not a frame"))?;
            let mesh = match index_text {
                "1" => OutfitMesh::Outfit,
                "2" => OutfitMesh::OutfitEnd,
                _ => return Err(bad("index must be 1 or 2")),
            };
            if start >= PLATE_FRAMES {
                return Err(bad("start outside the arc"));
            }
            match entries.last() {
                None if start != 0 => return Err(bad("first entry must start at 0")),
                Some((previous, _)) if start <= *previous => {
                    return Err(bad("starts must strictly increase"));
                }
                _ => {}
            }
            entries.push((start, mesh));
        }
        Ok(OutfitPlan { entries })
    }

    /// The outfit mesh drawn on a plate frame.
    pub fn mesh_for_frame(&self, frame: u32) -> OutfitMesh {
        assert!(!self.entries.is_empty(), "a parsed plan is never empty");
        let mut selected = self.entries[0].1;
        for (start, mesh) in &self.entries {
            if *start <= frame {
                selected = *mesh;
            }
        }
        selected
    }

    /// The plan in its input form, for manifests.
    pub fn describe(&self) -> String {
        let parts: Vec<String> = self
            .entries
            .iter()
            .map(|(start, mesh)| {
                let index = match mesh {
                    OutfitMesh::Outfit => 1,
                    OutfitMesh::OutfitEnd => 2,
                };
                format!("{start}:{index}")
            })
            .collect();
        parts.join(",")
    }
}

/// What the caller chooses for a cast load.
#[derive(Debug, Clone)]
pub struct CastOptions {
    /// Character slug (`alma`, `lux`, ...): lowercase ASCII
    /// letters, at most [`SLUG_BYTES_MAX`] bytes.
    pub slug: String,
    /// Her show's outfit plan.
    pub outfit_plan: OutfitPlan,
    /// `Some(w)` overrides [`BUST_TARGET_NAME`] to `w` on every
    /// mesh that carries it; `None` uses the saved weights.
    pub bust_weight: Option<f32>,
}

/// The decoded census the gates compare against the fleet
/// census: counts only, no geometry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CastCensus {
    pub joints: usize,
    pub nodes: usize,
    /// Per mesh, in GLB order: name, vertices, morph targets.
    pub meshes: Vec<(String, usize, usize)>,
}

/// Everything a cast character render needs, loaded.
pub struct CastInputs {
    pub slug: String,
    pub model: Model,
    pub skeleton: PrimaSkeleton,
    pub channels: PrimaChannels,
    pub clip: Clip,
    pub pose: ArcPose,
    pub scene: ArcScene,
    pub outfit_plan: OutfitPlan,
    pub bust_weight: Option<f32>,
    /// SHA-256 of each input file, by file name.
    pub input_hashes: Vec<(String, String)>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PlateCameraJson {
    /// The plate camera's world matrix, Blender space, rows.
    camera_world_blender_rows: [[f64; 4]; 4],
}

fn validate_slug(slug: &str) -> Result<(), RenderError> {
    let charset_ok = slug.bytes().all(|byte| byte.is_ascii_lowercase());
    if slug.is_empty() || slug.len() > SLUG_BYTES_MAX || !charset_ok {
        return Err(RenderError::Document(format!(
            "character slug {slug:?}: need 1..={SLUG_BYTES_MAX} lowercase ASCII letters"
        )));
    }
    Ok(())
}

fn parse_plate_camera(bytes: &[u8]) -> Result<Mat4, RenderError> {
    let parsed: PlateCameraJson = serde_json::from_slice(bytes)
        .map_err(|e| RenderError::Document(format!("plate camera: {e}")))?;
    let rows = parsed.camera_world_blender_rows;
    if rows.iter().flatten().any(|value| !value.is_finite()) {
        return Err(RenderError::Document(
            "plate camera: non-finite matrix entry".to_string(),
        ));
    }
    Ok(convert_matrix(Mat4::from_rows(rows)))
}

/// The render-time choices must name things her GLB has.
fn validate_choices(model: &Model, options: &CastOptions) -> Result<(), RenderError> {
    for (_, mesh) in &options.outfit_plan.entries {
        if model.mesh_named(mesh.suffix()).is_none() {
            return Err(RenderError::Document(format!(
                "outfit plan names a mesh the GLB lacks: *{}",
                mesh.suffix()
            )));
        }
    }
    if let Some(weight) = options.bust_weight {
        if !weight.is_finite() {
            return Err(RenderError::Document(
                "bust weight is not finite".to_string(),
            ));
        }
        let carried = model
            .meshes
            .iter()
            .any(|mesh| mesh.target_names.iter().any(|n| n == BUST_TARGET_NAME));
        if !carried {
            return Err(RenderError::Missing(
                "Body_Bust_Size target for bust weight",
            ));
        }
    }
    Ok(())
}

/// Load a cast character's inputs from `dir`. Fails typed
/// on: a bad slug, any unreadable or unparsable file, a def
/// whose token is not `<slug>-rig/1`, a clip whose length is
/// not the 360-plate-frame arc, a def bone with no GLB node,
/// and render-time choices naming data the GLB lacks.
pub fn load_cast_inputs(dir: &Path, options: &CastOptions) -> Result<CastInputs, RenderError> {
    validate_slug(&options.slug)?;
    let slug = options.slug.as_str();
    let mut input_hashes = Vec::with_capacity(5);
    let mut read = |name: String| -> Result<Vec<u8>, RenderError> {
        let bytes = read_file(&dir.join(&name))?;
        input_hashes.push((name, crate::compare::sha256_hex(&bytes)));
        Ok(bytes)
    };
    let glb = read(format!("{slug}.glb"))?;
    let clip_bytes = read(format!("{slug}.clip.json"))?;
    let def_bytes = read(format!("{slug}-skeleton.json"))?;
    let channels_bytes = read(format!("{slug}-channels.json"))?;
    let camera_bytes = read("plate-camera.json".to_string())?;

    let model = load_model(&glb)?;
    let skeleton = PrimaSkeleton::from_bytes(def_bytes)?;
    let expected_token = format!("{slug}-rig/1");
    if torogao_motion_clip::SkeletonDef::def_token(&skeleton) != expected_token {
        return Err(RenderError::Document(format!(
            "skeleton def is not {expected_token}"
        )));
    }
    let channels = PrimaChannels::from_bytes(&channels_bytes)?;
    let clip = parse_clip(&clip_bytes)?;
    if clip.frames.checked_mul(2) != PLATE_FRAMES.checked_mul(CLIP_FRAMES_PER_PLATE_FRAME_X2) {
        return Err(RenderError::Document(format!(
            "clip carries {} frames, not the {PLATE_FRAMES}-plate-frame arc",
            clip.frames
        )));
    }
    validate_choices(&model, options)?;
    let pose = ArcPose::prepare(&skeleton, &model, &clip, PLATE_FRAMES)?;
    let scene = plate_rig_scene(parse_plate_camera(&camera_bytes)?);
    Ok(CastInputs {
        slug: options.slug.clone(),
        model,
        skeleton,
        channels,
        clip,
        pose,
        scene,
        outfit_plan: options.outfit_plan.clone(),
        bust_weight: options.bust_weight,
        input_hashes,
    })
}

impl CastInputs {
    /// Bind the clip (borrows the inputs). A clip whose
    /// recorded skeleton SHA-256 is not this def's fails here.
    pub fn bind(&self) -> Result<BoundClip<'_>, RenderError> {
        BoundClip::bind(&self.clip, &self.skeleton, &self.channels)
            .map_err(|e| RenderError::Document(format!("clip bind: {e}")))
    }

    /// The packed scene for the backend boundary.
    pub fn packed_scene(&self) -> PackedScene {
        pack_scene(&self.model, &self.scene)
    }

    /// Index of her `_Body` mesh.
    pub fn body_index(&self) -> Result<usize, RenderError> {
        self.model
            .meshes
            .iter()
            .position(|mesh| mesh.name.ends_with("_Body"))
            .ok_or(RenderError::Missing("_Body mesh"))
    }

    /// The decoded census (counts only).
    pub fn census(&self) -> CastCensus {
        CastCensus {
            joints: self.model.skin.joints.len(),
            nodes: self.model.nodes.len(),
            meshes: self
                .model
                .meshes
                .iter()
                .map(|mesh| (mesh.name.clone(), mesh.vertex_count(), mesh.targets.len()))
                .collect(),
        }
    }

    /// Per-mesh morph weights for a plate frame: clip
    /// channels by name, the bust weight as chosen, every
    /// other target at the weight the GLB saves.
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
                    self.bust_weight,
                )
            })
            .collect())
    }

    /// Which character meshes draw on a plate frame: the
    /// plan's outfit mesh, and every non-outfit mesh.
    pub fn visible_meshes(&self, frame: u32) -> Vec<bool> {
        let selected = self.outfit_plan.mesh_for_frame(frame);
        self.model
            .meshes
            .iter()
            .map(|mesh| {
                if mesh.name.ends_with("_OutfitEnd") {
                    selected == OutfitMesh::OutfitEnd
                } else if mesh.name.ends_with("_Outfit") {
                    selected == OutfitMesh::Outfit
                } else {
                    true
                }
            })
            .collect()
    }

    /// The character-only draw packet for one plate frame.
    /// The set never draws on this path.
    pub fn draw_frame(&self, bound: &BoundClip<'_>, frame: u32) -> Result<DrawFrame, RenderError> {
        let evaluated = self.pose.evaluate_plate_frame(bound, frame)?;
        let joint_matrices = self
            .pose
            .joint_matrices(&self.model, &evaluated)
            .iter()
            .map(|matrix| matrix.to_f32_cols())
            .collect();
        Ok(DrawFrame {
            frame,
            joint_matrices,
            mesh_weights: self.frame_weights(bound, frame)?,
            visible_meshes: self.visible_meshes(frame),
            include_set: false,
            toy_model: Mat4::IDENTITY,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_parses_and_selects_by_frame() {
        let plan = OutfitPlan::parse("0:1,90:2").expect("valid plan");
        assert_eq!(plan.mesh_for_frame(0), OutfitMesh::Outfit);
        assert_eq!(plan.mesh_for_frame(89), OutfitMesh::Outfit);
        assert_eq!(plan.mesh_for_frame(90), OutfitMesh::OutfitEnd);
        assert_eq!(plan.mesh_for_frame(359), OutfitMesh::OutfitEnd);
        assert_eq!(plan.describe(), "0:1,90:2");
    }

    #[test]
    fn single_entry_plan_holds_for_the_whole_arc() {
        let plan = OutfitPlan::parse("0:2").expect("valid plan");
        assert_eq!(plan.mesh_for_frame(0), OutfitMesh::OutfitEnd);
        assert_eq!(plan.mesh_for_frame(359), OutfitMesh::OutfitEnd);
    }

    #[test]
    fn slug_accepts_cast_names() {
        for slug in ["alma", "lux", "piper", "prima", "sloane", "yui"] {
            validate_slug(slug).expect("cast slug");
        }
    }

    #[test]
    fn plate_camera_parses_rows() {
        let text = br#"{"camera_world_blender_rows":
            [[1,0,0,1.5],[0,1,0,-2.5],[0,0,1,0.5],[0,0,0,1]]}"#;
        let camera = parse_plate_camera(text).expect("camera");
        // Blender (x, y, z) -> glTF (x, z, -y).
        let position = camera.translation();
        assert_eq!((position.x, position.y, position.z), (1.5, 0.5, 2.5));
    }

    fn mesh(name: &str, target_names: &[&str]) -> crate::model::Mesh {
        crate::model::Mesh {
            name: name.to_string(),
            node: 0,
            material: 0,
            positions: Vec::new(),
            normals: Vec::new(),
            color0: Vec::new(),
            color1: Vec::new(),
            has_color1: false,
            joints: Vec::new(),
            weights: Vec::new(),
            indices: Vec::new(),
            targets: Vec::new(),
            target_names: target_names.iter().map(|n| n.to_string()).collect(),
            default_weights: vec![0.0; target_names.len()],
        }
    }

    fn model(meshes: Vec<crate::model::Mesh>) -> Model {
        Model {
            meshes,
            nodes: Vec::new(),
            skin: crate::model::Skin {
                name: String::new(),
                joints: Vec::new(),
                inverse_bind: Vec::new(),
            },
            materials: Vec::new(),
        }
    }

    fn options(plan: &str, bust_weight: Option<f32>) -> CastOptions {
        CastOptions {
            slug: "alma".to_string(),
            outfit_plan: OutfitPlan::parse(plan).expect("valid plan"),
            bust_weight,
        }
    }

    #[test]
    fn choices_accept_what_the_glb_carries() {
        let cast = model(vec![
            mesh("Alma_Body", &["Fac_Eye_Close", BUST_TARGET_NAME]),
            mesh("Alma_Outfit", &[BUST_TARGET_NAME]),
            mesh("Alma_OutfitEnd", &[BUST_TARGET_NAME]),
        ]);
        validate_choices(&cast, &options("0:1,90:2", Some(1.0))).expect("all present");
        validate_choices(&cast, &options("0:2", None)).expect("saved weights");
    }

    #[test]
    fn choices_reject_an_outfit_the_glb_lacks() {
        // `_OutfitEnd` must not satisfy a plan asking for `_Outfit`.
        let cast = model(vec![mesh("Alma_Body", &[]), mesh("Alma_OutfitEnd", &[])]);
        assert!(validate_choices(&cast, &options("0:1", None)).is_err());
        assert!(validate_choices(&cast, &options("0:2,90:1", None)).is_err());
    }

    #[test]
    fn choices_reject_a_bust_weight_with_no_target_or_no_value() {
        let without = model(vec![mesh("Alma_Body", &["Fac_Eye_Close"])]);
        assert!(validate_choices(&without, &options("0:1", Some(1.0))).is_err());
        let with = model(vec![
            mesh("Alma_Body", &[BUST_TARGET_NAME]),
            mesh("Alma_Outfit", &[]),
        ]);
        assert!(validate_choices(&with, &options("0:1", Some(f32::NAN))).is_err());
        assert!(validate_choices(&with, &options("0:1", Some(f32::INFINITY))).is_err());
    }

    #[test]
    fn plan_rejects_malformed_text() {
        for text in [
            "",
            "0",
            "0:3",
            "0:0",
            "1:1",
            "0:1,0:2",
            "90:2,0:1",
            "0:1,360:2",
            "a:1",
            "0:1,",
            "0:1;90:2",
            "-1:1",
        ] {
            assert!(OutfitPlan::parse(text).is_err(), "accepted {text:?}");
        }
    }

    #[test]
    fn plan_rejects_too_many_entries() {
        let text: Vec<String> = (0..=OUTFIT_PLAN_ENTRIES_MAX)
            .map(|i| format!("{i}:1"))
            .collect();
        assert!(OutfitPlan::parse(&text.join(",")).is_err());
    }

    #[test]
    fn slug_rejects_paths_and_noise() {
        let too_long = "a".repeat(SLUG_BYTES_MAX + 1);
        for slug in [
            "",
            "Alma",
            "../alma",
            "al ma",
            "alma.glb",
            "alma1",
            too_long.as_str(),
        ] {
            assert!(validate_slug(slug).is_err(), "accepted {slug:?}");
        }
    }

    #[test]
    fn plate_camera_rejects_bad_documents() {
        for text in [
            &b"{}"[..],
            br#"{"camera_world_blender_rows":[[1,0,0,0]]}"#,
            br#"{"camera_world_blender_rows":[[1,0,0,0],[0,1,0,0],[0,0,1,0],[0,0,0,1]],"x":1}"#,
            br#"{"camera_world_blender_rows":[[1e999,0,0,0],[0,1,0,0],[0,0,1,0],[0,0,0,1]]}"#,
            b"not json",
        ] {
            assert!(parse_plate_camera(text).is_err());
        }
    }
}
