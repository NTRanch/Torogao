//! The presentation view: Tier 2, the one float-bearing
//! surface in the runtime.
//!
//! A core's `present()` emits this derived view of the world
//! for a shell to render (puppet parameters, layer state, UI
//! model — reduced here to named integer meters and named
//! fractional gauges). It is emitted per frame, it is NEVER
//! fed back into `step`, and a hash of it (if a shell computes
//! one) is a per-platform diagnostic, not an identity: Tier 2
//! values may differ in last bits across architectures and
//! are compared under a recorded tolerance band (plan SS2.1).
//!
//! All float arithmetic in this crate lives in this module:
//! witnesses construct gauges through [`GaugeView::ratio`]
//! so the float site is singular and auditable, and the tier
//! lint ([`crate::tier1`]) forbids the float types everywhere
//! else.

use serde::{Deserialize, Serialize};

/// One named integer meter in the presentation view (a stat,
/// a gauge in milli-units, a counter — anything a shell shows
/// as a number or a bar with integer steps).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MeterView {
    /// The meter's stable id.
    pub id: String,
    /// The meter's current value.
    pub value: i64,
}

/// One named fractional gauge in the presentation view — the
/// float-bearing half of Tier 2. Display only: no code path
/// may read a gauge back into simulation state.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GaugeView {
    /// The gauge's stable id.
    pub id: String,
    /// The fraction, nominally in [0, 1] (cores may exceed it;
    /// shells clamp for display).
    pub fraction: f64,
}

impl GaugeView {
    /// Builds a gauge from an integer ratio — the singular
    /// float construction site for hosted cores. A zero
    /// denominator yields 0.0 rather than a NaN: presentation
    /// must never manufacture NaNs into a shell.
    #[must_use]
    #[allow(clippy::float_arithmetic)]
    pub fn ratio(id: impl Into<String>, numerator: i64, denominator: i64) -> Self {
        let fraction = if denominator == 0 {
            0.0
        } else {
            numerator as f64 / denominator as f64
        };
        Self {
            id: id.into(),
            fraction,
        }
    }
}

/// The derived presentation state a core emits per frame.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PresentationState {
    /// The tick this view describes.
    pub tick: u64,
    /// The headline a shell leads with (current scene, phase,
    /// or mode — the core's own naming).
    pub headline: String,
    /// A detail line (current dialogue line id, encounter
    /// name, ...), when the core has one.
    pub detail: Option<String>,
    /// Integer meters, in the core's declared order.
    pub meters: Vec<MeterView>,
    /// Fractional gauges, in the core's declared order.
    pub gauges: Vec<GaugeView>,
}
