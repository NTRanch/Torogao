//! The expression/arousal parameter system (toon-layer
//! commission brief section 3; gap G1's missing half).
//!
//! Design law (brief section 3.1):
//! - A stage is a parameter vector, not a baked face. The
//!   S0..S4 ladder is a versioned data table
//!   (`data/expr-stages.json`, format
//!   `torogao-render3d/expr-stages/1`) of named vectors
//!   over the parameter set; any intermediate vector is
//!   renderable.
//! - Parameters compose simultaneously. Per-girl signature
//!   tells are per-girl vector offsets over the shared
//!   set, never new parameters.
//! - Every parameter names its moving mechanism (Estate
//!   Rule 10): the exact morph targets or shader uniforms
//!   it drives, recorded in [`MECHANISMS`]. Morph bindings
//!   resolve by target name against whatever model is
//!   loaded, so the set is character-independent; a
//!   parameter whose mechanism a rig does not carry is
//!   reported unsupported by [`support`], never faked.
//!
//! The one partial mechanism on the pilot rig:
//! `pupil_dilate` drives `Fac_Pupil_Shrink` for the
//! constriction half (below neutral). No pupil-grow
//! target exists in Prima's panel (census: the pupil
//! meshes carry Heart / Shrink / Squash only), so the
//! dilation half is recorded unsupported in
//! [`MECHANISMS`] and resolves to no weight — the brief's
//! own note for this parameter asks that constriction be
//! reachable, and it is.
//!
//! Determinism: resolution is pure arithmetic over the
//! vector and the model's target names. No clock, no
//! randomness, no iteration-order dependence (bindings
//! are fixed tables, applied in declaration order).

use crate::error::RenderError;
use crate::model::Model;
use crate::toon::ToonSettings;

/// The parameter names, in declaration order. Every
/// vector, table and report uses these exact strings.
pub const PARAM_NAMES: [&str; 13] = [
    "eyelid_aperture",
    "pupil_dilate",
    "pupil_shape",
    "eye_gloss",
    "tear_sheen",
    "brow_raise",
    "brow_angle",
    "mouth_open",
    "mouth_shape",
    "blush_intensity",
    "blush_spread",
    "skin_flush",
    "skin_sheen",
];

/// One expression state: every parameter set at once,
/// each continuous on [0, 1]. Per-parameter neutral
/// conventions (what value reproduces the authored rest
/// face) are stated on the fields; the stage table's S0
/// is the composed rest state, and [`ExprVector::NEUTRAL`]
/// is the probe base (mechanisms at their rest weights).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ExprVector {
    /// 1 = fully open (rest), 0 = closed. Drives the
    /// Fac_Eye_HalfClose / Fac_Eye_Close morphs.
    pub eyelid_aperture: f32,
    /// 0.5 = neutral pupil. Below neutral drives
    /// Fac_Pupil_Shrink (constriction); above neutral is
    /// unsupported on rigs without a pupil-grow target.
    pub pupil_dilate: f32,
    /// 0 = round pupil, 1 = full heart. Drives
    /// Fac_Pupil_Heart where the rig carries it.
    pub pupil_shape: f32,
    /// Eye-material specular scale, mapped
    /// 0.3 + 1.7 * v (dull at 0, wet/bright at 1).
    pub eye_gloss: f32,
    /// Added white glint lobe on the sclera and
    /// EyeShadowFilm material IDs.
    pub tear_sheen: f32,
    /// 0.5 = authored rest brow height. Above drives
    /// Fac_Ebr_Up, below drives Fac_Ebr_Down.
    pub brow_raise: f32,
    /// 0.5 = neutral brow angle. Above drives
    /// Fac_Ebr_Sad (inner-brow raise, the pleading
    /// read), below drives Fac_Ebr_Angry.
    pub brow_angle: f32,
    /// Jaw/aperture continuum. Drives Fac_Mth_Open.
    pub mouth_open: f32,
    /// Shape axis with a rest band: 0 = full
    /// smile-corner (Fac_Mth_Smile + corner raises),
    /// the [0.3, 0.45] band is the authored rest mouth
    /// (no shape weight), 0.55 = round (Fac_Mth_Oo),
    /// 1 = shout (Fac_Mth_Shout, carrying Fac_Mth_Aa).
    pub mouth_shape: f32,
    /// Face-SDF G-mask blush mix strength (the shader's
    /// blush strength uniform, replacing the pre-rung-1
    /// hard-coded 0.55 factor's role at render time).
    pub blush_intensity: f32,
    /// Blush mask gain remap, 2^(2v-1): low values keep
    /// only the strongest painted blush (cheek-local),
    /// high values lift the faint mask (nose/ears).
    pub blush_spread: f32,
    /// Body-skin (material ID 0) albedo mix toward the
    /// flush colour. The face flushes through
    /// blush_intensity instead; the two never move
    /// together by default.
    pub skin_flush: f32,
    /// Skin specular scale, mapped 1.0 + 1.2 * v
    /// (sweat sheen; graded, never an on/off wet look).
    pub skin_sheen: f32,
}

impl ExprVector {
    /// The probe base: every mechanism at its rest
    /// weight (all bound morph weights resolve to 0,
    /// uniform mappings at their authored rest values).
    pub const NEUTRAL: ExprVector = ExprVector {
        eyelid_aperture: 1.0,
        pupil_dilate: 0.5,
        pupil_shape: 0.0,
        eye_gloss: 0.5,
        tear_sheen: 0.0,
        brow_raise: 0.5,
        brow_angle: 0.5,
        mouth_open: 0.0,
        mouth_shape: 0.375,
        blush_intensity: 0.55,
        blush_spread: 0.5,
        skin_flush: 0.0,
        skin_sheen: 0.0,
    };

    /// Read one parameter by its [`PARAM_NAMES`] string.
    pub fn get(&self, name: &str) -> Option<f32> {
        Some(match name {
            "eyelid_aperture" => self.eyelid_aperture,
            "pupil_dilate" => self.pupil_dilate,
            "pupil_shape" => self.pupil_shape,
            "eye_gloss" => self.eye_gloss,
            "tear_sheen" => self.tear_sheen,
            "brow_raise" => self.brow_raise,
            "brow_angle" => self.brow_angle,
            "mouth_open" => self.mouth_open,
            "mouth_shape" => self.mouth_shape,
            "blush_intensity" => self.blush_intensity,
            "blush_spread" => self.blush_spread,
            "skin_flush" => self.skin_flush,
            "skin_sheen" => self.skin_sheen,
            _ => return None,
        })
    }

    /// Write one parameter by name; unknown names are a
    /// typed error, never a silent no-op.
    pub fn set(&mut self, name: &str, value: f32) -> Result<(), RenderError> {
        match name {
            "eyelid_aperture" => self.eyelid_aperture = value,
            "pupil_dilate" => self.pupil_dilate = value,
            "pupil_shape" => self.pupil_shape = value,
            "eye_gloss" => self.eye_gloss = value,
            "tear_sheen" => self.tear_sheen = value,
            "brow_raise" => self.brow_raise = value,
            "brow_angle" => self.brow_angle = value,
            "mouth_open" => self.mouth_open = value,
            "mouth_shape" => self.mouth_shape = value,
            "blush_intensity" => self.blush_intensity = value,
            "blush_spread" => self.blush_spread = value,
            "skin_flush" => self.skin_flush = value,
            "skin_sheen" => self.skin_sheen = value,
            _ => {
                return Err(RenderError::Document(format!(
                    "unknown expression parameter {name:?}"
                )));
            }
        }
        Ok(())
    }

    /// Every parameter finite and inside [0, 1]: a vector
    /// is render input, so an out-of-range value is a
    /// typed error at authoring time.
    pub fn validate(&self) -> Result<(), RenderError> {
        for name in PARAM_NAMES {
            let v = self.get(name).expect("declared parameter");
            if !v.is_finite() || !(0.0..=1.0).contains(&v) {
                return Err(RenderError::Document(format!(
                    "expression parameter {name} = {v} outside [0, 1]"
                )));
            }
        }
        Ok(())
    }

    /// Component-wise interpolation; the intermediate
    /// vectors the ladder renders between stages.
    pub fn lerp(a: &ExprVector, b: &ExprVector, t: f32) -> ExprVector {
        let mut out = *a;
        for name in PARAM_NAMES {
            let av = a.get(name).expect("declared parameter");
            let bv = b.get(name).expect("declared parameter");
            out.set(name, av + (bv - av) * t)
                .expect("declared parameter");
        }
        out
    }
}

/// A weight curve over a parameter value: the named
/// shapes the morph bindings use. Evaluated in f32;
/// every curve maps [0, 1] into [0, 1].
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Curve {
    /// w = v.
    Direct,
    /// w = 1 - |2v - 1| (peaks at the midpoint).
    Tent,
    /// w = max(0, 1 - 2v) (full at 0, gone by midpoint).
    BelowHalf,
    /// w = max(0, 2v - 1) (gone until midpoint, full at 1).
    AboveHalf,
    /// w = max(0, (0.5 - v) * 2) (the below-neutral half
    /// of a neutral-at-0.5 parameter).
    BelowMid,
    /// w = max(0, (v - 0.5) * 2) (the above-neutral half).
    AboveMid,
    /// w = k * inner(v).
    Scaled(f32, &'static Curve),
    /// The mouth-shape smile zone: full at 0, gone by
    /// 0.3; the [0.3, 0.45] band above it is rest.
    SmileZone,
    /// The mouth-shape round zone: a tent over
    /// [0.45, 0.65], peaking at 0.55.
    OoZone,
    /// The mouth-shape shout zone: rises from 0.65,
    /// full at 1.
    ShoutZone,
}

impl Curve {
    pub fn eval(&self, v: f32) -> f32 {
        match self {
            Curve::Direct => v,
            Curve::Tent => 1.0 - (2.0 * v - 1.0).abs(),
            Curve::BelowHalf => (1.0 - 2.0 * v).max(0.0),
            Curve::AboveHalf => (2.0 * v - 1.0).max(0.0),
            Curve::BelowMid => ((0.5 - v) * 2.0).max(0.0),
            Curve::AboveMid => ((v - 0.5) * 2.0).max(0.0),
            Curve::Scaled(k, inner) => k * inner.eval(v),
            Curve::SmileZone => (1.0 - v / 0.3).max(0.0),
            Curve::OoZone => {
                if (0.45..=0.65).contains(&v) {
                    1.0 - (v - 0.55).abs() / 0.1
                } else {
                    0.0
                }
            }
            Curve::ShoutZone => ((v - 0.65) / 0.35).max(0.0),
        }
        .clamp(0.0, 1.0)
    }
}

const TENT: Curve = Curve::Tent;
const BELOW_HALF: Curve = Curve::BelowHalf;
const BELOW_MID: Curve = Curve::BelowMid;
const ABOVE_MID: Curve = Curve::AboveMid;
const DIRECT: Curve = Curve::Direct;
const SMILE_ZONE: Curve = Curve::SmileZone;
const OO_ZONE: Curve = Curve::OoZone;
const SHOUT_ZONE: Curve = Curve::ShoutZone;
const SMILE_CORNER: Curve = Curve::Scaled(0.6, &SMILE_ZONE);
const SHOUT_AA: Curve = Curve::Scaled(0.6, &SHOUT_ZONE);

/// The morph bindings: parameter -> (morph target name,
/// weight curve). A binding applies to every mesh that
/// carries the named target — the estate panel carries
/// one shape key on every mesh it deforms, and the
/// channel binds all its addresses.
pub fn morph_bindings(param: &str) -> &'static [(&'static str, Curve)] {
    match param {
        "eyelid_aperture" => &[
            ("Fac_Eye_HalfClose", TENT),
            ("Fac_Eye_Close", BELOW_HALF),
        ],
        "pupil_dilate" => &[("Fac_Pupil_Shrink", BELOW_MID)],
        "pupil_shape" => &[("Fac_Pupil_Heart", DIRECT)],
        "brow_raise" => &[
            ("Fac_Ebr_Up", ABOVE_MID),
            ("Fac_Ebr_Down", BELOW_MID),
        ],
        "brow_angle" => &[
            ("Fac_Ebr_Sad", ABOVE_MID),
            ("Fac_Ebr_Angry", BELOW_MID),
        ],
        "mouth_open" => &[("Fac_Mth_Open", DIRECT)],
        "mouth_shape" => &[
            ("Fac_Mth_Smile", SMILE_ZONE),
            ("Fac_Mth_L_Up", SMILE_CORNER),
            ("Fac_Mth_R_Up", SMILE_CORNER),
            ("Fac_Mth_Oo", OO_ZONE),
            ("Fac_Mth_Shout", SHOUT_ZONE),
            ("Fac_Mth_Aa", SHOUT_AA),
        ],
        _ => &[],
    }
}

/// The mechanism record (Estate Rule 10): for each
/// parameter, what actually moves when it moves, in
/// words a probe can check. Morph mechanisms name their
/// targets; uniform mechanisms name the shader uniform
/// and the term it scales.
pub fn mechanism(param: &str) -> &'static str {
    match param {
        "eyelid_aperture" => {
            "morphs Fac_Eye_HalfClose (tent over aperture) + Fac_Eye_Close (below-half) on every mesh carrying them"
        }
        "pupil_dilate" => {
            "morph Fac_Pupil_Shrink for constriction below neutral; dilation above neutral unsupported (no pupil-grow target in the panel)"
        }
        "pupil_shape" => "morph Fac_Pupil_Heart on the pupil meshes",
        "eye_gloss" => {
            "shader uniform toon8.x: specular scale 0.3 + 1.7v on material IDs 5 (sclera/highlight) and 6 (iris/pupil)"
        }
        "tear_sheen" => {
            "shader uniform toon8.y: added white Blinn lobe (exponent 28) on material IDs 5 (sclera) and 11 (EyeShadowFilm)"
        }
        "brow_raise" => "morphs Fac_Ebr_Up (above neutral) / Fac_Ebr_Down (below neutral)",
        "brow_angle" => {
            "morphs Fac_Ebr_Sad (inner raise, above neutral) / Fac_Ebr_Angry (below neutral)"
        }
        "mouth_open" => "morph Fac_Mth_Open on the face plate, mouth line, teeth, tongue, oral cavity",
        "mouth_shape" => {
            "morph blend over Fac_Mth_Smile + corner raises (smile zone) / rest band / Fac_Mth_Oo (round zone) / Fac_Mth_Shout + Fac_Mth_Aa (shout zone)"
        }
        "blush_intensity" => {
            "shader uniform toon4.w: face-SDF G-mask blush mix strength on the face plate (material ID 1)"
        }
        "blush_spread" => {
            "shader uniform toon9.x: gain 2^(2v-1) remapping the face-SDF G blush mask before the mix"
        }
        "skin_flush" => {
            "shader uniform toon8.z (+ toon9.yzw flush colour): body-skin material ID 0 albedo mix toward the flush colour, capped at 0.6"
        }
        "skin_sheen" => {
            "shader uniform toon8.w: specular scale 1.0 + 1.2v on material IDs 0 (body skin) and 1 (face skin)"
        }
        _ => "unknown parameter",
    }
}

/// Whether one parameter's mechanism exists on a loaded
/// model, and how much of it.
#[derive(Debug, Clone, PartialEq)]
pub enum ParamSupport {
    /// Morph-driven; the count of (mesh, target)
    /// addresses the binding resolves to on this model.
    Morph { addresses: usize },
    /// Shader-uniform-driven; always available when the
    /// toon model is on.
    Uniform,
    /// Morph-driven but only part of the parameter's
    /// range has a mechanism on this rig.
    Partial { addresses: usize, unsupported: &'static str },
    /// No mechanism on this rig. The parameter resolves
    /// to no-ops and renders must not claim it moved.
    Unsupported { reason: &'static str },
}

/// The support report for a whole model: one entry per
/// parameter, in [`PARAM_NAMES`] order.
pub fn support(model: &Model) -> Vec<(&'static str, ParamSupport)> {
    PARAM_NAMES
        .iter()
        .map(|name| {
            let bindings = morph_bindings(name);
            if bindings.is_empty() {
                return (*name, ParamSupport::Uniform);
            }
            let mut addresses = 0_usize;
            for mesh in &model.meshes {
                for (target, _) in bindings {
                    if mesh.target_names.iter().any(|t| t == target) {
                        addresses += 1;
                    }
                }
            }
            if *name == "pupil_dilate" {
                return (
                    *name,
                    ParamSupport::Partial {
                        addresses,
                        unsupported: "dilation above neutral: no pupil-grow target in the panel",
                    },
                );
            }
            if addresses == 0 {
                return (
                    *name,
                    ParamSupport::Unsupported {
                        reason: "no mesh carries the bound morph targets",
                    },
                );
            }
            (*name, ParamSupport::Morph { addresses })
        })
        .collect()
}

/// A vector resolved against a model: the per-mesh morph
/// weights to draw with, and the toon settings carrying
/// the uniform-driven parameters.
#[derive(Debug, Clone)]
pub struct ResolvedExpr {
    /// One weight array per model mesh, in model order:
    /// the mesh's default weights with every bound target
    /// overwritten by its binding's curve value.
    pub mesh_weights: Vec<Vec<f32>>,
    /// The base settings with the uniform-driven
    /// parameters applied.
    pub settings: ToonSettings,
    /// Morph weights this resolution actually set,
    /// as (mesh name, target name, weight) — the probe
    /// record. Zero weights are included when a binding
    /// overwrote a nonzero default.
    pub applied: Vec<(String, String, f32)>,
}

/// Resolve a vector against a model. Pure: identical
/// inputs resolve identically.
pub fn resolve(
    model: &Model,
    base: &ToonSettings,
    vector: &ExprVector,
) -> Result<ResolvedExpr, RenderError> {
    vector.validate()?;
    let mut mesh_weights: Vec<Vec<f32>> = model
        .meshes
        .iter()
        .map(|m| m.default_weights.clone())
        .collect();
    let mut applied = Vec::new();
    for name in PARAM_NAMES {
        let bindings = morph_bindings(name);
        if bindings.is_empty() {
            continue;
        }
        let v = vector.get(name).expect("declared parameter");
        for (mesh_idx, mesh) in model.meshes.iter().enumerate() {
            for (target, curve) in bindings {
                if let Some(slot) = mesh.target_names.iter().position(|t| t == target) {
                    let w = curve.eval(v);
                    if mesh_weights[mesh_idx][slot] != w {
                        applied.push((mesh.name.clone(), (*target).to_string(), w));
                    }
                    mesh_weights[mesh_idx][slot] = w;
                }
            }
        }
    }
    let mut settings = *base;
    settings.blush_strength = vector.blush_intensity;
    settings.blush_spread = vector.blush_spread;
    settings.eye_spec_scale = 0.3 + 1.7 * vector.eye_gloss;
    settings.tear_sheen = vector.tear_sheen;
    settings.skin_flush = vector.skin_flush;
    settings.skin_spec_scale = 1.0 + 1.2 * vector.skin_sheen;
    settings.validate()?;
    Ok(ResolvedExpr {
        mesh_weights,
        settings,
        applied,
    })
}

/// One girl's per-stage offset lists: (parameter,
/// delta) pairs keyed by stage index.
pub type GirlStageOffsets = [Vec<(String, f32)>; 5];

/// The stage table (brief section 3.3): S0..S4 default
/// vectors plus per-girl offset vectors, as versioned
/// data. The canonical copy is `data/expr-stages.json`
/// in this crate; [`StageTable::embedded`] parses the
/// copy compiled into the binary from that same file,
/// and callers may load an explicit file instead.
#[derive(Debug, Clone)]
pub struct StageTable {
    stages: [ExprVector; 5],
    girls: Vec<(String, GirlStageOffsets)>,
}

/// Stage names in ladder order.
pub const STAGE_NAMES: [&str; 5] = ["S0", "S1", "S2", "S3", "S4"];

impl StageTable {
    /// Parse a table document (format
    /// `torogao-render3d/expr-stages/1`). Unknown
    /// parameters, stages, or out-of-range values are
    /// typed errors: a corrupt table must fail at load,
    /// never shade wrong silently.
    pub fn parse(text: &str) -> Result<StageTable, RenderError> {
        let doc: serde_json::Value =
            serde_json::from_str(text).map_err(|e| RenderError::Document(format!("expr stages: {e}")))?;
        if doc["format"] != "torogao-render3d/expr-stages/1" {
            return Err(RenderError::Document(format!(
                "expr stages format {:?}, want torogao-render3d/expr-stages/1",
                doc["format"]
            )));
        }
        let read_vector = |value: &serde_json::Value| -> Result<ExprVector, RenderError> {
            let mut v = ExprVector::NEUTRAL;
            let obj = value.as_object().ok_or_else(|| {
                RenderError::Document("expr stages: vector is not an object".to_string())
            })?;
            for (key, val) in obj {
                let f = val.as_f64().ok_or_else(|| {
                    RenderError::Document(format!("expr stages: {key} is not a number"))
                })? as f32;
                v.set(key, f)?;
            }
            v.validate()?;
            Ok(v)
        };
        let mut stages = [ExprVector::NEUTRAL; 5];
        for (i, name) in STAGE_NAMES.iter().enumerate() {
            stages[i] = read_vector(&doc["stages"][name])?;
        }
        let mut girls = Vec::new();
        if let Some(map) = doc["girls"].as_object() {
            for (girl, entry) in map {
                let mut per_stage: [Vec<(String, f32)>; 5] =
                    Default::default();
                for (i, name) in STAGE_NAMES.iter().enumerate() {
                    if let Some(offsets) = entry["offsets"][name].as_object() {
                        for (key, val) in offsets {
                            if !PARAM_NAMES.contains(&key.as_str()) {
                                return Err(RenderError::Document(format!(
                                    "expr stages: unknown offset parameter {key:?}"
                                )));
                            }
                            let f = val.as_f64().ok_or_else(|| {
                                RenderError::Document(format!(
                                    "expr stages: offset {key} is not a number"
                                ))
                            })? as f32;
                            if !(-1.0..=1.0).contains(&f) {
                                return Err(RenderError::Document(format!(
                                    "expr stages: offset {key} = {f} outside [-1, 1]"
                                )));
                            }
                            per_stage[i].push((key.clone(), f));
                        }
                    }
                }
                girls.push((girl.clone(), per_stage));
            }
        }
        Ok(StageTable { stages, girls })
    }

    /// The table compiled in from `data/expr-stages.json`.
    pub fn embedded() -> Result<StageTable, RenderError> {
        StageTable::parse(include_str!("../data/expr-stages.json"))
    }

    /// The vector for one stage and girl: the stage
    /// default plus the girl's offsets for that stage,
    /// clamped into [0, 1]. An unknown girl is a typed
    /// error; `None` gives the shared default.
    pub fn vector(&self, stage: usize, girl: Option<&str>) -> Result<ExprVector, RenderError> {
        if stage >= 5 {
            return Err(RenderError::Document(format!(
                "expr stage {stage} outside 0..=4"
            )));
        }
        let mut v = self.stages[stage];
        if let Some(girl) = girl {
            let entry = self
                .girls
                .iter()
                .find(|(name, _)| name == girl)
                .ok_or_else(|| {
                    RenderError::Document(format!("expr stages: unknown girl {girl:?}"))
                })?;
            for (key, offset) in &entry.1[stage] {
                let base = v.get(key).expect("offset parameter validated at parse");
                v.set(key, (base + offset).clamp(0.0, 1.0))?;
            }
        }
        Ok(v)
    }

    /// The vector at a fractional ladder position
    /// (0.0 = S0 .. 4.0 = S4): offsets applied at the
    /// two bracketing stages, then interpolated — the
    /// graded nuance between stages the brief requires.
    pub fn vector_at(&self, position: f32, girl: Option<&str>) -> Result<ExprVector, RenderError> {
        if !position.is_finite() || !(0.0..=4.0).contains(&position) {
            return Err(RenderError::Document(format!(
                "expr ladder position {position} outside [0, 4]"
            )));
        }
        let lo = position.floor() as usize;
        let hi = (lo + 1).min(4);
        let t = position - lo as f32;
        let a = self.vector(lo, girl)?;
        let b = self.vector(hi, girl)?;
        Ok(ExprVector::lerp(&a, &b, t))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Material, Mesh, Model, Node, Skin};

    fn mesh(name: &str, targets: &[&str]) -> Mesh {
        Mesh {
            name: name.to_string(),
            node: 0,
            material: 0,
            positions: vec![[0.0; 3]; 4],
            normals: vec![[0.0, 0.0, 1.0]; 4],
            color0: vec![[1.0; 4]; 4],
            color1: vec![[0.0; 4]; 4],
            has_color1: false,
            texcoord0: vec![[0.0; 2]; 4],
            texcoord1: vec![[0.0; 2]; 4],
            has_texcoord1: false,
            texcoord2: Vec::new(),
            has_texcoord2: false,
            toon_normal: Vec::new(),
            has_toon_normal: false,
            outline_normal: vec![[0.0, 0.0, 1.0]; 4],
            joints: vec![[0; 4]; 4],
            weights: vec![[1.0, 0.0, 0.0, 0.0]; 4],
            indices: vec![0, 1, 2],
            targets: Vec::new(),
            target_names: targets.iter().map(|t| t.to_string()).collect(),
            default_weights: vec![0.0; targets.len()],
        }
    }

    fn model() -> Model {
        Model {
            meshes: vec![
                mesh(
                    "FacePlate",
                    &[
                        "Fac_Eye_Close",
                        "Fac_Eye_HalfClose",
                        "Fac_Ebr_Up",
                        "Fac_Ebr_Down",
                        "Fac_Ebr_Sad",
                        "Fac_Ebr_Angry",
                        "Fac_Mth_Open",
                        "Fac_Mth_Smile",
                        "Fac_Mth_Oo",
                        "Fac_Mth_Shout",
                        "Fac_Mth_Aa",
                        "Fac_Mth_L_Up",
                        "Fac_Mth_R_Up",
                    ],
                ),
                mesh("Pupil", &["Fac_Pupil_Heart", "Fac_Pupil_Shrink"]),
                mesh("Body", &[]),
            ],
            nodes: Vec::<Node>::new(),
            skin: Skin {
                name: String::new(),
                joints: Vec::new(),
                inverse_bind: Vec::new(),
            },
            materials: Vec::<Material>::new(),
            albedos: Vec::new(),
        }
    }

    fn weight(res: &ResolvedExpr, model: &Model, mesh: &str, target: &str) -> f32 {
        let (mi, mesh) = model
            .meshes
            .iter()
            .enumerate()
            .find(|(_, m)| m.name == mesh)
            .expect("mesh");
        let slot = mesh
            .target_names
            .iter()
            .position(|t| t == target)
            .expect("target");
        res.mesh_weights[mi][slot]
    }

    // -- validation half --------------------------------

    #[test]
    fn neutral_vector_validates_and_resolves_to_rest() {
        let m = model();
        let res = resolve(&m, &ToonSettings::default(), &ExprVector::NEUTRAL).unwrap();
        // Every bound morph sits at its rest weight: the
        // resolution changed nothing from the defaults.
        assert!(res.applied.is_empty(), "applied: {:?}", res.applied);
    }

    #[test]
    fn eyelid_curves_hit_the_named_intermediates() {
        let m = model();
        let mut v = ExprVector::NEUTRAL;
        v.eyelid_aperture = 0.0;
        let res = resolve(&m, &ToonSettings::default(), &v).unwrap();
        assert_eq!(weight(&res, &m, "FacePlate", "Fac_Eye_Close"), 1.0);
        assert_eq!(weight(&res, &m, "FacePlate", "Fac_Eye_HalfClose"), 0.0);
        v.eyelid_aperture = 0.5;
        let res = resolve(&m, &ToonSettings::default(), &v).unwrap();
        assert_eq!(weight(&res, &m, "FacePlate", "Fac_Eye_Close"), 0.0);
        assert_eq!(weight(&res, &m, "FacePlate", "Fac_Eye_HalfClose"), 1.0);
        v.eyelid_aperture = 0.25;
        let res = resolve(&m, &ToonSettings::default(), &v).unwrap();
        assert_eq!(weight(&res, &m, "FacePlate", "Fac_Eye_Close"), 0.5);
        assert_eq!(weight(&res, &m, "FacePlate", "Fac_Eye_HalfClose"), 0.5);
    }

    #[test]
    fn brow_parameters_split_at_neutral() {
        let m = model();
        let mut v = ExprVector::NEUTRAL;
        v.brow_raise = 1.0;
        v.brow_angle = 0.0;
        let res = resolve(&m, &ToonSettings::default(), &v).unwrap();
        assert_eq!(weight(&res, &m, "FacePlate", "Fac_Ebr_Up"), 1.0);
        assert_eq!(weight(&res, &m, "FacePlate", "Fac_Ebr_Down"), 0.0);
        assert_eq!(weight(&res, &m, "FacePlate", "Fac_Ebr_Sad"), 0.0);
        assert_eq!(weight(&res, &m, "FacePlate", "Fac_Ebr_Angry"), 1.0);
    }

    #[test]
    fn mouth_shape_axis_zones_and_rest_band() {
        let m = model();
        let mut v = ExprVector::NEUTRAL;
        v.mouth_shape = 0.0;
        let res = resolve(&m, &ToonSettings::default(), &v).unwrap();
        assert_eq!(weight(&res, &m, "FacePlate", "Fac_Mth_Smile"), 1.0);
        assert_eq!(weight(&res, &m, "FacePlate", "Fac_Mth_Oo"), 0.0);
        assert_eq!(weight(&res, &m, "FacePlate", "Fac_Mth_Shout"), 0.0);
        // The rest band: no shape weight at all.
        v.mouth_shape = 0.375;
        let res = resolve(&m, &ToonSettings::default(), &v).unwrap();
        assert_eq!(weight(&res, &m, "FacePlate", "Fac_Mth_Smile"), 0.0);
        assert_eq!(weight(&res, &m, "FacePlate", "Fac_Mth_Oo"), 0.0);
        assert_eq!(weight(&res, &m, "FacePlate", "Fac_Mth_Shout"), 0.0);
        v.mouth_shape = 0.55;
        let res = resolve(&m, &ToonSettings::default(), &v).unwrap();
        assert_eq!(weight(&res, &m, "FacePlate", "Fac_Mth_Oo"), 1.0);
        v.mouth_shape = 1.0;
        let res = resolve(&m, &ToonSettings::default(), &v).unwrap();
        assert_eq!(weight(&res, &m, "FacePlate", "Fac_Mth_Shout"), 1.0);
        assert_eq!(weight(&res, &m, "FacePlate", "Fac_Mth_Aa"), 0.6);
        assert_eq!(weight(&res, &m, "FacePlate", "Fac_Mth_Smile"), 0.0);
    }

    #[test]
    fn uniform_mappings_land_where_the_shader_reads_them() {
        let m = model();
        let mut v = ExprVector::NEUTRAL;
        v.blush_intensity = 0.8;
        v.eye_gloss = 1.0;
        v.skin_sheen = 0.5;
        v.tear_sheen = 0.25;
        v.skin_flush = 0.4;
        let res = resolve(&m, &ToonSettings::default(), &v).unwrap();
        assert_eq!(res.settings.blush_strength, 0.8);
        assert!((res.settings.eye_spec_scale - 2.0).abs() < 1e-6);
        assert!((res.settings.skin_spec_scale - 1.6).abs() < 1e-6);
        assert_eq!(res.settings.tear_sheen, 0.25);
        assert_eq!(res.settings.skin_flush, 0.4);
    }

    #[test]
    fn embedded_stage_table_carries_prima_tells() {
        let table = StageTable::embedded().unwrap();
        let shared_s1 = table.vector(1, None).unwrap();
        let prima_s1 = table.vector(1, Some("prima")).unwrap();
        // Prima's heart pupils kindle at S1; the shared
        // default does not move the channel at all.
        assert_eq!(shared_s1.pupil_shape, 0.0);
        assert!((prima_s1.pupil_shape - 0.45).abs() < 1e-6);
        let prima_s3 = table.vector(3, Some("prima")).unwrap();
        assert_eq!(prima_s3.pupil_shape, 1.0);
        // Afterglow is not warming reversed: S4 gloss
        // stays near peak while aperture drops.
        let prima_s4 = table.vector(4, Some("prima")).unwrap();
        assert!(prima_s4.eye_gloss > prima_s1.eye_gloss);
        assert!(prima_s4.eyelid_aperture < shared_s1.eyelid_aperture);
        // Fractional positions interpolate.
        let mid = table.vector_at(1.5, Some("prima")).unwrap();
        assert!((mid.pupil_shape - 0.575).abs() < 1e-6);
    }

    // -- adversarial half -------------------------------

    #[test]
    fn out_of_range_vectors_are_typed_errors() {
        let m = model();
        let mut v = ExprVector::NEUTRAL;
        v.blush_intensity = 1.5;
        assert!(resolve(&m, &ToonSettings::default(), &v).is_err());
        v.blush_intensity = f32::NAN;
        assert!(resolve(&m, &ToonSettings::default(), &v).is_err());
        assert!(v.set("nonsense", 0.5).is_err());
    }

    #[test]
    fn pupil_dilate_reports_partial_and_only_constricts() {
        let m = model();
        let report = support(&m);
        let dilate = report
            .iter()
            .find(|(n, _)| *n == "pupil_dilate")
            .expect("dilate entry");
        assert!(matches!(dilate.1, ParamSupport::Partial { .. }));
        let mut v = ExprVector::NEUTRAL;
        v.pupil_dilate = 0.0;
        let res = resolve(&m, &ToonSettings::default(), &v).unwrap();
        assert_eq!(weight(&res, &m, "Pupil", "Fac_Pupil_Shrink"), 1.0);
        v.pupil_dilate = 1.0;
        let res = resolve(&m, &ToonSettings::default(), &v).unwrap();
        assert_eq!(weight(&res, &m, "Pupil", "Fac_Pupil_Shrink"), 0.0);
    }

    #[test]
    fn missing_targets_report_unsupported_not_faked() {
        let mut m = model();
        m.meshes[1].target_names.clear();
        m.meshes[1].default_weights.clear();
        let report = support(&m);
        let shape = report
            .iter()
            .find(|(n, _)| *n == "pupil_shape")
            .expect("shape entry");
        assert!(matches!(shape.1, ParamSupport::Unsupported { .. }));
    }

    #[test]
    fn corrupt_stage_tables_are_typed_errors() {
        assert!(StageTable::parse("{}").is_err());
        assert!(StageTable::parse(
            "{\"format\":\"torogao-render3d/expr-stages/1\",\"stages\":{\"S0\":{\"bogus\":0.5}}}"
        )
        .is_err());
        let table = StageTable::embedded().unwrap();
        assert!(table.vector(0, Some("nobody")).is_err());
        assert!(table.vector_at(4.5, None).is_err());
        assert!(table.vector_at(-0.5, None).is_err());
    }
}
