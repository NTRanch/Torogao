//! The named parameter layer over the frozen builder spec.
//!
//! The record's `rig.derivation_spec` is the frozen base
//! document (the per-girl builder spec of record). The
//! parameters here are the phone-editable knobs over it, and
//! [`derive_spec`] is the ONE function that applies them —
//! client-side, deterministically. The primo adapter receives
//! the derived spec inside the job document, verifies its
//! hash, and builds exactly that document; it never applies
//! knobs itself. Same record revision ⇒ same derived spec ⇒
//! same GLB hash, or the build is a defect (D-1397).
//!
//! Knobs are absolute, not cumulative: they apply over the
//! base spec every time, so editing a knob twice does not
//! compound. v1 knob definitions are deliberately narrow —
//! the section-band scalings below name the exact rows they
//! touch, in the raw (pre-`z_scale`) frame the spec is
//! authored in.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// Torso-section rows at or below this raw-frame z are the
/// waist band (fleet spec geometry: torso sections descend
/// 1.452 → 0.94; the waist rows are 0.99 and 0.94).
const WAIST_BAND_MAX_Z: f64 = 1.00;
/// Pelvis-section rows inside this raw-frame z band carry
/// the hip circumference (pelvis sections span 0.97 → 0.782;
/// the hip rows are 0.905–0.825).
const HIP_BAND_MIN_Z: f64 = 0.80;
/// See [`HIP_BAND_MIN_Z`].
const HIP_BAND_MAX_Z: f64 = 0.92;

/// Body parameters: the phone-editable body knobs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BodyParams {
    /// Vertical scale of the whole build (builder `z_scale`);
    /// standing body height scales with it.
    pub z_scale: f64,
    /// Radial scale of the whole build (builder
    /// `radial_scale`).
    pub radial_scale: f64,
    /// Bust gaussian amplitude (builder `bust.amp`), metres
    /// of added projection at the bust row.
    pub bust_amp: f64,
    /// Multiplier on the waist-band torso rows' radii.
    pub waist_scale: f64,
    /// Multiplier on the hip-band pelvis rows' radii.
    pub hip_scale: f64,
    /// Builder hair style (`hair_style`): one of the styles
    /// the fleet builder implements.
    pub hair_style: String,
}

impl Default for BodyParams {
    fn default() -> Self {
        Self {
            z_scale: 1.0,
            radial_scale: 1.0,
            bust_amp: 0.052,
            waist_scale: 1.0,
            hip_scale: 1.0,
            hair_style: "waves".to_string(),
        }
    }
}

/// Hair styles the fleet builder implements (its `build_hair`
/// dispatch); anything else is refused at validation rather
/// than silently building the default.
pub const HAIR_STYLES: &[&str] = &[
    "waves",
    "hime",
    "bob",
    "bob_messy",
    "ponytail",
    "ponytail_high",
    "braid",
    "scraped",
    "curls",
];

impl BodyParams {
    /// Validate the knobs against their bounds.
    pub fn validate(&self) -> Vec<String> {
        let mut problems = Vec::new();
        let mut check = |field: &str, v: f64, lo: f64, hi: f64| {
            if !v.is_finite() || !(lo..=hi).contains(&v) {
                problems.push(format!("body param {field} = {v} is outside {lo}..={hi}"));
            }
        };
        check("z_scale", self.z_scale, 0.90, 1.15);
        check("radial_scale", self.radial_scale, 0.85, 1.15);
        check("bust_amp", self.bust_amp, 0.0, 0.09);
        check("waist_scale", self.waist_scale, 0.85, 1.15);
        check("hip_scale", self.hip_scale, 0.85, 1.15);
        if !HAIR_STYLES.contains(&self.hair_style.as_str()) {
            problems.push(format!(
                "hair_style {:?} is not one of the builder's styles",
                self.hair_style
            ));
        }
        problems
    }
}

/// Face parameters: the phone-editable face knobs, mirroring
/// the builder spec's `face` block fields of record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FaceParams {
    /// Eye centre |x| from the midline, metres (raw frame).
    pub eye_x: f64,
    /// Eye centre height, metres (raw frame).
    pub eye_z: f64,
    /// Mouth centre height, metres (raw frame).
    pub mouth_z: f64,
    /// Brow height, metres (raw frame).
    pub brow_z: f64,
    /// Iris radius, metres.
    pub iris_r: f64,
    /// Pupil radius, metres.
    pub pupil_r: f64,
}

impl Default for FaceParams {
    fn default() -> Self {
        // The fleet party spec's face block of record.
        Self {
            eye_x: 0.0325,
            eye_z: 1.578,
            mouth_z: 1.514,
            brow_z: 1.604,
            iris_r: 0.0062,
            pupil_r: 0.003,
        }
    }
}

impl FaceParams {
    /// Validate the knobs against their bounds.
    pub fn validate(&self) -> Vec<String> {
        let mut problems = Vec::new();
        let mut check = |field: &str, v: f64, lo: f64, hi: f64| {
            if !v.is_finite() || !(lo..=hi).contains(&v) {
                problems.push(format!("face param {field} = {v} is outside {lo}..={hi}"));
            }
        };
        check("eye_x", self.eye_x, 0.026, 0.040);
        check("eye_z", self.eye_z, 1.54, 1.62);
        check("mouth_z", self.mouth_z, 1.47, 1.55);
        check("brow_z", self.brow_z, 1.57, 1.64);
        check("iris_r", self.iris_r, 0.004, 0.009);
        check("pupil_r", self.pupil_r, 0.0015, 0.005);
        problems
    }
}

fn num(v: f64) -> Value {
    serde_json::Number::from_f64(v).map_or(Value::Null, Value::Number)
}

fn scale_band(
    spec: &mut Map<String, Value>,
    key: &str,
    in_band: impl Fn(f64) -> bool,
    factor: f64,
) {
    let Some(rows) = spec.get_mut(key).and_then(Value::as_array_mut) else {
        return;
    };
    for row in rows.iter_mut() {
        let Some(cells) = row.as_array_mut() else {
            continue;
        };
        let Some(z) = cells.first().and_then(Value::as_f64) else {
            continue;
        };
        if !in_band(z) {
            continue;
        }
        for idx in [1, 2] {
            if let Some(cell) = cells.get_mut(idx) {
                if let Some(r) = cell.as_f64() {
                    *cell = num(r * factor);
                }
            }
        }
    }
}

/// Apply the parameter layer over a base builder spec,
/// producing the derived spec a derive job builds. Pure and
/// total over validated inputs: same base + same params ⇒
/// same derived spec, byte for byte after canonical
/// serialisation.
///
/// # Errors
/// Returns the problems (base spec malformed, or a parameter
/// out of bounds) when the derivation cannot be stated.
pub fn derive_spec(
    base: &Value,
    body: &BodyParams,
    face: &FaceParams,
) -> Result<Value, Vec<String>> {
    let mut problems = body.validate();
    problems.extend(face.validate());
    let Some(mut spec) = base.as_object().cloned() else {
        problems.push("base derivation spec is not a JSON object".to_string());
        return Err(problems);
    };
    if !problems.is_empty() {
        return Err(problems);
    }
    spec.insert("z_scale".to_string(), num(body.z_scale));
    spec.insert("radial_scale".to_string(), num(body.radial_scale));
    spec.insert(
        "hair_style".to_string(),
        Value::String(body.hair_style.clone()),
    );
    match spec.get_mut("bust").and_then(Value::as_object_mut) {
        Some(bust) => {
            bust.insert("amp".to_string(), num(body.bust_amp));
        }
        None => return Err(vec!["base spec has no bust object".to_string()]),
    }
    match spec.get_mut("face").and_then(Value::as_object_mut) {
        Some(f) => {
            f.insert("eye_x".to_string(), num(face.eye_x));
            f.insert("eye_z".to_string(), num(face.eye_z));
            f.insert("mouth_z".to_string(), num(face.mouth_z));
            f.insert("brow_z".to_string(), num(face.brow_z));
            f.insert("iris_r".to_string(), num(face.iris_r));
            f.insert("pupil_r".to_string(), num(face.pupil_r));
        }
        None => return Err(vec!["base spec has no face object".to_string()]),
    }
    if body.waist_scale != 1.0 {
        scale_band(
            &mut spec,
            "torso_sections",
            |z| z <= WAIST_BAND_MAX_Z,
            body.waist_scale,
        );
    }
    if body.hip_scale != 1.0 {
        scale_band(
            &mut spec,
            "pelvis_sections",
            |z| (HIP_BAND_MIN_Z..=HIP_BAND_MAX_Z).contains(&z),
            body.hip_scale,
        );
    }
    Ok(Value::Object(spec))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn base_spec() -> Value {
        json!({
            "character": "Fixture",
            "z_scale": 1.0,
            "radial_scale": 1.0,
            "hair_style": "waves",
            "bust": {"amp": 0.052, "sigma": 0.044, "z": 1.235},
            "face": {"eye_x": 0.0325, "eye_z": 1.578, "mouth_z": 1.514,
                     "brow_z": 1.604, "iris_r": 0.0062, "pupil_r": 0.003,
                     "eyeball_r": 0.0128},
            "torso_sections": [[1.235, 0.150, 0.078, 0.0], [0.99, 0.100, 0.060, 0.0], [0.94, 0.098, 0.058, 0.0]],
            "pelvis_sections": [[0.97, 0.120, 0.070, 0.0], [0.86, 0.130, 0.075, 0.0], [0.782, 0.110, 0.065, 0.0]]
        })
    }

    // ---- validation ----

    #[test]
    fn defaults_validate() {
        assert!(BodyParams::default().validate().is_empty());
        assert!(FaceParams::default().validate().is_empty());
    }

    #[test]
    fn neutral_params_reproduce_the_base_spec() {
        let base = base_spec();
        let derived = derive_spec(&base, &BodyParams::default(), &FaceParams::default())
            .expect("neutral derivation");
        assert_eq!(derived, base);
    }

    #[test]
    fn derivation_is_deterministic() {
        let body = BodyParams {
            z_scale: 1.02,
            waist_scale: 0.95,
            hip_scale: 1.05,
            ..BodyParams::default()
        };
        let a = derive_spec(&base_spec(), &body, &FaceParams::default()).expect("a");
        let b = derive_spec(&base_spec(), &body, &FaceParams::default()).expect("b");
        assert_eq!(a, b);
        assert_eq!(a.to_string(), b.to_string());
    }

    // ---- adversarial ----

    #[test]
    fn knobs_move_only_their_own_fields() {
        let body = BodyParams {
            z_scale: 1.02,
            waist_scale: 0.95,
            hip_scale: 1.05,
            ..BodyParams::default()
        };
        let d = derive_spec(&base_spec(), &body, &FaceParams::default()).expect("derive");
        assert_eq!(d["z_scale"], json!(1.02));
        // Bust row (z 1.235) is above the waist band: untouched.
        assert_eq!(d["torso_sections"][0][1], json!(0.150));
        // Waist rows scaled.
        assert_eq!(d["torso_sections"][1][1], json!(0.100 * 0.95));
        assert_eq!(d["torso_sections"][2][2], json!(0.058 * 0.95));
        // Pelvis top row (0.97) is above the hip band: untouched;
        // the 0.86 row is scaled; the 0.782 row is below it.
        assert_eq!(d["pelvis_sections"][0][1], json!(0.120));
        assert_eq!(d["pelvis_sections"][1][1], json!(0.130 * 1.05));
        assert_eq!(d["pelvis_sections"][2][1], json!(0.110));
    }

    #[test]
    fn out_of_bounds_knobs_are_refused() {
        let body = BodyParams {
            z_scale: 3.0,
            ..BodyParams::default()
        };
        assert!(derive_spec(&base_spec(), &body, &FaceParams::default()).is_err());
        let face = FaceParams {
            eye_x: 0.5,
            ..FaceParams::default()
        };
        assert!(derive_spec(&base_spec(), &BodyParams::default(), &face).is_err());
    }

    #[test]
    fn a_malformed_base_is_refused() {
        assert!(
            derive_spec(
                &json!([1, 2]),
                &BodyParams::default(),
                &FaceParams::default()
            )
            .is_err()
        );
        assert!(
            derive_spec(
                &json!({"face": {}}),
                &BodyParams::default(),
                &FaceParams::default()
            )
            .is_err()
        );
    }

    #[test]
    fn an_unknown_hair_style_is_refused() {
        let body = BodyParams {
            hair_style: "mohawk".to_string(),
            ..BodyParams::default()
        };
        assert!(!body.validate().is_empty());
    }
}
