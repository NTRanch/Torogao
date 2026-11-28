//! The rig gates: Step 0 eligibility, A10.1/A10.2 verdicts,
//! and the identity-lock check.
//!
//! The verdict logic lives exactly here and runs facade-side
//! over the estate instruments' raw output — the primo
//! adapter executes the instruments and returns their JSON
//! untouched, the phone displays verdicts, neither re-derives
//! them. (The content screen's pattern: the same rule, in the
//! same code, on both sides of the wire.)
//!
//! Gate order is the design (ARCHITECTURE §5): a record with
//! no Step 0 identity lock cannot derive. Art origination is
//! not gated by the lock and is not this module's business.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::schema::{CharacterRecord, LockProportions, LockStatus, is_sha256_hex};

/// A10.1 tolerance of record: each battery pose's achieved
/// skeletal angle must land within this many degrees of the
/// applied angle (MoonDeep rigs RECORD: targets ±2°).
pub const BATTERY_TOLERANCE_DEG: f64 = 2.0;

/// Lock-check tolerance: a derivation's measured proportions
/// must land within this many metres of the locked values.
pub const LOCK_TOLERANCE_M: f64 = 0.01;

/// A gate's verdict.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Verdict {
    /// The gate passed.
    Pass,
    /// The gate ran and failed.
    Fail,
    /// The gate has not run.
    NotRun,
}

/// The Step 0 measurements the A10.1 instrument reports for
/// one GLB.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Step0Measured {
    /// Standing height including the hair cap, metres.
    pub height_m: f64,
    /// Bust circumference, metres.
    pub bust_circ_m: f64,
    /// Waist circumference, metres.
    pub waist_circ_m: f64,
    /// Hip circumference, metres.
    pub hip_circ_m: f64,
}

/// One battery pose reading.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PoseReading {
    /// Pose name as the instrument keys it (`elbow_flexion`, …).
    pub name: String,
    /// Applied angle, degrees.
    pub applied_deg: f64,
    /// Achieved skeletal angle, degrees (the primary
    /// instrument, L-01/L-02).
    pub achieved_deg: f64,
    /// Whether the reading is inside [`BATTERY_TOLERANCE_DEG`].
    pub within_tolerance: bool,
}

/// The A10.1 bend-battery gate result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BatteryGate {
    /// Overall verdict.
    pub verdict: Verdict,
    /// Per-pose readings, by pose name.
    pub poses: Vec<PoseReading>,
    /// Arm→belly weight-leak share, percent (reported by the
    /// instrument; the fleet's gated rigs read 0.0, and v1 of
    /// this gate requires exactly that).
    pub arm_belly_leak_pct: f64,
    /// The instrument's Step 0 measurement block.
    pub step0: Step0Measured,
}

/// The A10.2 export-census gate result (derived GLB against
/// its twin derivation, plus the record's structural
/// expectations).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CensusGate {
    /// Overall verdict.
    pub verdict: Verdict,
    /// Joint count of the derived GLB.
    pub joint_count: u32,
    /// Twin joint lists are identical, in order.
    pub joint_sets_equal: bool,
    /// Body POSITION accessor bytes hash-equal across the pair.
    pub body_position_equal: bool,
    /// Body JOINTS_0 accessor bytes hash-equal.
    pub body_joints0_equal: bool,
    /// Body WEIGHTS_0 accessor bytes hash-equal.
    pub body_weights0_equal: bool,
    /// Morph target sets identical across the pair.
    pub morph_sets_equal: bool,
    /// COLOR_0 differing-vertex count across the pair.
    pub color_0_diff_verts: u64,
    /// Record morphs missing from the derived GLB's targets.
    pub missing_morphs: Vec<String>,
}

/// One locked proportion compared against its measurement.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LockFieldCheck {
    /// Field name (`height_m`, `bust_circ_m`, …).
    pub field: String,
    /// The locked value, metres.
    pub locked_m: f64,
    /// The measured value, metres.
    pub measured_m: f64,
    /// Absolute difference, metres.
    pub delta_m: f64,
    /// Whether the field is inside [`LOCK_TOLERANCE_M`].
    pub within_tolerance: bool,
}

/// The identity-lock check: measured Step 0 proportions
/// against the record's locked proportions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LockCheck {
    /// Overall verdict (`NotRun` when the lock carries no
    /// proportions to check against).
    pub verdict: Verdict,
    /// Per-field checks.
    pub fields: Vec<LockFieldCheck>,
}

/// The full gate results for one derivation, as written into
/// the record's rig section primo-side and synced down.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GateResults {
    /// SHA-256 of the derived GLB.
    pub derived_glb_sha256: String,
    /// SHA-256 of the twin derivation's GLB.
    pub twin_glb_sha256: String,
    /// Twin-derivation determinism: the two GLBs are
    /// byte-identical (D-1397).
    pub determinism: bool,
    /// SHA-256 of the builder the adapter built with.
    pub builder_sha256: String,
    /// ISO-8601 time of the derivation.
    pub derived_at: String,
    /// A10.1 result.
    pub a10_1: BatteryGate,
    /// A10.2 result.
    pub a10_2: CensusGate,
    /// Identity-lock check result.
    pub lock_check: LockCheck,
}

impl GateResults {
    /// Whether the derivation may be labelled GATED in the
    /// UI: every gate passed and the twin derivation was
    /// deterministic. Anything else is CONSTRUCTION, in those
    /// words (ARCHITECTURE §8).
    pub fn is_gated(&self) -> bool {
        self.determinism
            && self.a10_1.verdict == Verdict::Pass
            && self.a10_2.verdict == Verdict::Pass
            && self.lock_check.verdict == Verdict::Pass
    }
}

/// The derive gate: may this record derive at all?
///
/// # Errors
/// Returns every unmet precondition when it may not: record
/// invalid, lock not held, a lock reference unhashed, lock
/// act unrecorded, or no derivation spec imported. Art
/// origination is never refused here — this gate guards
/// derivation only.
pub fn derive_eligibility(record: &CharacterRecord) -> Result<(), Vec<String>> {
    let mut reasons = record.validate();
    let lock = &record.identity_lock;
    if lock.status != LockStatus::Locked {
        reasons.push(
            "Step 0 identity lock is not held: derivation is refused until the \
             lock is recorded (art origination remains available)"
                .to_string(),
        );
    }
    if lock.refs.is_empty() {
        reasons.push("the identity lock names no lock artefacts".to_string());
    }
    for r in &lock.refs {
        if !is_sha256_hex(&r.sha256) {
            reasons.push(format!(
                "lock ref {:?} is unhashed: lock artefacts are hashed primo-side \
                 before a lock can carry a derivation",
                r.label
            ));
        }
    }
    if lock.locked_by.is_none() || lock.locked_at.is_none() {
        reasons.push("the lock act (locked_by / locked_at) is unrecorded".to_string());
    }
    if record.rig.derivation_spec.is_none() {
        reasons.push(
            "the record carries no derivation spec: a builder spec is imported \
             primo-side before the first derivation"
                .to_string(),
        );
    }
    if reasons.is_empty() {
        Ok(())
    } else {
        Err(reasons)
    }
}

fn f64_at(v: &Value, key: &str) -> Option<f64> {
    v.get(key).and_then(Value::as_f64)
}

/// Compute the A10.1 gate from the battery instrument's raw
/// output (`measure_battery.py` JSON: `battery`, `leak`,
/// `step0` blocks). Poses are read generically — every entry
/// carrying an applied and an achieved skeletal angle is a
/// pose — so a new battery pose joins the gate by existing,
/// never by a code change here.
pub fn battery_gate(battery_json: &Value) -> BatteryGate {
    let mut poses = Vec::new();
    if let Some(battery) = battery_json.get("battery").and_then(Value::as_object) {
        for (name, reading) in battery {
            // The pose's primary skeletal measure: most poses
            // report `achieved_skeletal_deg`; `spine_bend`
            // reports `achieved_headframe_deg` (its skeletal
            // measure, per the MoonDeep A10.1 record). A pose
            // with neither is not read — and a pose the
            // instrument measures must never silently vanish
            // from the gate, so both keys are named here.
            let achieved = f64_at(reading, "achieved_skeletal_deg")
                .or_else(|| f64_at(reading, "achieved_headframe_deg"));
            if let (Some(applied), Some(achieved)) = (f64_at(reading, "applied_deg"), achieved) {
                poses.push(PoseReading {
                    name: name.clone(),
                    applied_deg: applied,
                    achieved_deg: achieved,
                    within_tolerance: (achieved - applied).abs() <= BATTERY_TOLERANCE_DEG,
                });
            }
        }
    }
    poses.sort_by(|a, b| a.name.cmp(&b.name));
    let leak = battery_json
        .get("leak")
        .and_then(|l| f64_at(l, "arm_weight_share_pct"))
        .unwrap_or(f64::NAN);
    let step0 = battery_json.get("step0");
    let step0 = Step0Measured {
        height_m: step0
            .and_then(|s| f64_at(s, "height_m"))
            .unwrap_or(f64::NAN),
        bust_circ_m: step0
            .and_then(|s| f64_at(s, "bust_circ_m"))
            .unwrap_or(f64::NAN),
        waist_circ_m: step0
            .and_then(|s| f64_at(s, "waist_circ_m"))
            .unwrap_or(f64::NAN),
        hip_circ_m: step0
            .and_then(|s| f64_at(s, "hip_circ_m"))
            .unwrap_or(f64::NAN),
    };
    let verdict = if !poses.is_empty()
        && poses.iter().all(|p| p.within_tolerance)
        && leak == 0.0
        && step0.height_m.is_finite()
    {
        Verdict::Pass
    } else {
        Verdict::Fail
    };
    BatteryGate {
        verdict,
        poses,
        arm_belly_leak_pct: leak,
        step0,
    }
}

/// Compute the A10.2 gate from the census instrument's raw
/// output (`census_glb.py` JSON for the derived/twin pair),
/// the record's expected joint count (when carried), and the
/// morph names the GLB must carry (the record's expression
/// set; empty = the pair's own set equality is the whole
/// structural check).
pub fn census_gate(
    census_json: &Value,
    expected_joint_count: Option<u32>,
    required_morphs: &[String],
) -> CensusGate {
    let proof = census_json.get("one_body_proof");
    let get_bool = |key: &str| {
        proof
            .and_then(|p| p.get(key))
            .and_then(Value::as_bool)
            .unwrap_or(false)
    };
    let joint_count = proof
        .and_then(|p| p.get("joint_count"))
        .and_then(Value::as_u64)
        .unwrap_or(0) as u32;
    let color_0_diff_verts = proof
        .and_then(|p| p.get("color_0_diff"))
        .and_then(|d| d.get("verts"))
        .and_then(Value::as_u64)
        .unwrap_or(u64::MAX);
    let targets: Vec<String> = proof
        .and_then(|p| p.get("morph_targets_a"))
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|t| t.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    let missing_morphs: Vec<String> = required_morphs
        .iter()
        .filter(|m| !targets.contains(m))
        .cloned()
        .collect();
    let gate = CensusGate {
        verdict: Verdict::Fail,
        joint_count,
        joint_sets_equal: get_bool("joint_sets_equal"),
        body_position_equal: get_bool("position_equal"),
        body_joints0_equal: get_bool("joints0_equal"),
        body_weights0_equal: get_bool("weights0_equal"),
        morph_sets_equal: get_bool("morph_sets_equal"),
        color_0_diff_verts,
        missing_morphs,
    };
    let count_ok = expected_joint_count.is_none_or(|e| e == gate.joint_count);
    let verdict = if gate.joint_sets_equal
        && gate.body_position_equal
        && gate.body_joints0_equal
        && gate.body_weights0_equal
        && gate.morph_sets_equal
        && gate.color_0_diff_verts == 0
        && gate.missing_morphs.is_empty()
        && count_ok
        && gate.joint_count > 0
    {
        Verdict::Pass
    } else {
        Verdict::Fail
    };
    CensusGate { verdict, ..gate }
}

/// Compare measured Step 0 proportions against the lock's.
pub fn lock_check(measured: &Step0Measured, locked: &LockProportions) -> LockCheck {
    let field = |name: &str, locked_m: f64, measured_m: f64| {
        let delta = (measured_m - locked_m).abs();
        LockFieldCheck {
            field: name.to_string(),
            locked_m,
            measured_m,
            delta_m: delta,
            within_tolerance: delta <= LOCK_TOLERANCE_M,
        }
    };
    let fields = vec![
        field("height_m", locked.height_m, measured.height_m),
        field("bust_circ_m", locked.bust_circ_m, measured.bust_circ_m),
        field("waist_circ_m", locked.waist_circ_m, measured.waist_circ_m),
        field("hip_circ_m", locked.hip_circ_m, measured.hip_circ_m),
    ];
    let verdict = if fields.iter().all(|f| f.within_tolerance) {
        Verdict::Pass
    } else {
        Verdict::Fail
    };
    LockCheck { verdict, fields }
}

/// Assemble the full gate results for one derivation from
/// the instruments' raw outputs and the artefact hashes.
pub fn judge(
    record: &CharacterRecord,
    derived_glb_sha256: String,
    twin_glb_sha256: String,
    builder_sha256: String,
    derived_at: String,
    battery_json: &Value,
    census_json: &Value,
) -> GateResults {
    let a10_1 = battery_gate(battery_json);
    let required: Vec<String> = record.expressions.iter().map(|e| e.name.clone()).collect();
    let a10_2 = census_gate(census_json, record.rig.expected_joint_count, &required);
    let lock_check = match &record.identity_lock.proportions {
        Some(p) => lock_check(&a10_1.step0, p),
        None => LockCheck {
            verdict: Verdict::NotRun,
            fields: Vec::new(),
        },
    };
    GateResults {
        determinism: derived_glb_sha256 == twin_glb_sha256,
        derived_glb_sha256,
        twin_glb_sha256,
        builder_sha256,
        derived_at,
        a10_1,
        a10_2,
        lock_check,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{CharacterRecord, LockRef, LockStatus};
    use serde_json::json;

    fn eligible_record() -> CharacterRecord {
        let mut r = CharacterRecord::blank("solenne-test", "Solenne Test", "t");
        r.identity_lock.status = LockStatus::Locked;
        r.identity_lock.refs = vec![LockRef {
            label: "lock.md".to_string(),
            sha256: "b".repeat(64),
        }];
        r.identity_lock.locked_by = Some("Owner".to_string());
        r.identity_lock.locked_at = Some("2026-11-27T00:00:00Z".to_string());
        r.rig.derivation_spec = Some(json!({"character": "Solenne Test"}));
        r
    }

    fn battery_json() -> Value {
        // The MoonDeep Solenne battery figures of record.
        json!({
            "step0": {"height_m": 1.7202, "bust_circ_m": 0.8917,
                      "waist_circ_m": 0.6356, "hip_circ_m": 0.8644},
            "leak": {"arm_weight_share_pct": 0.0},
            "battery": {
                "elbow_flexion": {"applied_deg": 110.0, "achieved_skeletal_deg": 111.09},
                "knee_flexion": {"applied_deg": 100.0, "achieved_skeletal_deg": 99.35},
                "hip_flexion": {"applied_deg": 90.0, "achieved_skeletal_deg": 90.0},
                "shoulder_overhead": {"applied_deg": 165.0, "achieved_skeletal_deg": 165.0},
                "spine_bend": {"applied_deg": 30.0, "achieved_skeletal_deg": 30.0}
            }
        })
    }

    fn census_json() -> Value {
        json!({
            "one_body_proof": {
                "joint_sets_equal": true, "joint_count": 54,
                "position_equal": true, "joints0_equal": true,
                "weights0_equal": true, "morph_sets_equal": true,
                "morph_targets_a": ["Fac_Mth_Smile", "Fac_Eye_Close"],
                "color_0_diff": {"verts": 0}
            }
        })
    }

    // ---- validation: the gated path passes ----

    #[test]
    fn an_eligible_record_passes_the_derive_gate() {
        assert!(derive_eligibility(&eligible_record()).is_ok());
    }

    #[test]
    fn the_record_battery_passes_a10_1() {
        let g = battery_gate(&battery_json());
        assert_eq!(g.verdict, Verdict::Pass);
        assert_eq!(g.poses.len(), 5);
        assert_eq!(g.step0.bust_circ_m, 0.8917);
    }

    #[test]
    fn the_record_census_passes_a10_2() {
        let g = census_gate(&census_json(), Some(54), &["Fac_Mth_Smile".to_string()]);
        assert_eq!(g.verdict, Verdict::Pass);
    }

    #[test]
    fn the_lock_check_passes_on_the_locked_figures() {
        let measured = battery_gate(&battery_json()).step0;
        let locked = LockProportions {
            height_m: 1.7202,
            bust_circ_m: 0.8917,
            waist_circ_m: 0.6356,
            hip_circ_m: 0.8644,
        };
        assert_eq!(lock_check(&measured, &locked).verdict, Verdict::Pass);
    }

    // ---- adversarial: every refusal names its reason ----

    #[test]
    fn an_unlocked_record_cannot_derive() {
        let mut r = eligible_record();
        r.identity_lock.status = LockStatus::Unlocked;
        let reasons = derive_eligibility(&r).expect_err("must refuse");
        assert!(reasons.iter().any(|x| x.contains("identity lock")));
    }

    #[test]
    fn an_unhashed_lock_ref_cannot_carry_a_derivation() {
        let mut r = eligible_record();
        r.identity_lock.refs[0].sha256 = String::new();
        let reasons = derive_eligibility(&r).expect_err("must refuse");
        assert!(reasons.iter().any(|x| x.contains("unhashed")));
    }

    #[test]
    fn a_record_without_a_spec_cannot_derive() {
        let mut r = eligible_record();
        r.rig.derivation_spec = None;
        let reasons = derive_eligibility(&r).expect_err("must refuse");
        assert!(reasons.iter().any(|x| x.contains("derivation spec")));
    }

    #[test]
    fn an_out_of_tolerance_pose_fails_a10_1() {
        let mut b = battery_json();
        b["battery"]["knee_flexion"]["achieved_skeletal_deg"] = json!(92.0);
        assert_eq!(battery_gate(&b).verdict, Verdict::Fail);
    }

    #[test]
    fn a_weight_leak_fails_a10_1() {
        let mut b = battery_json();
        b["leak"]["arm_weight_share_pct"] = json!(1.5);
        assert_eq!(battery_gate(&b).verdict, Verdict::Fail);
    }

    #[test]
    fn a_twin_difference_fails_a10_2() {
        let mut c = census_json();
        c["one_body_proof"]["position_equal"] = json!(false);
        assert_eq!(census_gate(&c, Some(54), &[]).verdict, Verdict::Fail);
        let mut c = census_json();
        c["one_body_proof"]["color_0_diff"] = json!({"verts": 3});
        assert_eq!(census_gate(&c, Some(54), &[]).verdict, Verdict::Fail);
    }

    #[test]
    fn a_missing_morph_fails_a10_2_and_is_named() {
        let g = census_gate(&census_json(), Some(54), &["Fac_Mth_Aa".to_string()]);
        assert_eq!(g.verdict, Verdict::Fail);
        assert_eq!(g.missing_morphs, ["Fac_Mth_Aa"]);
    }

    #[test]
    fn a_wrong_joint_count_fails_a10_2() {
        assert_eq!(
            census_gate(&census_json(), Some(162), &[]).verdict,
            Verdict::Fail
        );
    }

    #[test]
    fn a_drifted_proportion_fails_the_lock_check() {
        let measured = battery_gate(&battery_json()).step0;
        let locked = LockProportions {
            height_m: 1.7202,
            bust_circ_m: 0.85,
            waist_circ_m: 0.6356,
            hip_circ_m: 0.8644,
        };
        let check = lock_check(&measured, &locked);
        assert_eq!(check.verdict, Verdict::Fail);
        assert!(!check.fields[1].within_tolerance);
    }
}
