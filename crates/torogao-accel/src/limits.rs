//! Named bounds for the acceleration layer.
//!
//! Every limit carries its unit in its name. Nothing in this
//! crate loops, allocates, waits, or stores without one of
//! these bounds (or a caller's own stricter bound) applying.

/// Wall-clock budget for a single subprocess probe. Capability
/// probing must never be the reason startup hangs.
pub const PROBE_DEADLINE_MS: u64 = 5_000;

/// Captured stdout budget for a single subprocess probe.
pub const PROBE_OUTPUT_BYTES_MAX: usize = 1_048_576;

/// Longest text value (device name, driver string, reason)
/// stored in a report, plan, or error. Driver-supplied strings
/// are untrusted display data.
pub const REPORT_TEXT_BYTES_MAX: usize = 256;

/// Most adapters a capability report will carry. A machine
/// enumerating more devices than this is broken or hostile;
/// the report truncates rather than growing without bound.
pub const ADAPTERS_MAX: usize = 64;

/// Most OpenCL platforms a capability report will carry.
pub const OPENCL_PLATFORMS_MAX: usize = 32;

/// Most degradations one plan will carry. Degradations are
/// rendered to the user; a plan needing more than this is a
/// bug, and truncation keeps the UI honest about volume.
pub const DEGRADATIONS_MAX: usize = 32;

/// Largest input the determinism gate harness will run, in
/// bytes. Workloads with bigger inputs chunk their comparisons
/// under their own brief, not by raising this bound silently.
pub const WORKLOAD_INPUT_BYTES_MAX: usize = 1_048_576;

/// How many times the gate runs each path. A path must be
/// stable against itself this many times before it is compared
/// against the other path.
pub const DETERMINISM_RUNS_PER_PATH: usize = 3;

/// Spring-chain workload bound: most nodes in one input.
pub const SPRING_NODES_MAX: u32 = 4_096;

/// Spring-chain workload bound: most steps in one input.
pub const SPRING_STEPS_MAX: u32 = 100_000;
