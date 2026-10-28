//! The arousal-engine interface (§5.2) that T4.1's surfaces
//! bind to — and the stub behind it.
//!
//! T4.1 does not implement the arousal engine: §5's
//! integration laws are the parallel T4.2 commission's scope
//! (its fixtures are constants-gated). What T4.1 owns is the
//! *interface*: the §5.2 state vector as a type, and the view
//! trait the Playroom dashboard reads. The dashboard reads
//! state through this trait; it never estimates it (§3.2).
//!
//! [`StubEngine`] is a STUB, labelled as such everywhere it
//! appears: it holds a state vector the caller sets, reports
//! it back verbatim, and honours the latch/reset verbs. It
//! integrates nothing. T4.2's real engine replaces it behind
//! this same trait when the two branches compose; neither
//! branch invents the other's internals.

/// The engine's phase (§5.5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArousalPhase {
    /// Afterglow hold.
    Afterglow,
    /// Baseline.
    Baseline,
    /// Culmination performance.
    Culminating,
    /// Refractory period.
    Refractory,
    /// Stimulated.
    Stimulated,
    /// Taper after the performance.
    Taper,
}

impl ArousalPhase {
    /// The phase's name, as dashboards render it.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Afterglow => "Afterglow",
            Self::Baseline => "Baseline",
            Self::Culminating => "Culminating",
            Self::Refractory => "Refractory",
            Self::Stimulated => "Stimulated",
            Self::Taper => "Taper",
        }
    }
}

/// The §5.2 state vector, exactly as specified: continuous
/// fields in permille, the phase, the exactly-once latch, and
/// the phase clock. This is the interface type; the engine
/// that *integrates* it is T4.2's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ArousalStateVector {
    /// Arousal (A), 0..=1000.
    pub arousal: u32,
    /// Pleasure charge (Q), stored 0..=1500.
    pub pleasure_charge: u32,
    /// Sensitivity (S), 500..=1500, 1000 neutral.
    pub sensitivity: u32,
    /// Arousal floor (F), 0..=600.
    pub floor: u32,
    /// Rolling peak input (I-hat), 0..=1000.
    pub peak_input: u32,
    /// Episode dose (D), 0..=1000.
    pub dose: u32,
    /// Current phase.
    pub phase: ArousalPhase,
    /// Exactly-once culmination latch.
    pub culmination_latch: bool,
    /// Episode the latch belongs to.
    pub episode_id: u64,
    /// Ticks spent in the current phase.
    pub phase_ticks: u64,
    /// Afterglow level (G), 0..=1000.
    pub afterglow_level: u32,
}

impl ArousalStateVector {
    /// The baseline vector (§5.8 reset target): everything at
    /// rest, sensitivity neutral, afterglow residue 150.
    pub fn baseline() -> Self {
        Self {
            arousal: 0,
            pleasure_charge: 0,
            sensitivity: 1000,
            floor: 0,
            peak_input: 0,
            dose: 0,
            phase: ArousalPhase::Baseline,
            culmination_latch: false,
            episode_id: 0,
            phase_ticks: 0,
            afterglow_level: 150,
        }
    }
}

/// The read/verb surface the Playroom dashboard and the
/// Remove All act bind to. Implemented by [`StubEngine`] in
/// T4.1 and by the real engine in T4.2.
pub trait ArousalEngineView {
    /// The current §5.2 state vector, read — never estimated.
    fn state_vector(&self) -> ArousalStateVector;

    /// Per-device effective input (i_j, §5.3) in permille, by
    /// item id, sorted by id.
    fn per_device_effective_input(&self) -> Vec<(String, u32)>;

    /// Per-item floor contributions (§5.4), by item id,
    /// sorted by id.
    fn floor_contributions(&self) -> Vec<(String, u32)>;

    /// Whether an ache mirror session is active. Remove All
    /// issues its stop only when this is true (§3.4 step 2).
    fn mirror_active(&self) -> bool;

    /// §3.4 step 1: latch local stimulation to zero,
    /// synchronously, before any bookkeeping.
    fn latch_stimulation_zero(&mut self);

    /// §5.8 reset: the state vector returns to baseline.
    fn reset_to_baseline(&mut self);
}

/// STUB ENGINE — a labelled stand-in behind
/// [`ArousalEngineView`] for T4.1's surfaces. It holds a
/// caller-set state vector and reports it verbatim; it runs
/// no integration law. It exists so the dashboard, the worn
/// set, and the Remove All act can be built and gated against
/// the §5.2 interface before T4.2's engine lands.
#[derive(Clone, Debug, PartialEq)]
pub struct StubEngine {
    state: ArousalStateVector,
    inputs: Vec<(String, u32)>,
    contributions: Vec<(String, u32)>,
    /// Whether stimulation has been latched to zero (§3.4
    /// step 1) since the last reset.
    pub stimulation_latched_zero: bool,
}

impl StubEngine {
    /// A stub at baseline.
    pub fn new() -> Self {
        Self {
            state: ArousalStateVector::baseline(),
            inputs: Vec::new(),
            contributions: Vec::new(),
            stimulation_latched_zero: false,
        }
    }

    /// STUB ONLY: sets the held state vector (test and
    /// preview scaffolding; the real engine's vector is
    /// integrated, never set).
    pub fn set_state(&mut self, state: ArousalStateVector) {
        self.state = state;
    }

    /// STUB ONLY: sets the reported per-device inputs.
    pub fn set_inputs(&mut self, inputs: Vec<(String, u32)>) {
        self.inputs = inputs;
    }

    /// STUB ONLY: sets the reported floor contributions.
    pub fn set_contributions(&mut self, contributions: Vec<(String, u32)>) {
        self.contributions = contributions;
    }
}

impl Default for StubEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl ArousalEngineView for StubEngine {
    fn state_vector(&self) -> ArousalStateVector {
        self.state
    }

    fn per_device_effective_input(&self) -> Vec<(String, u32)> {
        let mut out = self.inputs.clone();
        out.sort();
        out
    }

    fn floor_contributions(&self) -> Vec<(String, u32)> {
        let mut out = self.contributions.clone();
        out.sort();
        out
    }

    fn mirror_active(&self) -> bool {
        // The stub never mirrors; mirroring is T4.5 (§7).
        false
    }

    fn latch_stimulation_zero(&mut self) {
        self.stimulation_latched_zero = true;
        self.inputs.clear();
    }

    fn reset_to_baseline(&mut self) {
        self.state = ArousalStateVector::baseline();
        self.inputs.clear();
        self.contributions.clear();
        self.stimulation_latched_zero = false;
    }
}
