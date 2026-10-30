//! Determinism gate tests (design section 3): the harness
//! passes two independent implementations, and catches
//! sabotage, instability, oversize, and malformed inputs with
//! typed failures.

use std::cell::Cell;

use torogao_accel::determinism::{DeterminismError, WorkloadPath, run_gate, spring_demo_input};
use torogao_accel::{BlockedSprings, ScalarSprings};

#[test]
fn gate_passes_scalar_against_blocked_on_the_demo_input() {
    let input = spring_demo_input();
    let report =
        run_gate("spring-chain", &input, &ScalarSprings, &BlockedSprings).expect("gate passes");
    assert!(report.byte_identical);
    assert_eq!(report.reference_digest, report.candidate_digest);
    assert_eq!(report.reference_path, "spring-chain/scalar");
    assert_eq!(report.candidate_path, "spring-chain/blocked");
    assert_eq!(report.runs_per_path, 3);
}

#[test]
fn gate_digests_are_stable_across_gate_runs() {
    let input = spring_demo_input();
    let first = run_gate("spring-chain", &input, &ScalarSprings, &BlockedSprings)
        .expect("first gate passes");
    let second = run_gate("spring-chain", &input, &ScalarSprings, &BlockedSprings)
        .expect("second gate passes");
    assert_eq!(first.reference_digest, second.reference_digest);
}

/// A candidate that corrupts its last output byte: the gate
/// must catch it and locate the difference.
struct FlipLastByte;

impl WorkloadPath for FlipLastByte {
    fn name(&self) -> &str {
        "spring-chain/sabotaged"
    }

    fn run(&self, input: &[u8]) -> Result<Vec<u8>, DeterminismError> {
        let mut out = BlockedSprings.run(input)?;
        if let Some(last) = out.last_mut() {
            *last = last.wrapping_add(1);
        }
        Ok(out)
    }
}

#[test]
fn gate_catches_a_sabotaged_candidate_at_the_right_offset() {
    let input = spring_demo_input();
    let err = run_gate("spring-chain", &input, &ScalarSprings, &FlipLastByte)
        .expect_err("gate must fail");
    match err {
        DeterminismError::Mismatch {
            reference_digest,
            candidate_digest,
            first_diff_offset,
        } => {
            assert_ne!(reference_digest, candidate_digest);
            let output_len = BlockedSprings.run(&input).expect("path runs").len();
            assert_eq!(first_diff_offset, Some(output_len - 1));
        }
        other => panic!("wrong failure: {other:?}"),
    }
    assert_eq!(
        DeterminismError::Mismatch {
            reference_digest: String::new(),
            candidate_digest: String::new(),
            first_diff_offset: None,
        }
        .code(),
        "TG-ACCEL-DET-MISMATCH"
    );
}

/// A candidate that alternates its output between runs: the
/// gate must refuse it as unstable before any comparison.
struct Alternating {
    calls: Cell<usize>,
}

impl WorkloadPath for Alternating {
    fn name(&self) -> &str {
        "spring-chain/alternating"
    }

    fn run(&self, input: &[u8]) -> Result<Vec<u8>, DeterminismError> {
        let mut out = BlockedSprings.run(input)?;
        let call = self.calls.get();
        self.calls.set(call + 1);
        if call % 2 == 1 {
            if let Some(first) = out.first_mut() {
                *first = first.wrapping_add(1);
            }
        }
        Ok(out)
    }
}

#[test]
fn gate_refuses_an_unstable_candidate() {
    let input = spring_demo_input();
    let candidate = Alternating {
        calls: Cell::new(0),
    };
    let err =
        run_gate("spring-chain", &input, &ScalarSprings, &candidate).expect_err("gate must refuse");
    assert!(matches!(err, DeterminismError::Unstable { .. }));
    assert_eq!(err.code(), "TG-ACCEL-DET-UNSTABLE");
}

#[test]
fn gate_refuses_oversized_input() {
    let input = vec![0_u8; torogao_accel::limits::WORKLOAD_INPUT_BYTES_MAX + 1];
    let err = run_gate("spring-chain", &input, &ScalarSprings, &BlockedSprings)
        .expect_err("gate must refuse");
    assert!(matches!(err, DeterminismError::InputTooLarge { .. }));
}

#[test]
fn spring_paths_refuse_malformed_inputs_typed() {
    // Shorter than the header.
    let err = ScalarSprings.run(&[0_u8; 4]).expect_err("must refuse");
    assert!(matches!(err, DeterminismError::InputMalformed { .. }));

    // Node count of zero.
    let mut input = spring_demo_input();
    input[0..4].copy_from_slice(&0_u32.to_le_bytes());
    let err = BlockedSprings.run(&input).expect_err("must refuse");
    assert!(matches!(err, DeterminismError::InputMalformed { .. }));

    // Truncated body: length no longer matches the node count.
    let mut input = spring_demo_input();
    input.truncate(input.len() - 8);
    let err = ScalarSprings.run(&input).expect_err("must refuse");
    assert!(matches!(err, DeterminismError::InputMalformed { .. }));
}

#[test]
fn zero_stiffness_holds_positions_exactly() {
    // With stiffness zero and zero velocities, nothing moves:
    // the output is the input state verbatim.
    let node_count: u32 = 4;
    let mut input = Vec::new();
    input.extend_from_slice(&node_count.to_le_bytes());
    input.extend_from_slice(&10_u32.to_le_bytes());
    input.extend_from_slice(&0_i64.to_le_bytes());
    let mut expected = Vec::new();
    for node in 0..node_count {
        let position = i64::from(node) * 1_000 - 500;
        input.extend_from_slice(&position.to_le_bytes());
        input.extend_from_slice(&0_i64.to_le_bytes());
        expected.extend_from_slice(&position.to_le_bytes());
        expected.extend_from_slice(&0_i64.to_le_bytes());
    }
    assert_eq!(ScalarSprings.run(&input), Ok(expected.clone()));
    assert_eq!(BlockedSprings.run(&input), Ok(expected));
}
