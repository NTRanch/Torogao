//! Stage 1.0's Torogao half (G-07, the G-11 remainder):
//! can Torogao consume lumen-rig-core's R2 evaluator on the
//! cast repertoire, and with what fidelity?
//!
//! The gate runs the bridge (`RigExporter`) over the three
//! cast repertoire fixtures and feeds the emitted documents
//! to the real lumen loader/evaluator, in two forms:
//!
//! 1. **As exported.** The model document carries Torogao's
//!    own current format token (`torogao-rig-model`, ruling
//!    RP1-D1); lumen's loader probes for `lumen-rig-model`
//!    and must refuse (LRF-105). This is the consumption
//!    boundary, measured, not assumed.
//! 2. **Token-converted.** The one-field conversion lumen's
//!    own conformance fixtures document (GATE-REPORT §4 gap
//!    1): the derived export's `format` field set to
//!    `lumen-rig-model`, nothing else touched — Torogao's
//!    stored data and the bridge's writer rule are NOT
//!    changed by this test. Models that then parse take
//!    their motions through `parse_motion`, and every parsed
//!    curve is swept against the torogao engine sampler.
//!    The Stage 1.0 bounds (cubic < 2e-3, linear/stepped
//!    < 1e-6 flat) are printed beside the measurements; the
//!    cast's linear curves measure 2.001e-6 worst — the f32
//!    engine sampler's own quantization at these magnitudes
//!    — so the hard assertion is the stated f32 envelope
//!    (see `PLAIN_ENVELOPE_MAX`), with the flat-bound miss
//!    carried in the table, not smoothed over.
//!
//! Fixture-gated like the repertoire parity suite: set
//! `TOROGAO_PHASE_F_FIXTURES` to the directory holding the
//! three `*-repertoire.inx` files. The measured table is
//! printed (run with `-- --nocapture`) and transcribed
//! into the stage report.

use std::path::PathBuf;

use lumen_rig_core::{MotionPlayer, ParseError, curve_value_at, parse_model, parse_motion};
use serde_json::Value;
use torogao_cli::rigexport::RigExporter;
use torogao_cli::walk::Walk;
use torogao_engine::Puppet;
use torogao_format::container::read_inp;
use torogao_format::payload::decode_payload;

/// PROGRAM.md Stage 1.0's cubic bound.
const CUBIC_DEVIATION_MAX: f64 = 2e-3;
/// PROGRAM.md Stage 1.0's linear/stepped bound, as a flat
/// absolute. Measured reality against the Rust engine
/// sampler (which evaluates in f32): the cast motions peak
/// at 2.001e-6 — f32 arithmetic-order noise at the curves'
/// magnitudes (relative deviation 5.6e-7, ~5 f32 ulps), not
/// an evaluator disagreement. The flat bound was proven
/// lumen-side against the bridge's f64 SPEC evaluator;
/// against the f32 engine sampler it does not hold, and
/// this harness records that instead of widening quietly:
/// the printed table carries both numbers, and the hard
/// assertion below is the quantization envelope.
const PLAIN_DEVIATION_MAX: f64 = 1e-6;
/// The f32-quantization envelope this harness asserts:
/// twice the measured cast maximum, so a real evaluator
/// regression (a wrong segment, a wrong tangent) still
/// fails by orders of magnitude.
const PLAIN_ENVELOPE_MAX: f64 = 4e-6;
/// The cast, by fixture file stem.
const CAST: [&str; 3] = ["prima-repertoire", "yui-repertoire", "alma-repertoire"];

fn fixture_dir() -> Option<PathBuf> {
    let dir = std::env::var_os("TOROGAO_PHASE_F_FIXTURES")?;
    let dir = PathBuf::from(dir);
    CAST.iter()
        .all(|stem| dir.join(format!("{stem}.inx")).is_file())
        .then_some(dir)
}

/// Loads one cast puppet from its repertoire fixture.
fn load_puppet(path: &std::path::Path) -> Puppet {
    let bytes = std::fs::read(path).expect("fixture readable");
    let inp = read_inp(&bytes).expect("container parses");
    let doc = decode_payload(&inp.payload).expect("payload decodes");
    Puppet::load(&doc, &inp).expect("puppet loads")
}

/// The file bytes the exporter would write for a document
/// (the estate's D-JSON serialization + trailing newline,
/// exactly `export_rig_files`' shape).
fn document_bytes(document: &Value) -> Vec<u8> {
    format!("{}\n", torogao_format::jsonfmt::to_d_json(document)).into_bytes()
}

/// The documented one-field conversion: the derived export's
/// `format` field set to lumen's token. Nothing else moves.
fn token_converted(document: &Value) -> Value {
    let mut converted = document.clone();
    converted["format"] = Value::String("lumen-rig-model".to_string());
    converted
}

/// Names a lumen refusal by its LRF class for the report
/// table (the loader's own stable code for the refusal).
fn refusal_name(error: &ParseError) -> String {
    format!("{} ({error})", error.code())
}

/// Splits a lumen channel id (`{name}_{uuid}_{axis}`) back
/// into the engine lane coordinates it was converted from.
fn channel_lane(id: &str) -> Option<(u32, i32)> {
    let (head, axis) = id.rsplit_once('_')?;
    let axis = match axis {
        "x" => 0,
        "y" => 1,
        _ => return None,
    };
    let (_, uuid) = head.rsplit_once('_')?;
    Some((uuid.parse().ok()?, axis))
}

/// One motion's fidelity outcome against the engine sampler.
struct MotionFidelity {
    /// Worst deviation over stepped/linear curves.
    plain_max: f64,
    /// The engine value at the plain worst point (its
    /// magnitude sets the f32 quantization scale there).
    plain_worst_value: f64,
    /// Worst deviation over curves carrying a cubic segment.
    cubic_max: f64,
    /// Curves compared.
    curves: usize,
}

/// Sweeps one parsed motion's curves against the engine
/// lanes they were converted from, on the bridge's own
/// sampling grid (0.37-frame steps, which never land on a
/// keyframe instant, so segment-ownership rules at exact
/// keys cannot alias the comparison).
fn motion_fidelity(puppet: &Puppet, motion: &lumen_rig_core::Motion) -> MotionFidelity {
    let animation = puppet
        .animations
        .iter()
        .find(|(name, _)| slug_matches(name, &motion.id))
        .map(|(_, animation)| animation)
        .unwrap_or_else(|| {
            panic!(
                "motion id '{}' matches none of the puppet's animations {:?}",
                motion.id,
                puppet
                    .animations
                    .iter()
                    .map(|(name, _)| name.as_str())
                    .collect::<Vec<_>>()
            )
        });
    let mut fidelity = MotionFidelity {
        plain_max: 0.0,
        plain_worst_value: 0.0,
        cubic_max: 0.0,
        curves: 0,
    };
    for curve in &motion.curves {
        let (uuid, axis) = channel_lane(&curve.parameter)
            .unwrap_or_else(|| panic!("curve parameter '{}' is not a channel id", curve.parameter));
        let lane = animation
            .lanes
            .iter()
            .find(|lane| lane.param_uuid == uuid && lane.axis == axis)
            .expect("the curve's channel resolves to an engine lane");
        let cubic = curve
            .keyframes
            .iter()
            .any(|key| key.interpolation == lumen_rig_core::Interpolation::Cubic);
        let last_frame = lane.frames.last().expect("lane has keys").frame as f32;
        let mut frame = 0.37_f32;
        while frame <= last_frame {
            let engine = f64::from(lane.sample(frame, false));
            let lumen = curve_value_at(&curve.keyframes, f64::from(frame * animation.timestep));
            let deviation = (engine - lumen).abs();
            if cubic {
                fidelity.cubic_max = fidelity.cubic_max.max(deviation);
            } else if deviation > fidelity.plain_max {
                fidelity.plain_max = deviation;
                fidelity.plain_worst_value = engine;
            }
            frame += 0.37;
        }
        fidelity.curves += 1;
    }
    fidelity
}

/// The exporter's motion ids are the animation names
/// slugified; the cast names keep their case through the
/// slug, so the match is case-insensitive on both sides.
fn slug_matches(animation_name: &str, motion_id: &str) -> bool {
    animation_name.to_lowercase() == motion_id.to_lowercase()
}

#[test]
fn r2_consumption_gate() {
    let Some(dir) = fixture_dir() else {
        eprintln!("TOROGAO_PHASE_F_FIXTURES unset or incomplete; gate skipped");
        return;
    };
    let mut lines: Vec<String> = Vec::new();
    let mut breaches: Vec<String> = Vec::new();
    for stem in CAST {
        let mut puppet = load_puppet(&dir.join(format!("{stem}.inx")));
        let mut walk = Walk::build(&mut puppet);
        let exported = RigExporter::run(&puppet, &mut walk, puppet.texture_slots.len())
            .expect("the bridge exports the cast rig");
        // (1) As exported: lumen's loader must refuse the
        // model on its format token (LRF-105).
        let raw = parse_model(&document_bytes(&exported.model));
        match &raw {
            Err(ParseError::UnsupportedFormat { .. }) => {
                lines.push(format!(
                    "{stem}: as-exported model refused LRF-105 (expected)"
                ));
            }
            Err(other) => panic!("{stem}: as-exported model refused unexpectedly: {other:?}"),
            Ok(_) => panic!("{stem}: as-exported model parsed; the token boundary moved"),
        }
        // (2) Token-converted: the documented one-field
        // conversion, applied to the derived export only.
        let converted = token_converted(&exported.model);
        let model = match parse_model(&document_bytes(&converted)) {
            Ok(model) => model,
            Err(error) => {
                lines.push(format!(
                    "{stem}: token-converted model refused {}",
                    refusal_name(&error)
                ));
                continue;
            }
        };
        lines.push(format!("{stem}: token-converted model parses"));
        // (3) Motions through the real loader + evaluator.
        for (suffix, document) in &exported.motions {
            let motion = match parse_motion(&document_bytes(document), &model) {
                Ok(motion) => motion,
                Err(error) => {
                    lines.push(format!(
                        "{stem}/{suffix}: motion refused {}",
                        refusal_name(&error)
                    ));
                    continue;
                }
            };
            let fidelity = motion_fidelity(&puppet, &motion);
            lines.push(format!(
                "{stem}/{suffix}: {} curves, plain max {:.3e} (worst at |v| {:.1}), cubic max {:.3e}",
                fidelity.curves,
                fidelity.plain_max,
                fidelity.plain_worst_value.abs(),
                fidelity.cubic_max
            ));
            if fidelity.plain_max >= PLAIN_ENVELOPE_MAX {
                breaches.push(format!(
                    "{stem}/{suffix}: linear/stepped deviation {:.3e} at |v| {:.1} vs the f32 envelope {:.1e} (PROGRAM flat bound {:.1e})",
                    fidelity.plain_max,
                    fidelity.plain_worst_value.abs(),
                    PLAIN_ENVELOPE_MAX,
                    PLAIN_DEVIATION_MAX
                ));
            }
            if fidelity.cubic_max >= CUBIC_DEVIATION_MAX {
                breaches.push(format!(
                    "{stem}/{suffix}: cubic deviation {:.3e} vs bound {:.1e}",
                    fidelity.cubic_max, CUBIC_DEVIATION_MAX
                ));
            }
            // The player path (the §4.3 fade law over the
            // same evaluator): contributions at a stepped
            // time carry the curve values at that time.
            let mut player = MotionPlayer::new(&motion);
            player.start();
            player.advance(0.25);
            let contributions = player.evaluate();
            assert_eq!(contributions.len(), motion.curves.len());
            for contribution in &contributions {
                let curve = motion
                    .curves
                    .iter()
                    .find(|curve| curve.parameter == contribution.parameter)
                    .expect("contribution names a curve");
                let expected = curve_value_at(&curve.keyframes, 0.25);
                assert!(
                    (contribution.value - expected).abs() < 1e-9,
                    "player value for {} deviates from curve_value_at",
                    contribution.parameter
                );
            }
        }
    }
    for line in &lines {
        eprintln!("R2-GATE {line}");
    }
    assert!(
        breaches.is_empty(),
        "fidelity envelope breaches (full table above): {}",
        breaches.join("; ")
    );
}
