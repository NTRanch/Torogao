//! Gate tests for the torogao-coin shell binding. Half
//! validation (the binding is transparent: streamed inputs
//! produce exactly the systems layer's outcomes), half
//! adversarial (out-of-state inputs and hostile samples are
//! refused and leave the bound state untouched).

use torogao_coin::dynamics::StatTriple;
use torogao_coin::shell::{CoinShell, run_skeleton_sessions};
use torogao_coin::trajectory::{FIXTURE_DT_SECONDS, fixture_samples, run_measured_fixture};
use torogao_coin::{CoinError, Session, measured_dynamics};

const EPS: f64 = 1e-9;

fn approx(a: f64, b: f64) -> bool {
    (a - b).abs() <= EPS * (1.0 + a.abs() + b.abs())
}

fn prepared_shell() -> CoinShell {
    let mut shell = CoinShell::new(measured_dynamics().unwrap(), 9 * 60).unwrap();
    shell.prepare(13).unwrap();
    shell.prepare(8).unwrap();
    shell
}

// ---------- validation ----------

#[test]
fn streamed_fixture_matches_the_batch_attempt_and_the_fixture_hash() {
    // The same preparation and stream, two routes: streamed
    // through the binding, and batched through Session. The
    // outcomes must be identical, and the streamed attempt's
    // trajectory hash must equal the canonical fixture hash
    // (the S7 comparison value).
    let mut shell = prepared_shell();
    shell.begin_attempt(FIXTURE_DT_SECONDS).unwrap();
    let mut verdict = None;
    for x in fixture_samples() {
        if let Some(v) = shell.pointer_sample(x).unwrap() {
            verdict = Some(v);
            break;
        }
    }
    let verdict = verdict.expect("fixture stream resolves");
    assert!(verdict.win);
    let mut batch = Session::new(measured_dynamics().unwrap(), 9 * 60).unwrap();
    batch.apply_prep_action(13).unwrap();
    batch.apply_prep_action(8).unwrap();
    let batch_outcome = batch
        .run_attempt(&fixture_samples(), FIXTURE_DT_SECONDS)
        .unwrap();
    assert_eq!(verdict.outcome, batch_outcome);
    assert_eq!(
        verdict.trajectory_hash,
        run_measured_fixture().unwrap().trajectory_hash
    );
    assert_eq!(shell.level(), 1);
    assert_eq!(shell.clock_minute(), 595);
    assert!(!shell.attempt_active());
}

#[test]
fn prepare_through_the_shell_advances_clock_and_prep() {
    let mut shell = CoinShell::new(measured_dynamics().unwrap(), 9 * 60).unwrap();
    let spent = shell.prepare(13).unwrap();
    assert_eq!(spent, 20);
    assert_eq!(shell.clock_minute(), 9 * 60 + 20);
    // (0.2, 0.02, 0.16) * 0.35 through the product rule.
    assert!(approx(shell.prep().hp, 0.07));
    assert!(approx(shell.prep().mp, 0.007));
    assert!(approx(shell.prep().sp, 0.056));
}

#[test]
fn message_stats_through_the_shell_move_the_prep_triple() {
    let mut shell = CoinShell::new(measured_dynamics().unwrap(), 9 * 60).unwrap();
    shell
        .message_stats(StatTriple {
            hp: 100.0,
            mp: 0.0,
            sp: 0.0,
        })
        .unwrap();
    // 100 / 100 * 0.65 on the message path.
    assert!(approx(shell.prep().hp, 0.65));
}

#[test]
fn skeleton_sessions_match_their_scripts() {
    let sessions = run_skeleton_sessions(measured_dynamics().unwrap()).unwrap();
    assert_eq!(sessions.len(), 3);
    let fixture = &sessions[0];
    assert_eq!(fixture.name, "fixture-swing");
    assert_eq!(fixture.win, Some(true));
    assert_eq!(fixture.level_after, 1);
    assert_eq!(fixture.clock_minute, 595);
    assert!(!fixture.can_save);
    assert_eq!(
        fixture.trajectory_hash,
        Some(run_measured_fixture().unwrap().trajectory_hash)
    );
    let timeout = &sessions[1];
    assert_eq!(timeout.name, "one-sided-timeout");
    assert_eq!(timeout.win, Some(false));
    assert_eq!(timeout.level_after, 0);
    assert_eq!(timeout.clock_minute, 9 * 60);
    assert!(timeout.trajectory_hash.is_some());
    let day = &sessions[2];
    assert_eq!(day.name, "day-frame-save");
    assert_eq!(day.win, None);
    assert_eq!(day.trajectory_hash, None);
    assert!(day.can_save);
    assert_eq!(day.clock_minute, 9 * 60 + 16 * 35);
    // Twin-run determinism: the same scripts hash identically.
    let rerun = run_skeleton_sessions(measured_dynamics().unwrap()).unwrap();
    assert_eq!(sessions, rerun);
}

#[test]
fn attempt_gauge_is_visible_while_the_window_is_open() {
    let mut shell = prepared_shell();
    assert_eq!(shell.attempt_gauge(), None);
    shell.begin_attempt(FIXTURE_DT_SECONDS).unwrap();
    assert!(shell.attempt_active());
    assert_eq!(shell.attempt_gauge(), Some(0.0));
    for x in fixture_samples().into_iter().take(5) {
        assert!(shell.pointer_sample(x).unwrap().is_none());
    }
    assert!(shell.attempt_gauge().is_some());
}

#[test]
fn save_gate_opens_through_shell_preparation() {
    let mut shell = CoinShell::new(measured_dynamics().unwrap(), 15 * 60).unwrap();
    assert!(!shell.can_save());
    for index in 0..15 {
        shell.prepare(index).unwrap();
    }
    assert_eq!(shell.clock_minute(), 15 * 60 + 320);
    assert!(shell.can_save());
}

// ---------- adversarial ----------

#[test]
fn pointer_sample_with_no_attempt_is_refused() {
    let mut shell = prepared_shell();
    let before = shell.state_hash();
    assert!(matches!(
        shell.pointer_sample(1.0),
        Err(CoinError::InvalidStep { .. })
    ));
    assert_eq!(shell.state_hash(), before);
    assert!(!shell.attempt_active());
}

#[test]
fn second_begin_attempt_is_refused_and_the_first_survives() {
    let mut shell = prepared_shell();
    shell.begin_attempt(FIXTURE_DT_SECONDS).unwrap();
    assert!(matches!(
        shell.begin_attempt(FIXTURE_DT_SECONDS),
        Err(CoinError::InvalidStep { .. })
    ));
    // The first attempt is still the open one and resolves.
    let mut verdict = None;
    for x in fixture_samples() {
        if let Some(v) = shell.pointer_sample(x).unwrap() {
            verdict = Some(v);
            break;
        }
    }
    assert!(verdict.expect("first attempt resolves").win);
}

#[test]
fn preparation_inputs_are_refused_during_an_attempt() {
    let mut shell = prepared_shell();
    shell.begin_attempt(FIXTURE_DT_SECONDS).unwrap();
    let before = shell.state_hash();
    assert!(matches!(
        shell.prepare(0),
        Err(CoinError::InvalidStep { .. })
    ));
    assert!(matches!(
        shell.message_stats(StatTriple::ZERO),
        Err(CoinError::InvalidStep { .. })
    ));
    assert_eq!(shell.state_hash(), before);
    shell.cancel_attempt();
    // Between attempts the same inputs are admissible again.
    assert!(shell.prepare(0).is_ok());
}

#[test]
fn non_finite_sample_is_refused_and_the_attempt_is_unchanged() {
    let mut shell = prepared_shell();
    shell.begin_attempt(FIXTURE_DT_SECONDS).unwrap();
    let before = shell.state_hash();
    assert!(shell.pointer_sample(f64::NAN).is_err());
    assert!(shell.pointer_sample(f64::INFINITY).is_err());
    assert_eq!(shell.state_hash(), before);
    // The attempt still resolves with the clean fixture
    // outcome: the refused samples banked and buffered
    // nothing.
    let mut verdict = None;
    for x in fixture_samples() {
        if let Some(v) = shell.pointer_sample(x).unwrap() {
            verdict = Some(v);
            break;
        }
    }
    let verdict = verdict.expect("attempt resolves after refusals");
    assert!(verdict.win);
    assert!(approx(verdict.outcome.points, 172.5));
}

#[test]
fn invalid_dt_is_refused_and_opens_nothing() {
    let mut shell = prepared_shell();
    for dt in [0.0, -0.5, 2.0, f64::NAN, f64::INFINITY] {
        assert!(
            matches!(shell.begin_attempt(dt), Err(CoinError::InvalidStep { .. })),
            "dt {dt}"
        );
        assert!(!shell.attempt_active());
    }
}

#[test]
fn cancel_attempt_leaves_the_session_untouched() {
    let mut shell = prepared_shell();
    let before = shell.state_hash();
    shell.begin_attempt(FIXTURE_DT_SECONDS).unwrap();
    for x in fixture_samples().into_iter().take(5) {
        assert!(shell.pointer_sample(x).unwrap().is_none());
    }
    assert!(shell.cancel_attempt());
    assert!(!shell.cancel_attempt());
    assert_eq!(shell.state_hash(), before);
    assert_eq!(shell.level(), 0);
    assert_eq!(shell.clock_minute(), 595);
}
