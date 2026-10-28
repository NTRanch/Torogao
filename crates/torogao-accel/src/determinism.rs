//! The determinism gate harness (design section 3.2).
//!
//! The template every record-feeding offload workload must
//! pass before its accelerated path may ship: the workload
//! declares its input bytes and two paths — a CPU reference and
//! a candidate — and [`run_gate`] requires each path to be
//! stable against itself over repeated runs, then requires the
//! candidate's output to be **byte-identical** to the
//! reference's. Digests are reported; "close" is a failing
//! grade, by law.
//!
//! The bundled workload is a fixed-point spring chain with the
//! shape of the engine's physics candidate (design section
//! 2.6): integer-only arithmetic, because GPU floating-point
//! summation order and FMA contraction differ across vendors
//! and drivers, while integer semantics are portable. The two
//! bundled paths are independent implementations (scalar and
//! blocked traversal) so the harness is exercised honestly on
//! CPU-only machines; the primo GPU run of a WGSL candidate
//! through this same gate is the named follow-up (design
//! section 9).

use std::fmt;

use sha2::{Digest, Sha256};

use crate::limits;

/// Why a workload path or the gate itself refused or failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeterminismError {
    /// Input exceeded [`limits::WORKLOAD_INPUT_BYTES_MAX`].
    InputTooLarge { bytes: usize },
    /// Input bytes do not form a valid workload input.
    InputMalformed { reason: &'static str },
    /// A path returned an error of its own.
    PathFailed { path: String, reason: String },
    /// A path disagreed with itself across repeated runs; it
    /// is not deterministic enough to compare.
    Unstable { path: String },
    /// The candidate's output differs from the reference's.
    Mismatch {
        /// SHA-256 hex of the reference output.
        reference_digest: String,
        /// SHA-256 hex of the candidate output.
        candidate_digest: String,
        /// Offset of the first differing byte, when the outputs
        /// share a prefix (also `Some(min_len)` when one is a
        /// strict prefix of the other).
        first_diff_offset: Option<usize>,
    },
}

impl DeterminismError {
    /// The stable refusal/failure code for this error.
    pub fn code(&self) -> &'static str {
        match self {
            Self::InputTooLarge { .. } => "TG-ACCEL-DET-INPUT-TOO-LARGE",
            Self::InputMalformed { .. } => "TG-ACCEL-DET-INPUT-MALFORMED",
            Self::PathFailed { .. } => "TG-ACCEL-DET-PATH-FAILED",
            Self::Unstable { .. } => "TG-ACCEL-DET-UNSTABLE",
            Self::Mismatch { .. } => "TG-ACCEL-DET-MISMATCH",
        }
    }
}

impl fmt::Display for DeterminismError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InputTooLarge { bytes } => {
                write!(f, "workload input of {bytes} bytes exceeds the bound")
            }
            Self::InputMalformed { reason } => write!(f, "malformed workload input: {reason}"),
            Self::PathFailed { path, reason } => write!(f, "path {path} failed: {reason}"),
            Self::Unstable { path } => {
                write!(f, "path {path} is not stable across repeated runs")
            }
            Self::Mismatch {
                first_diff_offset, ..
            } => write!(
                f,
                "candidate output differs from reference (first difference at {first_diff_offset:?})"
            ),
        }
    }
}

impl std::error::Error for DeterminismError {}

/// SHA-256 of `bytes`, lowercase hex. Digests name outputs in
/// gate reports; equality is always decided on the full bytes,
/// never on digests alone.
pub fn digest_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

/// One executable path of a workload under the gate.
pub trait WorkloadPath {
    /// Display name, e.g. "spring-chain/scalar".
    fn name(&self) -> &str;
    /// Runs the path over the workload input.
    fn run(&self, input: &[u8]) -> Result<Vec<u8>, DeterminismError>;
}

/// A passed gate: both paths stable, outputs byte-identical.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GateReport {
    /// Workload name the gate ran under.
    pub workload: String,
    /// Reference path name.
    pub reference_path: String,
    /// Candidate path name.
    pub candidate_path: String,
    /// SHA-256 hex of the (identical) reference output.
    pub reference_digest: String,
    /// SHA-256 hex of the (identical) candidate output.
    pub candidate_digest: String,
    /// Always true in a report; failures are errors, not
    /// reports with a false flag.
    pub byte_identical: bool,
    /// Runs per path the gate performed.
    pub runs_per_path: usize,
}

/// Runs the determinism gate: each path
/// [`limits::DETERMINISM_RUNS_PER_PATH`] times (self-stability
/// first), then a full byte comparison across paths.
pub fn run_gate(
    workload: &str,
    input: &[u8],
    reference: &dyn WorkloadPath,
    candidate: &dyn WorkloadPath,
) -> Result<GateReport, DeterminismError> {
    if input.len() > limits::WORKLOAD_INPUT_BYTES_MAX {
        return Err(DeterminismError::InputTooLarge { bytes: input.len() });
    }
    let reference_output = run_stable(reference, input)?;
    let candidate_output = run_stable(candidate, input)?;
    if reference_output != candidate_output {
        return Err(DeterminismError::Mismatch {
            reference_digest: digest_hex(&reference_output),
            candidate_digest: digest_hex(&candidate_output),
            first_diff_offset: first_diff(&reference_output, &candidate_output),
        });
    }
    Ok(GateReport {
        workload: workload.to_owned(),
        reference_path: reference.name().to_owned(),
        candidate_path: candidate.name().to_owned(),
        reference_digest: digest_hex(&reference_output),
        candidate_digest: digest_hex(&candidate_output),
        byte_identical: true,
        runs_per_path: limits::DETERMINISM_RUNS_PER_PATH,
    })
}

fn run_stable(path: &dyn WorkloadPath, input: &[u8]) -> Result<Vec<u8>, DeterminismError> {
    let first = path.run(input)?;
    for _ in 1..limits::DETERMINISM_RUNS_PER_PATH {
        if path.run(input)? != first {
            return Err(DeterminismError::Unstable {
                path: path.name().to_owned(),
            });
        }
    }
    Ok(first)
}

fn first_diff(a: &[u8], b: &[u8]) -> Option<usize> {
    if let Some(offset) = a.iter().zip(b.iter()).position(|(x, y)| x != y) {
        return Some(offset);
    }
    if a.len() != b.len() {
        return Some(a.len().min(b.len()));
    }
    None
}

/// Fixed-point scale of the spring chain: values are i64 with
/// 16 fractional bits.
const FIXED_FRACTION_BITS: u32 = 16;

/// Header size of a spring-chain input: node count (u32),
/// step count (u32), stiffness (i64 fixed-point).
const SPRING_HEADER_BYTES: usize = 16;

/// Bytes per node in a spring-chain input/output: position
/// (i64) + velocity (i64).
const SPRING_NODE_BYTES: usize = 16;

/// Block width of the blocked traversal path.
const SPRING_BLOCK_NODES: usize = 8;

struct SpringSpec {
    steps: u32,
    stiffness: i64,
    positions: Vec<i64>,
    velocities: Vec<i64>,
}

fn parse_spring_input(input: &[u8]) -> Result<SpringSpec, DeterminismError> {
    let malformed = |reason: &'static str| DeterminismError::InputMalformed { reason };
    if input.len() < SPRING_HEADER_BYTES {
        return Err(malformed("input shorter than the spring header"));
    }
    // Index-based reads: every offset used below is proven in
    // range by the length checks around them, so no fallible
    // conversion is needed.
    let read_u32 =
        |at: usize| u32::from_le_bytes([input[at], input[at + 1], input[at + 2], input[at + 3]]);
    let read_i64 = |at: usize| {
        i64::from_le_bytes([
            input[at],
            input[at + 1],
            input[at + 2],
            input[at + 3],
            input[at + 4],
            input[at + 5],
            input[at + 6],
            input[at + 7],
        ])
    };
    let node_count = read_u32(0);
    let steps = read_u32(4);
    if node_count == 0 || node_count > limits::SPRING_NODES_MAX {
        return Err(malformed("node count outside its bound"));
    }
    if steps == 0 || steps > limits::SPRING_STEPS_MAX {
        return Err(malformed("step count outside its bound"));
    }
    let expected = SPRING_HEADER_BYTES + node_count as usize * SPRING_NODE_BYTES;
    if input.len() != expected {
        return Err(malformed("input length does not match the node count"));
    }
    let stiffness = read_i64(8);
    let mut positions = Vec::with_capacity(node_count as usize);
    let mut velocities = Vec::with_capacity(node_count as usize);
    for node in 0..node_count as usize {
        let at = SPRING_HEADER_BYTES + node * SPRING_NODE_BYTES;
        positions.push(read_i64(at));
        velocities.push(read_i64(at + 8));
    }
    Ok(SpringSpec {
        steps,
        stiffness,
        positions,
        velocities,
    })
}

fn serialize_spring_state(positions: &[i64], velocities: &[i64]) -> Vec<u8> {
    let mut out = Vec::with_capacity(positions.len() * SPRING_NODE_BYTES);
    for (position, velocity) in positions.iter().zip(velocities.iter()) {
        out.extend_from_slice(&position.to_le_bytes());
        out.extend_from_slice(&velocity.to_le_bytes());
    }
    out
}

/// One spring force in fixed point. The product is computed in
/// i128 so no intermediate can overflow; the final narrowing is
/// saturating, which is a defined, portable semantics (unlike
/// float rounding, it is identical on every machine).
fn spring_force(stiffness: i64, displacement: i64) -> i64 {
    let product = (stiffness as i128) * (displacement as i128);
    let scaled = product >> FIXED_FRACTION_BITS;
    match i64::try_from(scaled) {
        Ok(value) => value,
        Err(_) if scaled < 0 => i64::MIN,
        Err(_) => i64::MAX,
    }
}

/// Force on an interior node from its two neighbours, Jacobi
/// style: reads only the previous step's positions.
fn node_force(spec: &SpringSpec, positions: &[i64], node: usize) -> i64 {
    let left = positions[node - 1];
    let right = positions[node + 1];
    let here = positions[node];
    spring_force(spec.stiffness, left - here)
        .saturating_add(spring_force(spec.stiffness, right - here))
}

/// The scalar spring-chain path: one node at a time, in index
/// order. The reference implementation.
pub struct ScalarSprings;

impl WorkloadPath for ScalarSprings {
    fn name(&self) -> &str {
        "spring-chain/scalar"
    }

    fn run(&self, input: &[u8]) -> Result<Vec<u8>, DeterminismError> {
        let spec = parse_spring_input(input)?;
        let mut positions = spec.positions.clone();
        let mut velocities = spec.velocities.clone();
        let mut next_positions = positions.clone();
        let mut next_velocities = velocities.clone();
        let last = positions.len() - 1;
        for _ in 0..spec.steps {
            // Endpoints are pinned: they are never written, so
            // their recorded position and velocity carry through
            // every step unchanged.
            for node in 1..last {
                let force = node_force(&spec, &positions, node);
                next_velocities[node] = velocities[node].saturating_add(force);
                next_positions[node] = positions[node].saturating_add(next_velocities[node]);
            }
            std::mem::swap(&mut positions, &mut next_positions);
            std::mem::swap(&mut velocities, &mut next_velocities);
        }
        Ok(serialize_spring_state(&positions, &velocities))
    }
}

/// The blocked spring-chain path: the same mathematics with a
/// blocked traversal — forces for a block of nodes are computed
/// into scratch before any of them is applied. Included so the
/// gate's two paths are independent implementations whose
/// agreement is evidence, not a tautology.
pub struct BlockedSprings;

impl WorkloadPath for BlockedSprings {
    fn name(&self) -> &str {
        "spring-chain/blocked"
    }

    fn run(&self, input: &[u8]) -> Result<Vec<u8>, DeterminismError> {
        let spec = parse_spring_input(input)?;
        let mut positions = spec.positions.clone();
        let mut velocities = spec.velocities.clone();
        let mut next_positions = positions.clone();
        let mut next_velocities = velocities.clone();
        let last = positions.len() - 1;
        let mut block_forces = [0_i64; SPRING_BLOCK_NODES];
        for _ in 0..spec.steps {
            let mut block_start = 1_usize;
            while block_start < last {
                let block_end = (block_start + SPRING_BLOCK_NODES).min(last);
                for (slot, node) in (block_start..block_end).enumerate() {
                    block_forces[slot] = node_force(&spec, &positions, node);
                }
                for (slot, node) in (block_start..block_end).enumerate() {
                    next_velocities[node] = velocities[node].saturating_add(block_forces[slot]);
                    next_positions[node] = positions[node].saturating_add(next_velocities[node]);
                }
                block_start = block_end;
            }
            std::mem::swap(&mut positions, &mut next_positions);
            std::mem::swap(&mut velocities, &mut next_velocities);
        }
        Ok(serialize_spring_state(&positions, &velocities))
    }
}

/// A canonical spring-chain input for gate exercises and for
/// future candidates to be measured against: 64 nodes, 480
/// steps, stiffness 0.25 in fixed point, a deterministic
/// plucked initial shape, zero initial velocities.
pub fn spring_demo_input() -> Vec<u8> {
    let node_count: u32 = 64;
    let steps: u32 = 480;
    let stiffness: i64 = 1 << (FIXED_FRACTION_BITS - 2);
    let mut input =
        Vec::with_capacity(SPRING_HEADER_BYTES + node_count as usize * SPRING_NODE_BYTES);
    input.extend_from_slice(&node_count.to_le_bytes());
    input.extend_from_slice(&steps.to_le_bytes());
    input.extend_from_slice(&stiffness.to_le_bytes());
    for node in 0..node_count {
        let shape = ((i64::from(node) * 37) % 101) - 50;
        let position = shape << 12;
        input.extend_from_slice(&position.to_le_bytes());
        input.extend_from_slice(&0_i64.to_le_bytes());
    }
    input
}
