//! The conformance suite against witness (a): the AUTH-G
//! interpreter hosting the Changr-parity fixture.
//!
//! Half validation (the contract holds over a real AUTH-G
//! game), half adversarial (cross-core saves refuse, a wrong
//! seed refuses, a tampered envelope refuses).

use torogao_runtime::Session;
use torogao_runtime::contract::check_core;
use torogao_runtime::corpus::{INTERPRETER_SAVE_AT, interpreter_corpus};
use torogao_runtime::host::author_game::{InterpreterCore, changr_fixture_core};

fn make_core() -> InterpreterCore {
    changr_fixture_core(7).expect("fixture core constructs")
}

#[test]
fn interpreter_witness_passes_the_contract_suite() {
    let report = check_core(&make_core, &interpreter_corpus(), INTERPRETER_SAVE_AT);
    assert!(report.all_passed(), "contract suite:\n{}", report.summary());
    // The fixture's own semantics, seen through the contract:
    // control was applied on tick 1 and refreshed on tick 20,
    // so amy's control_level is 1 at the end of the corpus.
    let mut session = Session::new(make_core());
    for commands in &interpreter_corpus() {
        session.step(commands);
    }
    let state = session.core().state();
    assert_eq!(state.tick, 28);
    assert_eq!(state.day, 4);
    let key = torogao_author::game::state::StatKey {
        character: Some("amy".to_string()),
        stat: "control_level".to_string(),
    };
    assert_eq!(state.stats[&key], 1, "control holds at corpus end");
    assert_eq!(
        state.lifetimes[&key], 5,
        "the tick-20 refresh re-armed the full 6-day window; the day-4 rollover at tick 25 consumed one day"
    );
}

#[test]
fn golden_chain_tip_and_save_bytes_are_pinned() {
    // Golden pins (R0's golden-save discipline applied to the
    // runtime): the corpus chain tip and the checkpoint save's
    // digest are pinned values, computed from the landed
    // implementation and recorded in the R1 record. Any drift
    // in the interpreter, the envelope format, or the corpus
    // fails here instead of silently changing what a saved run
    // replays to.
    let report = check_core(&make_core, &interpreter_corpus(), INTERPRETER_SAVE_AT);
    let tip = report
        .reference_chain
        .last()
        .expect("chain is non-empty")
        .as_str()
        .to_string();
    assert_eq!(tip, GOLDEN_CHAIN_TIP, "chain tip drifted");

    let mut head = Session::new(make_core());
    for commands in &interpreter_corpus()[..INTERPRETER_SAVE_AT] {
        head.step(commands);
    }
    let text = head.save().expect("save at the checkpoint");
    let digest = torogao_runtime::Hash::sha256(text.as_bytes());
    assert_eq!(digest.as_str(), GOLDEN_SAVE_DIGEST, "save bytes drifted");
}

/// Pinned after the first green run; see the R1 record.
const GOLDEN_CHAIN_TIP: &str = "999d0574354d25529b79d97a11b6b695ce8a732c4f0318b8a24e0038136a6fe8";
/// Pinned after the first green run; see the R1 record.
const GOLDEN_SAVE_DIGEST: &str = "cad61ee82ccbdd36c1984c48fcc50e116c50cace1e84c5907505ea6db35a7fff";

#[test]
fn save_from_a_different_seed_refuses() {
    let mut head = Session::new(make_core());
    for commands in &interpreter_corpus()[..INTERPRETER_SAVE_AT] {
        head.step(commands);
    }
    let text = head.save().expect("save");
    let other = changr_fixture_core(8).expect("second fixture core");
    let mut foreign = Session::new(other);
    let error = foreign
        .load(&text)
        .expect_err("a save under seed 7 must refuse in a seed-8 core");
    assert!(
        error.to_string().contains("run seed"),
        "refusal names the seed binding: {error}"
    );
}

#[test]
fn tampered_envelope_refuses() {
    let mut head = Session::new(make_core());
    for commands in &interpreter_corpus()[..INTERPRETER_SAVE_AT] {
        head.step(commands);
    }
    let text = head.save().expect("save");
    // Flip the recorded tick: the restored state can no longer
    // match the envelope's binding.
    let tampered = text.replacen("\"tick\":12", "\"tick\":13", 1);
    assert_ne!(tampered, text, "the tamper actually changed the document");
    let mut session = Session::new(make_core());
    assert!(
        session.load(&tampered).is_err(),
        "a tampered envelope must refuse"
    );
}

#[test]
fn op_log_round_trips_through_canonical_text() {
    let mut session = Session::new(make_core());
    for commands in &interpreter_corpus()[..6] {
        session.step(commands);
    }
    let text = session.log().to_text();
    let parsed = torogao_runtime::OpLog::from_text(&text).expect("log parses");
    assert_eq!(
        &parsed,
        session.log(),
        "the log survives its canonical text"
    );
    assert_eq!(parsed.to_text(), text, "the text form is byte-stable");
}
