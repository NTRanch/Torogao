//! The Playroom's **Playtime** mode and its first setting,
//! **Blossom** (AMENDMENT-PLAYMODE as amended by D-226:
//! Playtime is the mode; Blossom is setting #1 inside it;
//! "the Fountain Seat" is retired and appears nowhere).
//!
//! Everything the amendment specifies as machinery lives
//! here, built **on** the T4 engine ([`crate::arousal`]),
//! never beside it: the stage overlay machine (§3.2), the
//! Hold input governor (§3.3), the contraction-pulse
//! generator (`pulse-gen/1`, §4), internal ejection
//! (`ejection/1`, §5), wetness accumulation (`wetness/1`,
//! §6), the mode's gear composition seam (§7), mode
//! validation (§8.4), and the cue engine — the engine half
//! of the §9 sound design, firing to a **null audio sink**:
//! the triggers, the cue track, and the determinism are
//! real; the audio assets are PM-1S's, and a PM-1 session
//! is honestly labelled sound-pending.
//!
//! ## Numeric contract
//!
//! Like the engine (§5.1 of AMENDMENT-T4), all mode state
//! is integer: permille quantities are stored in
//! micro-permille, volumes in micro-units, ballistics in
//! milli-pixels. No float ever touches mode state, so the
//! canonical mode trajectory (§9.6: one record format —
//! the engine's §5.11 record plus the mode segment) is
//! bit-identical on every machine.
//!
//! ## The governor's constants (named interpretation)
//!
//! §3.3 defines `I_hold` as "the input at which §5.3's
//! charge law nets to zero" and writes the formula with
//! §5.3's constants as they stood when the amendment was
//! drafted (charge 110, leak τ 45 s → denominator
//! 45·110 = 4950). D-211 — ruled *after* the amendment was
//! written — amended §5.3 itself (charge 150, leak τ
//! 200 s). The governor here is therefore computed from
//! the engine's **live** §5.3 constants, carried as mode
//! data ([`GovernorData`], defaulting to 150 / 200), i.e.
//! the formula's *derivation* is implemented, not its
//! superseded numerals. The measured conflict and this
//! resolution are recorded in GATE-REPORT-PM1.md; the
//! data can be re-pointed at 110 / 45 by a ruling, at
//! which point gate PM-G6 fails by the same measurement.

use crate::arousal::{
    ArousalEvent, ArousalHost, ArousalOp, CharacterProfile, EngineOutput, Phase, TickInputs,
    WaveSpec, fnv1a_64,
};
use crate::refusal::{R, Refusal};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The mode definition format (D-226: the mode is
/// Playtime; this data version covers the mode and its
/// settings architecture).
pub const MODE_FORMAT: &str = "playtime/1";
/// The first (and initially only) Playtime setting.
pub const BLOSSOM_SETTING_ID: &str = "blossom";
/// Engine data versions (§2.1 of the amendment), carried
/// inside the mode's data.
pub const PULSE_FORMAT: &str = "pulse-gen/1";
pub const EJECTION_FORMAT: &str = "ejection/1";
pub const WETNESS_FORMAT: &str = "wetness/1";
pub const MIX_FORMAT: &str = "playmode-mix/1";
/// The mode-settings / preference-set document format.
pub const SETTINGS_FORMAT: &str = "playtime-settings/1";
/// The persisted mode-session state format (§6.1, §5.9
/// pattern).
pub const STATE_FORMAT: &str = "playtime-state/1";
/// The take record format (§8.3).
pub const TAKE_FORMAT: &str = "playtime-take/1";

/// The starter-pack fixture's item id (§7.1, renamed per
/// D-226; the gear document lives in `torogao-gear`'s
/// `playtime_pack`).
pub const FIXTURE_ITEM_ID: &str = "fixture.blossom-seat";
/// The starter-pack mounted device's item id (§7.2).
pub const DEVICE_ITEM_ID: &str = "device.blossom-mount";
/// The device's contact site (§7.2): the canonical
/// `body.perineum` contact.
pub const DEVICE_SITE: &str = "perineum";
/// The ejection emitter's anchor (§5.1): the only anchor
/// an ejection emitter may be declared on.
pub const EJECT_ANCHOR: &str = "body.perineum";

/// Micro-permille in one permille (the engine's §5.1 unit).
const MP: i64 = 1_000_000;
/// Micro-permille at full scale (1000 permille).
const MP_FULL: i64 = 1_000_000_000;
/// Micro-units in one volume unit.
const UV: i64 = 1_000_000;
/// Fixed-step rate (the engine's §4.1 step).
const TICKS_PER_SECOND: u64 = 60;

/// Half-up division for non-negative values (the engine's
/// §5.1 rounding rule).
fn div_round(num: i128, den: i128) -> i128 {
    debug_assert!(den > 0 && num >= 0);
    (num + den / 2) / den
}

/// Signed half-up division (round half away from zero on
/// the magnitude): the §5.4 ballistic decomposition,
/// where the fan's sine term is negative on one side.
fn div_round_signed(num: i128, den: i128) -> i128 {
    debug_assert!(den > 0);
    if num < 0 {
        -div_round(-num, den)
    } else {
        div_round(num, den)
    }
}

/// Integer cube root (floor), Newton iteration on u128 —
/// deterministic, libm-free.
fn icbrt(v: u128) -> u128 {
    if v == 0 {
        return 0;
    }
    let mut x = v;
    let mut y = x.div_ceil(3);
    while y < x {
        x = y;
        y = (2 * x + v / (x * x)) / 3;
    }
    x
}

/// Sine of an angle in milli-degrees, returned in micro
/// units in [-1_000_000, 1_000_000]. Quadrant reduction
/// with the same integer polynomial kernels the engine
/// uses (§5.1's determinism discipline).
fn sin_millideg(mdeg: i64) -> i64 {
    const PI_HALF_MICRO: i64 = 1_570_796;
    fn sin_poly(t: i64) -> i64 {
        let t2 = div_round((t as i128) * (t as i128), 1_000_000) as i64;
        let t3 = div_round((t2 as i128) * (t as i128), 1_000_000) as i64;
        let t5 = {
            let t4 = div_round((t3 as i128) * (t as i128), 1_000_000) as i64;
            div_round((t4 as i128) * (t as i128), 1_000_000) as i64
        };
        t - t3 / 6 + t5 / 120
    }
    fn cos_poly(t: i64) -> i64 {
        let t2 = div_round((t as i128) * (t as i128), 1_000_000) as i64;
        let t4 = div_round((t2 as i128) * (t2 as i128), 1_000_000) as i64;
        1_000_000 - t2 / 2 + t4 / 24
    }
    let frac = (mdeg.rem_euclid(360_000) as i128) * 1_000_000 / 360_000;
    let quadrant = frac / 250_000;
    let rem = frac % 250_000;
    let t = div_round(rem * (PI_HALF_MICRO as i128), 250_000) as i64;
    match quadrant {
        0 => sin_poly(t),
        1 => cos_poly(t),
        2 => -sin_poly(t),
        _ => -cos_poly(t),
    }
}

/// Cosine in milli-degrees, micro units.
fn cos_millideg(mdeg: i64) -> i64 {
    sin_millideg(mdeg + 90_000)
}

/// The next value of a SplitMix64 stream (the engine's
/// §6.4 stream discipline): pure integer, identical
/// everywhere.
fn splitmix_next(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

// ------------------------------------------------------------------ data

/// The Hold governor's data (§3.3): the §5.3 charge-law
/// constants the equilibrium is derived from. Defaults
/// are the engine's live D-211 constants; see the module
/// header for the named interpretation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GovernorData {
    /// §5.3 charge coefficient (permille/s at I = 1,
    /// S = 1000).
    pub charge_coeff: i64,
    /// §5.3 Q leak time constant, seconds.
    pub leak_tau_s: i64,
}

impl GovernorData {
    /// `I_hold(Q, S)` in permille: the input at which the
    /// charge law nets to zero,
    /// `(Q / (τ · coeff · (S/1000)))^(2/3)`, in integer
    /// math (cube root over micro units).
    pub fn i_hold_permille(&self, q_permille: u32, s_permille: u32) -> u32 {
        if q_permille == 0 || s_permille == 0 {
            return 0;
        }
        // r = 1000·Q / (τ · coeff · S), a micro fraction.
        let r_micro = div_round(
            1000 * (q_permille as i128) * 1_000_000,
            (self.leak_tau_s as i128) * (self.charge_coeff as i128) * (s_permille as i128),
        ) as u128;
        // r^(2/3) in micro = cbrt(r_micro² · 1e6).
        let i_micro = icbrt(r_micro * r_micro * 1_000_000);
        ((i_micro + 500) / 1000).min(1000) as u32
    }
}

/// The pulse law as data (`pulse-gen/1`, §4.2). Bounds
/// are validator-enforced (§8.4 `TG-PM-PULSE-BOUND`):
/// rate ≤ 2000 millihertz, depth ≤ 1000, lobe gap
/// exactly 9 ticks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PulseLawData {
    /// Stimulated band 1 (A 250–549): rate, millihertz.
    pub band1_rate_millihz: u32,
    /// Band 1 depth: base + span·(Q/1000).
    pub band1_depth_base: u32,
    pub band1_depth_span: u32,
    /// Band 2 (A 550–799): rate ramps lo → hi millihertz
    /// linearly in A; depth base + span·(Q/1000).
    pub band2_rate_lo_millihz: u32,
    pub band2_rate_hi_millihz: u32,
    pub band2_depth_base: u32,
    pub band2_depth_span: u32,
    /// Band 3 (A ≥ 800 or Q ≥ 700): rate ramps lo → hi;
    /// depth base + span·(Q/1000).
    pub band3_rate_lo_millihz: u32,
    pub band3_rate_hi_millihz: u32,
    pub band3_depth_base: u32,
    pub band3_depth_span: u32,
    /// Culminating: rate = base + per-tier · tier,
    /// millihertz; depths per tier (Tiers 1–4).
    pub culminating_rate_base_millihz: u32,
    pub culminating_rate_per_tier_millihz: u32,
    pub culminating_depths: [u32; 4],
    /// Afterglow: one pulse per this many ticks while
    /// A ≥ 400; depth capped at `afterglow_depth_cap`.
    pub afterglow_interval_ticks: u64,
    pub afterglow_depth_cap: u32,
    /// The second lobe's delay, ticks (D2: 0.15 s).
    pub lobe_gap_ticks: u64,
    /// The second lobe's depth scale, permille of the
    /// main lobe's depth (D2: ×0.8).
    pub lobe_depth_scale_permille: u32,
    /// Lobe duration for the body-cycle consumer, ticks.
    pub lobe_duration_ticks: u64,
    /// Validator bounds (§4.2).
    pub max_rate_millihz: u32,
    pub max_depth_permille: u32,
}

impl Default for PulseLawData {
    /// The §4.2 table, verbatim.
    fn default() -> Self {
        PulseLawData {
            band1_rate_millihz: 500,
            band1_depth_base: 150,
            band1_depth_span: 250,
            band2_rate_lo_millihz: 500,
            band2_rate_hi_millihz: 1200,
            band2_depth_base: 300,
            band2_depth_span: 400,
            band3_rate_lo_millihz: 1200,
            band3_rate_hi_millihz: 1600,
            band3_depth_base: 550,
            band3_depth_span: 450,
            culminating_rate_base_millihz: 1000,
            culminating_rate_per_tier_millihz: 200,
            culminating_depths: [300, 550, 800, 1000],
            afterglow_interval_ticks: 300,
            afterglow_depth_cap: 250,
            lobe_gap_ticks: 9,
            lobe_depth_scale_permille: 800,
            lobe_duration_ticks: 6,
            max_rate_millihz: 2000,
            max_depth_permille: 1000,
        }
    }
}

/// The ejection data (`ejection/1`, §5).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EjectionData {
    /// The emitter's anchor (§5.1). Any other value is
    /// refused at validation (`TG-PM-EJECT-ANCHOR`).
    pub emitter_anchor: String,
    /// Pressure build coefficient (§5.2): permille/s at
    /// Q = 1000, I = 1, threshold 1000.
    pub build_rate: i64,
    /// Pressure decay time constant, seconds (§5.2: the
    /// `P_f / 20` term).
    pub decay_tau_s: i64,
    /// Leak band edge, permille (§5.2).
    pub leak_band_permille: u32,
    /// Burst threshold, permille (§5.2).
    pub burst_threshold_permille: u32,
    /// A burst discharges this share of current pressure
    /// — expressed as the retained permille (§5.2: 35%
    /// discharged → 650 retained).
    pub burst_retain_permille: u32,
    /// Reference episode volume unit at scale 1000
    /// (§5.3), abstract units.
    pub v_ep_reference_units: i64,
    /// Burst volume fraction of `V_ep` (§5.3: 0.08).
    pub burst_fraction_permille: u32,
    /// Final-release tier scalars (§5.3), permille of
    /// `V_ep`, Tiers 1–4.
    pub final_scalars_permille: [u32; 4],
    /// Ballistics (§5.4): v0 = base + span·(P_f/1000)
    /// rig-px/s.
    pub v0_base_px_s: i64,
    pub v0_span_px_s: i64,
    /// Fan half-angle around each direction-class axis,
    /// milli-degrees.
    pub fan_half_angle_millideg: i64,
    /// Direction-class axes from straight down,
    /// milli-degrees (centre, left-out, right-out).
    pub class_axes_millideg: [i64; 3],
    /// Gravity, rig-px/s².
    pub gravity_px_s2: i64,
    /// Droplet life bound, ticks (§5.4: ≤ 2.5 s).
    pub droplet_life_ticks: u64,
    /// Alive bound per emitter (§5.4).
    pub alive_bound: usize,
    /// Emission bound per tick per emitter (§5.4).
    pub tick_emission_bound: u32,
    /// Droplets per volume unit at emission (the
    /// volume → droplet-count calibration).
    pub droplets_per_unit: u32,
    /// Leak emission: droplet rate at the band edge and
    /// at full pressure, droplets/s (§5.2's bounded
    /// continuous droplet emission).
    pub leak_rate_lo_per_s: u32,
    pub leak_rate_hi_per_s: u32,
    /// Per-droplet volume for leak droplets, micro-units.
    pub leak_droplet_volume_micro: i64,
    /// The ejection stream's seed (engine data; changing
    /// it changes trajectories).
    pub stream_seed: u64,
}

impl Default for EjectionData {
    /// The §5 values, verbatim where the amendment gives
    /// them; calibration constants labelled as such.
    fn default() -> Self {
        EjectionData {
            emitter_anchor: EJECT_ANCHOR.to_string(),
            build_rate: 90,
            decay_tau_s: 20,
            leak_band_permille: 550,
            burst_threshold_permille: 400,
            burst_retain_permille: 650,
            v_ep_reference_units: 1000,
            burst_fraction_permille: 80,
            final_scalars_permille: [250, 500, 800, 1000],
            v0_base_px_s: 250,
            v0_span_px_s: 1350,
            fan_half_angle_millideg: 40_000,
            class_axes_millideg: [0, -40_000, 40_000],
            gravity_px_s2: 2600,
            droplet_life_ticks: 150,
            alive_bound: 4096,
            tick_emission_bound: 512,
            droplets_per_unit: 4,
            leak_rate_lo_per_s: 6,
            leak_rate_hi_per_s: 30,
            leak_droplet_volume_micro: 100_000,
            stream_seed: 0x853c_49e6_748f_ea9b,
        }
    }
}

/// The wetness data (`wetness/1`, §6).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WetnessData {
    /// The field's hit areas, in canonical (sorted)
    /// order: body areas, the fixture's surfaces, the
    /// floor zone (§6.1).
    pub areas: Vec<String>,
    /// The drip chain: area → the area below it (§6.2:
    /// body → fixture → floor). Floor areas pool (no
    /// entry).
    pub drip_below: BTreeMap<String, String>,
    /// Deposit calibration: permille of `w` per volume
    /// unit landed (the D4 anchor calibrates the scale
    /// so a peak episode reads at the reference's
    /// coverage class).
    pub deposit_permille_per_unit: i64,
    /// Sheen threshold (§6.2).
    pub sheen_threshold_permille: u32,
    /// Drip threshold (§6.2).
    pub drip_threshold_permille: u32,
    /// Drip interval at the threshold and at full
    /// wetness, ticks (§6.2: 240 → 45, linear in w).
    pub drip_interval_at_threshold_ticks: u64,
    pub drip_interval_at_full_ticks: u64,
    /// A drip transfers this share of the source area's
    /// wetness volume (§6.2: volume-conserving).
    pub drip_share_permille: u32,
    /// Drip droplet spawn position within the source
    /// area's extent is drawn from the wetness stream
    /// (seeded, §6.2's deterministic schedule).
    pub stream_seed: u64,
}

impl Default for WetnessData {
    fn default() -> Self {
        WetnessData {
            areas: vec![
                "body.lower".to_string(),
                "fixture.seat".to_string(),
                "floor.pool".to_string(),
            ],
            drip_below: BTreeMap::from([
                ("body.lower".to_string(), "fixture.seat".to_string()),
                ("fixture.seat".to_string(), "floor.pool".to_string()),
            ]),
            deposit_permille_per_unit: 40,
            sheen_threshold_permille: 150,
            drip_threshold_permille: 650,
            drip_interval_at_threshold_ticks: 240,
            drip_interval_at_full_ticks: 45,
            drip_share_permille: 25,
            stream_seed: 0x27d4_eb2f_1656_67c5,
        }
    }
}

/// The §9.3 mix table as data (`playmode-mix/1`):
/// lane priorities and ducking rules. PM-1 fires to a
/// null sink, so the table is contract data here;
/// PM-1S's pack QC verifies it by measurement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MixData {
    /// Priority of the vocal lane (0 = highest).
    pub vocal_priority: u32,
    pub fluid_priority: u32,
    pub device_priority: u32,
    pub body_priority: u32,
    pub room_priority: u32,
    /// Device hum duck under F-CULM-EJECT, dB (§9.3).
    pub hum_duck_db: i32,
    /// Hum sits at least this far below the vocal lane,
    /// dB.
    pub hum_below_vocal_db: i32,
    /// Ambience duck under any vocal cue, dB (§9.3: the
    /// Owner's law — ambience never masks the character
    /// lane).
    pub ambience_duck_db: i32,
}

/// A cue's asset kind (§9.2): a torori bundle kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssetKind {
    VoiceSet,
    BreathBed,
    HumRender,
    SfxOneShot,
    AmbienceBed,
    UiOneShot,
    /// No asset: a listed deliberate silence (§9.5).
    Silence,
}

/// A cue's trigger class (§9.2's named triggers, as
/// data). The cue engine evaluates these against the
/// tick's facts; a cue that cannot name its trigger
/// does not exist (§9.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CueTrigger {
    /// Continuous while the episode is active.
    BreathContinuous,
    /// A crosses 400 upward, or a pulse of depth ≥ 700
    /// (rate-limited, one per 90 ticks).
    Gasp,
    /// Pulse events in `Stimulated`, density by A band.
    MoanOnPulse,
    /// Edging-substate pulses of depth ≥ 300
    /// (rate-limited, one per 60 ticks).
    EdgeOnPulse,
    /// The §5.6 latch event — exactly once per episode.
    Latch,
    /// Afterglow pulses and the Afterglow phase entry.
    AfterglowCue,
    /// A listed deliberate silence (§9.5).
    DeliberateSilence,
    /// Touch input events with i ≥ 0.3.
    TouchInput,
    /// A face fluid-contact event (§6.5 of T4).
    FaceFluidContact,
    /// Continuous while the device drive u > 0.
    HumContinuous,
    /// Device activation (mode entry applies the device).
    DeviceOn,
    /// Device stop / mode exit.
    DeviceOff,
    /// Contact quality crosses a 0.25 band edge, or a
    /// fixture re-seat.
    ContactShift,
    /// Pulse events while the device is displaced.
    RattleOnPulse,
    /// The `FirstLeak` event; continuing leak emission.
    FirstLeak,
    /// Each ejection burst event.
    Burst,
    /// The culmination release train.
    CulminationTrain,
    /// A wetness deposit event cluster on a body area.
    DepositBody,
    /// A wetness deposit event cluster on the fixture.
    DepositFixture,
    /// A wetness deposit event cluster on the floor.
    DepositFloor,
    /// A wetness drip transfer event.
    DripTransfer,
    /// Each pulse event.
    PulseBody,
    /// Fixture strain crosses a band edge under pulses
    /// (rate-limited, one per 30 ticks).
    FixtureStrain,
    /// Clamp application/removal (free-play gear brought
    /// into the mode — not part of the mode composition,
    /// so the canonical session lists this cue
    /// not-applicable with that reason).
    ClampOp,
    /// An apparel state transition.
    ApparelTransition,
    /// Continuous while tremble is active.
    TrembleContinuous,
    /// Continuous while a mode session is active.
    AmbienceContinuous,
    /// Take start while the REC skin is on.
    RecStart,
    /// Take stop while the REC skin is on.
    RecStop,
    /// A session operation admitted.
    OpAdmitted,
    /// A session operation refused.
    OpRefused,
    /// The remove-all + reset act.
    RemoveAllAct,
}

/// One row of the §9.2 sound table, as data.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CueDef {
    /// The cue id (§9.2), e.g. `V-MOAN`.
    pub id: String,
    pub trigger: CueTrigger,
    pub asset_kind: AssetKind,
    /// Mix priority (§9.3): lower is higher priority.
    pub priority: u32,
    /// The asset-id template (§9.6): `{character}` and
    /// `{variant}` are resolved by the cue engine.
    pub asset_template: String,
}

/// One character's vocal set (§9.2(a), §9.6): per-class
/// stand-in asset counts. Every set in PM-1 is a
/// labelled **stand-in** — no VoiceIdentity reference is
/// locked for any girl yet (an Owner canon act), and
/// stand-ins are never presented as final.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VocalSet {
    /// Honesty class (§9.6): `stand-in` until the Owner
    /// locks final takes by ear.
    pub honesty: String,
    /// Vocal class key → number of stand-in variants.
    pub classes: BTreeMap<String, u32>,
}

/// The §9.2(a) vocal classes that must resolve per
/// character (gate PM-G11). `V-REFR` is a deliberate
/// near-silence and carries no asset class.
pub const VOCAL_CLASSES: [&str; 8] = [
    "breath",
    "gasp",
    "moan",
    "edge",
    "culm",
    "after",
    "touch",
    "fluid-react",
];

/// The governed expression keys (§6.5 of T4) the mode
/// profile must resolve per character.
pub const EXPRESSION_KEYS: [&str; 7] = [
    "ambient",
    "aroused",
    "building",
    "near",
    "peak",
    "afterglow",
    "fluid-contact",
];

/// One playmate's mode data (§3.1, §3.3 of the
/// amendment): her engine profile, her mode profile
/// fields, her governed expression recipes, her vocal
/// set. Profile *values* are Owner canon items; the
/// fixtures run on labelled placeholders (§3.1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CharacterModeData {
    /// Her arousal-engine profile (T4 §5.3's seam).
    pub profile: CharacterProfile,
    /// §3.1: scales the ejection pressure build rate,
    /// 500..=1500.
    pub eject_threshold_permille: u32,
    /// §3.1: scales her episode volume unit, 250..=2000.
    pub eject_volume_scale_permille: u32,
    /// Whether these field values are labelled
    /// placeholders (not Owner-ruled canon).
    pub fields_placeholder: bool,
    /// Governed expression key → her recipe id (§6.5:
    /// resolved per character, never substituted).
    pub expression_recipes: BTreeMap<String, String>,
    /// Her vocal set (§9.2(a)).
    pub vocal_set: VocalSet,
}

/// The Playtime mode definition as data (§2): the stage
/// table, the setting (Blossom), per-character data, the
/// extension data versions, the mix table, and the §9.2
/// cue table. Saving a composition built on this
/// definition is a Playroom preference set (§2.2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlaytimeDefinition {
    /// [`MODE_FORMAT`].
    pub format: String,
    /// The setting this definition carries (D-226:
    /// Blossom is setting #1 of Playtime).
    pub setting_id: String,
    pub governor: GovernorData,
    pub pulse: PulseLawData,
    pub ejection: EjectionData,
    pub wetness: WetnessData,
    pub mix: MixData,
    /// Playmate name → her mode data.
    pub characters: BTreeMap<String, CharacterModeData>,
    /// The §9.2 cue table.
    pub cues: Vec<CueDef>,
}

impl PlaytimeDefinition {
    /// The Blossom definition with the §3.1 labelled
    /// placeholder fields: Prima 1000/1000 (reference),
    /// Yui 900/850, Alma 1100/1150 (threshold/volume).
    pub fn blossom() -> PlaytimeDefinition {
        let character = |name: &str, threshold: u32, volume: u32| {
            let expression_recipes = EXPRESSION_KEYS
                .iter()
                .map(|key| (key.to_string(), format!("{name}.expr.{key}")))
                .collect();
            let vocal_set = VocalSet {
                honesty: "stand-in".to_string(),
                classes: VOCAL_CLASSES
                    .iter()
                    .map(|class| (class.to_string(), 3))
                    .collect(),
            };
            CharacterModeData {
                profile: CharacterProfile::default(),
                eject_threshold_permille: threshold,
                eject_volume_scale_permille: volume,
                fields_placeholder: true,
                expression_recipes,
                vocal_set,
            }
        };
        PlaytimeDefinition {
            format: MODE_FORMAT.to_string(),
            setting_id: BLOSSOM_SETTING_ID.to_string(),
            governor: GovernorData {
                charge_coeff: 150,
                leak_tau_s: 200,
            },
            pulse: PulseLawData::default(),
            ejection: EjectionData::default(),
            wetness: WetnessData::default(),
            mix: MixData {
                vocal_priority: 0,
                fluid_priority: 1,
                device_priority: 2,
                body_priority: 3,
                room_priority: 4,
                hum_duck_db: 6,
                hum_below_vocal_db: 6,
                ambience_duck_db: -12,
            },
            characters: BTreeMap::from([
                ("prima".to_string(), character("prima", 1000, 1000)),
                ("yui".to_string(), character("yui", 900, 850)),
                ("alma".to_string(), character("alma", 1100, 1150)),
            ]),
            cues: cue_table(),
        }
    }

    /// One character's mode data.
    pub fn character(&self, name: &str) -> R<&CharacterModeData> {
        self.characters.get(name).ok_or_else(|| {
            Refusal::new(
                "TG-PM-UNKNOWN-PLAYMATE",
                format!("no playmate {name:?} in the mode definition"),
            )
        })
    }

    /// Her episode volume unit `V_ep` in micro-units
    /// (§5.3: reference units scaled by her volume
    /// field).
    pub fn v_ep_micro(&self, data: &CharacterModeData) -> i64 {
        (self.ejection.v_ep_reference_units as i128
            * (data.eject_volume_scale_permille as i128)
            * (UV as i128)
            / 1000) as i64
    }
}

/// The §9.2 sound table as data (cue id → trigger →
/// asset kind → mix priority → asset template). This is
/// the frozen table PM-1S binds assets against.
fn cue_table() -> Vec<CueDef> {
    let def = |id: &str, trigger: CueTrigger, kind: AssetKind, priority: u32, tpl: &str| CueDef {
        id: id.to_string(),
        trigger,
        asset_kind: kind,
        priority,
        asset_template: tpl.to_string(),
    };
    let voice = "torori/voice/{character}/{class}/v{variant}";
    vec![
        // (a) character vocal lane — priority 0.
        def("V-BREATH", CueTrigger::BreathContinuous, AssetKind::BreathBed, 0, "torori/breath/{character}/bed"),
        def("V-GASP", CueTrigger::Gasp, AssetKind::VoiceSet, 0, voice),
        def("V-MOAN", CueTrigger::MoanOnPulse, AssetKind::VoiceSet, 0, voice),
        def("V-EDGE", CueTrigger::EdgeOnPulse, AssetKind::VoiceSet, 0, voice),
        def("V-CULM", CueTrigger::Latch, AssetKind::VoiceSet, 0, voice),
        def("V-AFTER", CueTrigger::AfterglowCue, AssetKind::VoiceSet, 0, voice),
        def("V-REFR", CueTrigger::DeliberateSilence, AssetKind::Silence, 0, ""),
        def("V-TOUCH", CueTrigger::TouchInput, AssetKind::VoiceSet, 0, voice),
        def("V-FLUID-REACT", CueTrigger::FaceFluidContact, AssetKind::VoiceSet, 0, voice),
        // (b) device — priority 2.
        def("D-HUM", CueTrigger::HumContinuous, AssetKind::HumRender, 2, "torori/hum/device/{wave}"),
        def("D-ON", CueTrigger::DeviceOn, AssetKind::SfxOneShot, 2, "torori/sfx/d-on/a0"),
        def("D-OFF", CueTrigger::DeviceOff, AssetKind::SfxOneShot, 2, "torori/sfx/d-off/a0"),
        def("D-SHIFT", CueTrigger::ContactShift, AssetKind::SfxOneShot, 2, "torori/sfx/d-shift/a0"),
        def("D-RATTLE", CueTrigger::RattleOnPulse, AssetKind::SfxOneShot, 2, "torori/sfx/d-rattle/a0"),
        // (c) fluid — priority 1.
        def("F-LEAK", CueTrigger::FirstLeak, AssetKind::SfxOneShot, 1, "torori/sfx/f-leak/a0"),
        def("F-SPURT", CueTrigger::Burst, AssetKind::SfxOneShot, 1, "torori/sfx/f-spurt/a0"),
        def("F-CULM-EJECT", CueTrigger::CulminationTrain, AssetKind::SfxOneShot, 1, "torori/sfx/f-culm-eject/a0"),
        def("F-SPLAT-BODY", CueTrigger::DepositBody, AssetKind::SfxOneShot, 1, "torori/sfx/f-splat-body/a0"),
        def("F-SPLAT-FIXTURE", CueTrigger::DepositFixture, AssetKind::SfxOneShot, 1, "torori/sfx/f-splat-fixture/a0"),
        def("F-SPLAT-FLOOR", CueTrigger::DepositFloor, AssetKind::SfxOneShot, 1, "torori/sfx/f-splat-floor/a0"),
        def("F-DRIP", CueTrigger::DripTransfer, AssetKind::SfxOneShot, 1, "torori/sfx/f-drip/a0"),
        // (d) body / fixture / gear — priority 3.
        def("B-CONTR", CueTrigger::PulseBody, AssetKind::SfxOneShot, 3, "torori/sfx/b-contr/a0"),
        def("B-CREAK", CueTrigger::FixtureStrain, AssetKind::SfxOneShot, 3, "torori/sfx/b-creak/a0"),
        def("B-CLAMP", CueTrigger::ClampOp, AssetKind::SfxOneShot, 3, "torori/sfx/b-clamp/a0"),
        def("B-FABRIC", CueTrigger::ApparelTransition, AssetKind::SfxOneShot, 3, "torori/sfx/b-fabric/a0"),
        def("B-TREMBLE", CueTrigger::TrembleContinuous, AssetKind::SfxOneShot, 3, "torori/sfx/b-tremble/loop"),
        // (e) room / UI — priority 4.
        def("R-AMB", CueTrigger::AmbienceContinuous, AssetKind::AmbienceBed, 4, "torori/ambience/playroom-mode/bed"),
        def("R-REC-START", CueTrigger::RecStart, AssetKind::SfxOneShot, 4, "torori/sfx/r-rec-start/a0"),
        def("R-REC-STOP", CueTrigger::RecStop, AssetKind::SfxOneShot, 4, "torori/sfx/r-rec-stop/a0"),
        def("U-CONFIRM", CueTrigger::OpAdmitted, AssetKind::UiOneShot, 4, "torori/ui/u-confirm/a0"),
        def("U-DENY", CueTrigger::OpRefused, AssetKind::UiOneShot, 4, "torori/ui/u-deny/a0"),
        def("U-RESET", CueTrigger::RemoveAllAct, AssetKind::UiOneShot, 4, "torori/ui/u-reset/a0"),
    ]
}

// ------------------------------------------------------------------ runtime

/// The mode's stages (§3.2): a presentation-and-control
/// overlay on the engine's phases. The mode adds exactly
/// one substate of its own (Edging) and one pre-session
/// stage (Setup); it adds no engine phases.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    Setup,
    WarmUp,
    Building,
    Edging,
    Culmination,
    Taper,
    Afterglow,
    Refractory,
}

impl Stage {
    /// The canonical record token.
    pub fn token(self) -> &'static str {
        match self {
            Stage::Setup => "setup",
            Stage::WarmUp => "warm-up",
            Stage::Building => "building",
            Stage::Edging => "edging",
            Stage::Culmination => "culmination",
            Stage::Taper => "taper",
            Stage::Afterglow => "afterglow",
            Stage::Refractory => "refractory",
        }
    }

    /// The panel's display name.
    pub fn name(self) -> &'static str {
        match self {
            Stage::Setup => "Setup",
            Stage::WarmUp => "Warm-Up",
            Stage::Building => "Building",
            Stage::Edging => "Edging",
            Stage::Culmination => "Culmination",
            Stage::Taper => "Taper",
            Stage::Afterglow => "Afterglow",
            Stage::Refractory => "Refractory / Aftercare",
        }
    }
}

/// Which lobe of a two-lobed pulse (§4.2, D2) an event
/// is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Lobe {
    Main,
    Second,
}

impl Lobe {
    fn token(self) -> &'static str {
        match self {
            Lobe::Main => "M",
            Lobe::Second => "S",
        }
    }
}

/// A pulse event (§4.3): emitted once by the generator,
/// consumed identically by the body-cycle modulation,
/// the ejection emitter, and (through the cue contract)
/// the sound sink.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PulseEvent {
    pub tick: u64,
    /// Depth, permille.
    pub depth_permille: u32,
    pub lobe: Lobe,
}

/// The body-cycle modulation for one tick (§4.3 consumer
/// (a)): depth-scaled excursions on **deform channels
/// only** — amplitude and posture. By D5 the pulse
/// consumer never writes `BodyRoot` translation; this
/// type has no such field, structurally.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BodyCycleMod {
    /// Excursion added to the body-cycle amplitude
    /// channel, permille.
    pub amplitude_excursion_permille: u32,
    /// Excursion added to the posture channel, permille.
    pub posture_excursion_permille: u32,
}

/// The contraction-pulse generator (`pulse-gen/1`, §4).
/// Pure function of (state vector, tick, engine data):
/// no wall clock, no RNG.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PulseGen {
    /// Ticks since the last pulse's main lobe (§4.1).
    pub pulse_phase_ticks: u64,
    /// Tick at which the pending second lobe fires, if
    /// one is scheduled.
    pub second_lobe_tick: Option<u64>,
    /// The main lobe's depth, when a second lobe is
    /// pending (the second lobe is ×0.8 of it).
    pub second_lobe_depth: u32,
    /// Depth of the most recent pulse, permille (§4.1).
    pub last_depth_permille: u32,
    /// Main lobes generated so far (the burst-side
    /// alternation counts from this, §5.4).
    pub main_count: u64,
    /// All lobes generated (census).
    pub event_count: u64,
}

/// The post-integration state the pulse law reads
/// (§4.4: generation is part of step 5).
#[derive(Debug, Clone, Copy)]
pub struct PulseInput {
    pub tick: u64,
    pub phase: Phase,
    /// Arousal A, permille.
    pub a: u32,
    /// Charge Q, permille.
    pub q: u32,
    /// The running tier while Culminating/Taper.
    pub tier: Option<u8>,
    /// The §5.7 envelope value, permille.
    pub envelope_permille: u32,
}

impl PulseGen {
    /// The (rate millihertz, depth permille) the law
    /// gives for the post-integration state, or `None`
    /// when the state carries no pulses (§4.2's table).
    fn law(
        law: &PulseLawData,
        phase: Phase,
        a: u32,
        q: u32,
        tier: Option<u8>,
        envelope_permille: u32,
    ) -> Option<(u32, u32)> {
        match phase {
            Phase::Baseline | Phase::Refractory => None,
            Phase::Culminating => {
                let tier = tier?;
                let rate = law.culminating_rate_base_millihz
                    + law.culminating_rate_per_tier_millihz * u32::from(tier);
                Some((rate, law.culminating_depths[(tier - 1) as usize]))
            }
            Phase::Taper => {
                let tier = tier?;
                let base = law.culminating_rate_base_millihz
                    + law.culminating_rate_per_tier_millihz * u32::from(tier);
                // Envelope rate × remaining-envelope
                // fraction; depth decays with the envelope.
                let rate = (u64::from(base) * u64::from(envelope_permille) / 1000) as u32;
                let depth = (u64::from(law.culminating_depths[(tier - 1) as usize])
                    * u64::from(envelope_permille)
                    / 1000) as u32;
                if rate == 0 || depth == 0 {
                    None
                } else {
                    Some((rate, depth))
                }
            }
            Phase::Afterglow => {
                if a >= 400 {
                    let depth = (law.band1_depth_base + law.band1_depth_span * q / 1000)
                        .min(law.afterglow_depth_cap);
                    // Rate expressed via the fixed interval
                    // in `generate`; return a marker rate.
                    Some((0, depth))
                } else {
                    None
                }
            }
            Phase::Stimulated => {
                if a < 250 {
                    None
                } else if a < 550 {
                    Some((
                        law.band1_rate_millihz,
                        law.band1_depth_base + law.band1_depth_span * q / 1000,
                    ))
                } else if a < 800 && q < 700 {
                    let rate = law.band2_rate_lo_millihz
                        + (law.band2_rate_hi_millihz - law.band2_rate_lo_millihz) * (a - 550)
                            / 250;
                    Some((rate, law.band2_depth_base + law.band2_depth_span * q / 1000))
                } else {
                    let rate = law.band3_rate_lo_millihz
                        + (law.band3_rate_hi_millihz - law.band3_rate_lo_millihz)
                            * (a.saturating_sub(800).min(200))
                            / 200;
                    Some((rate, law.band3_depth_base + law.band3_depth_span * q / 1000))
                }
            }
        }
    }

    /// Advances the generator one tick (step 5 of the
    /// solve order: it reads the post-integration state)
    /// and returns the pulse events generated this tick.
    pub fn generate(&mut self, law: &PulseLawData, input: &PulseInput) -> Vec<PulseEvent> {
        let PulseInput {
            tick,
            phase,
            a,
            q,
            tier,
            envelope_permille,
        } = *input;
        let mut events = Vec::new();
        // The second lobe of an earlier main lobe.
        if self.second_lobe_tick == Some(tick) {
            self.second_lobe_tick = None;
            let depth =
                (u64::from(self.second_lobe_depth) * u64::from(law.lobe_depth_scale_permille)
                    / 1000) as u32;
            self.last_depth_permille = depth;
            self.event_count += 1;
            events.push(PulseEvent {
                tick,
                depth_permille: depth,
                lobe: Lobe::Second,
            });
        }
        // The main-lobe schedule.
        match Self::law(law, phase, a, q, tier, envelope_permille) {
            None => {
                self.pulse_phase_ticks = 0;
            }
            Some((rate_millihz, depth)) => {
                let interval = if phase == Phase::Afterglow {
                    law.afterglow_interval_ticks
                } else if rate_millihz == 0 {
                    u64::MAX
                } else {
                    // interval = 60 s-rate ticks, half-up;
                    // the §4.2 bound (rate ≤ 2 Hz) keeps
                    // it ≥ 30.
                    ((60_000 + u64::from(rate_millihz) / 2) / u64::from(rate_millihz)).max(30)
                };
                self.pulse_phase_ticks += 1;
                if self.pulse_phase_ticks >= interval {
                    self.pulse_phase_ticks = 0;
                    self.main_count += 1;
                    self.event_count += 1;
                    self.last_depth_permille = depth;
                    self.second_lobe_tick = Some(tick + law.lobe_gap_ticks);
                    self.second_lobe_depth = depth;
                    events.push(PulseEvent {
                        tick,
                        depth_permille: depth,
                        lobe: Lobe::Main,
                    });
                }
            }
        }
        events
    }
}

/// A droplet in flight (rig space, milli-pixels; origin
/// at the emitter, +y down the body-local down-forward
/// axis, +x to her right).
#[derive(Debug, Clone, PartialEq, Eq)]
struct Droplet {
    x_milli: i64,
    y_milli: i64,
    vx_milli_s: i64,
    vy_milli_s: i64,
    age_ticks: u64,
    /// Volume carried, micro-units.
    volume_micro: i64,
    /// What emitted it — decides what its landing means:
    /// a burst/leak droplet deposits; a drip droplet
    /// transfers from its source area.
    kind: DropletKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum DropletKind {
    Emitted,
    Drip { from_area: String },
}

/// A burst event (§5.3): a pulse with pressure behind it
/// fired the emitter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BurstEvent {
    pub tick: u64,
    /// The pulse that fired it (§4.3: reconciles exactly
    /// with qualifying pulses, PM-G7).
    pub pulse: PulseEvent,
    /// Pressure before discharge, permille.
    pub pressure_before_permille: u32,
    /// Burst volume, micro-units.
    pub volume_micro: i64,
    /// Droplets emitted for the burst.
    pub droplets: u32,
    /// True for the culmination train's final release
    /// burst (§5.3).
    pub final_release: bool,
}

/// A wetness deposit event (§6.2): volume landed on an
/// area.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DepositEvent {
    pub tick: u64,
    pub area: String,
    /// Volume landed, micro-units.
    pub volume_micro: i64,
}

/// A wetness drip transfer (§6.2): volume moved from one
/// area to the area below it, conserving volume.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DripEvent {
    pub tick: u64,
    pub from_area: String,
    pub to_area: String,
    /// Wetness moved, micro-permille of `w`.
    pub share_mp: i64,
}

/// A landing surface segment in rig space (§5.4's
/// hit-area model for this build): a horizontal span at
/// a depth; a droplet lands on the first segment below
/// its x that it crosses. Depths are milli-pixels from
/// the emitter along +y.
struct Surface {
    area: &'static str,
    y_milli: i64,
    half_width_milli: i64,
}

/// The hit-area geometry: body underside, fixture seat,
/// floor — the named §6.1 areas as landing surfaces.
const SURFACES: [Surface; 3] = [
    Surface {
        area: "body.lower",
        y_milli: 120_000,
        half_width_milli: 160_000,
    },
    Surface {
        area: "fixture.seat",
        y_milli: 220_000,
        half_width_milli: 340_000,
    },
    Surface {
        area: "floor.pool",
        y_milli: 420_000,
        half_width_milli: i64::MAX / 4,
    },
];

/// The ejection emitter (`ejection/1`, §5): pressure,
/// bursts, the culmination release train, ballistics,
/// episode volume accounting.
#[derive(Debug, Clone)]
pub struct Ejection {
    /// Pressure P_f, micro-permille.
    pressure_mp: i64,
    /// Leak emission accumulator, micro-droplets.
    leak_acc_micro: i64,
    /// Episode emitted volume, micro-units.
    episode_volume_micro: i64,
    /// Whether `FirstLeak` has fired this episode.
    first_leak_fired: bool,
    /// Whether the culmination final release has fired
    /// this episode.
    final_release_fired: bool,
    /// In-flight droplets.
    droplets: Vec<Droplet>,
    /// The seeded emission stream (§5.4).
    rng: u64,
    /// Bursts this episode (census).
    burst_count: u64,
    /// Droplets spawned this tick (bound accounting).
    spawned_this_tick: u32,
    /// Droplets truncated by the alive bound this tick.
    truncated_this_tick: u32,
}

impl Ejection {
    fn new(data: &EjectionData) -> Ejection {
        Ejection {
            pressure_mp: 0,
            leak_acc_micro: 0,
            episode_volume_micro: 0,
            first_leak_fired: false,
            final_release_fired: false,
            droplets: Vec::new(),
            rng: data.stream_seed,
            burst_count: 0,
            spawned_this_tick: 0,
            truncated_this_tick: 0,
        }
    }

    /// Pressure in whole permille.
    pub fn pressure_permille(&self) -> u32 {
        ((self.pressure_mp + MP / 2) / MP).clamp(0, 1000) as u32
    }

    /// Episode emitted volume, micro-units.
    pub fn episode_volume_micro(&self) -> i64 {
        self.episode_volume_micro
    }

    /// Droplets currently alive.
    pub fn alive_droplets(&self) -> usize {
        self.droplets.len()
    }

    /// Droplets spawned on the most recent tick.
    pub fn spawned_this_tick(&self) -> u32 {
        self.spawned_this_tick
    }

    /// Droplets truncated by the alive bound on the
    /// most recent tick.
    pub fn truncated_this_tick(&self) -> u32 {
        self.truncated_this_tick
    }

    /// Reset for a new episode (re-arm): pressure and
    /// accounting zero; in-flight droplets continue to
    /// land.
    fn new_episode(&mut self) {
        self.pressure_mp = 0;
        self.leak_acc_micro = 0;
        self.episode_volume_micro = 0;
        self.first_leak_fired = false;
        self.final_release_fired = false;
        self.burst_count = 0;
    }

    /// Full reset (§3.4 via §5.2: reset zeroes pressure
    /// with everything else).
    fn reset(&mut self) {
        self.new_episode();
        self.droplets.clear();
    }

    /// The §5.2 pressure integration for one tick, from
    /// the post-integration (Q, I). Returns true when
    /// this tick is the episode's `FirstLeak` crossing.
    fn integrate_pressure(
        &mut self,
        data: &EjectionData,
        threshold_permille: u32,
        q_permille: u32,
        i_permille: u32,
        culminating: bool,
    ) -> bool {
        if culminating {
            // §5.3: at the latch the pressure is driven
            // to full and discharges across the train.
            self.pressure_mp = MP_FULL;
        } else {
            // build = 90 · (Q/1000) · (0.4 + 0.6·I) ·
            // (1000/threshold) permille/s, in
            // micro-permille per tick. Unit check: the
            // (400_000 + 600·I) term is (0.4 + 0.6·I)
            // scaled by 1e6, and the result is scaled
            // by MP = 1e6 — the two cancel, leaving one
            // /1000 (for Q) and the ×1000/threshold.
            let build = div_round(
                (data.build_rate as i128)
                    * (q_permille as i128)
                    * (400_000 + 600 * (i_permille as i128))
                    * 1000,
                1000 * (threshold_permille as i128) * (TICKS_PER_SECOND as i128),
            ) as i64;
            let decay = self.pressure_mp / (data.decay_tau_s * (TICKS_PER_SECOND as i64));
            self.pressure_mp = (self.pressure_mp + build - decay).clamp(0, MP_FULL);
        }
        let in_band = self.pressure_permille() >= data.leak_band_permille;
        if in_band && !self.first_leak_fired {
            self.first_leak_fired = true;
            return true;
        }
        false
    }

    /// Emits droplets for a volume: direction class by
    /// sequence number, fan angle from the seeded
    /// stream, §5.4 bounds applied. Returns the droplet
    /// count actually emitted.
    fn emit(
        &mut self,
        data: &EjectionData,
        volume_micro: i64,
        sequence: u64,
        count_wanted: u32,
    ) -> u32 {
        let mut count = count_wanted.min(data.tick_emission_bound - self.spawned_this_tick);
        let room = data.alive_bound.saturating_sub(self.droplets.len());
        if (count as usize) > room {
            self.truncated_this_tick += count - room as u32;
            count = room as u32;
        }
        if count == 0 {
            return 0;
        }
        let per_droplet = volume_micro / i64::from(count);
        let class = (sequence % 3) as usize;
        let axis = data.class_axes_millideg[class];
        let v0 = data.v0_base_px_s
            + div_round(
                (data.v0_span_px_s as i128) * (self.pressure_mp as i128),
                MP_FULL as i128,
            ) as i64;
        for _ in 0..count {
            let draw = splitmix_next(&mut self.rng);
            // Angle offset in [-half, +half] milli-degrees.
            let span = 2 * data.fan_half_angle_millideg;
            let offset = (draw % (span as u64 + 1)) as i64 - data.fan_half_angle_millideg;
            let angle = axis + offset;
            let vx = div_round_signed((v0 as i128) * (sin_millideg(angle) as i128), 1_000_000) as i64;
            let vy = div_round_signed((v0 as i128) * (cos_millideg(angle) as i128), 1_000_000) as i64;
            self.droplets.push(Droplet {
                x_milli: 0,
                y_milli: 0,
                vx_milli_s: vx * 1000,
                vy_milli_s: vy * 1000,
                age_ticks: 0,
                volume_micro: per_droplet,
                kind: DropletKind::Emitted,
            });
        }
        self.spawned_this_tick += count;
        count
    }

    /// Fires a burst for a qualifying pulse (§5.2/§5.3)
    /// and discharges pressure by 35%.
    fn burst(
        &mut self,
        data: &EjectionData,
        v_ep_micro: i64,
        tick: u64,
        pulse: PulseEvent,
        sequence: u64,
    ) -> BurstEvent {
        let pressure_before = self.pressure_permille();
        // volume = V_ep · 0.08 · (depth/1000) ·
        // (0.35 + 0.65 · P_f/1000)
        let volume_micro = div_round(
            (v_ep_micro as i128)
                * (data.burst_fraction_permille as i128)
                * (pulse.depth_permille as i128)
                * (350_000 + 650 * (pressure_before as i128)),
            1000 * 1000 * 1_000_000,
        ) as i64;
        let count_wanted = div_round(
            (volume_micro as i128) * (data.droplets_per_unit as i128),
            UV as i128,
        )
        .clamp(1, i128::from(data.tick_emission_bound)) as u32;
        let emitted = self.emit(data, volume_micro, sequence, count_wanted);
        self.pressure_mp = div_round(
            (self.pressure_mp as i128) * (data.burst_retain_permille as i128),
            1000,
        ) as i64;
        self.episode_volume_micro += volume_micro;
        self.burst_count += 1;
        BurstEvent {
            tick,
            pulse,
            pressure_before_permille: pressure_before,
            volume_micro,
            droplets: emitted,
            final_release: false,
        }
    }

    /// The culmination train's final release burst
    /// (§5.3): volume carries the tier scalar.
    /// `depth_permille` is the envelope train pulse's
    /// depth (§4.2's culminating table, supplied by the
    /// mode, which owns the pulse law).
    fn final_release(
        &mut self,
        data: &EjectionData,
        v_ep_micro: i64,
        tick: u64,
        tier: u8,
        depth_permille: u32,
        sequence: u64,
    ) -> BurstEvent {
        let volume_micro = div_round(
            (v_ep_micro as i128) * (data.final_scalars_permille[(tier - 1) as usize] as i128),
            1000,
        ) as i64;
        let count_wanted = div_round(
            (volume_micro as i128) * (data.droplets_per_unit as i128),
            UV as i128,
        )
        .clamp(1, i128::from(data.tick_emission_bound)) as u32;
        let emitted = self.emit(data, volume_micro, sequence, count_wanted);
        self.episode_volume_micro += volume_micro;
        self.burst_count += 1;
        self.final_release_fired = true;
        BurstEvent {
            tick,
            pulse: PulseEvent {
                tick,
                depth_permille,
                lobe: Lobe::Main,
            },
            pressure_before_permille: self.pressure_permille(),
            volume_micro,
            droplets: emitted,
            final_release: true,
        }
    }

    /// Spawns a wetness drip droplet (§6.2) from an
    /// area's lower edge.
    fn spawn_drip(&mut self, data: &EjectionData, from_area: &str, share_mp: i64, rng: &mut u64) {
        if self.spawned_this_tick >= data.tick_emission_bound
            || self.droplets.len() >= data.alive_bound
        {
            self.truncated_this_tick += 1;
            return;
        }
        let surface = SURFACES
            .iter()
            .find(|s| s.area == from_area)
            .expect("wetness areas are the surface areas");
        let draw = splitmix_next(rng);
        let x = (draw % ((2 * surface.half_width_milli) as u64 + 1)) as i64
            - surface.half_width_milli;
        self.droplets.push(Droplet {
            x_milli: x,
            y_milli: surface.y_milli,
            vx_milli_s: 0,
            vy_milli_s: 0,
            age_ticks: 0,
            volume_micro: share_mp,
            kind: DropletKind::Drip {
                from_area: from_area.to_string(),
            },
        });
        self.spawned_this_tick += 1;
    }

    /// Advances every droplet one tick (ballistics,
    /// §5.4) and resolves landings. Emitted droplets
    /// produce deposit events; drip droplets produce
    /// transfer events when they land on the area below.
    fn step_droplets(&mut self, data: &EjectionData, tick: u64) -> (Vec<DepositEvent>, Vec<DripEvent>) {
        let mut deposits: Vec<DepositEvent> = Vec::new();
        let mut drips: Vec<DripEvent> = Vec::new();
        let mut alive: Vec<Droplet> = Vec::with_capacity(self.droplets.len());
        for mut d in self.droplets.drain(..) {
            d.age_ticks += 1;
            d.vy_milli_s += (data.gravity_px_s2 * 1000) / (TICKS_PER_SECOND as i64);
            let prev_y = d.y_milli;
            d.x_milli += d.vx_milli_s / (TICKS_PER_SECOND as i64);
            d.y_milli += d.vy_milli_s / (TICKS_PER_SECOND as i64);
            // The first surface below the droplet's x
            // that it crossed this tick.
            let landed = SURFACES
                .iter()
                .filter(|s| d.x_milli.abs() <= s.half_width_milli && prev_y < s.y_milli)
                .find(|s| d.y_milli >= s.y_milli);
            match landed {
                Some(surface) => match d.kind {
                    DropletKind::Emitted => {
                        deposits.push(DepositEvent {
                            tick,
                            area: surface.area.to_string(),
                            volume_micro: d.volume_micro,
                        });
                    }
                    DropletKind::Drip { ref from_area } => {
                        // A drip transfers only when it
                        // lands on the area below its
                        // source (§6.2's chain); landing
                        // back on the source (spawned on
                        // its own surface edge) is not a
                        // transfer.
                        if surface.area != from_area {
                            drips.push(DripEvent {
                                tick,
                                from_area: from_area.clone(),
                                to_area: surface.area.to_string(),
                                share_mp: d.volume_micro,
                            });
                        }
                    }
                },
                None => {
                    if d.age_ticks <= data.droplet_life_ticks {
                        alive.push(d);
                    } else if let DropletKind::Emitted = d.kind {
                        // Life expired mid-flight above the
                        // floor span: it falls to the floor
                        // zone by the §6.1 field's geometry
                        // (the floor surface is unbounded).
                        deposits.push(DepositEvent {
                            tick,
                            area: "floor.pool".to_string(),
                            volume_micro: d.volume_micro,
                        });
                    }
                    // A drip droplet expiring in flight is
                    // a named loss: the share stays at the
                    // source (transfer happens at landing,
                    // atomically) — nothing is written.
                }
            }
        }
        self.droplets = alive;
        (deposits, drips)
    }
}



/// The wetness field (`wetness/1`, §6): per-area `w`,
/// deposit, sheen/drip thresholds, volume-conserving
/// transfers, floor pooling, and **no timed or
/// phase-driven decay anywhere** — removal is exactly
/// the two cleanup acts (§6.3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Wetness {
    /// Area → wetness, micro-permille.
    areas_mp: BTreeMap<String, i64>,
    /// Area → ticks until its next scheduled drip.
    drip_countdowns: BTreeMap<String, u64>,
    /// The seeded drip-position stream.
    rng: u64,
}

impl Wetness {
    fn new(data: &WetnessData) -> Wetness {
        Wetness {
            areas_mp: data.areas.iter().map(|a| (a.clone(), 0)).collect(),
            drip_countdowns: BTreeMap::new(),
            rng: data.stream_seed,
        }
    }

    /// Wetness of one area, whole permille.
    pub fn permille(&self, area: &str) -> u32 {
        let mp = self.areas_mp.get(area).copied().unwrap_or(0);
        ((mp + MP / 2) / MP).clamp(0, 1000) as u32
    }

    /// The whole field, whole permille, in canonical
    /// (sorted) order.
    pub fn field_permille(&self) -> Vec<(String, u32)> {
        self.areas_mp
            .keys()
            .map(|a| (a.clone(), self.permille(a)))
            .collect()
    }

    /// Total wetness across the field, micro-permille
    /// (conservation checks sum this with in-flight
    /// drip shares).
    pub fn total_mp(&self) -> i64 {
        self.areas_mp.values().sum()
    }

    /// A deterministic digest of the field (trajectory
    /// record + persistence checks).
    pub fn digest(&self) -> String {
        let parts: Vec<String> = self
            .areas_mp
            .iter()
            .map(|(a, w)| format!("{a}={w}"))
            .collect();
        format!("{:016x}", fnv1a_64(parts.join("|").as_bytes()))
    }

    /// Deposit landed volume into an area (§6.2).
    fn deposit(&mut self, data: &WetnessData, area: &str, volume_micro: i64) {
        // Permille per volume unit landed, expressed in
        // the field's micro-permille: units ×
        // (permille / unit) × MP. (MP == UV, so this is
        // exactly volume_micro × the calibration.)
        let add = div_round(
            (volume_micro as i128) * (data.deposit_permille_per_unit as i128) * (MP as i128),
            UV as i128,
        ) as i64;
        if let Some(w) = self.areas_mp.get_mut(area) {
            *w = (*w + add).clamp(0, MP_FULL);
        }
    }

    /// Applies a drip transfer at landing (§6.2):
    /// source falls by the share, target rises by the
    /// share — volume conserved exactly. Only what the
    /// target can hold actually moves: a saturated
    /// target takes nothing and the un-moved share
    /// stays at the source. (Clamping the gain alone
    /// would silently destroy the difference — the
    /// field's total must never change here.)
    fn transfer(&mut self, from: &str, to: &str, share_mp: i64) {
        let from_w = self.areas_mp.get(from).copied().unwrap_or(0);
        let to_w = self.areas_mp.get(to).copied().unwrap_or(0);
        let moved = share_mp.min(from_w).min((MP_FULL - to_w).max(0));
        if moved <= 0 {
            return;
        }
        if let Some(w) = self.areas_mp.get_mut(from) {
            *w -= moved;
        }
        if let Some(w) = self.areas_mp.get_mut(to) {
            *w += moved;
        }
    }

    /// The §6.3 cleanup act: the whole field zeroes.
    fn zero(&mut self) {
        for w in self.areas_mp.values_mut() {
            *w = 0;
        }
        self.drip_countdowns.clear();
    }

    /// One tick of the drip schedule (§6.2): for each
    /// area at or above the drip threshold with a chain
    /// target, count down and spawn a drip droplet when
    /// the interval elapses. Spawning is delegated to
    /// the emitter's droplet store so all ballistics
    /// live in one simulation.
    fn step_drips(&mut self, data: &WetnessData, ejection: &mut Ejection, edata: &EjectionData) {
        let areas: Vec<String> = self.areas_mp.keys().cloned().collect();
        for area in areas {
            if !data.drip_below.contains_key(&area) {
                continue; // floor areas pool (§6.2)
            }
            let w = self.areas_mp[&area];
            if w < (data.drip_threshold_permille as i64) * MP {
                self.drip_countdowns.remove(&area);
                continue;
            }
            let countdown = self.drip_countdowns.entry(area.clone()).or_insert_with(|| {
                // Interval shortens as w rises: 240 ticks
                // at the threshold → 45 at full (§6.2),
                // linear in w (the fraction in permille).
                let span = ((MP_FULL - (data.drip_threshold_permille as i64) * MP).max(1)) as i128;
                let frac_permille = div_round(
                    (w - (data.drip_threshold_permille as i64) * MP) as i128 * 1000,
                    span,
                );
                let lo = data.drip_interval_at_threshold_ticks as i128;
                let hi = data.drip_interval_at_full_ticks as i128;
                (lo + (hi - lo) * frac_permille / 1000).max(1) as u64
            });
            if *countdown > 0 {
                *countdown -= 1;
            }
            if *countdown == 0 {
                self.drip_countdowns.remove(&area);
                // The share leaves the field only when
                // the droplet lands (transfer is atomic
                // at landing); the droplet carries it.
                let share = div_round((w as i128) * (data.drip_share_permille as i128), 1000) as i64;
                ejection.spawn_drip(edata, &area, share, &mut self.rng);
            }
        }
    }
}

// ------------------------------------------------------------------ session

/// Per-tick inputs to a mode step: the engine's
/// non-device inputs (§5.3 of T4) in a recordable form
/// (a take carries these verbatim, §8.3).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModeTickInput {
    /// Direct touch at a named site, permille strength.
    pub touch: Option<(String, u32)>,
    /// A suction-prop cycle peak this tick.
    pub suction_peak: bool,
    /// An authored spray emitter's rate this tick,
    /// permille.
    pub spray_rate: Option<u32>,
    /// An external face-contact event this tick.
    pub face_contact: bool,
}

impl ModeTickInput {
    fn to_engine(&self) -> TickInputs {
        TickInputs {
            touch: self.touch.clone(),
            suction_peak: self.suction_peak,
            spray_rate: self.spray_rate,
            face_contact: self.face_contact,
        }
    }
}

/// A session operation on the mode (§2.3, §3.3, §8.3).
/// Mode ops are admitted by the session host's op path —
/// validated, applied, and appended to the mode op log
/// with the engine state hash — the §5.10 discipline the
/// engine's own ops use. The safety exit
/// ([`ModeOp::RemoveAllAndReset`]) is never refused.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum ModeOp {
    /// Enter the mode for a playmate (§2.3): applies the
    /// fixture, the mounted device, and the sample
    /// apparel set as session operations.
    Enter { character: String },
    /// Leave the mode (session end for the composition).
    Exit,
    /// Engage/release the Hold governor (§3.3).
    SetHold { on: bool },
    /// The device control: the user's amplitude setting,
    /// permille.
    SetAmplitude { permille: u32 },
    /// The physics layer's contact feed for the device
    /// (§4.3/§4.5 of T4).
    SetContact {
        contact_quality: u32,
        displaced: bool,
    },
    /// Toggle the REC presentation skin (§8.1).
    SetRecSkin { on: bool },
    /// Start recording a take (§8.3).
    StartTake,
    /// Stop the running take.
    StopTake,
    /// The standing control (§3.4 + §6.3): remove all
    /// gear, reset the engine, zero the wetness field —
    /// one act, never gated.
    RemoveAllAndReset,
}

/// One mode op-log entry: the op as admitted and the
/// engine state hash after it applied (§5.10's form).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModeLogEntry {
    pub seq: u64,
    pub tick: u64,
    pub character: String,
    pub op: serde_json::Value,
    pub state_hash: String,
}

/// A cue firing, recorded in the canonical cue track
/// (§9.6): tick-indexed, with the resolved asset id.
/// PM-1 fires cues to a null sink — the record is the
/// deliverable; the asset is PM-1S's.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CueRecord {
    pub tick: u64,
    pub cue_id: String,
    pub asset_id: String,
    /// `fire` (one-shot), `start` / `stop` (continuous
    /// lanes).
    pub edge: String,
}

impl CueRecord {
    /// The canonical cue-track line.
    pub fn line(&self) -> String {
        format!("cue:{}:{}:{}:{}", self.tick, self.cue_id, self.asset_id, self.edge)
    }
}

/// One mode tick's complete output: the engine's output
/// plus everything the mode's extensions produced.
#[derive(Debug, Clone, PartialEq)]
pub struct ModeOutput {
    pub engine: EngineOutput,
    pub stage: Stage,
    pub pulses: Vec<PulseEvent>,
    pub bursts: Vec<BurstEvent>,
    pub deposits: Vec<DepositEvent>,
    pub drips: Vec<DripEvent>,
    pub cues: Vec<CueRecord>,
    pub body_cycle: BodyCycleMod,
    /// Pressure P_f after this tick, whole permille.
    pub pressure_permille: u32,
    /// Episode emitted volume after this tick,
    /// micro-units.
    pub episode_volume_micro: i64,
    /// This tick was the episode's `FirstLeak` crossing.
    pub first_leak: bool,
    /// Leak droplets emitted this tick.
    pub leak_droplets: u32,
    /// Her governed expression recipe for this tick
    /// (§6.5: resolved per character).
    pub expression_recipe_id: String,
    /// The canonical trajectory record for this tick
    /// (§9.6's single format: the engine's §5.11 record
    /// plus the mode segment).
    pub trajectory_record: String,
}

/// Continuous-lane cue state.
#[derive(Debug, Default)]
struct CueState {
    breath_on: bool,
    hum_on: bool,
    tremble_on: bool,
    amb_on: bool,
    last_gasp_pulse_tick: u64,
    last_edge_tick: u64,
    last_creak_tick: u64,
    prev_a: u32,
    prev_strain_band: u32,
    prev_contact_band: Option<u32>,
}

/// A running take's recorder (§8.3).
#[derive(Debug)]
struct TakeRecorder {
    character: String,
    start_tick: u64,
    /// The REC skin state at take start (replay restores
    /// it before starting its own take, so the R-REC-*
    /// cues align).
    rec_on: bool,
    start_record_idx: usize,
    start_cue_idx: usize,
    inputs: Vec<ModeTickInput>,
    ops: Vec<(u64, ModeOp)>,
}

/// The Playtime mode session for one playmate at a time:
/// the stage overlay, the governor, the pulse generator,
/// the ejection emitter, the wetness field, and the cue
/// engine — all engine-adjacent state around one
/// [`ArousalHost`] engine (one store; the mode never
/// keeps a second engine).
#[derive(Debug)]
pub struct PlaytimeMode {
    definition: PlaytimeDefinition,
    character: Option<String>,
    entered: bool,
    stage: Stage,
    hold_engaged: bool,
    /// Ticks Q has been held in [800, 1000) (§3.2's
    /// Edging entry arm).
    edging_band_ticks: u64,
    /// The user's device amplitude setting, permille.
    user_amplitude: u32,
    /// The amplitude currently written to the engine
    /// (the governor's cap applies on top of the user's
    /// setting).
    applied_amplitude: u32,
    /// The device's contact quality as last fed (§4.3).
    device_contact_quality: u32,
    pulse: PulseGen,
    ejection: Ejection,
    wetness: Wetness,
    current_tier: Option<u8>,
    /// Recent lobes for the body-cycle consumer:
    /// (fire tick, depth).
    recent_lobes: Vec<(u64, u32)>,
    cue_state: CueState,
    /// Cue firings queued by ops, drained at the next
    /// step (cues fire on the fixed step, §9.1).
    pending_cues: Vec<(String, String)>, // (cue id, edge)
    rec_on: bool,
    take: Option<TakeRecorder>,
    trajectory: Vec<String>,
    cue_track: Vec<CueRecord>,
    log: Vec<ModeLogEntry>,
}

impl PlaytimeMode {
    /// A mode session over `definition`, not entered.
    pub fn new(definition: PlaytimeDefinition) -> PlaytimeMode {
        let ejection = Ejection::new(&definition.ejection);
        let wetness = Wetness::new(&definition.wetness);
        PlaytimeMode {
            definition,
            character: None,
            entered: false,
            stage: Stage::Setup,
            hold_engaged: false,
            edging_band_ticks: 0,
            user_amplitude: 0,
            applied_amplitude: 0,
            device_contact_quality: 1000,
            pulse: PulseGen::default(),
            ejection,
            wetness,
            current_tier: None,
            recent_lobes: Vec::new(),
            cue_state: CueState::default(),
            pending_cues: Vec::new(),
            rec_on: false,
            take: None,
            trajectory: Vec::new(),
            cue_track: Vec::new(),
            log: Vec::new(),
        }
    }

    /// The mode definition.
    pub fn definition(&self) -> &PlaytimeDefinition {
        &self.definition
    }

    /// The current stage (live overlay state).
    pub fn stage(&self) -> Stage {
        self.stage
    }

    /// Whether the mode is entered.
    pub fn entered(&self) -> bool {
        self.entered
    }

    /// The active playmate, if entered.
    pub fn character(&self) -> Option<&str> {
        self.character.as_deref()
    }

    /// Whether Hold is engaged.
    pub fn hold_engaged(&self) -> bool {
        self.hold_engaged
    }

    /// The user's device amplitude setting, permille.
    pub fn user_amplitude(&self) -> u32 {
        self.user_amplitude
    }

    /// Whether the REC skin is on.
    pub fn rec_on(&self) -> bool {
        self.rec_on
    }

    /// Whether a take is recording.
    pub fn take_active(&self) -> bool {
        self.take.is_some()
    }

    /// The wetness field (read for panels and gates).
    pub fn wetness(&self) -> &Wetness {
        &self.wetness
    }

    /// Pressure P_f, whole permille.
    pub fn pressure_permille(&self) -> u32 {
        self.ejection.pressure_permille()
    }

    /// Episode emitted volume, micro-units.
    pub fn episode_volume_micro(&self) -> i64 {
        self.ejection.episode_volume_micro()
    }

    /// Droplets currently alive (bound evidence).
    pub fn alive_droplets(&self) -> usize {
        self.ejection.alive_droplets()
    }

    /// The canonical trajectory records so far.
    pub fn trajectory(&self) -> &[String] {
        &self.trajectory
    }

    /// The canonical trajectory hash: FNV-1a over the
    /// records joined by newlines (the §5.11 pattern).
    pub fn trajectory_hash(&self) -> u64 {
        fnv1a_64(self.trajectory.join("\n").as_bytes())
    }

    /// The cue track so far.
    pub fn cue_track(&self) -> &[CueRecord] {
        &self.cue_track
    }

    /// The cue-track hash.
    pub fn cue_track_hash(&self) -> u64 {
        let lines: Vec<String> = self.cue_track.iter().map(CueRecord::line).collect();
        fnv1a_64(lines.join("\n").as_bytes())
    }

    /// The mode op log.
    pub fn log(&self) -> &[ModeLogEntry] {
        &self.log
    }

    fn require_entered(&self) -> R<&str> {
        if self.entered {
            Ok(self.character.as_deref().expect("entered has a character"))
        } else {
            Err(Refusal::new(
                "TG-PM-STATE",
                "the mode is not entered; there is no active playmate".to_string(),
            ))
        }
    }

    /// Appends an admitted op to the mode op log (and to
    /// the running take's op stream, §8.3).
    fn record_op(&mut self, host: &ArousalHost, character: &str, op: &ModeOp) {
        let tick = host.get(character).map_or(0, |e| e.tick());
        let state_hash = host
            .get(character)
            .map_or_else(|| "none".to_string(), |e| e.state_hash());
        self.log.push(ModeLogEntry {
            seq: self.log.len() as u64 + 1,
            tick,
            character: character.to_string(),
            op: serde_json::to_value(op).expect("mode op serialisation is total"),
            state_hash,
        });
        if let Some(take) = &mut self.take {
            take.ops.push((tick, op.clone()));
        }
    }

    fn queue_cue(&mut self, cue_id: &str, edge: &str) {
        self.pending_cues
            .push((cue_id.to_string(), edge.to_string()));
    }

    // -------------------------------------------------------------- ops

    /// Mode entry (§2.3): the playmate is chosen, the
    /// fixture + device are applied as engine session
    /// operations, the sample apparel set is staged with
    /// its underwear authored displaced (§7.3), and her
    /// engine episode begins. The Setup stage exits when
    /// the ops are admitted.
    pub fn enter(&mut self, host: &mut ArousalHost, character: &str) -> R<ModeLogEntry> {
        if self.entered {
            return Err(Refusal::new(
                "TG-PM-STATE",
                "the mode is already entered; exit before choosing another playmate",
            ));
        }
        let data = self.definition.character(character)?.clone();
        host.engine(character, data.profile.clone());
        // The fixture: a passive engine item (no wave, no
        // floor) — its engine presence puts the
        // composition in the canonical worn set; its pose
        // work is the gear layer's §4 solve.
        host.apply_op(
            character,
            &ArousalOp::ApplyItem {
                item: crate::arousal::DeviceSpec {
                    item_id: FIXTURE_ITEM_ID.to_string(),
                    site: "pelvis".to_string(),
                    wave: None,
                    contact_quality: 1000,
                    arousal_floor: 0,
                },
            },
        )?;
        host.apply_op(
            character,
            &ArousalOp::ApplyItem {
                item: crate::arousal::DeviceSpec {
                    item_id: DEVICE_ITEM_ID.to_string(),
                    site: DEVICE_SITE.to_string(),
                    wave: Some(WaveSpec::constant(self.user_amplitude)),
                    contact_quality: self.device_contact_quality,
                    arousal_floor: 0,
                },
            },
        )?;
        self.character = Some(character.to_string());
        self.entered = true;
        self.stage = Stage::WarmUp;
        self.applied_amplitude = self.user_amplitude;
        // §9.2: device activation, the sample set's
        // apparel transition (underwear displaced at
        // setup), the admitted-op cue.
        self.queue_cue("D-ON", "fire");
        self.queue_cue("B-FABRIC", "fire");
        self.queue_cue("U-CONFIRM", "fire");
        let op = ModeOp::Enter {
            character: character.to_string(),
        };
        self.record_op(host, character, &op);
        Ok(self.log.last().expect("just pushed").clone())
    }

    /// Fires any pending op-cues into the cue track
    /// NOW, at the engine's current tick. Terminal ops
    /// (exit, remove-all) end entry, so no later step
    /// exists to drain their cues — and §9.1 has op cues
    /// fire on the op itself. Without this, D-OFF and
    /// U-RESET could never reach the track at all.
    fn fire_pending_now(&mut self, host: &ArousalHost, character: &str) {
        if self.pending_cues.is_empty() {
            return;
        }
        let Ok(data) = self.definition.character(character) else {
            return;
        };
        let data = data.clone();
        let (tick, episode, phase) = host.get(character).map_or((0, 0, Phase::Baseline), |e| {
            let v = e.vector();
            (e.tick(), v.episode, v.phase)
        });
        let facts = CueFacts {
            tick,
            a: 0,
            phase,
            stage: self.stage,
            pulses: &[],
            bursts: &[],
            deposits: &[],
            drips: &[],
            latched: false,
            afterglow_entered: false,
            face_contact_event: false,
            first_leak: false,
            leak_droplets: 0,
            touch_input: None,
            drive_strength: 0,
            device_displaced: false,
            device_contact_quality: self.device_contact_quality,
            tremble_milli_px: 0,
            episode,
            final_release_fired_now: false,
        };
        let pending: Vec<(String, String)> = std::mem::take(&mut self.pending_cues);
        for (cue_id, edge) in pending {
            if let Some(def) = self.definition.cues.iter().find(|c| c.id == cue_id) {
                let asset_id = self.resolve_asset(def, &data, character, &facts);
                self.cue_track.push(CueRecord {
                    tick,
                    cue_id,
                    asset_id,
                    edge,
                });
            }
        }
    }

    /// Mode exit (session end for the composition): the
    /// device and fixture are removed as session
    /// operations and the wetness field ends with the
    /// scene (§6.3's second cleanup act).
    pub fn exit(&mut self, host: &mut ArousalHost) -> R<ModeLogEntry> {
        let character = self.require_entered()?.to_string();
        host.apply_op(
            &character,
            &ArousalOp::RemoveItem {
                item_id: DEVICE_ITEM_ID.to_string(),
            },
        )?;
        host.apply_op(
            &character,
            &ArousalOp::RemoveItem {
                item_id: FIXTURE_ITEM_ID.to_string(),
            },
        )?;
        self.entered = false;
        self.stage = Stage::Setup;
        self.hold_engaged = false;
        self.wetness.zero();
        self.ejection.reset();
        self.queue_cue("D-OFF", "fire");
        self.queue_cue("U-CONFIRM", "fire");
        let op = ModeOp::Exit;
        self.record_op(host, &character, &op);
        self.fire_pending_now(host, &character);
        Ok(self.log.last().expect("just pushed").clone())
    }

    /// The Hold control (§3.3): engages the input
    /// governor. An admitted session operation, op-logged
    /// with the engine state hash; never a safety exit.
    pub fn set_hold(&mut self, host: &mut ArousalHost, on: bool) -> R<ModeLogEntry> {
        let character = self.require_entered()?.to_string();
        self.hold_engaged = on;
        self.queue_cue("U-CONFIRM", "fire");
        let op = ModeOp::SetHold { on };
        self.record_op(host, &character, &op);
        Ok(self.log.last().expect("just pushed").clone())
    }

    /// The device amplitude control (§3.2): the user's
    /// setting. The engine write happens at the next
    /// step (the governor's cap composes there); a
    /// refused setting queues the U-DENY cue (§9.2(e)).
    pub fn set_amplitude(&mut self, host: &mut ArousalHost, permille: u32) -> R<ModeLogEntry> {
        let character = self.require_entered()?.to_string();
        if let Err(e) = WaveSpec::constant(permille).validate() {
            self.queue_cue("U-DENY", "fire");
            return Err(e);
        }
        self.user_amplitude = permille;
        self.queue_cue("U-CONFIRM", "fire");
        let op = ModeOp::SetAmplitude { permille };
        self.record_op(host, &character, &op);
        Ok(self.log.last().expect("just pushed").clone())
    }

    /// The physics layer's contact feed for the mounted
    /// device (§4.3/§4.5 of T4: a displaced device
    /// stimulates less — the engine applies the contact
    /// quality on its side).
    pub fn set_contact(
        &mut self,
        host: &mut ArousalHost,
        contact_quality: u32,
        displaced: bool,
    ) -> R<ModeLogEntry> {
        let character = self.require_entered()?.to_string();
        host.apply_op(
            &character,
            &ArousalOp::SetContact {
                item_id: DEVICE_ITEM_ID.to_string(),
                contact_quality,
                displaced,
            },
        )?;
        self.device_contact_quality = contact_quality;
        self.queue_cue("U-CONFIRM", "fire");
        let op = ModeOp::SetContact {
            contact_quality,
            displaced,
        };
        self.record_op(host, &character, &op);
        Ok(self.log.last().expect("just pushed").clone())
    }

    /// The REC skin toggle (§8.1): a view change, never
    /// an engine-state change.
    pub fn set_rec_skin(&mut self, host: &mut ArousalHost, on: bool) -> R<ModeLogEntry> {
        let character = self.require_entered()?.to_string();
        self.rec_on = on;
        self.queue_cue("U-CONFIRM", "fire");
        let op = ModeOp::SetRecSkin { on };
        self.record_op(host, &character, &op);
        Ok(self.log.last().expect("just pushed").clone())
    }

    /// Take start (§8.3). PM-1's replayable takes are
    /// recorded from a fresh entry (engine tick 0) so
    /// the replay aligns tick-for-tick.
    pub fn start_take(&mut self, host: &mut ArousalHost) -> R<ModeLogEntry> {
        let character = self.require_entered()?.to_string();
        if self.take.is_some() {
            return Err(Refusal::new("TG-PM-STATE", "a take is already recording"));
        }
        let tick = host.get(&character).map_or(0, |e| e.tick());
        self.take = Some(TakeRecorder {
            character: character.clone(),
            start_tick: tick,
            rec_on: self.rec_on,
            start_record_idx: self.trajectory.len(),
            start_cue_idx: self.cue_track.len(),
            inputs: Vec::new(),
            ops: Vec::new(),
        });
        if self.rec_on {
            self.queue_cue("R-REC-START", "fire");
        }
        self.queue_cue("U-CONFIRM", "fire");
        let op = ModeOp::StartTake;
        self.record_op(host, &character, &op);
        // The StartTake op itself is not part of the
        // take's op stream (the take begins after it).
        if let Some(take) = &mut self.take {
            take.ops.clear();
        }
        Ok(self.log.last().expect("just pushed").clone())
    }

    /// Take stop: finalises the take record (§8.3).
    pub fn stop_take(&mut self, host: &mut ArousalHost) -> R<TakeRecord> {
        let character = self.require_entered()?.to_string();
        let take = self.take.take().ok_or_else(|| {
            Refusal::new("TG-PM-STATE", "no take is recording")
        })?;
        if self.rec_on {
            self.queue_cue("R-REC-STOP", "fire");
        }
        self.queue_cue("U-CONFIRM", "fire");
        let op = ModeOp::StopTake;
        self.record_op(host, &character, &op);
        let records = &self.trajectory[take.start_record_idx..];
        let cues = &self.cue_track[take.start_cue_idx..];
        let cue_lines: Vec<String> = cues.iter().map(CueRecord::line).collect();
        Ok(TakeRecord {
            format: TAKE_FORMAT.to_string(),
            character: take.character,
            start_tick: take.start_tick,
            rec_on: take.rec_on,
            inputs: take.inputs,
            ops: take.ops,
            trajectory_hash: fnv1a_64(records.join("\n").as_bytes()),
            cue_track_hash: fnv1a_64(cue_lines.join("\n").as_bytes()),
        })
    }

    /// THE STANDING CONTROL (§3.4 + §6.3): remove all
    /// gear and reset state. Engine act first (the host's
    /// §3.4 order), then the mode's step of the reset
    /// act — the wetness field zeroes — in the same
    /// calling act, before any further tick advances.
    /// (The host appends its own op-log entry inside its
    /// act; the zeroing and this mode's log append both
    /// complete before the call returns, so the
    /// same-tick contract is what any observer sees.)
    /// Never refused, never gated, available in every
    /// stage including mid-Culmination.
    pub fn remove_all_and_reset(&mut self, host: &mut ArousalHost) -> R<ModeLogEntry> {
        let character = self.require_entered()?.to_string();
        host.apply_op(&character, &ArousalOp::RemoveAllAndReset)?;
        self.wetness.zero();
        self.ejection.reset();
        self.pulse = PulseGen::default();
        self.current_tier = None;
        self.hold_engaged = false;
        self.edging_band_ticks = 0;
        self.entered = false;
        self.stage = Stage::Setup;
        self.queue_cue("U-RESET", "fire");
        let op = ModeOp::RemoveAllAndReset;
        self.record_op(host, &character, &op);
        self.fire_pending_now(host, &character);
        Ok(self.log.last().expect("just pushed").clone())
    }

    // -------------------------------------------------------------- step

    /// Advances the mode one fixed step: the governor
    /// composes the device setting, the engine steps,
    /// then the mode's extensions run in §4.4's order —
    /// pulse generation at step 5 from the
    /// post-integration state, consumers (body cycle,
    /// ejection, wetness, cues) at step 6.
    pub fn step(
        &mut self,
        host: &mut ArousalHost,
        input: &ModeTickInput,
    ) -> R<ModeOutput> {
        let character = self.require_entered()?.to_string();
        let data = self.definition.character(&character)?.clone();
        // --- the governor (§3.3): caps input, never
        // output state. Reads the state carried into
        // this tick.
        let mut engine_inputs = input.to_engine();
        {
            let engine = host.engine(&character, data.profile.clone());
            let v = engine.vector();
            let weight = data.profile.site_weight(DEVICE_SITE);
            let i_hold = self
                .definition
                .governor
                .i_hold_permille(v.pleasure_charge, v.sensitivity);
            let desired = if self.hold_engaged {
                // u_cap: the amplitude whose effective
                // input is I_hold (effective =
                // u·q·w/1e6), floored — conservative.
                let denom = u64::from(self.device_contact_quality) * u64::from(weight);
                let cap = u64::from(i_hold)
                    .checked_mul(1_000_000)
                    .and_then(|n| n.checked_div(denom))
                    .map_or(0, |c| c.min(1000) as u32);
                self.user_amplitude.min(cap)
            } else {
                self.user_amplitude
            };
            if desired != self.applied_amplitude {
                engine.set_wave(DEVICE_ITEM_ID, WaveSpec::constant(desired));
                self.applied_amplitude = desired;
            }
            if self.hold_engaged {
                // Touch is input too: cap its strength
                // at the same equilibrium.
                if let Some((site, strength)) = &mut engine_inputs.touch {
                    let w = data.profile.site_weight(site);
                    if w > 0 {
                        let cap = (u64::from(i_hold) * 1000 / u64::from(w)).min(1000) as u32;
                        *strength = (*strength).min(cap);
                    }
                }
            }
        }
        // --- the engine step
        let out = host.step(&character, &engine_inputs)?;
        let (v, tick) = {
            let engine = host.get(&character).expect("engine exists");
            (engine.vector(), engine.tick())
        };
        // --- events census
        let mut latched_tier: Option<u8> = None;
        let mut rearmed = false;
        let mut face_contact_event = false;
        let mut afterglow_entered = false;
        for e in &out.events {
            match e {
                ArousalEvent::Culmination { tier, .. } => {
                    latched_tier = Some(*tier);
                    self.current_tier = Some(*tier);
                }
                ArousalEvent::Rearmed { .. } => rearmed = true,
                ArousalEvent::FluidFaceContact { .. } => face_contact_event = true,
                ArousalEvent::PhaseChanged { to, .. } if *to == Phase::Afterglow => {
                    afterglow_entered = true;
                }
                _ => {}
            }
        }
        if rearmed {
            self.ejection.new_episode();
            self.current_tier = None;
        }
        let envelope = out
            .effects
            .performance
            .as_ref()
            .map_or(0, |p| p.envelope_permille);
        if let Some(p) = &out.effects.performance {
            self.current_tier = Some(p.tier);
        }
        // --- step 5: pulse generation (§4) + pressure
        // integration (§5.2)
        let pulses = self.pulse.generate(
            &self.definition.pulse,
            &PulseInput {
                tick,
                phase: v.phase,
                a: v.arousal,
                q: v.pleasure_charge,
                tier: self.current_tier,
                envelope_permille: envelope,
            },
        );
        let culminating = v.phase == Phase::Culminating;
        let first_leak = self.ejection.integrate_pressure(
            &self.definition.ejection,
            data.eject_threshold_permille,
            v.pleasure_charge,
            out.effective_input,
            culminating,
        );
        // --- step 6 consumers
        // Per-tick emission accounting (§5.4's bounds)
        // starts fresh; emit() and spawn_drip() count
        // into it as they spawn.
        self.ejection.spawned_this_tick = 0;
        self.ejection.truncated_this_tick = 0;
        // Body cycle (§4.3(a)): excursions from active
        // lobes, deform channels only.
        for p in &pulses {
            self.recent_lobes.push((p.tick, p.depth_permille));
        }
        self.recent_lobes
            .retain(|(t, _)| tick.saturating_sub(*t) < self.definition.pulse.lobe_duration_ticks);
        let body_cycle = {
            let mut excursion: u64 = 0;
            for (t, depth) in &self.recent_lobes {
                let age = tick - t;
                let shape = self.definition.pulse.lobe_duration_ticks - age;
                excursion += u64::from(*depth) * shape
                    / self.definition.pulse.lobe_duration_ticks;
            }
            let excursion = excursion.min(1000) as u32;
            BodyCycleMod {
                amplitude_excursion_permille: excursion,
                posture_excursion_permille: excursion,
            }
        };
        // Ejection bursts (§4.3(b) + §5.2/§5.3).
        let v_ep = self.definition.v_ep_micro(&data);
        let mut bursts: Vec<BurstEvent> = Vec::new();
        for p in &pulses {
            if self.ejection.pressure_permille()
                >= self.definition.ejection.burst_threshold_permille
            {
                let seq = self.pulse.main_count + bursts.len() as u64;
                bursts.push(self.ejection.burst(
                    &self.definition.ejection,
                    v_ep,
                    tick,
                    *p,
                    seq,
                ));
            }
        }
        // The culmination train's final release, at the
        // envelope peak (§5.3).
        if culminating
            && !self.ejection.final_release_fired
            && envelope >= 1000
            && let Some(tier) = self.current_tier
        {
            let depth = self.definition.pulse.culminating_depths[(tier - 1) as usize];
            let seq = self.pulse.main_count + bursts.len() as u64;
            bursts.push(self.ejection.final_release(
                &self.definition.ejection,
                v_ep,
                tick,
                tier,
                depth,
                seq,
            ));
        }
        // Leak emission (§5.2): bounded continuous
        // droplets while pressure is in the leak band.
        let mut leak_droplets = 0u32;
        {
            let edata = &self.definition.ejection;
            let p = self.ejection.pressure_permille();
            if p >= edata.leak_band_permille {
                let rate = edata.leak_rate_lo_per_s
                    + (edata.leak_rate_hi_per_s - edata.leak_rate_lo_per_s)
                        * (p - edata.leak_band_permille)
                        / (1000 - edata.leak_band_permille);
                self.ejection.leak_acc_micro +=
                    i64::from(rate) * MP / (TICKS_PER_SECOND as i64);
                while self.ejection.leak_acc_micro >= MP {
                    self.ejection.leak_acc_micro -= MP;
                    let seq = 1_000_000 + self.ejection.burst_count + u64::from(leak_droplets);
                    let emitted = self.ejection.emit(
                        edata,
                        edata.leak_droplet_volume_micro,
                        seq,
                        1,
                    );
                    if emitted == 0 {
                        break;
                    }
                    self.ejection.episode_volume_micro += edata.leak_droplet_volume_micro;
                    leak_droplets += 1;
                }
            }
        }
        // Wetness drips spawn, then all droplets fly.
        self.wetness
            .step_drips(&self.definition.wetness, &mut self.ejection, &self.definition.ejection);
        let (deposits, drip_events) =
            self.ejection.step_droplets(&self.definition.ejection, tick);
        for d in &deposits {
            self.wetness
                .deposit(&self.definition.wetness, &d.area, d.volume_micro);
        }
        for d in &drip_events {
            self.wetness.transfer(&d.from_area, &d.to_area, d.share_mp);
        }
        // --- the stage overlay (§3.2)
        self.update_stage(&v);
        // --- cues (§9)
        let cues = self.evaluate_cues(
            &data,
            &CueFacts {
                tick,
                a: v.arousal,
                phase: v.phase,
                stage: self.stage,
                pulses: &pulses,
                bursts: &bursts,
                deposits: &deposits,
                drips: &drip_events,
                latched: latched_tier.is_some(),
                afterglow_entered,
                face_contact_event,
                first_leak,
                leak_droplets,
                touch_input: engine_inputs.touch.as_ref().map(|(site, s)| {
                    let w = data.profile.site_weight(site);
                    (u64::from(*s) * u64::from(w) / 1000) as u32
                }),
                drive_strength: out
                    .drive
                    .iter()
                    .find(|d| d.device_item_id == DEVICE_ITEM_ID)
                    .map_or(0, |d| d.strength_permille),
                device_displaced: out
                    .devices
                    .iter()
                    .find(|d| d.item_id == DEVICE_ITEM_ID)
                    .is_some_and(|d| d.displaced),
                device_contact_quality: out
                    .devices
                    .iter()
                    .find(|d| d.item_id == DEVICE_ITEM_ID)
                    .map_or(self.device_contact_quality, |d| d.contact_quality),
                tremble_milli_px: out.effects.tremble_milli_px,
                episode: v.episode,
                final_release_fired_now: bursts.iter().any(|b| b.final_release),
            },
        );
        self.cue_track.extend(cues.iter().cloned());
        // --- the canonical record (§9.6)
        let expression_recipe_id = data
            .expression_recipes
            .get(out.effects.expression_key)
            .cloned()
            .unwrap_or_else(|| format!("{character}.expr.{}", out.effects.expression_key));
        let record = format!(
            "{}|pm|st{}|pu{}|pf{}|ev{}|w{}|x{}|xr{}|c{}",
            {
                let engine = host.get(&character).expect("engine exists");
                engine.canonical_record(&out.events)
            },
            self.stage.token(),
            pulses
                .iter()
                .map(|p| format!("{}{}", p.lobe.token(), p.depth_permille))
                .collect::<Vec<_>>()
                .join(","),
            self.ejection.pressure_permille(),
            self.ejection.episode_volume_micro(),
            self.wetness.digest(),
            body_cycle.amplitude_excursion_permille,
            expression_recipe_id,
            cues.iter()
                .map(|c| c.cue_id.as_str())
                .collect::<Vec<_>>()
                .join(","),
        );
        self.trajectory.push(record.clone());
        if let Some(take) = &mut self.take {
            take.inputs.push(input.clone());
        }
        Ok(ModeOutput {
            engine: out,
            stage: self.stage,
            pulses,
            bursts,
            deposits,
            drips: drip_events,
            cues,
            body_cycle,
            pressure_permille: self.ejection.pressure_permille(),
            episode_volume_micro: self.ejection.episode_volume_micro(),
            first_leak,
            leak_droplets,
            expression_recipe_id,
            trajectory_record: record,
        })
    }

    /// The §3.2 stage overlay, from the post-step vector
    /// and the mode's control state.
    fn update_stage(&mut self, v: &crate::arousal::ArousalVector) {
        if !self.entered {
            self.stage = Stage::Setup;
            return;
        }
        match v.phase {
            Phase::Baseline => {
                self.edging_band_ticks = 0;
                self.stage = Stage::WarmUp;
            }
            Phase::Stimulated => {
                if v.arousal < 250 {
                    self.stage = Stage::WarmUp;
                    return;
                }
                if (800..1000).contains(&v.pleasure_charge) {
                    self.edging_band_ticks += 1;
                } else if v.pleasure_charge < 700 {
                    self.edging_band_ticks = 0;
                }
                let edging = if self.stage == Stage::Edging {
                    v.pleasure_charge >= 700
                } else {
                    v.pleasure_charge >= 800
                        && (self.hold_engaged || self.edging_band_ticks >= 600)
                };
                self.stage = if edging {
                    Stage::Edging
                } else {
                    Stage::Building
                };
            }
            Phase::Culminating => self.stage = Stage::Culmination,
            Phase::Taper => self.stage = Stage::Taper,
            Phase::Afterglow => self.stage = Stage::Afterglow,
            Phase::Refractory => self.stage = Stage::Refractory,
        }
    }
}

/// The tick facts the cue engine evaluates (§9.1: every
/// cue names its engine trigger).
struct CueFacts<'a> {
    tick: u64,
    a: u32,
    phase: Phase,
    stage: Stage,
    pulses: &'a [PulseEvent],
    bursts: &'a [BurstEvent],
    deposits: &'a [DepositEvent],
    drips: &'a [DripEvent],
    latched: bool,
    afterglow_entered: bool,
    face_contact_event: bool,
    first_leak: bool,
    leak_droplets: u32,
    /// The tick's touch effective input, permille.
    touch_input: Option<u32>,
    drive_strength: u32,
    device_displaced: bool,
    device_contact_quality: u32,
    tremble_milli_px: u32,
    episode: u32,
    final_release_fired_now: bool,
}

impl PlaytimeMode {
    /// Evaluates the §9.2 cue table against the tick's
    /// facts. Fires to the null sink: the returned
    /// records ARE the cue track.
    fn evaluate_cues(&mut self, data: &CharacterModeData, f: &CueFacts<'_>) -> Vec<CueRecord> {
        let character = self.character.clone().unwrap_or_default();
        let mut records: Vec<CueRecord> = Vec::new();
        // Op-queued cues drain first (they fired on this
        // step, §9.1).
        let pending: Vec<(String, String)> = std::mem::take(&mut self.pending_cues);
        for (cue_id, edge) in pending {
            if let Some(def) = self.definition.cues.iter().find(|c| c.id == cue_id) {
                records.push(CueRecord {
                    tick: f.tick,
                    cue_id: cue_id.clone(),
                    asset_id: self.resolve_asset(def, data, &character, f),
                    edge,
                });
            }
        }
        let fire = |records: &mut Vec<CueRecord>, id: &str, edge: &str, this: &Self| {
            if let Some(def) = this.definition.cues.iter().find(|c| c.id == id) {
                records.push(CueRecord {
                    tick: f.tick,
                    cue_id: id.to_string(),
                    asset_id: this.resolve_asset(def, data, &character, f),
                    edge: edge.to_string(),
                });
            }
        };
        // Continuous lanes: transitions only.
        let breath_want = self.entered;
        if breath_want != self.cue_state.breath_on {
            self.cue_state.breath_on = breath_want;
            fire(&mut records, "V-BREATH", if breath_want { "start" } else { "stop" }, self);
        }
        let amb_want = self.entered;
        if amb_want != self.cue_state.amb_on {
            self.cue_state.amb_on = amb_want;
            fire(&mut records, "R-AMB", if amb_want { "start" } else { "stop" }, self);
        }
        let hum_want = f.drive_strength > 0;
        if hum_want != self.cue_state.hum_on {
            self.cue_state.hum_on = hum_want;
            fire(&mut records, "D-HUM", if hum_want { "start" } else { "stop" }, self);
        }
        let tremble_want = f.tremble_milli_px > 0;
        if tremble_want != self.cue_state.tremble_on {
            self.cue_state.tremble_on = tremble_want;
            fire(&mut records, "B-TREMBLE", if tremble_want { "start" } else { "stop" }, self);
        }
        // Contact band edges (§9.2 D-SHIFT).
        let band = f.device_contact_quality / 250;
        match self.cue_state.prev_contact_band {
            None => self.cue_state.prev_contact_band = Some(band),
            Some(prev) if prev != band => {
                self.cue_state.prev_contact_band = Some(band);
                fire(&mut records, "D-SHIFT", "fire", self);
            }
            _ => {}
        }
        // Vocal lane.
        let crossed_400 = self.cue_state.prev_a < 400 && f.a >= 400;
        let deep_pulse = f.pulses.iter().any(|p| p.depth_permille >= 700);
        if crossed_400
            || (deep_pulse && f.tick.saturating_sub(self.cue_state.last_gasp_pulse_tick) >= 90)
        {
            if deep_pulse {
                self.cue_state.last_gasp_pulse_tick = f.tick;
            }
            fire(&mut records, "V-GASP", "fire", self);
        }
        if f.phase == Phase::Stimulated {
            for p in f.pulses {
                let min_depth = if f.a < 550 {
                    600
                } else if f.a < 800 {
                    400
                } else {
                    250
                };
                if p.depth_permille >= min_depth {
                    fire(&mut records, "V-MOAN", "fire", self);
                }
                if f.stage == Stage::Edging
                    && p.depth_permille >= 300
                    && f.tick.saturating_sub(self.cue_state.last_edge_tick) >= 60
                {
                    self.cue_state.last_edge_tick = f.tick;
                    fire(&mut records, "V-EDGE", "fire", self);
                }
            }
        }
        if f.latched {
            fire(&mut records, "V-CULM", "fire", self);
            fire(&mut records, "F-CULM-EJECT", "fire", self);
        }
        if f.afterglow_entered || (f.phase == Phase::Afterglow && !f.pulses.is_empty()) {
            fire(&mut records, "V-AFTER", "fire", self);
        }
        if let Some(i) = f.touch_input
            && i >= 300
        {
            fire(&mut records, "V-TOUCH", "fire", self);
        }
        if f.face_contact_event {
            fire(&mut records, "V-FLUID-REACT", "fire", self);
        }
        // Device / fluid / body one-shots.
        if f.device_displaced && !f.pulses.is_empty() {
            fire(&mut records, "D-RATTLE", "fire", self);
        }
        if f.first_leak || f.leak_droplets > 0 {
            fire(&mut records, "F-LEAK", "fire", self);
        }
        for b in f.bursts {
            if !b.final_release {
                fire(&mut records, "F-SPURT", "fire", self);
            }
        }
        let _ = f.final_release_fired_now;
        for area in ["body.lower", "fixture.seat", "floor.pool"] {
            if f.deposits.iter().any(|d| d.area == area) {
                let id = match area {
                    "body.lower" => "F-SPLAT-BODY",
                    "fixture.seat" => "F-SPLAT-FIXTURE",
                    _ => "F-SPLAT-FLOOR",
                };
                fire(&mut records, id, "fire", self);
            }
        }
        for _ in f.drips {
            fire(&mut records, "F-DRIP", "fire", self);
        }
        for _ in f.pulses {
            fire(&mut records, "B-CONTR", "fire", self);
        }
        // Fixture strain: the tick's deepest pulse while
        // seated crosses a band edge (§9.2 B-CREAK).
        let strain = f.pulses.iter().map(|p| p.depth_permille).max().unwrap_or(0);
        let strain_band = strain / 250;
        if strain_band > self.cue_state.prev_strain_band
            && f.tick.saturating_sub(self.cue_state.last_creak_tick) >= 30
        {
            self.cue_state.last_creak_tick = f.tick;
            fire(&mut records, "B-CREAK", "fire", self);
        }
        self.cue_state.prev_strain_band = strain_band;
        self.cue_state.prev_a = f.a;
        records
    }

    /// Resolves a cue's asset id from its template
    /// (§9.6): character, vocal class + deterministic
    /// variant (a function of cue, tick, episode), and
    /// the device's wave kind.
    fn resolve_asset(
        &self,
        def: &CueDef,
        data: &CharacterModeData,
        character: &str,
        f: &CueFacts<'_>,
    ) -> String {
        let class = match def.id.as_str() {
            "V-BREATH" => "breath",
            "V-GASP" => "gasp",
            "V-MOAN" => "moan",
            "V-EDGE" => "edge",
            "V-CULM" => "culm",
            "V-AFTER" => "after",
            "V-TOUCH" => "touch",
            "V-FLUID-REACT" => "fluid-react",
            _ => "",
        };
        let variant = if class.is_empty() {
            0
        } else {
            let count = data.vocal_set.classes.get(class).copied().unwrap_or(1).max(1);
            let h = fnv1a_64(format!("{}:{}:{}", def.id, f.tick, f.episode).as_bytes());
            (h % u64::from(count)) as u32
        };
        def.asset_template
            .replace("{character}", character)
            .replace("{class}", class)
            .replace("{variant}", &variant.to_string())
            .replace("{wave}", "constant")
    }
}

// ------------------------------------------------------------------ validation

/// A mode-validation refusal in T4 §2.7's pipeline form:
/// a stable code, the document path, and the field path
/// of the first refusal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModeRefusal {
    pub code: String,
    pub document_path: String,
    pub field_path: String,
    pub message: String,
}

impl std::fmt::Display for ModeRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} at {}:{}: {}",
            self.code, self.document_path, self.field_path, self.message
        )
    }
}

impl std::error::Error for ModeRefusal {}

/// The authority census list (T4 §2.7 step 9): field
/// names that carry authority a file may never hold.
/// Mirrors `torogao-gear`'s `AUTHORITY_FIELDS` — the
/// census is spec data shared by both validators; the
/// crates stay independent, so the list is carried here
/// verbatim with this provenance. `mirror_capable` is
/// deliberately absent (the one legal declaration).
pub const AUTHORITY_FIELDS: [&str; 13] = [
    "ache_ceiling",
    "bind_device",
    "bound_device",
    "ceiling",
    "ceiling_override",
    "consent",
    "consent_state",
    "device_binding",
    "mirror",
    "mirror_ceiling",
    "mirror_enable",
    "mirror_enabled",
    "session_consent",
];

/// A mode-settings document (§2.2 preference-set
/// content): the admitted form a settings file parses
/// to after validation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModeSettings {
    /// [`SETTINGS_FORMAT`].
    pub format: String,
    /// The setting these settings belong to.
    pub setting: String,
    /// The ejection emitter's declared anchor (§5.1).
    pub emitter_anchor: String,
    /// Playmate → (eject threshold, eject volume scale)
    /// permille (§3.1).
    pub characters: BTreeMap<String, (u32, u32)>,
    /// The pulse-law bounds carried by the settings
    /// (§4.2).
    pub pulse_max_rate_millihz: u32,
    pub pulse_max_depth_permille: u32,
    pub pulse_lobe_gap_ticks: u64,
    /// The user's device amplitude setting, permille
    /// (a mode setting; never mirror authority, §2.2).
    pub device_amplitude_permille: u32,
    /// Whether the REC skin is preferred on (§8.1: a
    /// presentation option).
    pub rec_skin: bool,
}

impl ModeSettings {
    /// The settings document for a definition + the
    /// live control state (what "save this composition
    /// as a preference set" writes, §2.2).
    pub fn from_definition(definition: &PlaytimeDefinition, amplitude: u32, rec_on: bool) -> Self {
        ModeSettings {
            format: SETTINGS_FORMAT.to_string(),
            setting: definition.setting_id.clone(),
            emitter_anchor: definition.ejection.emitter_anchor.clone(),
            characters: definition
                .characters
                .iter()
                .map(|(name, d)| {
                    (
                        name.clone(),
                        (d.eject_threshold_permille, d.eject_volume_scale_permille),
                    )
                })
                .collect(),
            pulse_max_rate_millihz: definition.pulse.max_rate_millihz,
            pulse_max_depth_permille: definition.pulse.max_depth_permille,
            pulse_lobe_gap_ticks: definition.pulse.lobe_gap_ticks,
            device_amplitude_permille: amplitude,
            rec_skin: rec_on,
        }
    }
}

fn mode_refusal(
    code: &str,
    document_path: &str,
    field_path: &str,
    message: impl Into<String>,
) -> ModeRefusal {
    ModeRefusal {
        code: code.to_string(),
        document_path: document_path.to_string(),
        field_path: field_path.to_string(),
        message: message.into(),
    }
}

/// Finds an authority field anywhere in a JSON value
/// tree, in document order, returning its field path
/// (T4 §2.7 step 9's census, JSON form).
fn find_authority_field(value: &serde_json::Value, path: &str) -> Option<String> {
    match value {
        serde_json::Value::Object(map) => {
            for (key, child) in map {
                let child_path = if path.is_empty() {
                    key.clone()
                } else {
                    format!("{path}.{key}")
                };
                if AUTHORITY_FIELDS.contains(&key.as_str()) {
                    return Some(child_path);
                }
                if let Some(found) = find_authority_field(child, &child_path) {
                    return Some(found);
                }
            }
            None
        }
        serde_json::Value::Array(items) => items.iter().enumerate().find_map(|(i, child)| {
            find_authority_field(child, &format!("{path}[{i}]"))
        }),
        _ => None,
    }
}

/// The known fields per object level of a settings
/// document, for the unknown-field census (step 2).
/// Authority field names are deliberately *known* at
/// this step so an authority-bearing document reaches
/// the authority census and meets its own refusal
/// (T4-G1.4's discipline).
fn known_fields(path: &str) -> Option<&'static [&'static str]> {
    const ROOT: &[&str] = &[
        "format",
        "setting",
        "emitter",
        "characters",
        "pulse",
        "device_amplitude_permille",
        "rec_skin",
    ];
    const EMITTER: &[&str] = &["anchor"];
    const CHARACTER: &[&str] = &["eject_threshold_permille", "eject_volume_scale_permille"];
    const PULSE: &[&str] = &["max_rate_millihz", "max_depth_permille", "lobe_gap_ticks"];
    match path {
        "" => Some(ROOT),
        "emitter" => Some(EMITTER),
        "pulse" => Some(PULSE),
        _ if path.starts_with("characters.") && path.matches('.').count() == 1 => Some(CHARACTER),
        _ => None,
    }
}

fn census_unknown(value: &serde_json::Value, path: &str, doc: &str) -> Result<(), ModeRefusal> {
    if let serde_json::Value::Object(map) = value {
        if let Some(known) = known_fields(path) {
            for key in map.keys() {
                if !known.contains(&key.as_str()) && !AUTHORITY_FIELDS.contains(&key.as_str()) {
                    let field = if path.is_empty() {
                        key.clone()
                    } else {
                        format!("{path}.{key}")
                    };
                    return Err(mode_refusal(
                        "TG-GEAR-FIELD-UNKNOWN",
                        doc,
                        &field,
                        format!("unknown field {key:?} in a mode settings document"),
                    ));
                }
            }
        }
        for (key, child) in map {
            let child_path = if path.is_empty() {
                key.clone()
            } else {
                format!("{path}.{key}")
            };
            census_unknown(child, &child_path, doc)?;
        }
    }
    Ok(())
}

/// Validates a mode-settings document through T4
/// §2.7's pipeline, extended with §8.4's three refusals,
/// returning the first refusal in pipeline order or the
/// admitted settings. The pipeline: (1) parse + format;
/// (2) unknown-field census; (3) class completeness;
/// (4) emitter anchor (`TG-PM-EJECT-ANCHOR`); (5)
/// profile bounds (`TG-PM-VOLUME-BOUND`); (8) pulse-law
/// bounds (`TG-PM-PULSE-BOUND`); (9) authority census
/// (`TG-GEAR-MIRROR-AUTHORITY`). Steps 6/7/10 (part
/// references, hooks, pack consistency) have no
/// settings-document content.
pub fn validate_settings_document(text: &str, document_path: &str) -> Result<ModeSettings, ModeRefusal> {
    let schema = |field: &str, message: String| {
        mode_refusal("TG-GEAR-SCHEMA", document_path, field, message)
    };
    // (1) parse + schema version.
    let value: serde_json::Value = serde_json::from_str(text).map_err(|e| {
        schema("", format!("malformed settings document: {e}"))
    })?;
    let obj = value
        .as_object()
        .ok_or_else(|| schema("", "settings document must be an object".to_string()))?;
    match obj.get("format").and_then(|f| f.as_str()) {
        Some(SETTINGS_FORMAT) => {}
        other => {
            return Err(schema(
                "format",
                format!("format must be {SETTINGS_FORMAT}, found {other:?}"),
            ));
        }
    }
    // (2) unknown-field census.
    census_unknown(&value, "", document_path)?;
    // (3) class completeness.
    for field in ["setting", "emitter", "characters", "pulse"] {
        if !obj.contains_key(field) {
            return Err(mode_refusal(
                "TG-GEAR-CLASS-INCOMPLETE",
                document_path,
                field,
                format!("required block {field} missing"),
            ));
        }
    }
    let emitter = obj["emitter"].as_object().ok_or_else(|| {
        schema("emitter", "emitter must be an object".to_string())
    })?;
    if !emitter.contains_key("anchor") {
        return Err(mode_refusal(
            "TG-GEAR-CLASS-INCOMPLETE",
            document_path,
            "emitter.anchor",
            "the emitter block must declare its anchor",
        ));
    }
    let characters = obj["characters"].as_object().ok_or_else(|| {
        schema("characters", "characters must be an object".to_string())
    })?;
    for (name, entry) in characters {
        let entry = entry.as_object().ok_or_else(|| {
            schema(&format!("characters.{name}"), "character entry must be an object".to_string())
        })?;
        for field in ["eject_threshold_permille", "eject_volume_scale_permille"] {
            if !entry.contains_key(field) {
                return Err(mode_refusal(
                    "TG-GEAR-CLASS-INCOMPLETE",
                    document_path,
                    &format!("characters.{name}.{field}"),
                    "per-character mode fields are required (§3.1)",
                ));
            }
        }
    }
    // (4) the emitter anchor (§8.4).
    let anchor = emitter["anchor"].as_str().unwrap_or("");
    if anchor != EJECT_ANCHOR {
        return Err(mode_refusal(
            "TG-PM-EJECT-ANCHOR",
            document_path,
            "emitter.anchor",
            format!(
                "an ejection emitter may be declared only on {EJECT_ANCHOR}, found {anchor:?}"
            ),
        ));
    }
    // (5) profile bounds (§8.4).
    let mut parsed_characters: BTreeMap<String, (u32, u32)> = BTreeMap::new();
    for (name, entry) in characters {
        let threshold = entry["eject_threshold_permille"].as_u64().unwrap_or(0) as u32;
        let volume = entry["eject_volume_scale_permille"].as_u64().unwrap_or(0) as u32;
        if !(500..=1500).contains(&threshold) {
            return Err(mode_refusal(
                "TG-PM-VOLUME-BOUND",
                document_path,
                &format!("characters.{name}.eject_threshold_permille"),
                format!("eject_threshold_permille {threshold} outside 500..=1500"),
            ));
        }
        if !(250..=2000).contains(&volume) {
            return Err(mode_refusal(
                "TG-PM-VOLUME-BOUND",
                document_path,
                &format!("characters.{name}.eject_volume_scale_permille"),
                format!("eject_volume_scale_permille {volume} outside 250..=2000"),
            ));
        }
        parsed_characters.insert(name.clone(), (threshold, volume));
    }
    // (8) pulse-law bounds (§8.4).
    let pulse = obj["pulse"].as_object().ok_or_else(|| {
        schema("pulse", "pulse must be an object".to_string())
    })?;
    let max_rate = pulse
        .get("max_rate_millihz")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;
    let max_depth = pulse
        .get("max_depth_permille")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;
    let lobe_gap = pulse
        .get("lobe_gap_ticks")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    if max_rate > 2000 {
        return Err(mode_refusal(
            "TG-PM-PULSE-BOUND",
            document_path,
            "pulse.max_rate_millihz",
            format!("pulse rate bound {max_rate} millihertz exceeds 2000 (§4.2)"),
        ));
    }
    if max_depth > 1000 {
        return Err(mode_refusal(
            "TG-PM-PULSE-BOUND",
            document_path,
            "pulse.max_depth_permille",
            format!("pulse depth bound {max_depth} exceeds 1000 (§4.2)"),
        ));
    }
    if lobe_gap != 9 {
        return Err(mode_refusal(
            "TG-PM-PULSE-BOUND",
            document_path,
            "pulse.lobe_gap_ticks",
            format!("the two-lobed pulse shape fixes the lobe gap at 9 ticks (D2), found {lobe_gap}"),
        ));
    }
    // (9) authority census (§2.2: a preference set may
    // carry settings; it may not carry mirror
    // enablement, a bound device, or a ceiling).
    if let Some(field) = find_authority_field(&value, "") {
        return Err(mode_refusal(
            "TG-GEAR-MIRROR-AUTHORITY",
            document_path,
            &field,
            "a settings file may not enable mirroring, bind a device, carry consent, or override a ceiling",
        ));
    }
    Ok(ModeSettings {
        format: SETTINGS_FORMAT.to_string(),
        setting: obj["setting"].as_str().unwrap_or("").to_string(),
        emitter_anchor: anchor.to_string(),
        characters: parsed_characters,
        pulse_max_rate_millihz: max_rate,
        pulse_max_depth_permille: max_depth,
        pulse_lobe_gap_ticks: lobe_gap,
        device_amplitude_permille: obj
            .get("device_amplitude_permille")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u32,
        rec_skin: obj
            .get("rec_skin")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
    })
}

/// The mode profile validator (§8.4's companion check
/// the gates consume): per-character fields in bounds,
/// and expression/vocal class completeness for one
/// playmate. A missing class fails naming the class and
/// the character (PM-G11's form).
pub fn validate_mode_profile(definition: &PlaytimeDefinition, character: &str) -> R<()> {
    let data = definition.character(character)?;
    if !(500..=1500).contains(&data.eject_threshold_permille) {
        return Err(Refusal::new(
            "TG-PM-VOLUME-BOUND",
            format!(
                "characters.{character}.eject_threshold_permille {} outside 500..=1500",
                data.eject_threshold_permille
            ),
        ));
    }
    if !(250..=2000).contains(&data.eject_volume_scale_permille) {
        return Err(Refusal::new(
            "TG-PM-VOLUME-BOUND",
            format!(
                "characters.{character}.eject_volume_scale_permille {} outside 250..=2000",
                data.eject_volume_scale_permille
            ),
        ));
    }
    for key in EXPRESSION_KEYS {
        if !data.expression_recipes.contains_key(key) {
            return Err(Refusal::new(
                "TG-PM-PROFILE",
                format!("character {character} expression class {key} has no governed recipe"),
            ));
        }
    }
    for class in VOCAL_CLASSES {
        match data.vocal_set.classes.get(class) {
            Some(count) if *count > 0 => {}
            _ => {
                return Err(Refusal::new(
                    "TG-PM-PROFILE",
                    format!(
                        "character {character} vocal class {class} does not resolve to an asset in her set"
                    ),
                ));
            }
        }
    }
    Ok(())
}

impl PlaytimeMode {
    /// Saves the current composition as a Playroom
    /// preference set (§2.2) under
    /// `<dir>/playroom/<name>.json` — the settings only:
    /// never mirror enablement, a bound device, or a
    /// ceiling (§8.3). Written with §5.9's sibling-temp +
    /// rename pattern.
    pub fn save_preference_set(&self, dir: &Path, name: &str) -> R<PathBuf> {
        let settings = ModeSettings::from_definition(
            &self.definition,
            self.user_amplitude,
            self.rec_on,
        );
        // The file is a settings *document* (§2.2): the
        // nested form §8.4's pipeline admits (emitter /
        // characters / pulse blocks), NOT the admitted
        // struct's flat field layout — the loader
        // validates through the pipeline, so writer and
        // reader must agree on the document form.
        let mut characters = serde_json::Map::new();
        for (name, (threshold, volume)) in &settings.characters {
            characters.insert(
                name.clone(),
                serde_json::json!({
                    "eject_threshold_permille": threshold,
                    "eject_volume_scale_permille": volume,
                }),
            );
        }
        let document = serde_json::json!({
            "format": settings.format,
            "setting": settings.setting,
            "emitter": { "anchor": settings.emitter_anchor },
            "characters": characters,
            "pulse": {
                "max_rate_millihz": settings.pulse_max_rate_millihz,
                "max_depth_permille": settings.pulse_max_depth_permille,
                "lobe_gap_ticks": settings.pulse_lobe_gap_ticks,
            },
            "device_amplitude_permille": settings.device_amplitude_permille,
            "rec_skin": settings.rec_skin,
        });
        let text = serde_json::to_string_pretty(&document)
            .map_err(|e| Refusal::new("TG-PM-STATE", format!("serialise settings: {e}")))?;
        let playroom = dir.join("playroom");
        std::fs::create_dir_all(&playroom).map_err(|e| {
            Refusal::new("TG-PM-STATE", format!("create {}: {e}", playroom.display()))
        })?;
        let path = playroom.join(format!("{name}.json"));
        let tmp = tmp_sibling(&path);
        std::fs::write(&tmp, format!("{text}\n"))
            .map_err(|e| Refusal::new("TG-PM-STATE", format!("write {}: {e}", tmp.display())))?;
        std::fs::rename(&tmp, &path).map_err(|e| {
            Refusal::new("TG-PM-STATE", format!("rename to {}: {e}", path.display()))
        })?;
        Ok(path)
    }

    /// Loads a preference set, validating through
    /// §8.4's pipeline on the way in (a settings file
    /// that attempts authority is refused, never
    /// applied).
    pub fn load_preference_set(dir: &Path, name: &str) -> Result<ModeSettings, ModeRefusal> {
        let path = dir.join("playroom").join(format!("{name}.json"));
        let text = std::fs::read_to_string(&path).map_err(|e| {
            mode_refusal(
                "TG-GEAR-SCHEMA",
                &path.display().to_string(),
                "",
                format!("read {}: {e}", path.display()),
            )
        })?;
        validate_settings_document(&text, &path.display().to_string())
    }
}

// ------------------------------------------------------------------ persistence

/// The persisted mode-session record (§6.1: wetness is
/// session state persisted with the session under
/// §5.9's pattern; pressure and episode accounting ride
/// with it so a restored session restores its state).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ModePersist {
    format: String,
    character: String,
    wetness: Wetness,
    pressure_mp: i64,
    episode_volume_micro: i64,
    first_leak_fired: bool,
}

impl PlaytimeMode {
    /// Saves the mode-session state to
    /// `<dir>/<character>.playtime.json` (§5.9's write
    /// pattern: sibling temp + rename).
    pub fn save_state(&self, dir: &Path) -> R<()> {
        let character = self.require_entered()?.to_string();
        let rec = ModePersist {
            format: STATE_FORMAT.to_string(),
            character: character.clone(),
            wetness: self.wetness.clone(),
            pressure_mp: self.ejection.pressure_mp,
            episode_volume_micro: self.ejection.episode_volume_micro,
            first_leak_fired: self.ejection.first_leak_fired,
        };
        let text = serde_json::to_string(&rec)
            .map_err(|e| Refusal::new("TG-PM-STATE", format!("serialise: {e}")))?;
        let path = dir.join(format!("{character}.playtime.json"));
        let tmp = tmp_sibling(&path);
        std::fs::write(&tmp, format!("{text}\n"))
            .map_err(|e| Refusal::new("TG-PM-STATE", format!("write {}: {e}", tmp.display())))?;
        std::fs::rename(&tmp, &path).map_err(|e| {
            Refusal::new("TG-PM-STATE", format!("rename to {}: {e}", path.display()))
        })?;
        Ok(())
    }

    /// Restores a saved mode-session state (wetness,
    /// pressure, episode accounting) into this mode,
    /// which must be entered for the same character. A
    /// corrupt or out-of-bounds record refuses whole.
    pub fn restore_state(&mut self, dir: &Path) -> R<()> {
        let character = self.require_entered()?.to_string();
        let path = dir.join(format!("{character}.playtime.json"));
        let text = std::fs::read_to_string(&path)
            .map_err(|e| Refusal::new("TG-PM-STATE", format!("read {}: {e}", path.display())))?;
        let rec: ModePersist = serde_json::from_str(&text)
            .map_err(|e| Refusal::new("TG-PM-STATE", format!("persisted state: {e}")))?;
        let bad = |what: &str| Refusal::new("TG-PM-STATE", format!("persisted state: {what}"));
        if rec.format != STATE_FORMAT {
            return Err(bad(&format!("format must be {STATE_FORMAT}")));
        }
        if rec.character != character {
            return Err(bad(&format!(
                "state belongs to {}, not {character}",
                rec.character
            )));
        }
        if !(0..=MP_FULL).contains(&rec.pressure_mp) {
            return Err(bad(&format!("pressure {} out of bounds", rec.pressure_mp)));
        }
        if rec.episode_volume_micro < 0 {
            return Err(bad("episode volume negative"));
        }
        for (area, w) in &rec.wetness.areas_mp {
            if !self.definition.wetness.areas.contains(area) {
                return Err(bad(&format!("wetness area {area:?} not in the mode data")));
            }
            if !(0..=MP_FULL).contains(w) {
                return Err(bad(&format!("wetness {area} {w} out of bounds")));
            }
        }
        self.wetness = rec.wetness;
        self.ejection.pressure_mp = rec.pressure_mp;
        self.ejection.episode_volume_micro = rec.episode_volume_micro;
        self.ejection.first_leak_fired = rec.first_leak_fired;
        Ok(())
    }
}

fn tmp_sibling(path: &Path) -> PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(".tmp");
    PathBuf::from(s)
}

// ------------------------------------------------------------------ takes

/// A recorded take of a mode session (§8.3): the input
/// stream and the op stream, with the trajectory and
/// cue-track hashes the live run produced. Replay
/// re-evaluates the full mode identically and actuates
/// nothing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TakeRecord {
    /// [`TAKE_FORMAT`].
    pub format: String,
    pub character: String,
    /// Engine tick the take started at (PM-1 takes are
    /// recorded from a fresh entry: 0).
    pub start_tick: u64,
    /// The REC skin state at take start (§8.1/§8.3:
    /// replay restores it so the cue track aligns).
    pub rec_on: bool,
    /// The per-tick input stream.
    pub inputs: Vec<ModeTickInput>,
    /// The op stream: (engine tick at the op, op).
    pub ops: Vec<(u64, ModeOp)>,
    pub trajectory_hash: u64,
    pub cue_track_hash: u64,
}

impl TakeRecord {
    /// Tick count of the take.
    pub fn ticks(&self) -> u64 {
        self.inputs.len() as u64
    }

    /// Saves the take as a file (the PM-G13 deliverable
    /// form: takes reach the Owner as files).
    pub fn save_to(&self, path: &Path) -> R<()> {
        let text = serde_json::to_string_pretty(self)
            .map_err(|e| Refusal::new("TG-PM-STATE", format!("serialise take: {e}")))?;
        let tmp = tmp_sibling(path);
        std::fs::write(&tmp, format!("{text}\n"))
            .map_err(|e| Refusal::new("TG-PM-STATE", format!("write {}: {e}", tmp.display())))?;
        std::fs::rename(&tmp, path).map_err(|e| {
            Refusal::new("TG-PM-STATE", format!("rename to {}: {e}", path.display()))
        })?;
        Ok(())
    }

    /// Loads a take file.
    pub fn load_from(path: &Path) -> R<TakeRecord> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| Refusal::new("TG-PM-STATE", format!("read {}: {e}", path.display())))?;
        let take: TakeRecord = serde_json::from_str(&text)
            .map_err(|e| Refusal::new("TG-PM-STATE", format!("take record: {e}")))?;
        if take.format != TAKE_FORMAT {
            return Err(Refusal::new(
                "TG-PM-STATE",
                format!("take format must be {TAKE_FORMAT}"),
            ));
        }
        Ok(take)
    }
}

/// The outcome of replaying a take (§8.3, PM-G10(c)).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplayOutcome {
    pub trajectory_hash: u64,
    pub cue_track_hash: u64,
    /// Mirror/drive actuations performed by the replay:
    /// always 0 — replay re-evaluates state and routes
    /// no drive signal anywhere (§7.7 of T4).
    pub actuations: u32,
    /// The replay engine's mirror generation after the
    /// run (0: no mirror session ever existed).
    pub mirror_generation: u64,
}

/// Replays a take from a fresh entry under
/// `definition`, re-evaluating the full mode (pulses,
/// ejection, wetness, cues) tick-for-tick.
pub fn replay_take(definition: &PlaytimeDefinition, take: &TakeRecord) -> R<ReplayOutcome> {
    if take.start_tick != 0 {
        return Err(Refusal::new(
            "TG-PM-STATE",
            "PM-1 replay covers takes recorded from a fresh entry (start_tick 0)",
        ));
    }
    let mut host = ArousalHost::new();
    let mut mode = PlaytimeMode::new(definition.clone());
    mode.enter(&mut host, &take.character)?;
    if take.rec_on {
        mode.set_rec_skin(&mut host, true)?;
    }
    mode.start_take(&mut host)?;
    let mut ops: Vec<(u64, ModeOp)> = take.ops.clone();
    ops.sort_by_key(|(tick, _)| *tick);
    let mut next_op = 0usize;
    for (idx, input) in take.inputs.iter().enumerate() {
        let tick_now = idx as u64;
        while next_op < ops.len() && ops[next_op].0 == tick_now {
            let op = ops[next_op].1.clone();
            apply_take_op(&mut mode, &mut host, &op)?;
            next_op += 1;
        }
        mode.step(&mut host, input)?;
    }
    // Ops stamped after the final recorded step (they
    // happened between the last step and the take stop).
    while next_op < ops.len() {
        let op = ops[next_op].1.clone();
        apply_take_op(&mut mode, &mut host, &op)?;
        next_op += 1;
    }
    let replayed = mode.stop_take(&mut host)?;
    let mirror_generation = host
        .get(&take.character)
        .map_or(0, |e| e.mirror_generation());
    Ok(ReplayOutcome {
        trajectory_hash: replayed.trajectory_hash,
        cue_track_hash: replayed.cue_track_hash,
        actuations: 0,
        mirror_generation,
    })
}

/// Applies one recorded op during replay.
fn apply_take_op(mode: &mut PlaytimeMode, host: &mut ArousalHost, op: &ModeOp) -> R<()> {
    match op {
        ModeOp::SetHold { on } => mode.set_hold(host, *on).map(|_| ()),
        ModeOp::SetAmplitude { permille } => mode.set_amplitude(host, *permille).map(|_| ()),
        ModeOp::SetContact {
            contact_quality,
            displaced,
        } => mode
            .set_contact(host, *contact_quality, *displaced)
            .map(|_| ()),
        ModeOp::SetRecSkin { on } => mode.set_rec_skin(host, *on).map(|_| ()),
        ModeOp::RemoveAllAndReset => mode.remove_all_and_reset(host).map(|_| ()),
        ModeOp::Enter { .. }
        | ModeOp::Exit
        | ModeOp::StartTake
        | ModeOp::StopTake => Err(Refusal::new(
            "TG-PM-STATE",
            "lifecycle ops are not part of a take's op stream",
        )),
    }
}
