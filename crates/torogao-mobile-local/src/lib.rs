//! Phone-local Torogao — the trial build's offline engine.
//!
//! Everything in this crate runs on the handset: character
//! records (`torogao-character/1`) are created, edited, saved,
//! and exported on-device; expression morphs, the bust body
//! morph, and joint poses are applied on-device to a bundled
//! canonical-base GLB; and the result is previewed on-device by
//! a Tier-2 (f32) software rasteriser. There is no network
//! code, no seat client, and no primo anywhere in this crate —
//! that absence is the trial's whole point.
//!
//! What this crate deliberately does NOT do: rig *derivation*
//! (solving a new base from parameters is Blender-class work
//! and stays a primo builder job — the record it would consume
//! is exactly the record this crate edits), and art generation
//! (probed separately; see the trial record).

#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod glb;
pub mod hash;
pub mod math;
pub mod record;
pub mod render;
pub mod store;

pub use glb::{BaseMesh, GlbError};
pub use record::{CharacterRecord, RecordError};
pub use render::{Camera, PoseMap, PreviewImage, PreviewStats, RenderError};
pub use store::{LocalStore, StoreError};

use std::path::Path;

/// The bundled canonical base: Prima's gated fleet GLB,
/// embedded so the trial build needs no download and no seat.
///
/// Provenance: `prima.glb`, sha256
/// `8cafa7ae32372319970a174cbadd9d48b82c7b0efa424cacd159a938970a8dfd`,
/// from `torogao-models-20261104` (the same artefact the primo
/// pipeline gates on; 22,340 verts, 54-joint Mixamo skin, the
/// full 36-name face morph set plus `Body_Bust_Size`).
pub const BASE_GLB: &[u8] = include_bytes!("../assets/prima-base.glb");

/// Which preview framing to render.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreviewView {
    /// Full body.
    Body,
    /// Face close-up (framed on the head joint).
    Face,
}

/// Engine-level failures.
#[derive(Debug)]
pub enum LocalError {
    /// The bundled base failed to parse.
    Glb(GlbError),
    /// The record store failed.
    Store(StoreError),
}

impl std::fmt::Display for LocalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LocalError::Glb(e) => write!(f, "{e}"),
            LocalError::Store(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for LocalError {}

impl From<GlbError> for LocalError {
    fn from(e: GlbError) -> Self {
        LocalError::Glb(e)
    }
}

impl From<StoreError> for LocalError {
    fn from(e: StoreError) -> Self {
        LocalError::Store(e)
    }
}

/// One editable parameter row: the shell renders these as
/// sliders; the engine owns the mapping to record fields.
#[derive(Debug, Clone, PartialEq)]
pub struct ParamDef {
    /// What the slider writes.
    pub key: ParamKey,
    /// Group heading in the UI ("Expression", "Body", "Pose").
    pub group: &'static str,
    /// Row label.
    pub label: String,
    /// Current value from the record.
    pub value: f32,
    /// Slider minimum.
    pub min: f32,
    /// Slider maximum.
    pub max: f32,
}

/// The record field a [`ParamDef`] writes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParamKey {
    /// An expression morph by target name.
    Morph(String),
    /// The `Body_Bust_Size` morph, edited as a body knob
    /// (stored in the record's expressions, per the schema:
    /// it is a morph the builder emits).
    Bust,
    /// `body_params.z_scale` (preview root scale).
    Height,
    /// One Euler axis of one joint's pose override. Pose is
    /// preview state, never record data.
    Pose(String, usize),
}

/// The curated pose axes the trial exposes: head pitch/turn/
/// tilt, arm raises, elbow bends. (Full pose authoring is a
/// rig-editor surface; the trial proves the mechanism.)
const POSE_AXES: &[(&str, usize, &str)] = &[
    ("mixamorig:Head", 0, "Head pitch"),
    ("mixamorig:Head", 1, "Head turn"),
    ("mixamorig:Head", 2, "Head tilt"),
    ("mixamorig:LeftArm", 2, "Left arm raise"),
    ("mixamorig:RightArm", 2, "Right arm raise"),
    ("mixamorig:LeftForeArm", 0, "Left elbow bend"),
    ("mixamorig:RightForeArm", 0, "Right elbow bend"),
];

/// The offline engine: parsed base mesh + record store.
pub struct LocalEngine {
    mesh: BaseMesh,
    store: LocalStore,
}

impl LocalEngine {
    /// Open the engine over the app's private data dir: parse
    /// the bundled base, open the store, and seed the Prima
    /// template record on first run.
    pub fn open(data_dir: &Path) -> Result<Self, LocalError> {
        let mesh = glb::load_glb(BASE_GLB)?;
        let store = LocalStore::open(data_dir)?;
        if store.list().is_empty() {
            store.save(&record::prima_template())?;
        }
        Ok(Self { mesh, store })
    }

    /// The parsed base mesh.
    pub fn mesh(&self) -> &BaseMesh {
        &self.mesh
    }

    /// The record store.
    pub fn store(&self) -> &LocalStore {
        &self.store
    }

    /// The parameter rows for a record and the current
    /// preview pose, in UI order: expressions (declaration
    /// order), body, pose.
    pub fn params(&self, record: &CharacterRecord, pose: &PoseMap) -> Vec<ParamDef> {
        let weights = record.morph_weights();
        let mut out = Vec::new();
        for name in &self.mesh.morph_names {
            if name == "Body_Bust_Size" {
                continue;
            }
            out.push(ParamDef {
                key: ParamKey::Morph(name.clone()),
                group: "Expression",
                label: name.strip_prefix("Fac_").unwrap_or(name).replace('_', " "),
                value: weights.get(name).copied().unwrap_or(0.0),
                min: 0.0,
                max: 1.0,
            });
        }
        out.push(ParamDef {
            key: ParamKey::Bust,
            group: "Body",
            label: "Bust size".to_string(),
            value: record.expression_weight("Body_Bust_Size"),
            min: 0.0,
            max: 1.0,
        });
        out.push(ParamDef {
            key: ParamKey::Height,
            group: "Body",
            label: "Height (z-scale)".to_string(),
            value: record.z_scale(),
            min: 0.90,
            max: 1.15,
        });
        for (joint, axis, label) in POSE_AXES {
            if self.mesh.joint_index(joint).is_none() {
                continue;
            }
            let value = pose.get(*joint).map(|e| e[*axis]).unwrap_or(0.0);
            out.push(ParamDef {
                key: ParamKey::Pose((*joint).to_string(), *axis),
                group: "Pose",
                label: (*label).to_string(),
                value,
                min: -45.0,
                max: 45.0,
            });
        }
        out
    }

    /// Write one parameter value. Record keys write the
    /// record (near-zero expression values remove the entry,
    /// keeping saved records sparse and diffs honest); pose
    /// keys write the preview pose map only — pose is never
    /// record data.
    pub fn apply_param(
        record: &mut CharacterRecord,
        pose: &mut PoseMap,
        key: &ParamKey,
        value: f32,
    ) {
        match key {
            ParamKey::Morph(name) => record.set_expression(name, value),
            ParamKey::Bust => record.set_expression("Body_Bust_Size", value),
            ParamKey::Height => {
                record.body_params.insert("z_scale".to_string(), value);
            }
            ParamKey::Pose(joint, axis) => {
                let entry = pose.entry(joint.clone()).or_insert([0.0; 3]);
                entry[*axis] = value;
                if entry.iter().all(|v| v.abs() < 0.01) {
                    pose.remove(joint);
                }
            }
        }
    }

    /// Render a preview of a record under a preview pose,
    /// fully on-device.
    pub fn preview(
        &self,
        record: &CharacterRecord,
        pose: &PoseMap,
        view: PreviewView,
        width: u32,
        height: u32,
    ) -> Result<(PreviewImage, PreviewStats), RenderError> {
        let camera = match view {
            PreviewView::Body => self.body_camera(),
            PreviewView::Face => self.face_camera(record, pose)?,
        };
        render::render(&self.mesh, record, pose, &camera, width, height)
    }

    /// Full-body framing from the base's bind-pose bounds.
    pub fn body_camera(&self) -> Camera {
        let (min, max) = self.bounds();
        let height = max[1] - min[1];
        let cx = (min[0] + max[0]) * 0.5;
        let cz = (min[2] + max[2]) * 0.5;
        let target = [cx, min[1] + height * 0.52, cz];
        Camera {
            eye: [
                cx + height * 0.16,
                min[1] + height * 0.58,
                cz + height * 1.9,
            ],
            target,
            up: [0.0, 1.0, 0.0],
            fov_y_deg: 38.0,
        }
    }

    /// Face framing on the head joint (bind pose if the joint
    /// is missing — which validation would have refused first
    /// for any pose that names it).
    pub fn face_camera(
        &self,
        record: &CharacterRecord,
        pose: &PoseMap,
    ) -> Result<Camera, RenderError> {
        let (min, max) = self.bounds();
        let height = max[1] - min[1];
        let head = render::joint_world_position(&self.mesh, record, pose, "mixamorig:Head")?
            .unwrap_or([0.0, min[1] + height * 0.93, 0.0]);
        Ok(Camera {
            eye: [
                head[0] + height * 0.03,
                head[1] + height * 0.035,
                head[2] + height * 0.30,
            ],
            target: [head[0], head[1] - height * 0.01, head[2]],
            up: [0.0, 1.0, 0.0],
            fov_y_deg: 34.0,
        })
    }

    fn bounds(&self) -> ([f32; 3], [f32; 3]) {
        let mut min = [f32::INFINITY; 3];
        let mut max = [f32::NEG_INFINITY; 3];
        for prim in &self.mesh.prims {
            for p in &prim.positions {
                for i in 0..3 {
                    min[i] = min[i].min(p[i]);
                    max[i] = max[i].max(p[i]);
                }
            }
        }
        (min, max)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apply_param_bookkeeping() {
        // The engine is exercised against the real bundled base
        // in tests/trial.rs; here: apply_param bookkeeping —
        // expressions land in the record, pose lands in the
        // pose map and never in the record.
        let mut rec = record::prima_template();
        let mut pose = PoseMap::new();
        LocalEngine::apply_param(
            &mut rec,
            &mut pose,
            &ParamKey::Morph("Fac_Mth_Smile".to_string()),
            0.8,
        );
        assert_eq!(rec.expression_weight("Fac_Mth_Smile"), 0.8);
        LocalEngine::apply_param(
            &mut rec,
            &mut pose,
            &ParamKey::Morph("Fac_Mth_Smile".to_string()),
            0.0,
        );
        assert_eq!(rec.expression_weight("Fac_Mth_Smile"), 0.0);
        assert!(rec.expressions.is_empty());
        LocalEngine::apply_param(
            &mut rec,
            &mut pose,
            &ParamKey::Pose("mixamorig:Head".to_string(), 1),
            20.0,
        );
        assert_eq!(pose.get("mixamorig:Head"), Some(&[0.0, 20.0, 0.0]));
        LocalEngine::apply_param(
            &mut rec,
            &mut pose,
            &ParamKey::Pose("mixamorig:Head".to_string(), 1),
            0.0,
        );
        assert!(!pose.contains_key("mixamorig:Head"));
    }
}
