//! Torogao's hardware-acceleration policy layer.
//!
//! This crate owns everything about acceleration that is
//! *policy* rather than rendering: the capability report the
//! app collects at startup ([`capability`]), the user's
//! acceleration preferences as a typed, validated value with a
//! persisted record shape ([`settings`]), the bounded probes
//! that gather capability facts from the machine ([`probe`]),
//! and the determinism gate harness every record-feeding
//! offload workload must pass before it may ship
//! ([`determinism`]).
//!
//! It deliberately contains no renderer and no wgpu dependency:
//! the app (RP4/RP5) enumerates its own adapters, feeds them
//! into the same report shape, and builds its wgpu instance
//! from the [`capability::EffectivePlan`] this crate evaluates.
//! Design of record: `ACCELERATION-DESIGN.md` in the
//! acceleration program directory.

#![forbid(unsafe_code)]

pub mod capability;
pub mod determinism;
pub mod limits;
pub mod probe;
pub mod settings;

pub use capability::{
    AdapterBackend, AdapterRecord, CapabilityReport, ComputeBackend, ComputePlan, CudaRecord,
    Degradation, DegradationSeverity, DeviceKind, EffectivePlan, EffectiveRenderBackend, GlRecord,
    IcdRecord, IcdVendor, OpenClPlatform, RenderPlan, SessionKind, SessionRecord, evaluate,
};
pub use determinism::{
    BlockedSprings, DeterminismError, GateReport, ScalarSprings, WorkloadPath, run_gate,
    spring_demo_input,
};
pub use probe::{ProbeFindings, ProbeKind, ProbeStatus, ProbeSummary, collect_findings};
pub use settings::{
    AccelSettings, AccelSettingsRecord, ChangeClass, ComputeIcdPin, PowerPref, RenderBackendPref,
    SettingsError, classify_change,
};

/// Bounds untrusted or driver-supplied text to the report field
/// limit, on a char boundary. Driver strings are display data,
/// never control input; bounding them keeps a hostile or broken
/// driver from inflating reports, logs, or error messages.
pub(crate) fn bounded_text(text: &str) -> String {
    text.chars().take(limits::REPORT_TEXT_BYTES_MAX).collect()
}
