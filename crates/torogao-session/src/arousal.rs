//! The persistent arousal engine (`arousal-engine/1`,
//! AMENDMENT-T4 §§5–6): a deterministic fixed-step state
//! machine, one instance per character, living in the
//! session-host layer so the Playroom and (later) the
//! desktop companion present the same state, never two
//! stores (AMENDMENT-T4 §3.3).
//!
//! ## Numeric contract (§5.1)
//!
//! Stored state is integer micro-permille (1 permille =
//! 1_000_000 micro-permille; full scale 1000 permille =
//! 1_000_000_000). Integration is the spec's Euler form at
//! dt = 1/60, in pure integer arithmetic — no float ever
//! touches the state, so the canonical trajectory (§5.11)
//! is bit-identical on every machine. Every division of a
//! non-negative value rounds half-up; signed deltas round
//! half away from zero. That is the specified rounding rule
//! §5.1 requires.
//!
//! ## Constants (D-211 erratum)
//!
//! The integration constants are the D-211 amended set,
//! recorded as a dated erratum in AMENDMENT-T4 §5.3: charge
//! 150, Q leak 200 s, A-curve 4.6, dose law 140·I^2.5. The
//! §8 fixtures stand as written; the set was re-simulated
//! against the full §8 battery before implementation
//! (`t4-conflict-check/sim_d211.py`, GATE-T42).
//!
//! ## Admission (§5.10)
//!
//! Wearing, removing, device changes, and reset are
//! session operations. No authoring verb owns them, so —
//! by this crate's own AUTH mapping discipline (see
//! `gate`) — they are session-local: validated here under
//! the session-local rules (the §2.5/§5 bounds) and
//! appended to the host's op log with the engine state
//! hash. The two safety exits (device stop, remove-all +
//! reset) are never refused and never queued: they apply
//! in the calling act and are logged as completed acts
//! (§5.10's ordering exception).

use crate::refusal::{R, Refusal};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The engine data version (AMENDMENT-T4 §5.3).
pub const ENGINE_FORMAT: &str = "arousal-engine/1";

/// Micro-permille in one permille.
pub const MP_PER_PERMILLE: i64 = 1_000_000;
/// Micro-permille at full scale (1000 permille).
pub const MP_FULL: i64 = 1_000_000_000;
/// Fixed-step rate: 60 steps per second (§4.1).
pub const TICKS_PER_SECOND: u64 = 60;

/// D-211 charge coefficient (permille/s at I = 1, S = 1000).
const CHARGE_PERMILLE_PER_S: i64 = 150;
/// D-211 Q leak time constant, seconds.
const Q_LEAK_TAU_S: i64 = 200;
/// Presence charge rate (permille/s) for floor gear with no
/// active stimulation; the §5.3 cap (250) is the binding
/// constraint, enforced as a clamp.
const PRESENCE_PERMILLE_PER_S: i64 = 5;
/// §5.3 presence cap: Q from presence alone never exceeds.
const PRESENCE_CAP_MP: i64 = 250 * MP_PER_PERMILLE;
/// Q storage ceiling (§5.2: stored to 1500).
const Q_MAX_MP: i64 = 1500 * MP_PER_PERMILLE;
/// Sensitivity storage bounds (§5.2).
const S_MIN_MP: i64 = 500 * MP_PER_PERMILLE;
const S_MAX_MP: i64 = 1500 * MP_PER_PERMILLE;
/// §5.6 re-arm threshold: Q must be below this (permille).
const REARM_Q_PERMILLE: i64 = 150;
/// Default afterglow hold (§5.5: 30 s).
const AFTERGLOW_DEFAULT_TICKS: u64 = 30 * TICKS_PER_SECOND;
/// Default refractory duration (§5.5: 45 s).
const REFRACTORY_DEFAULT_TICKS: u64 = 45 * TICKS_PER_SECOND;
/// Profile duration bounds (§5.5: scalable, never to zero).
const PHASE_DURATION_MIN_TICKS: u64 = 60;
const PHASE_DURATION_MAX_TICKS: u64 = 600 * TICKS_PER_SECOND;
/// §6.5: a face fluid-contact reaction holds this long.
const FLUID_CONTACT_TICKS: u64 = 90;
/// §6.4 saliva ceiling: strands per second at rate 1000.
const SALIVA_MAX_PER_S: i64 = 2;
/// §6.4 spray ceiling: droplets per second at rate 1000.
const SPRAY_MAX_PER_S: i64 = 30;
/// §6.4 per-tick emission bound (all behaviours combined).
const EMISSION_TICK_MAX: u32 = 32;
/// The fluid layer's seed (§6.4: the layer is seeded).
/// Engine data; changing it changes trajectories.
const FLUID_SEED: u64 = 0x243F_6A88_85A3_08D3;

/// Rounds a micro-permille value to whole permille, half-up
/// (the §5.11 canonical record and the §5.2 snapshot use
/// whole permille; internal state keeps full precision).
fn permille_of(mp: i64) -> u32 {
    debug_assert!(mp >= 0);
    ((mp + MP_PER_PERMILLE / 2) / MP_PER_PERMILLE) as u32
}

/// Half-up division for non-negative values.
fn div_round(num: i128, den: i128) -> i128 {
    debug_assert!(den > 0 && num >= 0);
    (num + den / 2) / den
}

/// Half-away-from-zero division for signed deltas.
fn div_round_signed(num: i128, den: i128) -> i128 {
    debug_assert!(den > 0);
    if num >= 0 {
        (num + den / 2) / den
    } else {
        -((-num + den / 2) / den)
    }
}

/// Integer square root (floor), deterministic Newton
/// iteration on u128.
fn isqrt(v: u128) -> u128 {
    if v == 0 {
        return 0;
    }
    let mut x = v;
    let mut y = x.div_ceil(2);
    while y < x {
        x = y;
        y = (x + v / x) / 2;
    }
    x
}

/// `x^1.5` for a fraction in micro units (1.0 = 1_000_000),
/// returned in micro units.
fn pow_3_2_micro(x_micro: i64) -> i64 {
    if x_micro <= 0 {
        return 0;
    }
    let sqrt_x = isqrt((x_micro as u128) * 1_000_000) as i64;
    div_round((x_micro as i128) * (sqrt_x as i128), 1_000_000) as i64
}

/// `x^2.5` for a fraction in micro units, micro units out.
fn pow_5_2_micro(x_micro: i64) -> i64 {
    if x_micro <= 0 {
        return 0;
    }
    let sqrt_x = isqrt((x_micro as u128) * 1_000_000) as i64;
    let x2 = div_round((x_micro as i128) * (x_micro as i128), 1_000_000);
    div_round(x2 * (sqrt_x as i128), 1_000_000) as i64
}

/// `exp(-y)` for `y_micro` in [0, 4_600_000], returned in
/// micro units. Range reduction by 64 with a 5th-order
/// Taylor kernel, then six squarings — pure integer math,
/// identical on every machine (unlike libm `exp`).
fn exp_neg_micro(y_micro: i64) -> i64 {
    debug_assert!((0..=4_600_000).contains(&y_micro));
    let z = div_round(y_micro as i128, 64) as i64; // 0..=71_875
    let z2 = div_round((z as i128) * (z as i128), 1_000_000) as i64;
    let z3 = div_round((z2 as i128) * (z as i128), 1_000_000) as i64;
    let z4 = div_round((z3 as i128) * (z as i128), 1_000_000) as i64;
    let z5 = div_round((z4 as i128) * (z as i128), 1_000_000) as i64;
    let mut v: i64 = 1_000_000 - z + z2 / 2 - z3 / 6 + z4 / 24 - z5 / 120;
    for _ in 0..6 {
        v = div_round((v as i128) * (v as i128), 1_000_000) as i64;
    }
    v
}

/// Sine of a cycle fraction (micro, 0..1_000_000), returned
/// in micro units in [-1_000_000, 1_000_000]. Quadrant
/// reduction with integer polynomial kernels; deterministic.
fn sin_cycle_micro(frac_micro: i64) -> i64 {
    const PI_HALF_MICRO: i64 = 1_570_796;
    fn sin_poly(t: i64) -> i64 {
        // t in [0, pi/2] micro-radians: t - t^3/6 + t^5/120
        let t2 = div_round((t as i128) * (t as i128), 1_000_000) as i64;
        let t3 = div_round((t2 as i128) * (t as i128), 1_000_000) as i64;
        let t5 = {
            let t4 = div_round((t3 as i128) * (t as i128), 1_000_000) as i64;
            div_round((t4 as i128) * (t as i128), 1_000_000) as i64
        };
        t - t3 / 6 + t5 / 120
    }
    fn cos_poly(t: i64) -> i64 {
        // 1 - t^2/2 + t^4/24
        let t2 = div_round((t as i128) * (t as i128), 1_000_000) as i64;
        let t4 = div_round((t2 as i128) * (t2 as i128), 1_000_000) as i64;
        1_000_000 - t2 / 2 + t4 / 24
    }
    let frac = frac_micro.rem_euclid(1_000_000);
    let quadrant = frac / 250_000;
    let rem = frac % 250_000;
    let t = div_round((rem as i128) * (PI_HALF_MICRO as i128), 250_000) as i64;
    match quadrant {
        0 => sin_poly(t),
        1 => cos_poly(t),
        2 => -sin_poly(t),
        _ => -cos_poly(t),
    }
}

/// FNV-1a 64-bit over bytes (the §5.11 trajectory hash and
/// the engine state hash share the primitive; the resonance
/// D.3 pattern).
pub fn fnv1a_64(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        hash ^= u64::from(b);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// The engine phases (§5.5), in lifecycle order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Baseline,
    Stimulated,
    Culminating,
    Taper,
    Afterglow,
    Refractory,
}

impl Phase {
    /// The canonical record token (§5.11).
    pub fn token(self) -> &'static str {
        match self {
            Phase::Baseline => "baseline",
            Phase::Stimulated => "stimulated",
            Phase::Culminating => "culminating",
            Phase::Taper => "taper",
            Phase::Afterglow => "afterglow",
            Phase::Refractory => "refractory",
        }
    }
}

/// The §5.7 tier table: performance peaks per tier.
/// Durations are (culminating, taper) seconds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TierSpec {
    /// Peak body-cycle amplitude scale, permille.
    pub body_amplitude: u32,
    /// Peak posture excursion, permille of template range.
    pub posture: u32,
    /// Peak tremble amplitude, milli-pixels at 1024 canvas.
    pub tremble_milli_px: u32,
    /// Tremble rate, millihertz.
    pub tremble_rate_millihz: u32,
    /// Breath depth scale, permille (1000 = 1.0).
    pub breath_scale: u32,
    /// `ParamEyeRoll` peak, permille.
    pub eye_roll: u32,
    /// `ParamEyeCross` peak, permille.
    pub eye_cross: u32,
    /// Hair/breast secondary amplitude scale, permille.
    pub secondary_scale: u32,
    /// Culminating duration, ticks.
    pub culminating_ticks: u64,
    /// Taper duration, ticks.
    pub taper_ticks: u64,
}

/// The §5.7 table, verbatim (tier index 0 = Tier 1).
pub const TIERS: [TierSpec; 4] = [
    TierSpec {
        body_amplitude: 250,
        posture: 150,
        tremble_milli_px: 500,
        tremble_rate_millihz: 7000,
        breath_scale: 1100,
        eye_roll: 250,
        eye_cross: 300,
        secondary_scale: 400,
        culminating_ticks: 4 * TICKS_PER_SECOND,
        taper_ticks: 4 * TICKS_PER_SECOND,
    },
    TierSpec {
        body_amplitude: 500,
        posture: 350,
        tremble_milli_px: 1500,
        tremble_rate_millihz: 9000,
        breath_scale: 1300,
        eye_roll: 500,
        eye_cross: 550,
        secondary_scale: 700,
        culminating_ticks: 7 * TICKS_PER_SECOND,
        taper_ticks: 6 * TICKS_PER_SECOND,
    },
    TierSpec {
        body_amplitude: 800,
        posture: 650,
        tremble_milli_px: 3000,
        tremble_rate_millihz: 11000,
        breath_scale: 1700,
        eye_roll: 850,
        eye_cross: 850,
        secondary_scale: 1000,
        culminating_ticks: 11 * TICKS_PER_SECOND,
        taper_ticks: 9 * TICKS_PER_SECOND,
    },
    TierSpec {
        body_amplitude: 1000,
        posture: 900,
        tremble_milli_px: 5000,
        tremble_rate_millihz: 13000,
        breath_scale: 2100,
        eye_roll: 1000,
        eye_cross: 1000,
        secondary_scale: 1200,
        culminating_ticks: 16 * TICKS_PER_SECOND,
        taper_ticks: 12 * TICKS_PER_SECOND,
    },
];

/// Culmination tier from strength P (permille): the §5.7
/// edges (T1 < 300, T2 < 550, T3 < 800, else T4).
pub fn tier_of(p_permille: u32) -> u8 {
    if p_permille < 300 {
        1
    } else if p_permille < 550 {
        2
    } else if p_permille < 800 {
        3
    } else {
        4
    }
}

/// The device waveform contract (§2.5), mirrored at the
/// engine seam: gear documents (T4.1) map their `device`
/// block onto this shape; the engine samples it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WaveSpec {
    pub kind: WaveKind,
    /// 0..=1000.
    pub amplitude_permille: u32,
    /// 0..=20_000 (millihertz).
    pub frequency_millihz: u32,
    /// Pulse/Pattern duty, 0..=1000.
    pub duty_permille: u32,
    /// Pattern steps (Pattern kind only), 1..=64 entries
    /// of 0..=1000.
    pub pattern: Vec<u32>,
    /// Ticks per pattern step, 1..=3600.
    pub pattern_step_ticks: u64,
    /// Initial phase, ticks.
    pub phase_ticks: u64,
}

/// Waveform kinds (§2.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WaveKind {
    Constant,
    Pulse,
    Wave,
    Ramp,
    Pattern,
}

impl WaveKind {
    /// The canonical token (§7.2's `wave_kind`).
    pub fn token(self) -> &'static str {
        match self {
            WaveKind::Constant => "constant",
            WaveKind::Pulse => "pulse",
            WaveKind::Wave => "wave",
            WaveKind::Ramp => "ramp",
            WaveKind::Pattern => "pattern",
        }
    }
}

impl WaveSpec {
    /// A constant waveform at `amplitude_permille`.
    pub fn constant(amplitude_permille: u32) -> WaveSpec {
        WaveSpec {
            kind: WaveKind::Constant,
            amplitude_permille,
            frequency_millihz: 0,
            duty_permille: 1000,
            pattern: Vec::new(),
            pattern_step_ticks: 1,
            phase_ticks: 0,
        }
    }

    /// The §2.5 bounds, as session-local validation.
    pub fn validate(&self) -> R<()> {
        if self.amplitude_permille > 1000 {
            return Err(bound("amplitude_permille", self.amplitude_permille, 1000));
        }
        if self.frequency_millihz > 20_000 {
            return Err(bound("frequency_millihz", self.frequency_millihz, 20_000));
        }
        if self.duty_permille > 1000 {
            return Err(bound("duty_permille", self.duty_permille, 1000));
        }
        if self.pattern_step_ticks == 0 || self.pattern_step_ticks > 3600 {
            return Err(Refusal::new(
                "TG-ARO-WAVEFORM",
                format!(
                    "pattern_step_ticks {} outside 1..=3600",
                    self.pattern_step_ticks
                ),
            ));
        }
        if self.kind == WaveKind::Pattern {
            if self.pattern.is_empty() || self.pattern.len() > 64 {
                return Err(Refusal::new(
                    "TG-ARO-WAVEFORM",
                    format!("pattern has {} steps, must be 1..=64", self.pattern.len()),
                ));
            }
            for (i, v) in self.pattern.iter().enumerate() {
                if *v > 1000 {
                    return Err(bound(&format!("pattern[{i}]"), *v, 1000));
                }
            }
        }
        Ok(())
    }

    /// Waveform period in ticks for the cyclic kinds;
    /// 0 for Constant (and for a zero frequency, which the
    /// cyclic kinds sample as silence).
    fn period_ticks(&self) -> u64 {
        if self.frequency_millihz == 0 {
            return 0;
        }
        // period in ticks = 60 ticks/s / (f_millihz/1000 Hz)
        // = 60_000 / f_millihz.
        (60_000 / u64::from(self.frequency_millihz)).max(1)
    }

    /// Samples the waveform at runtime phase `phase`
    /// (ticks since activation): the §2.5 sample `u`,
    /// permille. Amplitude zero is a zero drive level,
    /// never a stop (§2.5).
    pub fn sample(&self, phase: u64) -> u32 {
        let amp = i64::from(self.amplitude_permille);
        let out = match self.kind {
            WaveKind::Constant => amp,
            WaveKind::Pulse => {
                let period = self.period_ticks();
                if period == 0 {
                    0
                } else {
                    let pos = phase % period;
                    let on_ticks = period * u64::from(self.duty_permille) / 1000;
                    if pos < on_ticks { amp } else { 0 }
                }
            }
            WaveKind::Wave => {
                let period = self.period_ticks();
                if period == 0 {
                    0
                } else {
                    let pos = phase % period;
                    let frac = (pos as i128) * 1_000_000 / (period as i128);
                    let sin = sin_cycle_micro(frac as i64);
                    div_round((amp as i128) * ((1_000_000 + sin) as i128), 2_000_000) as i64
                }
            }
            WaveKind::Ramp => {
                let period = self.period_ticks();
                if period == 0 {
                    0
                } else {
                    let pos = phase % period;
                    div_round((amp as i128) * (pos as i128), period as i128) as i64
                }
            }
            WaveKind::Pattern => {
                if self.pattern.is_empty() {
                    0
                } else {
                    let idx = ((phase / self.pattern_step_ticks) as usize) % self.pattern.len();
                    div_round((amp as i128) * (self.pattern[idx] as i128), 1000) as i64
                }
            }
        };
        out.clamp(0, 1000) as u32
    }
}

fn bound(field: &str, value: u32, max: u32) -> Refusal {
    Refusal::new("TG-ARO-BOUND", format!("{field} {value} outside 0..={max}"))
}

/// A worn item as the engine sees it: the T4.1 gear layer
/// resolves a gear document to this seam (item id, contact
/// site, waveform, contact quality, floor). The engine
/// owns no gear documents.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeviceSpec {
    /// Stable item id (gear id + version, host-supplied).
    pub item_id: String,
    /// Contact site name; the profile's site weight for
    /// this name scales the item's effective input (§5.3).
    pub site: String,
    /// The waveform for a vibratory device; `None` for a
    /// passive attachment (clamps: pressure + floor only).
    pub wave: Option<WaveSpec>,
    /// Contact quality q at application, permille (§4.3 —
    /// computed by the physics layer, fed here).
    pub contact_quality: u32,
    /// Arousal floor while attached and not displaced,
    /// permille, 0..=600 (§2.3 B).
    pub arousal_floor: u32,
}

impl DeviceSpec {
    /// Bounds validation under the session-local rules.
    pub fn validate(&self) -> R<()> {
        if self.item_id.is_empty() || self.item_id.len() > 128 {
            return Err(Refusal::new(
                "TG-ARO-SCHEMA",
                "item_id must be 1..=128 bytes",
            ));
        }
        if self.site.is_empty() || self.site.len() > 128 {
            return Err(Refusal::new("TG-ARO-SCHEMA", "site must be 1..=128 bytes"));
        }
        if self.contact_quality > 1000 {
            return Err(bound("contact_quality", self.contact_quality, 1000));
        }
        if self.arousal_floor > 600 {
            return Err(bound("arousal_floor", self.arousal_floor, 600));
        }
        if let Some(wave) = &self.wave {
            wave.validate()?;
        }
        Ok(())
    }

    /// Whether this item emits a drive signal (§7.2).
    pub fn is_vibratory(&self) -> bool {
        self.wave.is_some()
    }
}

/// Per-character profile data (§5.3's seam): site weights,
/// effect ceilings, phase-duration scaling. Bounded and
/// versioned with the engine; a gear file never carries
/// these (§5.3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CharacterProfile {
    /// Site name → weight, permille (0..=1000).
    pub site_weights: BTreeMap<String, u32>,
    /// Weight for a site the map does not name.
    pub default_site_weight: u32,
    /// Blush ceiling the §6.1 ratio scales, permille.
    pub blush_max: u32,
    /// Afterglow hold, ticks (§5.5: scalable, never zero).
    pub afterglow_ticks: u64,
    /// Refractory duration, ticks (§5.5).
    pub refractory_ticks: u64,
    /// Lactation state for the §6.4 milk behaviour.
    pub lactation_active: bool,
    /// Touch-class input weight for face fluid contact
    /// (§6.4). Zero in the default profile: contact fires
    /// the expression path only. (The §8 fixtures run the
    /// default profile.)
    pub fluid_contact_input_permille: u32,
}

impl Default for CharacterProfile {
    /// The default profile the §8 fixtures name: unit site
    /// weights, full blush, §5.5 default durations.
    fn default() -> Self {
        CharacterProfile {
            site_weights: BTreeMap::new(),
            default_site_weight: 1000,
            blush_max: 1000,
            afterglow_ticks: AFTERGLOW_DEFAULT_TICKS,
            refractory_ticks: REFRACTORY_DEFAULT_TICKS,
            lactation_active: false,
            fluid_contact_input_permille: 0,
        }
    }
}

impl CharacterProfile {
    /// Bounds validation under the session-local rules.
    pub fn validate(&self) -> R<()> {
        for (site, w) in &self.site_weights {
            if *w > 1000 {
                return Err(bound(&format!("site_weights[{site}]"), *w, 1000));
            }
        }
        if self.default_site_weight > 1000 {
            return Err(bound("default_site_weight", self.default_site_weight, 1000));
        }
        if self.blush_max > 1000 {
            return Err(bound("blush_max", self.blush_max, 1000));
        }
        if self.fluid_contact_input_permille > 1000 {
            return Err(bound(
                "fluid_contact_input_permille",
                self.fluid_contact_input_permille,
                1000,
            ));
        }
        for (field, ticks) in [
            ("afterglow_ticks", self.afterglow_ticks),
            ("refractory_ticks", self.refractory_ticks),
        ] {
            if !(PHASE_DURATION_MIN_TICKS..=PHASE_DURATION_MAX_TICKS).contains(&ticks) {
                return Err(Refusal::new(
                    "TG-ARO-BOUND",
                    format!(
                        "{field} {ticks} outside {PHASE_DURATION_MIN_TICKS}..={PHASE_DURATION_MAX_TICKS} ticks (never zero, §5.5)"
                    ),
                ));
            }
        }
        Ok(())
    }

    /// The site weight for `site` (§5.3).
    pub fn site_weight(&self, site: &str) -> u32 {
        self.site_weights
            .get(site)
            .copied()
            .unwrap_or(self.default_site_weight)
    }
}

/// Non-device inputs for one tick (§5.3's other inputs,
/// §6.4's authored effects).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TickInputs {
    /// Direct touch at a named site, permille strength:
    /// converted to the same effective-input form as a
    /// device at that site (§5.3).
    pub touch: Option<(String, u32)>,
    /// A suction-prop cycle pressure peak this tick
    /// (Addition C props; drives §6.4 milk with the
    /// profile's lactation state).
    pub suction_peak: bool,
    /// An authored spray `Effect` emitter's rate this
    /// tick, permille (§6.4 spray); `None` = inactive.
    pub spray_rate: Option<u32>,
    /// An external face-contact event this tick (fires
    /// the §6.5 fluid-contact reaction row).
    pub face_contact: bool,
}

/// The canonical drive signal (§7.2), emitted per worn
/// vibratory device per tick. Contact quality is NOT part
/// of it (§7.2: contact is a property of the fictional
/// body; the engine applies it on its own side).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriveSignal {
    pub session_tick: u64,
    pub device_item_id: String,
    /// `u(tick)` pre-contact, permille.
    pub strength_permille: u32,
    pub wave_kind: WaveKind,
    pub mirror_generation: u64,
}

/// Events the engine emits (§5.6, §6.4). Drained per tick
/// through `EngineOutput::events`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArousalEvent {
    /// §5.6: exactly once per episode.
    Culmination {
        episode: u32,
        tier: u8,
        /// Strength P at the firing tick, permille.
        p_permille: u32,
    },
    /// §5.6: the latch cleared; a new episode may fire.
    Rearmed { episode: u32 },
    /// Phase transition (audit + trajectory record).
    PhaseChanged { from: Phase, to: Phase },
    /// §6.4: a fluid strand/droplet landed on a face area.
    FluidFaceContact { area: &'static str },
}

/// §6 effects output for one tick: pure functions of
/// engine state + contact events, evaluated per tick.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectsOutput {
    /// §6.1 blush opacity, permille (already scaled by
    /// the profile's blush ceiling).
    pub blush_permille: u32,
    /// §6.2 sweat level, permille.
    pub sweat_permille: u32,
    /// §6.3 tremble amplitude, milli-pixels at the 1024
    /// canvas (tier table replaces the ambient law while
    /// Culminating).
    pub tremble_milli_px: u32,
    /// Tremble rate, millihertz (tier table while
    /// Culminating; 0 when the ambient law applies).
    pub tremble_rate_millihz: u32,
    /// §6.5's selected governed-name key for this tick:
    /// one of `ambient`, `aroused`, `building`, `near`,
    /// `peak`, `afterglow`, `fluid-contact`. Resolution
    /// to the character's recipe is the presentation's
    /// Addition G mapping; the engine never substitutes
    /// another character's recipe (§6.5).
    pub expression_key: &'static str,
    /// §5.7 performance envelope, while Culminating/Taper:
    /// channel peaks scaled by the envelope shape.
    pub performance: Option<PerformanceOutput>,
    /// §6.4 milk pulses emitted this tick.
    pub milk_pulses: u32,
    /// §6.4 saliva strands emitted this tick.
    pub saliva_strands: u32,
    /// §6.4 spray droplets emitted this tick.
    pub spray_droplets: u32,
}

/// The §5.7 performance channels at the current envelope
/// value (all permille unless the field says otherwise).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PerformanceOutput {
    pub tier: u8,
    /// Envelope shape value, permille (0..=1000).
    pub envelope_permille: u32,
    pub body_amplitude: u32,
    pub posture: u32,
    pub breath_scale: u32,
    pub eye_roll: u32,
    pub eye_cross: u32,
    pub secondary_scale: u32,
}

/// One tick's complete engine output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineOutput {
    /// Combined effective input I for the tick, permille
    /// (the §5.3 combination), for the dashboard.
    pub effective_input: u32,
    /// The canonical drive signals (§7.2).
    pub drive: Vec<DriveSignal>,
    /// Per-device dashboard rows: item id, last `u`,
    /// contact quality, effective input — permille.
    pub devices: Vec<DeviceOutput>,
    pub effects: EffectsOutput,
    pub events: Vec<ArousalEvent>,
}

/// A per-device dashboard row (§3.2: per-device effective
/// input is dashboard data, read from state).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceOutput {
    pub item_id: String,
    /// Last sampled `u`, permille (0 for passive items).
    pub u_permille: u32,
    pub contact_quality: u32,
    pub effective_permille: u32,
    pub displaced: bool,
    pub on: bool,
}

/// The §5.2 state vector, snapshotted in whole permille
/// (the specified rounding of §5.1). This is the
/// interface the Playroom dashboard and the T4.1 gear
/// layer compose against — field names are §5.2's.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArousalVector {
    pub arousal: u32,
    pub pleasure_charge: u32,
    pub sensitivity: u32,
    pub floor: u32,
    pub peak_input: u32,
    pub dose: u32,
    pub phase: Phase,
    pub culmination_latch: bool,
    pub episode: u32,
    pub phase_ticks: u64,
    pub afterglow_level: u32,
}

/// A worn item's runtime state (engine-adjacent, §5.2:
/// recorded in the trajectory, not part of the vector).
#[derive(Debug, Clone, PartialEq, Eq)]
struct DeviceRuntime {
    spec: DeviceSpec,
    /// Waveform runtime phase, ticks (§4.6: phase-stateful).
    phase: u64,
    on: bool,
    displaced: bool,
    last_u: u32,
    last_effective: u32,
}

/// The arousal engine for one character.
#[derive(Debug, Clone)]
pub struct ArousalEngine {
    profile: CharacterProfile,
    // §5.2 vector, micro-permille internals.
    arousal_mp: i64,
    charge_mp: i64,
    sensitivity_mp: i64,
    floor_mp: i64,
    peak_mp: i64,
    dose_mp: i64,
    phase: Phase,
    latch: bool,
    episode: u32,
    phase_ticks: u64,
    afterglow_level_mp: i64,
    // Bookkeeping.
    tick: u64,
    tier: Option<u8>,
    culmination_count: u32,
    mirror_generation: u64,
    devices: Vec<DeviceRuntime>,
    // §6 effects state (engine-adjacent).
    sweat_mp: i64,
    saliva_acc: i64,
    spray_acc: i64,
    fluid_rng: u64,
    fluid_contact_ticks: u64,
    pending_face_input: Option<u32>,
}

impl ArousalEngine {
    /// A fresh engine at baseline under `profile`.
    pub fn new(profile: CharacterProfile) -> ArousalEngine {
        ArousalEngine {
            profile,
            arousal_mp: 0,
            charge_mp: 0,
            sensitivity_mp: 1000 * MP_PER_PERMILLE,
            floor_mp: 0,
            peak_mp: 0,
            dose_mp: 0,
            phase: Phase::Baseline,
            latch: false,
            episode: 1,
            phase_ticks: 0,
            afterglow_level_mp: 150 * MP_PER_PERMILLE,
            tick: 0,
            tier: None,
            culmination_count: 0,
            mirror_generation: 0,
            devices: Vec::new(),
            sweat_mp: 0,
            saliva_acc: 0,
            spray_acc: 0,
            fluid_rng: FLUID_SEED,
            fluid_contact_ticks: 0,
            pending_face_input: None,
        }
    }

    /// The engine's tick index (the session tick of §7.2).
    pub fn tick(&self) -> u64 {
        self.tick
    }

    /// The current phase.
    pub fn phase(&self) -> Phase {
        self.phase
    }

    /// Culmination events emitted so far (the audit count;
    /// reset retains it, §5.8).
    pub fn culmination_count(&self) -> u32 {
        self.culmination_count
    }

    /// The mirror fencing generation (§7.6).
    pub fn mirror_generation(&self) -> u64 {
        self.mirror_generation
    }

    /// The §5.2 vector snapshot (whole permille).
    pub fn vector(&self) -> ArousalVector {
        ArousalVector {
            arousal: permille_of(self.arousal_mp),
            pleasure_charge: permille_of(self.charge_mp),
            sensitivity: permille_of(self.sensitivity_mp),
            floor: permille_of(self.floor_mp),
            peak_input: permille_of(self.peak_mp),
            dose: permille_of(self.dose_mp),
            phase: self.phase,
            culmination_latch: self.latch,
            episode: self.episode,
            phase_ticks: self.phase_ticks,
            afterglow_level: permille_of(self.afterglow_level_mp),
        }
    }

    /// Worn item ids, sorted (the canonical record's worn
    /// set, §5.11).
    pub fn worn_ids(&self) -> Vec<&str> {
        let mut ids: Vec<&str> = self
            .devices
            .iter()
            .map(|d| d.spec.item_id.as_str())
            .collect();
        ids.sort_unstable();
        ids
    }

    fn device_mut(&mut self, item_id: &str) -> Option<&mut DeviceRuntime> {
        self.devices.iter_mut().find(|d| d.spec.item_id == item_id)
    }

    /// Recomputes the floor (§5.4): the maximum floor over
    /// attached, non-displaced items — max, never sum.
    fn recompute_floor(&mut self) {
        self.floor_mp = self
            .devices
            .iter()
            .filter(|d| !d.displaced)
            .map(|d| i64::from(d.spec.arousal_floor) * MP_PER_PERMILLE)
            .max()
            .unwrap_or(0);
    }

    /// Attaches an item (§5.10's wear operation, applied
    /// by the host after admission).
    pub fn apply_item(&mut self, spec: DeviceSpec) {
        let phase0 = spec.wave.as_ref().map_or(0, |w| w.phase_ticks);
        self.devices.push(DeviceRuntime {
            spec,
            phase: phase0,
            on: true,
            displaced: false,
            last_u: 0,
            last_effective: 0,
        });
        self.recompute_floor();
    }

    /// Removes an item (§5.6: its `u` is zero from the
    /// next tick — the item is gone, so no further sample
    /// exists; a vibratory removal bumps the mirror
    /// generation, §7.6).
    pub fn remove_item(&mut self, item_id: &str) {
        if let Some(pos) = self.devices.iter().position(|d| d.spec.item_id == item_id) {
            let was_vibratory = self.devices[pos].spec.is_vibratory();
            self.devices.remove(pos);
            if was_vibratory {
                self.mirror_generation += 1;
            }
            self.recompute_floor();
        }
    }

    /// Replaces a worn device's waveform (a device
    /// parameter change, §5.10). Runtime phase is kept
    /// (§4.6: cycles are phase-stateful).
    pub fn set_wave(&mut self, item_id: &str, wave: WaveSpec) {
        if let Some(d) = self.device_mut(item_id) {
            d.spec.wave = Some(wave);
        }
    }

    /// Feeds the physics layer's contact result for an
    /// item (§4.3/§4.5): contact quality + displacement.
    /// A displaced clamp contributes no floor (§5.4).
    pub fn set_contact(&mut self, item_id: &str, contact_quality: u32, displaced: bool) {
        if let Some(d) = self.device_mut(item_id) {
            d.spec.contact_quality = contact_quality;
            d.displaced = displaced;
        }
        self.recompute_floor();
    }

    /// Device STOP (§5.8): the device's `u` is zero from
    /// the next tick; the mirror generation bumps in the
    /// calling act (§7.6). Never gated.
    pub fn stop_device(&mut self, item_id: &str) {
        if let Some(d) = self.device_mut(item_id) {
            d.on = false;
            d.last_u = 0;
        }
        self.mirror_generation += 1;
    }

    /// (Re)activates a stopped device.
    pub fn start_device(&mut self, item_id: &str) {
        if let Some(d) = self.device_mut(item_id) {
            d.on = true;
        }
    }

    /// `arousal.reset` (§5.8): state to baseline, latch
    /// cleared, envelopes cancelled in the same tick; the
    /// episode counter (audit) is retained.
    pub fn reset(&mut self) {
        self.arousal_mp = 0;
        self.charge_mp = 0;
        self.dose_mp = 0;
        self.peak_mp = 0;
        self.sensitivity_mp = 1000 * MP_PER_PERMILLE;
        self.phase = Phase::Baseline;
        self.phase_ticks = 0;
        self.latch = false;
        self.tier = None;
        self.afterglow_level_mp = 150 * MP_PER_PERMILLE;
        self.sweat_mp = 0;
        self.saliva_acc = 0;
        self.spray_acc = 0;
        self.fluid_contact_ticks = 0;
        self.pending_face_input = None;
        for d in &mut self.devices {
            d.last_u = 0;
            d.last_effective = 0;
        }
    }

    /// The §3.4 control's engine act, in its order: latch
    /// stimulation to zero + bump the mirror generation
    /// synchronously, remove every worn item, reset the
    /// engine. (The ache stop and the op-log append are
    /// the host's steps 2 and 5; no mirror exists in this
    /// build, T4.5.)
    pub fn remove_all_and_reset(&mut self) {
        self.mirror_generation += 1;
        for d in &mut self.devices {
            d.on = false;
            d.last_u = 0;
        }
        self.devices.clear();
        self.recompute_floor();
        self.reset();
    }

    fn set_phase(&mut self, to: Phase, events: &mut Vec<ArousalEvent>) {
        if to != self.phase {
            events.push(ArousalEvent::PhaseChanged {
                from: self.phase,
                to,
            });
            self.phase = to;
            self.phase_ticks = 0;
        }
    }

    /// The performance envelope shape (§5.7): onset →
    /// peak hold → release across Culminating + Taper,
    /// in permille. Attack is the first fifth, release
    /// the last two fifths.
    fn envelope_permille(&self) -> u32 {
        let Some(tier) = self.tier else { return 0 };
        let spec = TIERS[(tier - 1) as usize];
        let total = spec.culminating_ticks + spec.taper_ticks;
        let elapsed = if self.phase == Phase::Culminating {
            self.phase_ticks
        } else {
            spec.culminating_ticks + self.phase_ticks
        };
        let attack = total / 5;
        let release_start = total * 3 / 5;
        let shape = if elapsed < attack {
            div_round((elapsed as i128) * 1000, attack as i128)
        } else if elapsed < release_start {
            1000
        } else {
            div_round(
                ((total - elapsed) as i128) * 1000,
                (total - release_start) as i128,
            )
        };
        shape.clamp(0, 1000) as u32
    }

    /// Advances the fluid layer's seeded stream (§6.4)
    /// and returns the next value.
    fn fluid_next(&mut self) -> u64 {
        // SplitMix64: pure integer, identical everywhere.
        self.fluid_rng = self.fluid_rng.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.fluid_rng;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Advances one fixed step (§4.1's engine order:
    /// device samples → attachments/contacts are the
    /// caller's layer → arousal integration → expression
    /// and effect resolution → drive signal + events).
    pub fn step(&mut self, inputs: &TickInputs) -> EngineOutput {
        self.tick += 1;
        self.phase_ticks += 1;
        let phase_at_start = self.phase;
        // The state carried into this tick: §5.6's re-arm
        // conditions are evaluated on it (D-211 erratum —
        // post-integration Q makes re-arm unsatisfiable).
        let charge_at_start = self.charge_mp;
        let mut events: Vec<ArousalEvent> = Vec::new();

        // --- device samples + effective inputs (§5.3)
        let mut effective_list: Vec<i64> = Vec::with_capacity(self.devices.len() + 2);
        let mut drive: Vec<DriveSignal> = Vec::new();
        for d in &mut self.devices {
            let u = match (&d.spec.wave, d.on) {
                (Some(wave), true) => wave.sample(d.phase),
                _ => 0,
            };
            if d.spec.wave.is_some() && d.on {
                d.phase += 1;
            }
            d.last_u = u;
            let site_weight = self.profile.site_weight(&d.spec.site);
            let effective = div_round(
                (i64::from(u) as i128)
                    * (i64::from(d.spec.contact_quality) as i128)
                    * (i64::from(site_weight) as i128),
                1_000_000,
            ) as i64;
            d.last_effective = effective as u32;
            if effective > 0 {
                effective_list.push(effective * MP_PER_PERMILLE);
            }
            if d.spec.is_vibratory() {
                drive.push(DriveSignal {
                    session_tick: self.tick,
                    device_item_id: d.spec.item_id.clone(),
                    strength_permille: if d.on { u } else { 0 },
                    wave_kind: d.spec.wave.as_ref().expect("vibratory").kind,
                    mirror_generation: self.mirror_generation,
                });
            }
        }
        // Direct touch (§5.3): same form, at the touched site.
        if let Some((site, strength)) = &inputs.touch {
            let weight = self.profile.site_weight(site);
            let effective = div_round(
                (i64::from(*strength) as i128) * (i64::from(weight) as i128),
                1000,
            ) as i64;
            if effective > 0 {
                effective_list.push(effective * MP_PER_PERMILLE);
            }
        }
        // A pending face fluid-contact input (§6.4) lands
        // as touch at the face site.
        if let Some(strength) = self.pending_face_input.take() {
            let weight = self.profile.site_weight("face");
            let effective = div_round(
                (i64::from(strength) as i128) * (i64::from(weight) as i128),
                1000,
            ) as i64;
            if effective > 0 {
                effective_list.push(effective * MP_PER_PERMILLE);
            }
        }
        // Combination: strongest leads, the rest add at 0.35.
        let combined_mp = {
            let max = effective_list.iter().copied().max().unwrap_or(0);
            let rest: i64 = effective_list.iter().sum::<i64>() - max;
            (max + div_round((rest as i128) * 35, 100) as i64).clamp(0, MP_FULL)
        };
        let i_permille = permille_of(combined_mp);

        // --- sensitivity law (§5.3)
        let (s_target, s_divisor) = if phase_at_start == Phase::Refractory {
            (650 * MP_PER_PERMILLE, 20 * TICKS_PER_SECOND as i64)
        } else if self.arousal_mp >= 400 * MP_PER_PERMILLE && combined_mp > 0 {
            (1350 * MP_PER_PERMILLE, 60 * TICKS_PER_SECOND as i64)
        } else {
            (1000 * MP_PER_PERMILLE, 120 * TICKS_PER_SECOND as i64)
        };
        self.sensitivity_mp +=
            div_round_signed((s_target - self.sensitivity_mp) as i128, s_divisor as i128) as i64;
        self.sensitivity_mp = self.sensitivity_mp.clamp(S_MIN_MP, S_MAX_MP);

        // --- phase machinery
        if phase_at_start == Phase::Culminating || phase_at_start == Phase::Taper {
            // §5.6 item 4 + §5.7 taper law: Q is driven by
            // the taper law, not by input integration.
            self.charge_mp -= div_round(self.charge_mp as i128, 20 * 60) as i64;
            if self.charge_mp < 0 {
                self.charge_mp = 0;
            }
            self.arousal_mp +=
                div_round_signed((self.afterglow_level_mp - self.arousal_mp) as i128, 12 * 60)
                    as i64;
            let tier_spec = TIERS[(self.tier.unwrap_or(1) - 1) as usize];
            let duration = if phase_at_start == Phase::Culminating {
                tier_spec.culminating_ticks
            } else {
                tier_spec.taper_ticks
            };
            if self.phase_ticks >= duration {
                let next = if phase_at_start == Phase::Culminating {
                    Phase::Taper
                } else {
                    Phase::Afterglow
                };
                self.set_phase(next, &mut events);
            }
        } else {
            // --- charge (§5.3, D-211 constants)
            if combined_mp > 0 {
                let x_micro = combined_mp / 1000;
                let charge_rate = div_round(
                    (CHARGE_PERMILLE_PER_S as i128)
                        * (pow_3_2_micro(x_micro) as i128)
                        * (self.sensitivity_mp as i128),
                    1_000_000_000,
                );
                self.charge_mp += div_round(charge_rate, TICKS_PER_SECOND as i128) as i64;
                self.charge_mp -= div_round(
                    self.charge_mp as i128,
                    (Q_LEAK_TAU_S as i128) * (TICKS_PER_SECOND as i128),
                ) as i64;
            } else if self.floor_mp > 0 {
                // Passive presence: small charge rate, the
                // 250 cap enforced as a clamp (§5.3).
                self.charge_mp += div_round(
                    (PRESENCE_PERMILLE_PER_S * MP_PER_PERMILLE) as i128,
                    TICKS_PER_SECOND as i128,
                ) as i64;
                self.charge_mp -= div_round(
                    self.charge_mp as i128,
                    (Q_LEAK_TAU_S as i128) * (TICKS_PER_SECOND as i128),
                ) as i64;
                if self.charge_mp > PRESENCE_CAP_MP {
                    self.charge_mp = PRESENCE_CAP_MP;
                }
            } else {
                self.charge_mp -= div_round(
                    self.charge_mp as i128,
                    (Q_LEAK_TAU_S as i128) * (TICKS_PER_SECOND as i128),
                ) as i64;
            }
            self.charge_mp = self.charge_mp.clamp(0, Q_MAX_MP);
            // --- dose (§5.3, D-211 law 140·I^2.5)
            if combined_mp > 0 {
                let x_micro = combined_mp / 1000;
                self.dose_mp += div_round(
                    (140 * pow_5_2_micro(x_micro)) as i128,
                    TICKS_PER_SECOND as i128,
                ) as i64;
                if self.dose_mp > MP_FULL {
                    self.dose_mp = MP_FULL;
                }
            }
            // --- rolling peak, 10 s decay (§5.3, Euler form)
            let decayed = self.peak_mp - div_round(self.peak_mp as i128, 600) as i64;
            self.peak_mp = combined_mp.max(decayed);
            // --- arousal (§5.3, D-211 A-curve)
            if phase_at_start == Phase::Afterglow {
                // §5.7: afterglow holds A near afterglow_level.
                self.arousal_mp +=
                    div_round_signed((self.afterglow_level_mp - self.arousal_mp) as i128, 12 * 60)
                        as i64;
            } else {
                let curve = {
                    let y_micro = div_round((combined_mp as i128 / 1000) * 46, 10) as i64;
                    MP_FULL - exp_neg_micro(y_micro) * 1000
                };
                let target = self.floor_mp.max(curve);
                let divisor: i64 = if target > self.arousal_mp {
                    6 * TICKS_PER_SECOND as i64
                } else {
                    45 * TICKS_PER_SECOND as i64
                };
                self.arousal_mp +=
                    div_round_signed((target - self.arousal_mp) as i128, divisor as i128) as i64;
            }
            if self.floor_mp > 0 {
                // The floor law (§5.4).
                self.arousal_mp = self.arousal_mp.max(self.floor_mp);
            }
            self.arousal_mp = self.arousal_mp.clamp(0, MP_FULL);

            // --- transitions (§5.5)
            match phase_at_start {
                Phase::Baseline if combined_mp > 0 || self.floor_mp > 0 => {
                    self.set_phase(Phase::Stimulated, &mut events);
                }
                Phase::Stimulated
                    if combined_mp == 0
                        && self.floor_mp == 0
                        && self.arousal_mp < 100 * MP_PER_PERMILLE =>
                {
                    self.set_phase(Phase::Baseline, &mut events);
                }
                Phase::Afterglow if self.phase_ticks >= self.profile.afterglow_ticks => {
                    self.set_phase(Phase::Refractory, &mut events);
                }
                Phase::Refractory if self.phase_ticks >= self.profile.refractory_ticks => {
                    // §5.6 re-arm, on the state carried into
                    // this tick (D-211 erratum).
                    if charge_at_start < REARM_Q_PERMILLE * MP_PER_PERMILLE && combined_mp > 0 {
                        self.latch = false;
                        self.episode += 1;
                        self.dose_mp = 0;
                        self.peak_mp = 0;
                        self.tier = None;
                        events.push(ArousalEvent::Rearmed {
                            episode: self.episode,
                        });
                        self.set_phase(Phase::Stimulated, &mut events);
                    }
                }
                _ => {}
            }

            // --- firing predicate (§5.6)
            if self.phase == Phase::Stimulated
                && !self.latch
                && self.charge_mp >= MP_FULL
                && self.arousal_mp >= 650 * MP_PER_PERMILLE
            {
                let p = div_round(
                    65 * (self.dose_mp as i128) + 35 * (self.peak_mp as i128),
                    100_000,
                ) as i64;
                let p_permille = (p / 1000).clamp(0, 1000) as u32;
                let tier = tier_of(p_permille);
                self.tier = Some(tier);
                self.latch = true;
                self.culmination_count += 1;
                self.afterglow_level_mp = if self.floor_mp > 0 {
                    self.floor_mp
                } else {
                    150 * MP_PER_PERMILLE
                };
                events.push(ArousalEvent::Culmination {
                    episode: self.episode,
                    tier,
                    p_permille,
                });
                self.set_phase(Phase::Culminating, &mut events);
            }
        }

        // --- effects (§6)
        let effects = self.effects(inputs, i_permille, &mut events);
        let devices_out: Vec<DeviceOutput> = self
            .devices
            .iter()
            .map(|d| DeviceOutput {
                item_id: d.spec.item_id.clone(),
                u_permille: d.last_u,
                contact_quality: d.spec.contact_quality,
                effective_permille: d.last_effective,
                displaced: d.displaced,
                on: d.on,
            })
            .collect();
        EngineOutput {
            effective_input: i_permille,
            drive,
            devices: devices_out,
            effects,
            events,
        }
    }

    /// §6 effects resolution for the current tick.
    fn effects(
        &mut self,
        inputs: &TickInputs,
        i_permille: u32,
        events: &mut Vec<ArousalEvent>,
    ) -> EffectsOutput {
        let a_permille = permille_of(self.arousal_mp);
        // §6.2 sweat: accumulates, decays on the minutes scale.
        if a_permille >= 550 || i_permille >= 500 {
            self.sweat_mp += MP_FULL / (90 * TICKS_PER_SECOND as i64);
        } else {
            self.sweat_mp -= MP_FULL / (240 * TICKS_PER_SECOND as i64);
        }
        self.sweat_mp = self.sweat_mp.clamp(0, MP_FULL);
        // §6.1 blush.
        let mut blush_ratio = ((i64::from(a_permille) - 250).max(0) * 1000 / 750) as u32;
        if self.phase == Phase::Culminating || self.phase == Phase::Taper {
            blush_ratio = blush_ratio.max(850);
        }
        let blush = blush_ratio.min(1000) * self.profile.blush_max / 1000;
        // §6.3 tremble + §5.7 performance envelope.
        let performance = match self.tier {
            Some(tier) if self.phase == Phase::Culminating || self.phase == Phase::Taper => {
                let spec = TIERS[(tier - 1) as usize];
                let env = self.envelope_permille();
                let scale = |peak: u32| (u64::from(peak) * u64::from(env) / 1000) as u32;
                Some(PerformanceOutput {
                    tier,
                    envelope_permille: env,
                    body_amplitude: scale(spec.body_amplitude),
                    posture: scale(spec.posture),
                    breath_scale: scale(spec.breath_scale),
                    eye_roll: scale(spec.eye_roll),
                    eye_cross: scale(spec.eye_cross),
                    secondary_scale: scale(spec.secondary_scale),
                })
            }
            _ => None,
        };
        let (tremble_milli_px, tremble_rate_millihz) = match &performance {
            Some(p) => {
                let spec = TIERS[(p.tier - 1) as usize];
                (
                    (u64::from(spec.tremble_milli_px) * u64::from(p.envelope_permille) / 1000)
                        as u32,
                    spec.tremble_rate_millihz,
                )
            }
            None => {
                if a_permille > 700 {
                    (2000 * (a_permille - 700) / 300, 0)
                } else {
                    (0, 0)
                }
            }
        };
        // §6.4 fluids.
        let milk_pulses = if inputs.suction_peak && self.profile.lactation_active {
            1
        } else {
            0
        };
        let saliva_rate: u32 =
            if self.phase == Phase::Culminating && self.tier.is_some_and(|t| t >= 3) {
                1000
            } else if a_permille > 600 {
                ((a_permille - 600) * 1000 / 400).min(1000)
            } else {
                0
            };
        self.saliva_acc += div_round(
            (i64::from(saliva_rate) * SALIVA_MAX_PER_S * MP_PER_PERMILLE) as i128,
            (1000 * TICKS_PER_SECOND) as i128,
        ) as i64;
        if let Some(rate) = inputs.spray_rate {
            self.spray_acc += div_round(
                (i64::from(rate) * SPRAY_MAX_PER_S * MP_PER_PERMILLE) as i128,
                (1000 * TICKS_PER_SECOND) as i128,
            ) as i64;
        }
        let mut emitted: u32 = 0;
        let land = |this: &mut Self, events: &mut Vec<ArousalEvent>| {
            const AREAS: [&str; 4] = ["face.left", "face.right", "chest", "body"];
            let area = AREAS[(this.fluid_next() % 4) as usize];
            if area.starts_with("face") {
                this.fluid_contact_ticks = FLUID_CONTACT_TICKS;
                if this.profile.fluid_contact_input_permille > 0 {
                    this.pending_face_input = Some(this.profile.fluid_contact_input_permille);
                }
                events.push(ArousalEvent::FluidFaceContact { area });
            }
        };
        let mut saliva_strands = 0u32;
        while self.saliva_acc >= MP_PER_PERMILLE && emitted < EMISSION_TICK_MAX {
            self.saliva_acc -= MP_PER_PERMILLE;
            saliva_strands += 1;
            emitted += 1;
            land(self, events);
        }
        let mut spray_droplets = 0u32;
        while self.spray_acc >= MP_PER_PERMILLE && emitted < EMISSION_TICK_MAX {
            self.spray_acc -= MP_PER_PERMILLE;
            spray_droplets += 1;
            emitted += 1;
            land(self, events);
        }
        if emitted >= EMISSION_TICK_MAX {
            // Bounded work: excess accumulation is dropped,
            // never queued (§6.4's bounded emission).
            self.saliva_acc = self.saliva_acc.min(MP_PER_PERMILLE - 1);
            self.spray_acc = self.spray_acc.min(MP_PER_PERMILLE - 1);
        }
        if inputs.face_contact {
            self.fluid_contact_ticks = FLUID_CONTACT_TICKS;
        }
        // §6.5 expression selection (band, then resolved
        // per character by the presentation).
        let expression_key = if self.fluid_contact_ticks > 0 {
            "fluid-contact"
        } else if self.phase == Phase::Culminating {
            "peak"
        } else if self.phase == Phase::Taper || self.phase == Phase::Afterglow {
            "afterglow"
        } else if self.phase == Phase::Stimulated && a_permille >= 800 {
            "near"
        } else if a_permille >= 550 || permille_of(self.charge_mp) >= 700 {
            "building"
        } else if a_permille >= 250 {
            "aroused"
        } else {
            "ambient"
        };
        if self.fluid_contact_ticks > 0 {
            self.fluid_contact_ticks -= 1;
        }
        EffectsOutput {
            blush_permille: blush,
            sweat_permille: permille_of(self.sweat_mp),
            tremble_milli_px,
            tremble_rate_millihz,
            expression_key,
            performance,
            milk_pulses,
            saliva_strands,
            spray_droplets,
        }
    }

    /// The canonical per-tick record (§5.11): the §5.2
    /// vector in whole permille, the worn set, and the
    /// tick's events, in a fixed field order.
    pub fn canonical_record(&self, events: &[ArousalEvent]) -> String {
        let v = self.vector();
        let worn = self.worn_ids().join(",");
        let ev: Vec<String> = events
            .iter()
            .map(|e| match e {
                ArousalEvent::Culmination {
                    episode,
                    tier,
                    p_permille,
                } => format!("culm:{episode}:{tier}:{p_permille}"),
                ArousalEvent::Rearmed { episode } => format!("rearm:{episode}"),
                ArousalEvent::PhaseChanged { from, to } => {
                    format!("ph:{}>{}", from.token(), to.token())
                }
                ArousalEvent::FluidFaceContact { area } => format!("fluid:{area}"),
            })
            .collect();
        format!(
            "t{}|A{}|Q{}|S{}|F{}|P{}|D{}|ph{}|L{}|E{}|W{}|V{}",
            self.tick,
            v.arousal,
            v.pleasure_charge,
            v.sensitivity,
            v.floor,
            v.peak_input,
            v.dose,
            v.phase.token(),
            u8::from(v.culmination_latch),
            v.episode,
            worn,
            ev.join(",")
        )
    }

    /// A hash of the engine's full internal state (the
    /// op log's pre/post anchor; finer than the vector —
    /// it distinguishes states the whole-permille record
    /// rounds together).
    pub fn state_hash(&self) -> String {
        let mut parts: Vec<String> = vec![
            format!("a{}", self.arousal_mp),
            format!("q{}", self.charge_mp),
            format!("s{}", self.sensitivity_mp),
            format!("f{}", self.floor_mp),
            format!("p{}", self.peak_mp),
            format!("d{}", self.dose_mp),
            format!("ph{}", self.phase.token()),
            format!("l{}", u8::from(self.latch)),
            format!("e{}", self.episode),
            format!("pt{}", self.phase_ticks),
            format!("g{}", self.afterglow_level_mp),
            format!("t{}", self.tick),
            format!("cc{}", self.culmination_count),
            format!("mg{}", self.mirror_generation),
            format!("sw{}", self.sweat_mp),
        ];
        for d in &self.devices {
            parts.push(format!(
                "dev{}:{}:{}:{}:{}",
                d.spec.item_id,
                u8::from(d.on),
                u8::from(d.displaced),
                d.phase,
                d.spec.contact_quality
            ));
        }
        format!("{:016x}", fnv1a_64(parts.join("|").as_bytes()))
    }
}

/// The persisted engine record (§5.9): the state vector
/// at full internal precision, the worn set with runtime
/// state, and the profile. Mirror state is NOT in this
/// record — §5.9: it is never persisted, and after a
/// restart the mirror generation restarts at 0 with no
/// mirror session in existence.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PersistRecord {
    format: String,
    profile: CharacterProfile,
    arousal_mp: i64,
    charge_mp: i64,
    sensitivity_mp: i64,
    floor_mp: i64,
    peak_mp: i64,
    dose_mp: i64,
    phase: Phase,
    latch: bool,
    episode: u32,
    phase_ticks: u64,
    afterglow_level_mp: i64,
    tick: u64,
    tier: Option<u8>,
    culmination_count: u32,
    sweat_mp: i64,
    fluid_rng: u64,
    devices: Vec<PersistDevice>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PersistDevice {
    spec: DeviceSpec,
    phase: u64,
    on: bool,
    displaced: bool,
}

impl ArousalEngine {
    fn to_record(&self) -> PersistRecord {
        PersistRecord {
            format: ENGINE_FORMAT.to_string(),
            profile: self.profile.clone(),
            arousal_mp: self.arousal_mp,
            charge_mp: self.charge_mp,
            sensitivity_mp: self.sensitivity_mp,
            floor_mp: self.floor_mp,
            peak_mp: self.peak_mp,
            dose_mp: self.dose_mp,
            phase: self.phase,
            latch: self.latch,
            episode: self.episode,
            phase_ticks: self.phase_ticks,
            afterglow_level_mp: self.afterglow_level_mp,
            tick: self.tick,
            tier: self.tier,
            culmination_count: self.culmination_count,
            sweat_mp: self.sweat_mp,
            fluid_rng: self.fluid_rng,
            devices: self
                .devices
                .iter()
                .map(|d| PersistDevice {
                    spec: d.spec.clone(),
                    phase: d.phase,
                    on: d.on,
                    displaced: d.displaced,
                })
                .collect(),
        }
    }

    fn from_record(rec: PersistRecord) -> R<ArousalEngine> {
        let bad = |what: &str| Refusal::new("TG-ARO-STATE", format!("persisted state: {what}"));
        if rec.format != ENGINE_FORMAT {
            return Err(bad(&format!("format must be {ENGINE_FORMAT}")));
        }
        rec.profile
            .validate()
            .map_err(|_| bad("profile out of bounds"))?;
        for (what, v, lo, hi) in [
            ("arousal", rec.arousal_mp, 0, MP_FULL),
            ("charge", rec.charge_mp, 0, Q_MAX_MP),
            ("sensitivity", rec.sensitivity_mp, S_MIN_MP, S_MAX_MP),
            ("floor", rec.floor_mp, 0, 600 * MP_PER_PERMILLE),
            ("peak", rec.peak_mp, 0, MP_FULL),
            ("dose", rec.dose_mp, 0, MP_FULL),
            ("afterglow_level", rec.afterglow_level_mp, 0, MP_FULL),
            ("sweat", rec.sweat_mp, 0, MP_FULL),
        ] {
            if !(lo..=hi).contains(&v) {
                return Err(bad(&format!("{what} {v} outside {lo}..={hi}")));
            }
        }
        if let Some(t) = rec.tier {
            if !(1..=4).contains(&t) {
                return Err(bad(&format!("tier {t} outside 1..=4")));
            }
        }
        let mut devices = Vec::with_capacity(rec.devices.len());
        for pd in rec.devices {
            pd.spec
                .validate()
                .map_err(|_| bad("device spec out of bounds"))?;
            devices.push(DeviceRuntime {
                spec: pd.spec,
                phase: pd.phase,
                on: pd.on,
                displaced: pd.displaced,
                last_u: 0,
                last_effective: 0,
            });
        }
        Ok(ArousalEngine {
            profile: rec.profile,
            arousal_mp: rec.arousal_mp,
            charge_mp: rec.charge_mp,
            sensitivity_mp: rec.sensitivity_mp,
            floor_mp: rec.floor_mp,
            peak_mp: rec.peak_mp,
            dose_mp: rec.dose_mp,
            phase: rec.phase,
            latch: rec.latch,
            episode: rec.episode,
            phase_ticks: rec.phase_ticks,
            afterglow_level_mp: rec.afterglow_level_mp,
            tick: rec.tick,
            tier: rec.tier,
            culmination_count: rec.culmination_count,
            // §5.9: mirror state does not exist after a
            // restart; the generation restarts at 0 and no
            // mirror session can be confused with a live one
            // because none can be restored as enabled.
            mirror_generation: 0,
            devices,
            sweat_mp: rec.sweat_mp,
            saliva_acc: 0,
            spray_acc: 0,
            fluid_rng: rec.fluid_rng,
            fluid_contact_ticks: 0,
            pending_face_input: None,
        })
    }

    /// Field-for-field internal equality (§5.9's gate
    /// form): every persisted field plus the runtime
    /// fields a restore zeroes by rule.
    pub fn internal_eq(&self, other: &ArousalEngine) -> bool {
        self.to_record() == other.to_record() && self.mirror_generation == other.mirror_generation
    }

    /// Saves the engine state (§5.9's write pattern:
    /// sibling temp + rename).
    pub fn save_to(&self, path: &Path) -> R<()> {
        let text = serde_json::to_string(&self.to_record())
            .map_err(|e| Refusal::new("TG-ARO-STATE", format!("serialise: {e}")))?;
        let tmp = tmp_sibling(path);
        std::fs::write(&tmp, format!("{text}\n"))
            .map_err(|e| Refusal::new("TG-ARO-STATE", format!("write {}: {e}", tmp.display())))?;
        std::fs::rename(&tmp, path).map_err(|e| {
            Refusal::new("TG-ARO-STATE", format!("rename to {}: {e}", path.display()))
        })?;
        Ok(())
    }

    /// Loads a saved engine. A corrupt or out-of-bounds
    /// record refuses whole (`TG-ARO-STATE`); a state that
    /// cannot be validated is never partially loaded.
    pub fn load_from(path: &Path) -> R<ArousalEngine> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| Refusal::new("TG-ARO-STATE", format!("read {}: {e}", path.display())))?;
        let rec: PersistRecord = serde_json::from_str(&text).map_err(|e| {
            let code = if e.to_string().contains("unknown field") {
                "TG-ARO-UNKNOWN-FIELD"
            } else {
                "TG-ARO-STATE"
            };
            Refusal::new(code, format!("persisted state: {e}"))
        })?;
        ArousalEngine::from_record(rec)
    }
}

fn tmp_sibling(path: &Path) -> PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(".tmp");
    PathBuf::from(s)
}

/// A session operation on one character's engine (§5.10).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum ArousalOp {
    /// Wear/attach an item.
    ApplyItem { item: DeviceSpec },
    /// Remove an item.
    RemoveItem { item_id: String },
    /// Change a worn device's waveform.
    SetWave { item_id: String, wave: WaveSpec },
    /// Feed the physics layer's contact result (§4.3).
    SetContact {
        item_id: String,
        contact_quality: u32,
        displaced: bool,
    },
    /// Device STOP (§5.8) — a safety exit, never refused.
    StopDevice { item_id: String },
    /// Reactivate a stopped device.
    StartDevice { item_id: String },
    /// The §3.4 control — a safety exit, never refused.
    RemoveAllAndReset,
    /// `arousal.reset` (§5.8) — never refused.
    Reset,
}

/// One op-log entry (§5.10): the op as admitted and the
/// engine state hash after it applied.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArousalLogEntry {
    pub seq: u64,
    pub character: String,
    pub op: serde_json::Value,
    pub state_hash: String,
}

/// The session-host layer's arousal service: one engine
/// per character, the §5.10 op log, and the §5.9 save
/// cadence. The Playroom dashboard and (later) the
/// companion present engines from here — one store.
#[derive(Debug, Default)]
pub struct ArousalHost {
    engines: BTreeMap<String, HostEngine>,
    log: Vec<ArousalLogEntry>,
}

#[derive(Debug)]
struct HostEngine {
    engine: ArousalEngine,
    /// State hash at the last successful save (cadence).
    saved_hash: Option<String>,
    /// Ticks since the last save.
    ticks_since_save: u64,
    /// A phase/latch/worn change happened since the save.
    structural_dirty: bool,
}

impl ArousalHost {
    /// An empty host.
    pub fn new() -> ArousalHost {
        ArousalHost {
            engines: BTreeMap::new(),
            log: Vec::new(),
        }
    }

    /// The op log (§5.10).
    pub fn log(&self) -> &[ArousalLogEntry] {
        &self.log
    }

    /// Gets or creates the character's engine under
    /// `profile` (an existing engine keeps its profile).
    pub fn engine(&mut self, character: &str, profile: CharacterProfile) -> &mut ArousalEngine {
        &mut self
            .engines
            .entry(character.to_string())
            .or_insert_with(|| HostEngine {
                engine: ArousalEngine::new(profile),
                saved_hash: None,
                ticks_since_save: 0,
                structural_dirty: true,
            })
            .engine
    }

    /// Read access to a character's engine, when present.
    pub fn get(&self, character: &str) -> Option<&ArousalEngine> {
        self.engines.get(character).map(|h| &h.engine)
    }

    fn require(&mut self, character: &str) -> R<&mut HostEngine> {
        self.engines.get_mut(character).ok_or_else(|| {
            Refusal::new(
                "TG-ARO-UNKNOWN-CHARACTER",
                format!("no engine for character {character:?}"),
            )
        })
    }

    /// Admits and applies one session operation (§5.10).
    /// Validation is the session-local rule set; the
    /// safety exits are admitted unconditionally.
    pub fn apply_op(&mut self, character: &str, op: &ArousalOp) -> R<ArousalLogEntry> {
        // Create-on-first-use with the default profile:
        // hosts that need a custom profile create the
        // engine through `engine()` first.
        if !self.engines.contains_key(character) {
            self.engine(character, CharacterProfile::default());
        }
        let host = self.require(character)?;
        let mut structural = false;
        let state_hash = {
            let engine = &mut host.engine;
            match op {
                ArousalOp::ApplyItem { item } => {
                    item.validate()?;
                    if engine.worn_ids().contains(&item.item_id.as_str()) {
                        return Err(Refusal::new(
                            "TG-ARO-CONFLICT",
                            format!("item {:?} is already worn", item.item_id),
                        ));
                    }
                    engine.apply_item(item.clone());
                    structural = true;
                }
                ArousalOp::RemoveItem { item_id } => {
                    require_item(engine, item_id)?;
                    engine.remove_item(item_id);
                    structural = true;
                }
                ArousalOp::SetWave { item_id, wave } => {
                    wave.validate()?;
                    require_item(engine, item_id)?;
                    engine.set_wave(item_id, wave.clone());
                }
                ArousalOp::SetContact {
                    item_id,
                    contact_quality,
                    displaced,
                } => {
                    if *contact_quality > 1000 {
                        return Err(bound("contact_quality", *contact_quality, 1000));
                    }
                    require_item(engine, item_id)?;
                    engine.set_contact(item_id, *contact_quality, *displaced);
                }
                ArousalOp::StopDevice { item_id } => {
                    require_item(engine, item_id)?;
                    engine.stop_device(item_id);
                }
                ArousalOp::StartDevice { item_id } => {
                    require_item(engine, item_id)?;
                    engine.start_device(item_id);
                }
                ArousalOp::RemoveAllAndReset => {
                    engine.remove_all_and_reset();
                    structural = true;
                }
                ArousalOp::Reset => {
                    engine.reset();
                    structural = true;
                }
            }
            engine.state_hash()
        };
        if structural {
            host.structural_dirty = true;
        }
        let entry = ArousalLogEntry {
            seq: self.log.len() as u64 + 1,
            character: character.to_string(),
            op: serde_json::to_value(op).expect("op serialisation is total"),
            state_hash,
        };
        self.log.push(entry.clone());
        Ok(entry)
    }

    /// Advances one character's engine one tick and folds
    /// the output into the §5.9 save cadence bookkeeping.
    pub fn step(&mut self, character: &str, inputs: &TickInputs) -> R<EngineOutput> {
        let host = self.require(character)?;
        let before_phase = host.engine.phase;
        let before_latch = host.engine.vector().culmination_latch;
        let out = host.engine.step(inputs);
        host.ticks_since_save += 1;
        if host.engine.phase != before_phase
            || host.engine.vector().culmination_latch != before_latch
        {
            host.structural_dirty = true;
        }
        Ok(out)
    }

    /// §5.9's cadence: save every engine whose state
    /// changed since its last save — immediately on a
    /// phase/latch/worn change, otherwise at most once
    /// per second of ticks. Returns the characters saved.
    /// Files land at `<dir>/<character>.arousal.json`.
    pub fn persist_if_due(&mut self, dir: &Path) -> R<Vec<String>> {
        let mut saved = Vec::new();
        for (character, host) in &mut self.engines {
            let hash = host.engine.state_hash();
            let changed = host.saved_hash.as_deref() != Some(hash.as_str());
            if !changed {
                continue;
            }
            if !host.structural_dirty && host.ticks_since_save < TICKS_PER_SECOND {
                continue;
            }
            let path = dir.join(format!("{character}.arousal.json"));
            host.engine.save_to(&path)?;
            host.saved_hash = Some(hash);
            host.ticks_since_save = 0;
            host.structural_dirty = false;
            saved.push(character.clone());
        }
        Ok(saved)
    }

    /// Restores a character's engine from `<dir>/<character>.arousal.json`
    /// (§5.9). The restored latch stays latched; no mirror
    /// state exists to restore.
    pub fn restore(&mut self, dir: &Path, character: &str) -> R<()> {
        let path = dir.join(format!("{character}.arousal.json"));
        let engine = ArousalEngine::load_from(&path)?;
        let hash = engine.state_hash();
        self.engines.insert(
            character.to_string(),
            HostEngine {
                engine,
                saved_hash: Some(hash),
                ticks_since_save: 0,
                structural_dirty: false,
            },
        );
        Ok(())
    }
}

fn require_item(engine: &ArousalEngine, item_id: &str) -> R<()> {
    if engine.worn_ids().contains(&item_id) {
        Ok(())
    } else {
        Err(Refusal::new(
            "TG-ARO-UNKNOWN-ITEM",
            format!("item {item_id:?} is not worn"),
        ))
    }
}
