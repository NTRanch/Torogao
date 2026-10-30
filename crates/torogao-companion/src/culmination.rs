//! Stage 2.2 culmination presentation (AMENDMENT-T4
//! §5.7, `torogao-culmination/1`): the presentation layer
//! that plays the culmination performance package on the
//! landed arousal engine.
//!
//! The engine decides *that* a culmination fires and how
//! strong it is: `ArousalEvent::Culmination { episode,
//! tier, .. }` fires exactly once per episode with the
//! tier frozen (§5.6). This module decides *what plays*.
//! It loads the performance package
//! (`culmination.json`, torogao-culmination/1) as-is —
//! the data is consumed, never forked — validates it
//! against the engine's own §5.7 tier table
//! (`torogao_session::TIERS`, the one source of truth the
//! two must agree on) and the package laws (finite,
//! non-looping envelope; one peak convention per
//! performance; the Stage 2.2 peak eye bands), and then,
//! per fixed-step tick while the engine phase is
//! `Culminating` or `Taper`, emits:
//!
//! - rig parameter curves for the channels the rig
//!   carries (`ParamEyeRoll`, `ParamEyeCross`,
//!   `ParamBodyAngleY` as a fraction of template range,
//!   `ParamBreath` as a depth scale, and the hair/breast
//!   secondary parameters as amplitude scales),
//! - the evaluator-side descriptors that are not model
//!   parameters (body cycle, tremble),
//! - face segment markers (onset / peak hold / release /
//!   taper) resolved through the character's governed
//!   expression map — a state her record does not carry
//!   refuses typed, never substitutes (§6.5).
//!
//! Playback is keyed to the engine phase: it runs only
//! while the phase is `Culminating`/`Taper`, and a phase
//! exit mid-performance (the §5.8 reset lands in
//! `Baseline` the same tick) cancels it on the next
//! step. `notify_remove_all` is the shell's direct seam
//! for the §3.4 control; the phase keying enforces the
//! same cancellation without it.
//!
//! The engine's own per-tick `PerformanceOutput` is a
//! separate, coarser feed (its envelope shape is the
//! engine's internal 20/40/40 linear law); its peaks and
//! durations agree with this package exactly, its shape
//! does not — the package master envelope is the
//! presentation authority (package SPEC §6.3).

use std::collections::BTreeMap;
use std::path::Path;

use serde::Deserialize;
use torogao_session::{ArousalEvent, Phase, TIERS};

use crate::error::{CompanionError, CompanionResult};

/// The package format token this loader consumes.
pub const PACKAGE_FORMAT: &str = "torogao-culmination/1";
/// The package format version this loader consumes.
pub const PACKAGE_FORMAT_VERSION: u32 = 1;
/// The engine's fixed step (AMENDMENT-T4 §5.3:
/// `TICKS_PER_SECOND` = 60); package times are seconds
/// on this step.
pub const TICKS_PER_SECOND: u64 = 60;

/// Canon state for the approach segment (face canon).
pub const STATE_APPROACH: &str = "F-03";
/// Canon state for the default peak convention.
pub const STATE_PEAK_ROLL: &str = "F-05";
/// Canon state for the white-roll peak convention.
pub const STATE_PEAK_WHITE_ROLL: &str = "N-05";
/// Canon state for release target / taper / afterglow.
pub const STATE_AFTERGLOW: &str = "F-10";

/// Float agreement bound for package-vs-engine values
/// (the JSON carries decimal fractions; the engine
/// carries permille integers — agreement is exact at
/// this bound or the document is not the §5.7 table).
const EPS: f64 = 1e-9;

// ---------------------------------------------------------------- package

/// One master-envelope keyframe from the package.
#[derive(Debug, Clone, PartialEq, Deserialize)]
struct RawKeyframe {
    t: f64,
    value: f64,
    #[serde(default)]
    interpolation: Option<String>,
    #[serde(default)]
    settle: bool,
}

/// One face-path segment from the package.
#[derive(Debug, Clone, PartialEq, Deserialize)]
struct RawFaceSegment {
    segment: String,
    governed_name: String,
    #[serde(default)]
    state: Option<String>,
    #[serde(default)]
    transition: Option<[String; 2]>,
    t_start: f64,
    t_end: f64,
}

/// The §5.7 channel peaks of one tier, as carried.
#[derive(Debug, Clone, PartialEq, Deserialize)]
struct RawChannels {
    body_cycle_amplitude_scale: f64,
    posture_excursion_fraction: f64,
    tremble_amplitude_px: f64,
    tremble_rate_hz: f64,
    breath_depth_scale: f64,
    eye_roll_peak: f64,
    eye_cross_peak: f64,
    secondary_amplitude_scale: f64,
    governed_peak_expression: String,
}

/// Peak eye targets (Stage 2.2 band centres), T3/T4
/// under the roll convention only.
#[derive(Debug, Clone, PartialEq, Deserialize)]
struct RawEyeTargets {
    visible_iris_fraction: f64,
    sclera_field: f64,
}

/// One tier envelope, as carried.
#[derive(Debug, Clone, PartialEq, Deserialize)]
struct RawTier {
    tier: u8,
    p_range: [f64; 2],
    performance_seconds: f64,
    taper_seconds: f64,
    total_seconds: f64,
    channels: RawChannels,
    face_path: Vec<RawFaceSegment>,
    master_envelope: Vec<RawKeyframe>,
    peak_eye_targets: Option<RawEyeTargets>,
}

/// The Stage 2.2 peak eye bands, as carried.
#[derive(Debug, Clone, PartialEq, Deserialize)]
struct RawEyeBands {
    visible_iris_fraction: [f64; 2],
    sclera_field: [f64; 2],
    applies_to_tiers: Vec<u8>,
    applies_to_convention: String,
}

/// The §5.7 decay law constants, as carried.
#[derive(Debug, Clone, PartialEq, Deserialize)]
struct RawDecay {
    tau_a_seconds: f64,
    tau_q_seconds: f64,
}

/// The whole package document, as carried. Unknown
/// fields are the document's own context (sources,
/// notes, frame tables); the loader reads the
/// load-bearing fields and validates them.
#[derive(Debug, Clone, PartialEq, Deserialize)]
struct RawPackage {
    format: String,
    format_version: u32,
    #[serde(rename = "loop")]
    looping: bool,
    peak_convention_default: String,
    peak_eye_bands: RawEyeBands,
    decay: RawDecay,
    body_cycle_period: RawBodyCyclePeriod,
    release_end_value: f64,
    tiers: Vec<RawTier>,
}

/// The body-cycle period band (MG-02), as carried.
#[derive(Debug, Clone, PartialEq, Deserialize)]
struct RawBodyCyclePeriod {
    band_seconds: [f64; 2],
}

/// The peak conventions a performance may declare.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PeakConvention {
    /// F-05: rolled to a bottom sliver, hearts solid.
    Roll,
    /// N-05: the pupil dissolves (white-roll).
    WhiteRoll,
}

impl PeakConvention {
    /// The canon state code of this convention's peak.
    pub fn state_code(self) -> &'static str {
        match self {
            PeakConvention::Roll => STATE_PEAK_ROLL,
            PeakConvention::WhiteRoll => STATE_PEAK_WHITE_ROLL,
        }
    }

    /// Parses the package's convention token.
    fn from_token(token: &str) -> CompanionResult<PeakConvention> {
        match token {
            STATE_PEAK_ROLL => Ok(PeakConvention::Roll),
            STATE_PEAK_WHITE_ROLL => Ok(PeakConvention::WhiteRoll),
            other => Err(format_err(format!(
                "unknown peak convention {other:?} (the package defines F-05 and N-05)"
            ))),
        }
    }
}

/// One validated tier envelope.
#[derive(Debug, Clone, PartialEq)]
pub struct TierEnvelope {
    /// Tier number, 1..=4.
    pub tier: u8,
    /// Performance (Culminating) duration, seconds.
    pub performance_seconds: f64,
    /// Taper duration, seconds.
    pub taper_seconds: f64,
    /// Performance + taper, seconds.
    pub total_seconds: f64,
    /// §5.7 channel peaks for this tier.
    pub channels: Channels,
    /// The face path, in segment order.
    pub face_path: Vec<FaceSegment>,
    /// Master-envelope keyframes, time-ordered.
    pub master_envelope: Vec<MasterKeyframe>,
    /// Stage 2.2 peak eye targets (roll convention,
    /// tiers 3–4); `None` otherwise.
    pub peak_eye_targets: Option<PeakEyeTarget>,
}

/// The §5.7 channel peaks of one tier (validated
/// against the engine's table).
#[derive(Debug, Clone, PartialEq)]
pub struct Channels {
    /// Body-cycle amplitude scale (`ParamBodyAmplitude`).
    pub body_cycle_amplitude_scale: f64,
    /// Posture excursion, fraction of template range.
    pub posture_excursion_fraction: f64,
    /// Tremble amplitude, px at the 1024 canvas.
    pub tremble_amplitude_px: f64,
    /// Tremble rate, Hz.
    pub tremble_rate_hz: f64,
    /// Breath depth scale (1.0 = template breath).
    pub breath_depth_scale: f64,
    /// `ParamEyeRoll` peak, 0..=1.
    pub eye_roll_peak: f64,
    /// `ParamEyeCross` peak, 0..=1.
    pub eye_cross_peak: f64,
    /// Hair/breast secondary amplitude scale.
    pub secondary_amplitude_scale: f64,
    /// The governed name of the peak expression.
    pub governed_peak_expression: String,
}

/// One face-path segment (validated).
#[derive(Debug, Clone, PartialEq)]
pub struct FaceSegment {
    /// The segment kind.
    pub segment: Segment,
    /// The governed name (§6.5) for the segment.
    pub governed_name: String,
    /// The canon state at the segment's start.
    pub canon_from: String,
    /// The canon state at the segment's end (equal to
    /// `canon_from` outside the release transition).
    pub canon_to: String,
    /// Segment start, seconds from the firing tick.
    pub t_start: f64,
    /// Segment end, seconds from the firing tick.
    pub t_end: f64,
}

/// The performance segments, in play order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Segment {
    /// Approach (F-03).
    Onset,
    /// Peak hold (the declared peak convention).
    PeakHold,
    /// Release (peak → F-10 across the segment).
    Release,
    /// Taper (F-10 holds under the decay law).
    Taper,
}

impl Segment {
    /// The package's segment token.
    pub fn token(self) -> &'static str {
        match self {
            Segment::Onset => "onset",
            Segment::PeakHold => "peak_hold",
            Segment::Release => "release",
            Segment::Taper => "taper",
        }
    }

    fn from_token(token: &str) -> CompanionResult<Segment> {
        match token {
            "onset" => Ok(Segment::Onset),
            "peak_hold" => Ok(Segment::PeakHold),
            "release" => Ok(Segment::Release),
            "taper" => Ok(Segment::Taper),
            other => Err(spec_err(format!("unknown face segment {other:?}"))),
        }
    }
}

/// One master-envelope keyframe (validated).
#[derive(Debug, Clone, PartialEq)]
pub struct MasterKeyframe {
    /// Time, seconds from the firing tick.
    pub t: f64,
    /// Master value, 0..=1.
    pub value: f64,
    /// How the segment *from* this keyframe is sampled.
    pub interpolation: Interpolation,
    /// The terminal settle keyframe (value 0 at the
    /// tier's total duration).
    pub settle: bool,
}

/// Segment sampling shapes the package defines.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Interpolation {
    /// Smoothstep blend to the next keyframe.
    Smoothstep,
    /// Hold this keyframe's value.
    Hold,
    /// Exponential blend toward the next keyframe (the
    /// stored keyframes lie on the §5.7 decay law).
    Exponential,
}

impl Interpolation {
    fn from_token(token: &str) -> CompanionResult<Interpolation> {
        match token {
            "smoothstep" => Ok(Interpolation::Smoothstep),
            "hold" => Ok(Interpolation::Hold),
            "exponential" => Ok(Interpolation::Exponential),
            other => Err(spec_err(format!(
                "unknown master-envelope interpolation {other:?}"
            ))),
        }
    }
}

/// The Stage 2.2 peak eye targets.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PeakEyeTarget {
    /// Visible-iris fraction at peak (band 0.20..=0.35).
    pub visible_iris_fraction: f64,
    /// Sclera field at peak (band 0.63..=0.73).
    pub sclera_field: f64,
}

/// A validated culmination performance package
/// (torogao-culmination/1): the four tier envelopes,
/// validated against the engine's §5.7 table and the
/// package laws.
#[derive(Debug, Clone, PartialEq)]
pub struct CulminationPackage {
    tiers: [TierEnvelope; 4],
    document_convention: PeakConvention,
    eye_bands: ([f64; 2], [f64; 2]),
    body_cycle_period_band: [f64; 2],
    tau_a_seconds: f64,
}

impl CulminationPackage {
    /// Parses and validates one package document. The
    /// document is consumed as-is; every load-bearing
    /// value is checked against the engine's §5.7 tier
    /// table (`torogao_session::TIERS`) and the package
    /// laws, so a document that is not the §5.7 table —
    /// or that loops, blends peak conventions, drops its
    /// taper, or aims outside the Stage 2.2 eye bands —
    /// refuses typed instead of playing.
    pub fn from_json(text: &str) -> CompanionResult<CulminationPackage> {
        let raw: RawPackage = serde_json::from_str(text)
            .map_err(|err| format_err(format!("package is not valid culmination JSON: {err}")))?;
        validate_package(raw)
    }

    /// Loads and validates the package document at
    /// `path`.
    pub fn load_file(path: &Path) -> CompanionResult<CulminationPackage> {
        let text = std::fs::read_to_string(path).map_err(|err| {
            CompanionError::new(
                "TG-COMP-CULMINATION-IO",
                format!("cannot read package {}: {err}", path.display()),
            )
        })?;
        CulminationPackage::from_json(&text)
    }

    /// The validated envelope for `tier` (1..=4).
    pub fn tier(&self, tier: u8) -> CompanionResult<&TierEnvelope> {
        if !(1..=4).contains(&tier) {
            return Err(spec_err(format!(
                "tier {tier} is outside the §5.7 table (1..=4)"
            )));
        }
        Ok(&self.tiers[(tier - 1) as usize])
    }

    /// The peak convention the document itself plays
    /// (its face path's peak state).
    pub fn document_convention(&self) -> PeakConvention {
        self.document_convention
    }

    /// The Stage 2.2 peak eye bands the document
    /// carries and was validated against:
    /// `(visible_iris_fraction, sclera_field)`.
    pub fn eye_bands(&self) -> ([f64; 2], [f64; 2]) {
        self.eye_bands
    }

    /// The body-cycle period band (MG-02), seconds.
    pub fn body_cycle_period_band(&self) -> [f64; 2] {
        self.body_cycle_period_band
    }

    /// Builds the performance plan for one firing under
    /// `convention`. The variant swaps the whole peak —
    /// peak-hold state and the release transition's
    /// source — and nulls the peak eye targets (under
    /// white-roll there is no iris to gate). Deriving the
    /// roll convention from a white-roll document
    /// refuses: that document carries no roll peak to
    /// play.
    pub fn plan(&self, tier: u8, convention: PeakConvention) -> CompanionResult<PerformancePlan> {
        let envelope = self.tier(tier)?;
        if convention == self.document_convention {
            return Ok(PerformancePlan {
                envelope: envelope.clone(),
                convention,
                tau_a_seconds: self.tau_a_seconds,
                body_cycle_period_band: self.body_cycle_period_band,
            });
        }
        if self.document_convention == PeakConvention::WhiteRoll {
            return Err(spec_err(
                "the document's peak is N-05 white-roll; it carries no F-05 peak to derive",
            ));
        }
        // F-05 document, N-05 performance: swap the whole
        // peak, null the eye targets.
        let mut swapped = envelope.clone();
        for segment in &mut swapped.face_path {
            match segment.segment {
                Segment::PeakHold => {
                    segment.canon_from = STATE_PEAK_WHITE_ROLL.to_string();
                    segment.canon_to = STATE_PEAK_WHITE_ROLL.to_string();
                }
                Segment::Release => {
                    segment.canon_from = STATE_PEAK_WHITE_ROLL.to_string();
                }
                _ => {}
            }
        }
        swapped.peak_eye_targets = None;
        Ok(PerformancePlan {
            envelope: swapped,
            convention,
            tau_a_seconds: self.tau_a_seconds,
            body_cycle_period_band: self.body_cycle_period_band,
        })
    }

    /// Samples the tier's master envelope at `t`
    /// seconds. The sampler is the package's own law:
    /// the envelope starts at 0, plateaus once, settles
    /// to exactly 0 at the tier's total duration, and is
    /// never sampled past its end.
    pub fn sample_master(&self, tier: u8, t: f64) -> CompanionResult<f64> {
        let envelope = self.tier(tier)?;
        Ok(sample_master(envelope, self.tau_a_seconds, t))
    }
}

/// One firing's resolved performance: a tier envelope
/// plus the declared peak convention already applied.
#[derive(Debug, Clone, PartialEq)]
pub struct PerformancePlan {
    envelope: TierEnvelope,
    convention: PeakConvention,
    tau_a_seconds: f64,
    body_cycle_period_band: [f64; 2],
}

impl PerformancePlan {
    /// The frozen tier.
    pub fn tier(&self) -> u8 {
        self.envelope.tier
    }

    /// The declared peak convention.
    pub fn convention(&self) -> PeakConvention {
        self.convention
    }

    /// Total playback ticks (performance + taper) on
    /// the fixed step.
    pub fn total_ticks(&self) -> u64 {
        (self.envelope.total_seconds * TICKS_PER_SECOND as f64).round() as u64
    }

    /// The tier envelope in play.
    pub fn envelope(&self) -> &TierEnvelope {
        &self.envelope
    }
}

/// Samples one tier's master envelope at `t` seconds
/// (see `CulminationPackage::sample_master`).
fn sample_master(envelope: &TierEnvelope, tau_a_seconds: f64, t: f64) -> f64 {
    let keyframes = &envelope.master_envelope;
    if t <= 0.0 {
        return keyframes[0].value;
    }
    if t >= envelope.total_seconds {
        return 0.0;
    }
    for pair in keyframes.windows(2) {
        let (a, b) = (&pair[0], &pair[1]);
        if t >= a.t && t <= b.t {
            if b.settle {
                // The terminal segment decays under the
                // A time constant from the segment's start
                // keyframe; the settle value itself is
                // reached only at the total duration (the
                // phase law ends Taper there).
                return a.value * (-(t - a.t) / tau_a_seconds).exp();
            }
            let span = b.t - a.t;
            let u = if span > 0.0 { (t - a.t) / span } else { 0.0 };
            return match a.interpolation {
                Interpolation::Hold => a.value,
                Interpolation::Smoothstep => {
                    let s = u * u * (3.0 - 2.0 * u);
                    a.value + (b.value - a.value) * s
                }
                Interpolation::Exponential => {
                    if a.value > 0.0 && b.value > 0.0 {
                        a.value * (b.value / a.value).powf(u)
                    } else {
                        a.value + (b.value - a.value) * u
                    }
                }
            };
        }
    }
    0.0
}

fn format_err(message: impl Into<String>) -> CompanionError {
    CompanionError::new("TG-COMP-CULMINATION-FORMAT", message.into())
}

fn spec_err(message: impl Into<String>) -> CompanionError {
    CompanionError::new("TG-COMP-CULMINATION-SPEC", message.into())
}

/// Validates a parsed document into the package: the
/// §5.7 cross-checks against the engine's tier table,
/// then the package laws.
fn validate_package(raw: RawPackage) -> CompanionResult<CulminationPackage> {
    if raw.format != PACKAGE_FORMAT || raw.format_version != PACKAGE_FORMAT_VERSION {
        return Err(format_err(format!(
            "format {:?} version {} is not {PACKAGE_FORMAT} version {PACKAGE_FORMAT_VERSION}",
            raw.format, raw.format_version
        )));
    }
    if raw.looping {
        return Err(format_err(
            "the culmination envelope is finite and non-looping; a document with loop = true is refused",
        ));
    }
    if (raw.decay.tau_a_seconds - 12.0).abs() > EPS || (raw.decay.tau_q_seconds - 20.0).abs() > EPS
    {
        return Err(spec_err(format!(
            "decay constants {} s / {} s are not the §5.7 law (12 s / 20 s)",
            raw.decay.tau_a_seconds, raw.decay.tau_q_seconds
        )));
    }
    if raw.peak_eye_bands.applies_to_convention != STATE_PEAK_ROLL
        || raw.peak_eye_bands.applies_to_tiers != [3, 4]
    {
        return Err(spec_err(
            "the Stage 2.2 eye bands apply to tiers 3–4 under the F-05 convention",
        ));
    }
    let iris_band = raw.peak_eye_bands.visible_iris_fraction;
    let sclera_band = raw.peak_eye_bands.sclera_field;
    if (iris_band[0] - 0.20).abs() > EPS
        || (iris_band[1] - 0.35).abs() > EPS
        || (sclera_band[0] - 0.63).abs() > EPS
        || (sclera_band[1] - 0.73).abs() > EPS
    {
        return Err(spec_err(format!(
            "peak eye bands {iris_band:?} / {sclera_band:?} are not the Stage 2.2 bands (0.20..=0.35 / 0.63..=0.73)"
        )));
    }
    let band = raw.body_cycle_period.band_seconds;
    if (band[0] - 0.5).abs() > EPS || (band[1] - 0.8).abs() > EPS {
        return Err(spec_err(format!(
            "body-cycle period band {band:?} is not the MG-02 band [0.5, 0.8]"
        )));
    }
    if raw.tiers.len() != 4 {
        return Err(spec_err(format!(
            "the package carries {} tiers; the §5.7 table has exactly 4",
            raw.tiers.len()
        )));
    }
    let document_convention = PeakConvention::from_token(&raw.peak_convention_default)?;
    let mut tiers: Vec<TierEnvelope> = Vec::with_capacity(4);
    for (index, raw_tier) in raw.tiers.iter().enumerate() {
        tiers.push(validate_tier(raw_tier, index, document_convention, &raw)?);
    }
    // The document's own peak state must match its
    // declared default convention in every tier.
    for tier in &tiers {
        let peak = &tier.face_path[1];
        if peak.canon_from != document_convention.state_code() {
            return Err(spec_err(format!(
                "tier {} peak state {} does not match the document convention {}",
                tier.tier,
                peak.canon_from,
                document_convention.state_code()
            )));
        }
    }
    Ok(CulminationPackage {
        tiers: [
            tiers[0].clone(),
            tiers[1].clone(),
            tiers[2].clone(),
            tiers[3].clone(),
        ],
        document_convention,
        eye_bands: (iris_band, sclera_band),
        body_cycle_period_band: raw.body_cycle_period.band_seconds,
        tau_a_seconds: raw.decay.tau_a_seconds,
    })
}

/// Validates one tier against the engine's table and
/// the package laws.
fn validate_tier(
    raw: &RawTier,
    index: usize,
    _document_convention: PeakConvention,
    package: &RawPackage,
) -> CompanionResult<TierEnvelope> {
    let tier_number = (index + 1) as u8;
    if raw.tier != tier_number {
        return Err(spec_err(format!(
            "tier entry {} declares tier {}; tiers are carried in §5.7 order",
            index, raw.tier
        )));
    }
    let spec = TIERS[index];
    let fps = TICKS_PER_SECOND as f64;
    if (raw.performance_seconds * fps - spec.culminating_ticks as f64).abs() > EPS
        || (raw.taper_seconds * fps - spec.taper_ticks as f64).abs() > EPS
        || (raw.total_seconds - raw.performance_seconds - raw.taper_seconds).abs() > EPS
    {
        return Err(spec_err(format!(
            "tier {tier_number} durations {} s + {} s are not the §5.7 table ({} ticks + {} ticks)",
            raw.performance_seconds, raw.taper_seconds, spec.culminating_ticks, spec.taper_ticks
        )));
    }
    // Channel peaks against the engine's permille table.
    let ch = &raw.channels;
    let channels: [(&str, f64, u32); 8] = [
        (
            "body_cycle_amplitude_scale",
            ch.body_cycle_amplitude_scale,
            spec.body_amplitude,
        ),
        (
            "posture_excursion_fraction",
            ch.posture_excursion_fraction,
            spec.posture,
        ),
        (
            "tremble_amplitude_px",
            ch.tremble_amplitude_px,
            spec.tremble_milli_px,
        ),
        (
            "tremble_rate_hz",
            ch.tremble_rate_hz,
            spec.tremble_rate_millihz,
        ),
        (
            "breath_depth_scale",
            ch.breath_depth_scale,
            spec.breath_scale,
        ),
        ("eye_roll_peak", ch.eye_roll_peak, spec.eye_roll),
        ("eye_cross_peak", ch.eye_cross_peak, spec.eye_cross),
        (
            "secondary_amplitude_scale",
            ch.secondary_amplitude_scale,
            spec.secondary_scale,
        ),
    ];
    for (field, value, permille) in channels {
        if (value - f64::from(permille) / 1000.0).abs() > 1e-6 {
            return Err(spec_err(format!(
                "tier {tier_number} channel {field} = {value} is not the §5.7 value {}",
                f64::from(permille) / 1000.0
            )));
        }
    }
    // Strength range edges (§5.7: T1 < 0.30, T2 < 0.55,
    // T3 < 0.80, T4 to 1.00).
    let (want_lo, want_hi) = [(0.0, 0.29), (0.30, 0.54), (0.55, 0.79), (0.80, 1.00)][index];
    if (raw.p_range[0] - want_lo).abs() > EPS || (raw.p_range[1] - want_hi).abs() > EPS {
        return Err(spec_err(format!(
            "tier {tier_number} strength range {:?} is not the §5.7 range [{want_lo}, {want_hi}]",
            raw.p_range
        )));
    }
    // Master envelope: finite, non-looping, one plateau.
    let keyframes = validate_envelope(raw, package.release_end_value)?;
    // Face path: order, contiguity, canon law.
    let face_path = validate_face_path(raw)?;
    // Peak eye targets: present exactly for tiers 3–4
    // under the roll convention, inside the bands.
    let peak_state = &face_path[1].canon_from;
    let peak_eye_targets = match &raw.peak_eye_targets {
        Some(targets) => {
            if tier_number < 3 || peak_state != STATE_PEAK_ROLL {
                return Err(spec_err(format!(
                    "tier {tier_number} claims Stage 2.2 peak eye targets outside tiers 3–4 under F-05"
                )));
            }
            let (iris_band, sclera_band) = (
                package.peak_eye_bands.visible_iris_fraction,
                package.peak_eye_bands.sclera_field,
            );
            if targets.visible_iris_fraction < iris_band[0] - EPS
                || targets.visible_iris_fraction > iris_band[1] + EPS
                || targets.sclera_field < sclera_band[0] - EPS
                || targets.sclera_field > sclera_band[1] + EPS
            {
                return Err(spec_err(format!(
                    "tier {tier_number} peak eye targets ({}, {}) sit outside the Stage 2.2 bands {iris_band:?} / {sclera_band:?}",
                    targets.visible_iris_fraction, targets.sclera_field
                )));
            }
            Some(PeakEyeTarget {
                visible_iris_fraction: targets.visible_iris_fraction,
                sclera_field: targets.sclera_field,
            })
        }
        None => {
            if tier_number >= 3 && peak_state == STATE_PEAK_ROLL {
                return Err(spec_err(format!(
                    "tier {tier_number} plays the F-05 peak without Stage 2.2 peak eye targets"
                )));
            }
            None
        }
    };
    Ok(TierEnvelope {
        tier: tier_number,
        performance_seconds: raw.performance_seconds,
        taper_seconds: raw.taper_seconds,
        total_seconds: raw.total_seconds,
        channels: Channels {
            body_cycle_amplitude_scale: ch.body_cycle_amplitude_scale,
            posture_excursion_fraction: ch.posture_excursion_fraction,
            tremble_amplitude_px: ch.tremble_amplitude_px,
            tremble_rate_hz: ch.tremble_rate_hz,
            breath_depth_scale: ch.breath_depth_scale,
            eye_roll_peak: ch.eye_roll_peak,
            eye_cross_peak: ch.eye_cross_peak,
            secondary_amplitude_scale: ch.secondary_amplitude_scale,
            governed_peak_expression: ch.governed_peak_expression.clone(),
        },
        face_path,
        master_envelope: keyframes,
        peak_eye_targets,
    })
}

/// Validates the master envelope's shape law.
fn validate_envelope(
    raw: &RawTier,
    release_end_value: f64,
) -> CompanionResult<Vec<MasterKeyframe>> {
    let raw_keyframes = &raw.master_envelope;
    if raw_keyframes.len() < 3 {
        return Err(spec_err(format!(
            "tier {} master envelope has {} keyframes; onset, plateau, release, and settle are required",
            raw.tier,
            raw_keyframes.len()
        )));
    }
    let mut keyframes: Vec<MasterKeyframe> = Vec::with_capacity(raw_keyframes.len());
    for raw_keyframe in raw_keyframes {
        let interpolation = match &raw_keyframe.interpolation {
            Some(token) => Interpolation::from_token(token)?,
            None => {
                if raw_keyframe.settle {
                    Interpolation::Hold
                } else {
                    return Err(spec_err(format!(
                        "tier {} keyframe at {} s carries no interpolation",
                        raw.tier, raw_keyframe.t
                    )));
                }
            }
        };
        keyframes.push(MasterKeyframe {
            t: raw_keyframe.t,
            value: raw_keyframe.value,
            interpolation,
            settle: raw_keyframe.settle,
        });
    }
    let first = &keyframes[0];
    if first.t != 0.0 || first.value != 0.0 {
        return Err(spec_err(format!(
            "tier {} master envelope starts at ({} s, {}); the envelope starts at (0, 0)",
            raw.tier, first.t, first.value
        )));
    }
    let last = &keyframes[keyframes.len() - 1];
    if !last.settle || last.value != 0.0 || (last.t - raw.total_seconds).abs() > EPS {
        return Err(spec_err(format!(
            "tier {} master envelope does not settle to 0 at its total duration {} s",
            raw.tier, raw.total_seconds
        )));
    }
    for pair in keyframes.windows(2) {
        if pair[1].t <= pair[0].t {
            return Err(spec_err(format!(
                "tier {} master envelope keyframe times are not strictly increasing",
                raw.tier
            )));
        }
        if !(0.0..=1.0).contains(&pair[0].value) || !(0.0..=1.0).contains(&pair[1].value) {
            return Err(spec_err(format!(
                "tier {} master envelope leaves the 0..=1 band",
                raw.tier
            )));
        }
    }
    // One peak plateau: non-decreasing to the first
    // maximum, non-increasing after the last maximum,
    // and the maximum is the full 1.0.
    let max = keyframes.iter().map(|k| k.value).fold(f64::MIN, f64::max);
    if (max - 1.0).abs() > EPS {
        return Err(spec_err(format!(
            "tier {} master envelope peaks at {max}; the plateau is 1.0",
            raw.tier
        )));
    }
    let first_max = keyframes.iter().position(|k| k.value == max).unwrap();
    let last_max = keyframes.iter().rposition(|k| k.value == max).unwrap();
    for pair in keyframes[..=first_max].windows(2) {
        if pair[1].value < pair[0].value - EPS {
            return Err(spec_err(format!(
                "tier {} master envelope falls before its plateau",
                raw.tier
            )));
        }
    }
    for pair in keyframes[last_max..].windows(2) {
        if pair[1].value > pair[0].value + EPS {
            return Err(spec_err(format!(
                "tier {} master envelope rises after its plateau — the envelope progresses once and ends, it does not loop",
                raw.tier
            )));
        }
    }
    // The release hands over to the taper law at the
    // package's release-end value, at the performance
    // boundary.
    let at_performance_end = keyframes
        .iter()
        .find(|k| (k.t - raw.performance_seconds).abs() <= EPS);
    match at_performance_end {
        Some(keyframe) if (keyframe.value - release_end_value).abs() <= 1e-6 => {}
        _ => {
            return Err(spec_err(format!(
                "tier {} master envelope has no release-end keyframe of {release_end_value} at {} s",
                raw.tier, raw.performance_seconds
            )));
        }
    }
    Ok(keyframes)
}

/// Validates the face path's order and canon law.
fn validate_face_path(raw: &RawTier) -> CompanionResult<Vec<FaceSegment>> {
    let want = [
        Segment::Onset,
        Segment::PeakHold,
        Segment::Release,
        Segment::Taper,
    ];
    if raw.face_path.len() != want.len() {
        return Err(spec_err(format!(
            "tier {} face path has {} segments; onset, peak hold, release, and taper are required",
            raw.tier,
            raw.face_path.len()
        )));
    }
    let mut segments: Vec<FaceSegment> = Vec::with_capacity(want.len());
    for (raw_segment, want_segment) in raw.face_path.iter().zip(want.iter()) {
        let segment = Segment::from_token(&raw_segment.segment)?;
        if segment != *want_segment {
            return Err(spec_err(format!(
                "tier {} face path segment {} is out of order (expected {})",
                raw.tier,
                raw_segment.segment,
                want_segment.token()
            )));
        }
        let (canon_from, canon_to) = match segment {
            Segment::Release => {
                let transition = raw_segment.transition.as_ref().ok_or_else(|| {
                    spec_err(format!(
                        "tier {} release segment carries no transition",
                        raw.tier
                    ))
                })?;
                (transition[0].clone(), transition[1].clone())
            }
            _ => {
                let state = raw_segment.state.clone().ok_or_else(|| {
                    spec_err(format!(
                        "tier {} segment {} carries no state",
                        raw.tier, raw_segment.segment
                    ))
                })?;
                (state.clone(), state)
            }
        };
        segments.push(FaceSegment {
            segment,
            governed_name: raw_segment.governed_name.clone(),
            canon_from,
            canon_to,
            t_start: raw_segment.t_start,
            t_end: raw_segment.t_end,
        });
    }
    // Contiguity over [0, total].
    if segments[0].t_start != 0.0 || (segments[3].t_end - raw.total_seconds).abs() > EPS {
        return Err(spec_err(format!(
            "tier {} face path does not span [0, {}]",
            raw.tier, raw.total_seconds
        )));
    }
    for pair in segments.windows(2) {
        if (pair[1].t_start - pair[0].t_end).abs() > EPS || pair[0].t_end <= pair[0].t_start {
            return Err(spec_err(format!(
                "tier {} face path segments are not contiguous",
                raw.tier
            )));
        }
    }
    // Canon law: F-03 approach, one peak convention,
    // release into F-10, F-10 taper. A peak that mixes
    // conventions refuses here.
    let onset = &segments[0];
    let peak = &segments[1];
    let release = &segments[2];
    let taper = &segments[3];
    if onset.canon_from != STATE_APPROACH {
        return Err(spec_err(format!(
            "tier {} approach state is {}; the face path opens on F-03",
            raw.tier, onset.canon_from
        )));
    }
    if peak.canon_from != STATE_PEAK_ROLL && peak.canon_from != STATE_PEAK_WHITE_ROLL {
        return Err(spec_err(format!(
            "tier {} peak state {} is not a defined peak convention (F-05 or N-05)",
            raw.tier, peak.canon_from
        )));
    }
    if release.canon_from != peak.canon_from || release.canon_to != STATE_AFTERGLOW {
        return Err(spec_err(format!(
            "tier {} release transition {} → {} does not carry the peak ({}) into F-10 — peak conventions never blend inside one performance",
            raw.tier, release.canon_from, release.canon_to, peak.canon_from
        )));
    }
    if taper.canon_from != STATE_AFTERGLOW {
        return Err(spec_err(format!(
            "tier {} taper state is {}; the taper holds F-10",
            raw.tier, taper.canon_from
        )));
    }
    if onset.governed_name != "near" || taper.governed_name != "afterglow" {
        return Err(spec_err(format!(
            "tier {} governed names ({} / {}) are not the §6.5 path names (near / afterglow)",
            raw.tier, onset.governed_name, taper.governed_name
        )));
    }
    if peak.governed_name != raw.channels.governed_peak_expression {
        return Err(spec_err(format!(
            "tier {} peak governed name {} disagrees with the channel table's {}",
            raw.tier, peak.governed_name, raw.channels.governed_peak_expression
        )));
    }
    Ok(segments)
}

// ---------------------------------------------------------------- expressions

/// A character's governed expression map: canon face
/// state → the state name in *her* expression record.
/// Resolution is exact — a canon state her record does
/// not carry refuses typed (§6.5: another character's
/// recipe is never substituted).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExpressionMap {
    by_canon: BTreeMap<String, String>,
}

impl ExpressionMap {
    /// Builds a map from `(canon state, rig state)`
    /// pairs.
    pub fn new(entries: impl IntoIterator<Item = (String, String)>) -> ExpressionMap {
        ExpressionMap {
            by_canon: entries.into_iter().collect(),
        }
    }

    /// Prima's governed record: her ten-state record
    /// carries the F-code states under their own names
    /// (the cast-rig record's target list). N-05 is not
    /// in her record — the white-roll convention is the
    /// Marsha-corpus idiom, and an N-05 performance for
    /// Prima refuses at resolution rather than borrowing
    /// another character's state.
    pub fn prima() -> ExpressionMap {
        ExpressionMap::new(
            [
                "F-01", "F-02", "F-03", "F-04", "F-05", "F-06", "F-07", "F-08", "F-09", "F-10",
            ]
            .into_iter()
            .map(|code| (code.to_string(), code.to_string())),
        )
    }

    /// Resolves one canon state to her rig state name.
    pub fn resolve(&self, canon_state: &str) -> CompanionResult<&str> {
        self.by_canon.get(canon_state).map(String::as_str).ok_or_else(|| {
            CompanionError::new(
                "TG-COMP-CULMINATION-EXPRESSION",
                format!(
                    "the character's governed expression record carries no state {canon_state}; another character's recipe is never substituted (§6.5)"
                ),
            )
        })
    }
}

// ---------------------------------------------------------------- tick output

/// What a curve value means (the consumer's contract).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CurveUnit {
    /// 0..=1 of the parameter's own range.
    Normalized,
    /// Fraction of the rig template's range for the
    /// parameter; the template supplies the range.
    RangeFraction,
    /// A depth scale on the ongoing breath
    /// (1.0 = template breath).
    DepthScale,
    /// An amplitude scale on the character's own
    /// secondary motion.
    AmplitudeScale,
}

/// One rig parameter curve sample for one tick.
#[derive(Debug, Clone, PartialEq)]
pub struct ParamCurveSample {
    /// The parameter's canonical (base) name; the rig
    /// binding resolves it to the model's instance id.
    pub parameter: &'static str,
    /// The sampled value, in `unit`.
    pub value: f64,
    /// The value's unit contract.
    pub unit: CurveUnit,
}

/// The body-cycle descriptor: evaluator-side, because
/// the rig carries no literal body-amplitude parameter
/// (package SPEC §6.3).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BodyCycleDescriptor {
    /// Amplitude scale on the body-cycle oscillator
    /// (§5.7 peak × master envelope).
    pub amplitude_scale: f64,
    /// The MG-02 period band the evaluator picks
    /// within, seconds.
    pub period_band_seconds: [f64; 2],
}

/// The tremble descriptor (§6.3 application: a bounded
/// offset to `BodyRoot` and the head anchor, applied by
/// the runtime, never entering the attachment solve).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TrembleDescriptor {
    /// Amplitude at this tick, px at the 1024 canvas
    /// (§5.7 peak × master envelope).
    pub amplitude_px_at_1024: f64,
    /// Rate, Hz (the tier's, constant across the
    /// performance).
    pub rate_hz: f64,
}

/// The face segment marker for one tick: the segment,
/// its canon states, and their resolution through the
/// character's governed expression map.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FaceMarker {
    /// The segment in play.
    pub segment: Segment,
    /// The governed name (§6.5) for the segment.
    pub governed_name: String,
    /// The canon state at the segment's start.
    pub canon_from: String,
    /// The canon state at the segment's end.
    pub canon_to: String,
    /// `canon_from` resolved through the map.
    pub rig_from: String,
    /// `canon_to` resolved through the map.
    pub rig_to: String,
}

/// One tick of culmination playback: everything the
/// presentation applies for that fixed step.
#[derive(Debug, Clone, PartialEq)]
pub struct PerformanceTick {
    /// The episode the performance belongs to.
    pub episode: u32,
    /// The frozen tier.
    pub tier: u8,
    /// Playback tick, 0-based from the firing tick.
    pub tick: u64,
    /// Seconds from the firing tick.
    pub t_seconds: f64,
    /// The engine phase this tick was emitted under
    /// (`Culminating` or `Taper`).
    pub phase: Phase,
    /// The master envelope value at this tick.
    pub master: f64,
    /// Rig parameter curves, in parameter-name order.
    pub curves: Vec<ParamCurveSample>,
    /// The body-cycle descriptor.
    pub body_cycle: BodyCycleDescriptor,
    /// The tremble descriptor.
    pub tremble: TrembleDescriptor,
    /// The face segment marker.
    pub face: FaceMarker,
    /// The Stage 2.2 peak eye targets, present on
    /// peak-hold ticks of tiers 3–4 under the roll
    /// convention; `None` otherwise (under white-roll
    /// there is no iris to gate).
    pub peak_eye_target: Option<PeakEyeTarget>,
}

/// How a playback ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackOutcome {
    /// Still playing.
    Playing,
    /// The envelope completed (phase left
    /// `Culminating`/`Taper` with every tick emitted).
    Finished,
    /// Cancelled mid-performance (remove-all / reset:
    /// the phase keying or the direct seam).
    Cancelled,
}

// ---------------------------------------------------------------- player

/// The Stage 2.2 culmination player: subscribes to the
/// arousal engine's culmination events and plays the
/// frozen tier's envelope from the loaded package.
///
/// Discipline:
///
/// - No package loaded → a culmination event refuses
///   (`TG-COMP-CULMINATION-NOENVELOPE`); nothing plays
///   on a guess.
/// - The tier is frozen at the firing tick; a second
///   culmination event mid-performance refuses
///   (`TG-COMP-CULMINATION-FROZEN`) and the playing
///   performance continues untouched.
/// - The peak convention is declared per performance
///   before it starts; changing it mid-performance
///   refuses on the same code.
/// - Playback is keyed to the engine phase (see the
///   module docs).
#[derive(Debug)]
pub struct CulminationPlayer {
    package: Option<CulminationPackage>,
    expressions: ExpressionMap,
    convention: PeakConvention,
    active: Option<ActivePerformance>,
    outcome: PlaybackOutcome,
}

/// One in-flight performance.
#[derive(Debug)]
struct ActivePerformance {
    episode: u32,
    plan: PerformancePlan,
    elapsed_ticks: u64,
}

impl CulminationPlayer {
    /// A player with no package loaded, resolving faces
    /// through `expressions`.
    pub fn new(expressions: ExpressionMap) -> CulminationPlayer {
        CulminationPlayer {
            package: None,
            expressions,
            convention: PeakConvention::Roll,
            active: None,
            outcome: PlaybackOutcome::Finished,
        }
    }

    /// A player with the package already loaded.
    pub fn with_package(
        package: CulminationPackage,
        expressions: ExpressionMap,
    ) -> CulminationPlayer {
        let mut player = CulminationPlayer::new(expressions);
        player.package = Some(package);
        player
    }

    /// Loads (or replaces) the performance package.
    /// Refused while a performance is playing: the data
    /// under a frozen performance never changes.
    pub fn load_package(&mut self, package: CulminationPackage) -> CompanionResult<()> {
        if self.active.is_some() {
            return Err(frozen_err(
                "cannot load a package while a performance is playing",
            ));
        }
        self.package = Some(package);
        Ok(())
    }

    /// Whether a package is loaded.
    pub fn has_package(&self) -> bool {
        self.package.is_some()
    }

    /// Declares the peak convention for the next
    /// performance. Refused while one is playing.
    pub fn set_peak_convention(&mut self, convention: PeakConvention) -> CompanionResult<()> {
        if self.active.is_some() {
            return Err(frozen_err(
                "the peak convention is frozen for the playing performance",
            ));
        }
        self.convention = convention;
        Ok(())
    }

    /// The declared peak convention.
    pub fn peak_convention(&self) -> PeakConvention {
        self.convention
    }

    /// Whether a performance is playing.
    pub fn is_playing(&self) -> bool {
        self.active.is_some()
    }

    /// How the last playback ended (or `Playing`).
    pub fn outcome(&self) -> PlaybackOutcome {
        if self.active.is_some() {
            PlaybackOutcome::Playing
        } else {
            self.outcome
        }
    }

    /// The frozen tier of the playing performance.
    pub fn active_tier(&self) -> Option<u8> {
        self.active.as_ref().map(|active| active.plan.tier())
    }

    /// Feeds one tick's engine events. A culmination
    /// event starts playback of its frozen tier; events
    /// are the engine's, drained from `EngineOutput`.
    pub fn observe(&mut self, events: &[ArousalEvent]) -> CompanionResult<()> {
        for event in events {
            if let ArousalEvent::Culmination {
                episode,
                tier,
                p_permille: _,
            } = event
            {
                self.start(*episode, *tier)?;
            }
        }
        Ok(())
    }

    fn start(&mut self, episode: u32, tier: u8) -> CompanionResult<()> {
        if let Some(active) = &self.active {
            return Err(frozen_err(format!(
                "culmination for episode {episode} tier {tier} arrived while episode {} tier {} is playing; the frozen performance continues",
                active.episode,
                active.plan.tier()
            )));
        }
        let Some(package) = &self.package else {
            return Err(CompanionError::new(
                "TG-COMP-CULMINATION-NOENVELOPE",
                format!(
                    "culmination (episode {episode}, tier {tier}) fired with no culmination package loaded; nothing plays on a guess"
                ),
            ));
        };
        let plan = package.plan(tier, self.convention)?;
        self.active = Some(ActivePerformance {
            episode,
            plan,
            elapsed_ticks: 0,
        });
        self.outcome = PlaybackOutcome::Playing;
        Ok(())
    }

    /// The §3.4 remove-all seam: cancels playback in
    /// the same tick, unconditionally. (The phase
    /// keying in `step` enforces the same cancellation
    /// when the shell does not call this.)
    pub fn notify_remove_all(&mut self) {
        if self.active.take().is_some() {
            self.outcome = PlaybackOutcome::Cancelled;
        }
    }

    /// Advances playback by one fixed step against the
    /// engine's current phase. Returns the tick to
    /// apply, or `None` when nothing plays (idle,
    /// finished, or cancelled on this step). A face the
    /// character's record cannot resolve refuses typed
    /// and cancels the performance — nothing is ever
    /// substituted (§6.5).
    pub fn step(&mut self, phase: Phase) -> CompanionResult<Option<PerformanceTick>> {
        let Some(active) = self.active.as_ref() else {
            return Ok(None);
        };
        if phase != Phase::Culminating && phase != Phase::Taper {
            // Keyed to the engine phase: leaving the two
            // performance phases ends playback — finished
            // when every tick was emitted, cancelled
            // otherwise (the §5.8 reset lands here).
            let total = active.plan.total_ticks();
            let emitted = active.elapsed_ticks;
            self.active = None;
            self.outcome = if emitted >= total {
                PlaybackOutcome::Finished
            } else {
                PlaybackOutcome::Cancelled
            };
            return Ok(None);
        }
        let (tick, elapsed) = {
            let active = self.active.as_ref().expect("checked above");
            let envelope = active.plan.envelope();
            let t_seconds = active.elapsed_ticks as f64 / TICKS_PER_SECOND as f64;
            let master = sample_master(envelope, active.plan.tau_a_seconds, t_seconds);
            let segment = segment_at(envelope, t_seconds).clone();
            let tick = match build_tick(
                active,
                envelope,
                &segment,
                t_seconds,
                master,
                phase,
                &self.expressions,
            ) {
                Ok(tick) => tick,
                Err(err) => {
                    self.active = None;
                    self.outcome = PlaybackOutcome::Cancelled;
                    return Err(err);
                }
            };
            (tick, active.elapsed_ticks)
        };
        if let Some(active) = self.active.as_mut() {
            active.elapsed_ticks = elapsed + 1;
        }
        Ok(Some(tick))
    }

    /// Pre-resolves the face marker for a plan at `t`
    /// seconds — the surface through which expression
    /// refusals are reported before playback starts.
    pub fn face_marker(
        &self,
        plan: &PerformancePlan,
        t_seconds: f64,
    ) -> CompanionResult<FaceMarker> {
        let envelope = plan.envelope();
        let segment = segment_at(envelope, t_seconds);
        face_marker(segment, &self.expressions)
    }
}

fn frozen_err(message: impl Into<String>) -> CompanionError {
    CompanionError::new("TG-COMP-CULMINATION-FROZEN", message.into())
}

/// The face segment in play at `t` seconds.
fn segment_at(envelope: &TierEnvelope, t: f64) -> &FaceSegment {
    for segment in &envelope.face_path {
        if t >= segment.t_start && t < segment.t_end {
            return segment;
        }
    }
    &envelope.face_path[envelope.face_path.len() - 1]
}

/// Resolves one segment's face marker through the
/// character's governed expression map.
fn face_marker(segment: &FaceSegment, expressions: &ExpressionMap) -> CompanionResult<FaceMarker> {
    Ok(FaceMarker {
        segment: segment.segment,
        governed_name: segment.governed_name.clone(),
        canon_from: segment.canon_from.clone(),
        canon_to: segment.canon_to.clone(),
        rig_from: expressions.resolve(&segment.canon_from)?.to_string(),
        rig_to: expressions.resolve(&segment.canon_to)?.to_string(),
    })
}

/// Builds one playback tick from the plan at `t`.
fn build_tick(
    active: &ActivePerformance,
    envelope: &TierEnvelope,
    segment: &FaceSegment,
    t_seconds: f64,
    master: f64,
    phase: Phase,
    expressions: &ExpressionMap,
) -> CompanionResult<PerformanceTick> {
    let channels = &envelope.channels;
    let curves = vec![
        ParamCurveSample {
            parameter: "ParamBodyAngleY",
            value: channels.posture_excursion_fraction * master,
            unit: CurveUnit::RangeFraction,
        },
        ParamCurveSample {
            parameter: "ParamBreath",
            value: 1.0 + (channels.breath_depth_scale - 1.0) * master,
            unit: CurveUnit::DepthScale,
        },
        ParamCurveSample {
            parameter: "ParamBreastY",
            value: channels.secondary_amplitude_scale * master,
            unit: CurveUnit::AmplitudeScale,
        },
        ParamCurveSample {
            parameter: "ParamEyeCross",
            value: channels.eye_cross_peak * master,
            unit: CurveUnit::Normalized,
        },
        ParamCurveSample {
            parameter: "ParamEyeRoll",
            value: channels.eye_roll_peak * master,
            unit: CurveUnit::Normalized,
        },
        ParamCurveSample {
            parameter: "ParamHairSway1",
            value: channels.secondary_amplitude_scale * master,
            unit: CurveUnit::AmplitudeScale,
        },
    ];
    Ok(PerformanceTick {
        episode: active.episode,
        tier: envelope.tier,
        tick: active.elapsed_ticks,
        t_seconds,
        phase,
        master,
        curves,
        body_cycle: BodyCycleDescriptor {
            amplitude_scale: channels.body_cycle_amplitude_scale * master,
            period_band_seconds: active.plan.body_cycle_period_band,
        },
        tremble: TrembleDescriptor {
            amplitude_px_at_1024: channels.tremble_amplitude_px * master,
            rate_hz: channels.tremble_rate_hz,
        },
        face: face_marker(segment, expressions)?,
        peak_eye_target: if segment.segment == Segment::PeakHold {
            envelope.peak_eye_targets
        } else {
            None
        },
    })
}

// ---------------------------------------------------------------- tests

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    /// A valid package document, generated from the
    /// engine's own §5.7 table plus the package's
    /// authored shapes (30/40/30 segments, smoothstep
    /// onset, release to 0.35, exponential taper) — the
    /// same construction the shipped document carries.
    fn valid_document() -> Value {
        let governed = ["aroused", "building", "peak", "peak"];
        let p_ranges = [[0.0, 0.29], [0.30, 0.54], [0.55, 0.79], [0.80, 1.00]];
        let mut tiers = Vec::new();
        for (index, spec) in TIERS.iter().enumerate() {
            let performance = spec.culminating_ticks as f64 / TICKS_PER_SECOND as f64;
            let taper = spec.taper_ticks as f64 / TICKS_PER_SECOND as f64;
            let total = performance + taper;
            let onset = performance * 0.3;
            let hold = performance * 0.4;
            let peak_end = onset + hold;
            let mid_taper = performance + taper / 2.0;
            let mid_value = (0.35 * (-(taper / 2.0) / 12.0).exp() * 10_000.0).round() / 10_000.0;
            let permille = |v: u32| f64::from(v) / 1000.0;
            tiers.push(json!({
                "tier": index + 1,
                "p_range": p_ranges[index],
                "performance_seconds": performance,
                "taper_seconds": taper,
                "total_seconds": total,
                "channels": {
                    "body_cycle_amplitude_scale": permille(spec.body_amplitude),
                    "posture_excursion_fraction": permille(spec.posture),
                    "tremble_amplitude_px": permille(spec.tremble_milli_px),
                    "tremble_rate_hz": permille(spec.tremble_rate_millihz),
                    "breath_depth_scale": permille(spec.breath_scale),
                    "eye_roll_peak": permille(spec.eye_roll),
                    "eye_cross_peak": permille(spec.eye_cross),
                    "secondary_amplitude_scale": permille(spec.secondary_scale),
                    "governed_peak_expression": governed[index],
                },
                "face_path": [
                    { "segment": "onset", "governed_name": "near", "state": "F-03",
                      "t_start": 0.0, "t_end": onset },
                    { "segment": "peak_hold", "governed_name": governed[index], "state": "F-05",
                      "t_start": onset, "t_end": peak_end },
                    { "segment": "release", "governed_name": "afterglow",
                      "transition": ["F-05", "F-10"],
                      "t_start": peak_end, "t_end": performance },
                    { "segment": "taper", "governed_name": "afterglow", "state": "F-10",
                      "t_start": performance, "t_end": total },
                ],
                "master_envelope": [
                    { "t": 0.0, "value": 0.0, "interpolation": "smoothstep" },
                    { "t": onset, "value": 1.0, "interpolation": "hold" },
                    { "t": peak_end, "value": 1.0, "interpolation": "smoothstep" },
                    { "t": performance, "value": 0.35, "interpolation": "exponential" },
                    { "t": mid_taper, "value": mid_value, "interpolation": "exponential" },
                    { "t": total, "value": 0.0, "settle": true },
                ],
                "peak_eye_targets": if index >= 2 {
                    json!({ "visible_iris_fraction": 0.275, "sclera_field": 0.68 })
                } else {
                    Value::Null
                },
            }));
        }
        json!({
            "format": "torogao-culmination/1",
            "format_version": 1,
            "loop": false,
            "peak_convention_default": "F-05",
            "peak_eye_bands": {
                "visible_iris_fraction": [0.20, 0.35],
                "sclera_field": [0.63, 0.73],
                "applies_to_tiers": [3, 4],
                "applies_to_convention": "F-05",
            },
            "decay": { "tau_a_seconds": 12, "tau_q_seconds": 20 },
            "body_cycle_period": { "band_seconds": [0.5, 0.8] },
            "release_end_value": 0.35,
            "tiers": tiers,
        })
    }

    fn valid_package() -> CulminationPackage {
        CulminationPackage::from_json(&valid_document().to_string())
            .expect("the generated document is the §5.7 table")
    }

    fn culmination_event(episode: u32, tier: u8) -> ArousalEvent {
        ArousalEvent::Culmination {
            episode,
            tier,
            p_permille: 700,
        }
    }

    #[test]
    fn valid_document_loads_and_agrees_with_the_engine_table() {
        let package = valid_package();
        let tier3 = package.tier(3).expect("tier 3");
        assert_eq!(tier3.performance_seconds, 11.0);
        assert_eq!(tier3.taper_seconds, 9.0);
        assert_eq!(tier3.total_seconds, 20.0);
        assert_eq!(tier3.channels.eye_roll_peak, 0.85);
        let targets = tier3.peak_eye_targets.expect("T3 carries targets");
        let (iris_band, sclera_band) = package.eye_bands();
        assert!(iris_band[0] <= targets.visible_iris_fraction);
        assert!(targets.visible_iris_fraction <= iris_band[1]);
        assert!(sclera_band[0] <= targets.sclera_field);
        assert!(targets.sclera_field <= sclera_band[1]);
        assert!(package.tier(2).expect("tier 2").peak_eye_targets.is_none());
        assert_eq!(package.document_convention(), PeakConvention::Roll);
        assert!(package.tier(5).is_err());
    }

    #[test]
    fn sampler_follows_the_package_law() {
        let package = valid_package();
        // Starts at 0, settles at exactly 0 at the total.
        assert_eq!(package.sample_master(3, 0.0).unwrap(), 0.0);
        assert_eq!(package.sample_master(3, 20.0).unwrap(), 0.0);
        assert_eq!(package.sample_master(3, 25.0).unwrap(), 0.0);
        // Peak plateau through the hold.
        assert_eq!(package.sample_master(3, 5.0).unwrap(), 1.0);
        // Release ends at the release-end value.
        let at_end = package.sample_master(3, 11.0).unwrap();
        assert!((at_end - 0.35).abs() < 1e-9, "release end {at_end}");
        // The stored mid-taper keyframe lies on the
        // sampled curve.
        let mid = package.sample_master(3, 15.5).unwrap();
        assert!((mid - 0.2406).abs() < 1e-3, "mid taper {mid}");
        // Non-increasing across release + taper (the
        // envelope progresses once and ends).
        let mut previous = f64::MAX;
        for step in 0..=180 {
            let t = 7.7 + (20.0 - 7.7) * f64::from(step) / 180.0;
            let value = package.sample_master(3, t).unwrap();
            assert!(
                value <= previous + 1e-12,
                "rises at {t}: {value} > {previous}"
            );
            previous = value;
        }
    }

    #[test]
    fn looping_document_is_refused() {
        let mut doc = valid_document();
        doc["loop"] = json!(true);
        let err = CulminationPackage::from_json(&doc.to_string()).expect_err("loop refuses");
        assert_eq!(err.code, "TG-COMP-CULMINATION-FORMAT");
    }

    #[test]
    fn blended_peak_document_is_refused() {
        // Peak holds F-05 but the release starts from
        // N-05: two conventions in one performance.
        let mut doc = valid_document();
        doc["tiers"][2]["face_path"][2]["transition"] = json!(["N-05", "F-10"]);
        let err = CulminationPackage::from_json(&doc.to_string()).expect_err("blend refuses");
        assert_eq!(err.code, "TG-COMP-CULMINATION-SPEC");
        // And the mirror image: N-05 peak releasing from
        // F-05.
        let mut doc = valid_document();
        doc["tiers"][2]["face_path"][1]["state"] = json!("N-05");
        doc["tiers"][2]["peak_eye_targets"] = Value::Null;
        let err = CulminationPackage::from_json(&doc.to_string()).expect_err("blend refuses");
        assert_eq!(err.code, "TG-COMP-CULMINATION-SPEC");
    }

    #[test]
    fn wrong_duration_and_missing_taper_are_refused() {
        let mut doc = valid_document();
        doc["tiers"][2]["performance_seconds"] = json!(10.0);
        let err = CulminationPackage::from_json(&doc.to_string()).expect_err("duration refuses");
        assert_eq!(err.code, "TG-COMP-CULMINATION-SPEC");

        let mut doc = valid_document();
        doc["tiers"][0]["face_path"]
            .as_array_mut()
            .unwrap()
            .remove(3);
        let err =
            CulminationPackage::from_json(&doc.to_string()).expect_err("missing taper refuses");
        assert_eq!(err.code, "TG-COMP-CULMINATION-SPEC");
    }

    #[test]
    fn out_of_band_eye_target_is_refused() {
        let mut doc = valid_document();
        doc["tiers"][3]["peak_eye_targets"]["visible_iris_fraction"] = json!(0.50);
        let err = CulminationPackage::from_json(&doc.to_string()).expect_err("band refuses");
        assert_eq!(err.code, "TG-COMP-CULMINATION-SPEC");
    }

    #[test]
    fn envelope_rising_after_its_plateau_is_refused() {
        let mut doc = valid_document();
        doc["tiers"][0]["master_envelope"][4]["value"] = json!(0.9);
        let err = CulminationPackage::from_json(&doc.to_string()).expect_err("rise refuses");
        assert_eq!(err.code, "TG-COMP-CULMINATION-SPEC");
    }

    #[test]
    fn event_without_a_package_refuses() {
        let mut player = CulminationPlayer::new(ExpressionMap::prima());
        let err = player
            .observe(&[culmination_event(1, 3)])
            .expect_err("no envelope loaded refuses");
        assert_eq!(err.code, "TG-COMP-CULMINATION-NOENVELOPE");
        assert!(!player.is_playing());
    }

    #[test]
    fn second_event_mid_performance_refuses_and_playback_continues() {
        let mut player = CulminationPlayer::with_package(valid_package(), ExpressionMap::prima());
        player.observe(&[culmination_event(1, 3)]).expect("starts");
        for _ in 0..30 {
            player
                .step(Phase::Culminating)
                .expect("plays")
                .expect("tick");
        }
        let err = player
            .observe(&[culmination_event(2, 4)])
            .expect_err("a second, different-tier event refuses");
        assert_eq!(err.code, "TG-COMP-CULMINATION-FROZEN");
        assert_eq!(player.active_tier(), Some(3), "the frozen tier continues");
        let tick = player
            .step(Phase::Culminating)
            .expect("still plays")
            .expect("tick");
        assert_eq!(tick.tier, 3);
        // The convention is frozen too.
        let err = player
            .set_peak_convention(PeakConvention::WhiteRoll)
            .expect_err("convention change refuses");
        assert_eq!(err.code, "TG-COMP-CULMINATION-FROZEN");
    }

    #[test]
    fn n05_plan_swaps_the_whole_peak_and_nulls_the_eye_targets() {
        let package = valid_package();
        let plan = package.plan(4, PeakConvention::WhiteRoll).expect("plan");
        let envelope = plan.envelope();
        assert_eq!(envelope.face_path[0].canon_from, "F-03");
        assert_eq!(envelope.face_path[1].canon_from, "N-05");
        assert_eq!(envelope.face_path[2].canon_from, "N-05");
        assert_eq!(envelope.face_path[2].canon_to, "F-10");
        assert_eq!(envelope.face_path[3].canon_from, "F-10");
        assert!(envelope.peak_eye_targets.is_none());
        // The F-05 plan is untouched.
        let roll = package.plan(4, PeakConvention::Roll).expect("roll plan");
        assert_eq!(roll.envelope().face_path[1].canon_from, "F-05");
        assert!(roll.envelope().peak_eye_targets.is_some());
    }

    #[test]
    fn prima_record_refuses_the_white_roll_state() {
        // Prima's governed record carries no N-05:
        // resolving a white-roll peak for her refuses
        // instead of substituting (§6.5).
        let package = valid_package();
        let plan = package.plan(3, PeakConvention::WhiteRoll).expect("plan");
        let player = CulminationPlayer::with_package(package, ExpressionMap::prima());
        let err = player
            .face_marker(&plan, 5.0)
            .expect_err("N-05 is not in Prima's record");
        assert_eq!(err.code, "TG-COMP-CULMINATION-EXPRESSION");
    }

    #[test]
    fn white_roll_plays_for_a_record_that_carries_it() {
        let mut map = ExpressionMap::prima();
        map.by_canon.insert("N-05".to_string(), "N-05".to_string());
        let mut player = CulminationPlayer::with_package(valid_package(), map);
        player
            .set_peak_convention(PeakConvention::WhiteRoll)
            .expect("declare before start");
        player.observe(&[culmination_event(1, 1)]).expect("starts");
        // Tier 1: onset is 72 ticks; the peak follows.
        let mut peak_tick = None;
        for _ in 0..200 {
            let tick = player
                .step(Phase::Culminating)
                .expect("plays")
                .expect("tick");
            if tick.face.segment == Segment::PeakHold {
                peak_tick = Some(tick);
                break;
            }
        }
        let peak_tick = peak_tick.expect("reached the peak");
        assert_eq!(peak_tick.face.canon_from, "N-05");
        assert!(peak_tick.peak_eye_target.is_none());
    }

    #[test]
    fn remove_all_cancels_within_one_tick() {
        // Direct seam.
        let mut player = CulminationPlayer::with_package(valid_package(), ExpressionMap::prima());
        player.observe(&[culmination_event(1, 4)]).expect("starts");
        for _ in 0..60 {
            player
                .step(Phase::Culminating)
                .expect("plays")
                .expect("tick");
        }
        player.notify_remove_all();
        assert!(!player.is_playing());
        assert_eq!(player.outcome(), PlaybackOutcome::Cancelled);
        assert!(
            player.step(Phase::Baseline).expect("no tick").is_none(),
            "no curve is emitted after remove-all"
        );
        // Phase keying alone (no direct seam call): the
        // §5.8 reset lands in Baseline and the next step
        // cancels.
        let mut player = CulminationPlayer::with_package(valid_package(), ExpressionMap::prima());
        player.observe(&[culmination_event(1, 4)]).expect("starts");
        player
            .step(Phase::Culminating)
            .expect("plays")
            .expect("tick");
        assert!(player.step(Phase::Baseline).expect("no tick").is_none());
        assert_eq!(player.outcome(), PlaybackOutcome::Cancelled);
    }

    #[test]
    fn full_playback_finishes_with_every_tick_emitted() {
        // Tier 1: 240 ticks Culminating + 240 Taper on
        // the synthetic phase feed (the real engine
        // drives the same counts in the harness).
        let mut player = CulminationPlayer::with_package(valid_package(), ExpressionMap::prima());
        player.observe(&[culmination_event(1, 1)]).expect("starts");
        let mut emitted = 0u64;
        let mut segments: Vec<Segment> = Vec::new();
        for _ in 0..240 {
            let tick = player
                .step(Phase::Culminating)
                .expect("plays")
                .expect("tick");
            emitted += 1;
            if segments.last() != Some(&tick.face.segment) {
                segments.push(tick.face.segment);
            }
        }
        for _ in 0..240 {
            let tick = player.step(Phase::Taper).expect("plays").expect("tick");
            emitted += 1;
            if segments.last() != Some(&tick.face.segment) {
                segments.push(tick.face.segment);
            }
        }
        assert_eq!(emitted, 480);
        assert_eq!(
            segments,
            [
                Segment::Onset,
                Segment::PeakHold,
                Segment::Release,
                Segment::Taper
            ]
        );
        assert!(player.step(Phase::Afterglow).expect("no tick").is_none());
        assert_eq!(player.outcome(), PlaybackOutcome::Finished);
    }
}
