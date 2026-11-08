//! The measured tuning table for the coin dynamic, as data.
//!
//! The table ships as RON (`data/coin-dynamics.ron`, schema
//! `torogao-coin-dynamics/1`) following the estate's RON
//! contract discipline: the file is the contract, this module
//! validates it at load, and any value outside the measured
//! shape is a typed refusal — never a silent default.

use serde::Deserialize;

/// The embedded measured table (see the file header for
/// provenance and the IL offsets behind every value).
pub const DYNAMICS_RON: &str = include_str!("../data/coin-dynamics.ron");

/// Bound on the measured gauge table. The source table has 5
/// entries; the bound exists so a corrupt file cannot allocate
/// an unbounded table, not as a claim about the source.
pub const GAUGE_MAXIMA_COUNT_MAX: usize = 16;
/// Bound on preparation actions (source: 15) and direct tuples
/// (source: 9). Same rationale as above.
pub const PREP_ACTIONS_COUNT_MAX: usize = 64;
/// A single preparation action may cost at most one day of
/// game minutes (source maximum: 35).
pub const ACTION_MINUTES_MAX: u32 = 24 * 60;
/// Last valid minute-of-day index (23:59).
pub const MINUTE_OF_DAY_MAX: u32 = 23 * 60 + 59;

/// The three preparation stats, in the assembly's own field
/// order (hp, mp, sp). The mapping of these fields to the RE
/// record's Fatigue / Spirit-Submission / Arousal labels is
/// UNVERIFIED (COIN-DYNAMIC.md §5), so this crate keeps the
/// assembly's neutral names and asserts no fiction label.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StatTriple {
    pub hp: f64,
    pub mp: f64,
    pub sp: f64,
}

impl StatTriple {
    pub const ZERO: StatTriple = StatTriple {
        hp: 0.0,
        mp: 0.0,
        sp: 0.0,
    };
    pub const ONE: StatTriple = StatTriple {
        hp: 1.0,
        mp: 1.0,
        sp: 1.0,
    };

    fn from_tuple(t: (f64, f64, f64)) -> StatTriple {
        StatTriple {
            hp: t.0,
            mp: t.1,
            sp: t.2,
        }
    }

    fn components_finite_nonnegative(&self) -> bool {
        [self.hp, self.mp, self.sp]
            .iter()
            .all(|v| v.is_finite() && *v >= 0.0)
    }
}

/// One preparation action: a base stat delta plus its cost in
/// game minutes (gmpara::addStatus / gmpara::passTime).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PrepAction {
    pub delta: StatTriple,
    pub minutes: u32,
}

/// Failure modes of loading, validating, or running the
/// dynamic. Invalid data is an error, never an assertion: the
/// RON file is an external input.
#[derive(Debug)]
pub enum CoinError {
    /// The RON text did not parse as the dynamics schema.
    Parse(ron::de::SpannedError),
    /// A parsed value fell outside the measured shape.
    Invalid {
        field: &'static str,
        reason: &'static str,
    },
    /// A preparation action index outside the table.
    UnknownPrepAction { index: usize },
    /// An attempt was made at a level the measured table does
    /// not cover (a sixth maximum is UNKNOWN in the source).
    LevelTableExhausted { level: usize },
    /// A step input (dt, sample stream) broke the contract.
    InvalidStep { reason: &'static str },
}

impl std::fmt::Display for CoinError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CoinError::Parse(e) => write!(f, "dynamics RON parse failed: {e}"),
            CoinError::Invalid { field, reason } => {
                write!(f, "invalid dynamics field {field}: {reason}")
            }
            CoinError::UnknownPrepAction { index } => {
                write!(f, "unknown preparation action index {index}")
            }
            CoinError::LevelTableExhausted { level } => {
                write!(f, "no measured gauge maximum for level {level}")
            }
            CoinError::InvalidStep { reason } => write!(f, "invalid step input: {reason}"),
        }
    }
}

impl std::error::Error for CoinError {}

/// The raw deserialized shape of `torogao-coin-dynamics/1`.
#[derive(Debug, Deserialize)]
struct DynamicsWire {
    gauge_maxima: Vec<f64>,
    timer_start: f64,
    timer_divisor: f64,
    setup_wait_seconds: f64,
    swing_gain: f64,
    bonus_threshold: f64,
    bonus_multiplier: f64,
    pointer_animation_scale: f64,
    prep_scale: f64,
    message_divisor: f64,
    message_scale: f64,
    gauge_coefficient: f64,
    prep_actions: Vec<((f64, f64, f64), u32)>,
    direct_tuples: Vec<(f64, f64, f64)>,
    leaving_minute: u32,
}

/// The validated dynamics table. Construct only through
/// `Dynamics::from_ron` or `measured_dynamics`, so every
/// instance satisfies the measured shape.
#[derive(Clone, Debug)]
pub struct Dynamics {
    gauge_maxima: Vec<f64>,
    timer_start: f64,
    timer_divisor: f64,
    setup_wait_seconds: f64,
    swing_gain: f64,
    bonus_threshold: f64,
    bonus_multiplier: f64,
    pointer_animation_scale: f64,
    prep_scale: f64,
    message_divisor: f64,
    message_scale: f64,
    gauge_coefficient: f64,
    prep_actions: Vec<PrepAction>,
    direct_tuples: Vec<StatTriple>,
    leaving_minute: u32,
}

impl Dynamics {
    /// Parse and validate a dynamics table from RON text.
    pub fn from_ron(text: &str) -> Result<Dynamics, CoinError> {
        let wire: DynamicsWire = ron::from_str(text).map_err(CoinError::Parse)?;
        let d = Dynamics {
            gauge_maxima: wire.gauge_maxima,
            timer_start: wire.timer_start,
            timer_divisor: wire.timer_divisor,
            setup_wait_seconds: wire.setup_wait_seconds,
            swing_gain: wire.swing_gain,
            bonus_threshold: wire.bonus_threshold,
            bonus_multiplier: wire.bonus_multiplier,
            pointer_animation_scale: wire.pointer_animation_scale,
            prep_scale: wire.prep_scale,
            message_divisor: wire.message_divisor,
            message_scale: wire.message_scale,
            gauge_coefficient: wire.gauge_coefficient,
            prep_actions: wire
                .prep_actions
                .into_iter()
                .map(|(t, minutes)| PrepAction {
                    delta: StatTriple::from_tuple(t),
                    minutes,
                })
                .collect(),
            direct_tuples: wire
                .direct_tuples
                .into_iter()
                .map(StatTriple::from_tuple)
                .collect(),
            leaving_minute: wire.leaving_minute,
        };
        d.validate()?;
        Ok(d)
    }

    fn invalid(field: &'static str, reason: &'static str) -> CoinError {
        CoinError::Invalid { field, reason }
    }

    fn validate(&self) -> Result<(), CoinError> {
        if self.gauge_maxima.is_empty() || self.gauge_maxima.len() > GAUGE_MAXIMA_COUNT_MAX {
            return Err(Self::invalid("gauge_maxima", "count outside 1..=16"));
        }
        for (i, m) in self.gauge_maxima.iter().enumerate() {
            if !m.is_finite() || *m <= 0.0 {
                return Err(Self::invalid(
                    "gauge_maxima",
                    "entries must be finite and positive",
                ));
            }
            if i > 0 && *m <= self.gauge_maxima[i - 1] {
                return Err(Self::invalid(
                    "gauge_maxima",
                    "entries must strictly increase",
                ));
            }
        }
        if !self.timer_start.is_finite() || self.timer_start <= 0.0 {
            return Err(Self::invalid("timer_start", "must be finite and positive"));
        }
        if !self.timer_divisor.is_finite() || self.timer_divisor <= 0.0 {
            return Err(Self::invalid(
                "timer_divisor",
                "must be finite and positive",
            ));
        }
        if !self.setup_wait_seconds.is_finite() || self.setup_wait_seconds < 0.0 {
            return Err(Self::invalid(
                "setup_wait_seconds",
                "must be finite and non-negative",
            ));
        }
        if !self.swing_gain.is_finite() || self.swing_gain <= 0.0 {
            return Err(Self::invalid("swing_gain", "must be finite and positive"));
        }
        if !self.bonus_threshold.is_finite() || self.bonus_threshold < 0.0 {
            return Err(Self::invalid(
                "bonus_threshold",
                "must be finite and non-negative",
            ));
        }
        if !self.bonus_multiplier.is_finite() || self.bonus_multiplier < 1.0 {
            return Err(Self::invalid("bonus_multiplier", "must be finite and >= 1"));
        }
        if !self.pointer_animation_scale.is_finite() || self.pointer_animation_scale <= 0.0 {
            return Err(Self::invalid(
                "pointer_animation_scale",
                "must be finite and positive",
            ));
        }
        if !self.prep_scale.is_finite() || self.prep_scale <= 0.0 {
            return Err(Self::invalid("prep_scale", "must be finite and positive"));
        }
        if !self.message_divisor.is_finite() || self.message_divisor <= 0.0 {
            return Err(Self::invalid(
                "message_divisor",
                "must be finite and positive",
            ));
        }
        if !self.message_scale.is_finite() || self.message_scale < 0.0 {
            return Err(Self::invalid(
                "message_scale",
                "must be finite and non-negative",
            ));
        }
        if !self.gauge_coefficient.is_finite() || self.gauge_coefficient <= 0.0 {
            return Err(Self::invalid(
                "gauge_coefficient",
                "must be finite and positive",
            ));
        }
        if self.prep_actions.is_empty() || self.prep_actions.len() > PREP_ACTIONS_COUNT_MAX {
            return Err(Self::invalid("prep_actions", "count outside 1..=64"));
        }
        for a in &self.prep_actions {
            if !a.delta.components_finite_nonnegative() {
                return Err(Self::invalid(
                    "prep_actions",
                    "deltas must be finite and >= 0",
                ));
            }
            if a.minutes == 0 || a.minutes > ACTION_MINUTES_MAX {
                return Err(Self::invalid("prep_actions", "minutes outside 1..=1440"));
            }
        }
        if self.direct_tuples.len() > PREP_ACTIONS_COUNT_MAX {
            return Err(Self::invalid("direct_tuples", "count outside 0..=64"));
        }
        for t in &self.direct_tuples {
            if !t.components_finite_nonnegative() {
                return Err(Self::invalid(
                    "direct_tuples",
                    "components must be finite and >= 0",
                ));
            }
        }
        if self.leaving_minute > MINUTE_OF_DAY_MAX {
            return Err(Self::invalid("leaving_minute", "outside 0..=1439"));
        }
        Ok(())
    }

    /// The measured gauge maximum for a level index, or a
    /// typed refusal past the measured table.
    pub fn gauge_maximum(&self, level: usize) -> Result<f64, CoinError> {
        self.gauge_maxima
            .get(level)
            .copied()
            .ok_or(CoinError::LevelTableExhausted { level })
    }

    /// Number of levels the measured table covers.
    pub fn measured_level_count(&self) -> usize {
        self.gauge_maxima.len()
    }

    pub fn timer_start(&self) -> f64 {
        self.timer_start
    }

    pub fn timer_divisor(&self) -> f64 {
        self.timer_divisor
    }

    pub fn setup_wait_seconds(&self) -> f64 {
        self.setup_wait_seconds
    }

    pub fn swing_gain(&self) -> f64 {
        self.swing_gain
    }

    pub fn bonus_threshold(&self) -> f64 {
        self.bonus_threshold
    }

    pub fn bonus_multiplier(&self) -> f64 {
        self.bonus_multiplier
    }

    pub fn pointer_animation_scale(&self) -> f64 {
        self.pointer_animation_scale
    }

    pub fn prep_scale(&self) -> f64 {
        self.prep_scale
    }

    pub fn message_divisor(&self) -> f64 {
        self.message_divisor
    }

    pub fn message_scale(&self) -> f64 {
        self.message_scale
    }

    pub fn gauge_coefficient(&self) -> f64 {
        self.gauge_coefficient
    }

    /// A preparation action by switch index, or a typed
    /// refusal for an index the table does not carry.
    pub fn prep_action(&self, index: usize) -> Result<PrepAction, CoinError> {
        self.prep_actions
            .get(index)
            .copied()
            .ok_or(CoinError::UnknownPrepAction { index })
    }

    pub fn prep_action_count(&self) -> usize {
        self.prep_actions.len()
    }

    pub fn direct_tuples(&self) -> &[StatTriple] {
        &self.direct_tuples
    }

    pub fn leaving_minute(&self) -> u32 {
        self.leaving_minute
    }
}

/// Load the embedded measured table (data/coin-dynamics.ron).
pub fn measured_dynamics() -> Result<Dynamics, CoinError> {
    Dynamics::from_ron(DYNAMICS_RON)
}
