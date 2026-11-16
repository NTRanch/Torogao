//! The performed-prop minigame host (Phase 2 slice P2-10,
//! D-465): performed minigames are hosted here, in the app —
//! continuous performed input is reduced to ONE aggregate
//! event at a tick boundary and logged in the choice log
//! like any other input. The interpreter's tick contract is
//! not extended: the aggregate is an ordinary step-3 `event`
//! input whose name is the result and whose option carries
//! the bounded summary. Replay from the log therefore needs
//! no input stream — the stream existed only here, and the
//! prop's internal simulation appears nowhere in AUTH-G
//! records (the Item G1 boundary).
//!
//! The reducer is a pure function of the tick's samples:
//! integer milliunits only, no clocks, no randomness — the
//! same stream reduces to the same aggregate, always. The
//! study behind the seam is RJ318915's `goenScr` pendulum
//! (a coin swung between two finger anchors, gain scaled by
//! the one global `saimin_coefficient`): a swing is one
//! performed extremum whose amplitude clears the host's
//! authored floor; the result name quantizes the swing
//! count through authored bands, and the summary records
//! exactly two bounded values — the swing count and the
//! peak amplitude.

use serde_json::{Value, json};

/// One performed extremum observed within a tick: the
/// prop's swing reaching a turning point. Amplitude is in
/// milliunits (1000 = the prop's authored full swing).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PerformedSample {
    /// The extremum's amplitude, in milliunits.
    pub amplitude_milli: i64,
}

/// One result band: a tick whose counted swings reach
/// `min_swings` names `result`. Bands are authored in
/// descending threshold order and must include a band at
/// `min_swings == 1`, so every non-empty performance names
/// exactly one result.
#[derive(Clone, Debug, PartialEq)]
pub struct ResultBand {
    /// The smallest swing count naming this result (>= 1).
    pub min_swings: u32,
    /// The result name (the aggregate event's name).
    pub result: String,
}

/// The aggregate event: what one tick of performance
/// becomes. Logged as a choice-log `event` input — `result`
/// is the event name, and [`AggregateEvent::summary`] is
/// the bounded summary the log entry's option carries.
#[derive(Clone, Debug, PartialEq)]
pub struct AggregateEvent {
    /// The named result (the event name).
    pub result: String,
    /// How many swings the tick counted.
    pub swings: u32,
    /// The tick's peak amplitude, in milliunits.
    pub peak_milli: i64,
}

impl AggregateEvent {
    /// The bounded summary: exactly two values, always in
    /// this order — `swings=<n>;peak=<milli>`.
    pub fn summary(&self) -> String {
        format!("swings={};peak={}", self.swings, self.peak_milli)
    }

    /// The choice-log entry for this aggregate at `tick`:
    /// an ordinary event input, indistinguishable in shape
    /// from any other — which is the seam's whole point.
    pub fn log_entry(&self, tick: u64) -> Value {
        json!({
            "tick": tick,
            "kind": "event",
            "id": self.result,
            "option": self.summary(),
        })
    }
}

/// Why a prop host configuration was refused.
#[derive(Clone, Debug, PartialEq)]
pub enum PropHostError {
    /// The band list is empty.
    NoBands,
    /// A band's threshold is zero (a result must require a
    /// performance).
    ZeroThreshold(String),
    /// Band thresholds are not strictly descending.
    BandsNotDescending,
    /// No band sits at `min_swings == 1`, so a one-swing
    /// tick would name no result.
    NoBaseBand,
    /// A result name is empty.
    EmptyResult(u32),
    /// The amplitude floor is negative.
    NegativeFloor,
}

impl std::fmt::Display for PropHostError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PropHostError::NoBands => write!(f, "a prop host needs at least one result band"),
            PropHostError::ZeroThreshold(result) => {
                write!(f, "result band '{result}' requires zero swings")
            }
            PropHostError::BandsNotDescending => {
                write!(f, "result bands must be strictly descending by min_swings")
            }
            PropHostError::NoBaseBand => {
                write!(f, "result bands must include a band at min_swings == 1")
            }
            PropHostError::EmptyResult(min_swings) => {
                write!(f, "result band at min_swings {min_swings} names no result")
            }
            PropHostError::NegativeFloor => {
                write!(f, "the amplitude floor is negative")
            }
        }
    }
}

impl std::error::Error for PropHostError {}

/// A performed-prop host: reduces one tick of performed
/// samples to at most one aggregate event.
#[derive(Clone, Debug, PartialEq)]
pub struct PropHost {
    /// The amplitude floor: an extremum below it is motion,
    /// not a swing (milliunits).
    min_amplitude_milli: i64,
    /// Result bands, strictly descending by threshold.
    bands: Vec<ResultBand>,
}

impl PropHost {
    /// Builds a host from its authored configuration,
    /// refusing shapes under which a performance could name
    /// no result — or two.
    pub fn new(min_amplitude_milli: i64, bands: Vec<ResultBand>) -> Result<Self, PropHostError> {
        if min_amplitude_milli < 0 {
            return Err(PropHostError::NegativeFloor);
        }
        if bands.is_empty() {
            return Err(PropHostError::NoBands);
        }
        for band in &bands {
            if band.min_swings == 0 {
                return Err(PropHostError::ZeroThreshold(band.result.clone()));
            }
            if band.result.is_empty() {
                return Err(PropHostError::EmptyResult(band.min_swings));
            }
        }
        if bands
            .windows(2)
            .any(|pair| pair[0].min_swings <= pair[1].min_swings)
        {
            return Err(PropHostError::BandsNotDescending);
        }
        if bands.last().map(|band| band.min_swings) != Some(1) {
            return Err(PropHostError::NoBaseBand);
        }
        Ok(Self {
            min_amplitude_milli,
            bands,
        })
    }

    /// Reduces one tick's performed samples to its
    /// aggregate event. A tick with no swing above the
    /// floor produces no event at all — the log gains no
    /// entry, exactly as if the prop had not been touched.
    pub fn reduce(&self, samples: &[PerformedSample]) -> Option<AggregateEvent> {
        let mut swings: u32 = 0;
        let mut peak_milli: i64 = 0;
        for sample in samples {
            if sample.amplitude_milli > peak_milli {
                peak_milli = sample.amplitude_milli;
            }
            if sample.amplitude_milli >= self.min_amplitude_milli {
                swings = swings.saturating_add(1);
            }
        }
        if swings == 0 {
            return None;
        }
        let result = self
            .bands
            .iter()
            .find(|band| swings >= band.min_swings)
            .map(|band| band.result.clone())
            .expect("the base band at min_swings == 1 names every performance");
        Some(AggregateEvent {
            result,
            swings,
            peak_milli,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{AggregateEvent, PerformedSample, PropHost, PropHostError, ResultBand};

    fn band(min_swings: u32, result: &str) -> ResultBand {
        ResultBand {
            min_swings,
            result: result.to_string(),
        }
    }

    fn host() -> PropHost {
        PropHost::new(
            400,
            vec![
                band(6, "induction_deep"),
                band(3, "induction_mid"),
                band(1, "induction_light"),
            ],
        )
        .expect("host config")
    }

    fn samples(amplitudes: &[i64]) -> Vec<PerformedSample> {
        amplitudes
            .iter()
            .map(|amplitude_milli| PerformedSample {
                amplitude_milli: *amplitude_milli,
            })
            .collect()
    }

    #[test]
    fn reduction_counts_swings_above_the_floor_and_names_the_band() {
        let host = host();
        // Five swings above the floor (one at exactly the
        // floor), two below it: the count is five, the band
        // is mid, the peak rides along.
        let aggregate = host
            .reduce(&samples(&[900, 850, 400, 700, 620, 120, 390]))
            .expect("aggregate");
        assert_eq!(
            aggregate,
            AggregateEvent {
                result: "induction_mid".to_string(),
                swings: 5,
                peak_milli: 900,
            }
        );
        assert_eq!(aggregate.summary(), "swings=5;peak=900");
        // Six above the floor crosses into the deep band.
        let deep = host
            .reduce(&samples(&[500, 500, 500, 500, 500, 500]))
            .expect("aggregate");
        assert_eq!(deep.result, "induction_deep");
        assert_eq!(deep.swings, 6);
    }

    #[test]
    fn reduction_is_deterministic_and_silent_ticks_emit_nothing() {
        let host = host();
        let stream = samples(&[800, 300, 760]);
        assert_eq!(host.reduce(&stream), host.reduce(&stream));
        assert_eq!(host.reduce(&[]), None, "no performance, no event");
        assert_eq!(
            host.reduce(&samples(&[100, 399])),
            None,
            "motion below the floor is not a swing"
        );
    }

    #[test]
    fn aggregate_log_entry_is_an_ordinary_event_input() {
        let aggregate = AggregateEvent {
            result: "induction_deep".to_string(),
            swings: 6,
            peak_milli: 990,
        };
        let entry = aggregate.log_entry(4);
        assert_eq!(entry["tick"], 4);
        assert_eq!(entry["kind"], "event");
        assert_eq!(entry["id"], "induction_deep");
        assert_eq!(entry["option"], "swings=6;peak=990");
    }

    #[test]
    fn host_refuses_configurations_that_cannot_name_every_performance() {
        assert_eq!(PropHost::new(400, vec![]), Err(PropHostError::NoBands));
        assert_eq!(
            PropHost::new(-1, vec![band(1, "x")]),
            Err(PropHostError::NegativeFloor)
        );
        assert_eq!(
            PropHost::new(400, vec![band(0, "free")]),
            Err(PropHostError::ZeroThreshold("free".to_string()))
        );
        assert_eq!(
            PropHost::new(400, vec![band(3, "mid"), band(6, "deep"), band(1, "light")]),
            Err(PropHostError::BandsNotDescending)
        );
        assert_eq!(
            PropHost::new(400, vec![band(6, "deep"), band(3, "mid")]),
            Err(PropHostError::NoBaseBand)
        );
        assert_eq!(
            PropHost::new(400, vec![band(1, "")]),
            Err(PropHostError::EmptyResult(1))
        );
    }
}
