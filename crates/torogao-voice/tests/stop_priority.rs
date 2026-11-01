//! STOP priority gate (design §8.4): the stop class is
//! checked before every template and before the confidence
//! floor. A stop lexeme as the first token yields `Stop` at
//! any confidence, including utterances whose remainder
//! would otherwise parse as a valid command.

use torogao_voice::embedded_grammar;
use torogao_voice::parse::{ParseOutcome, Transcript, parse};

/// Parses with the given confidence.
fn outcome(text: &str, confidence: Option<f32>) -> ParseOutcome {
    let grammar = embedded_grammar().expect("embedded grammar parses");
    parse(
        &grammar,
        &Transcript {
            text: text.to_string(),
            confidence,
        },
    )
}

#[test]
fn stop_fires_at_any_confidence() {
    for confidence in [Some(0.99), Some(0.30), Some(0.0), None] {
        assert_eq!(
            outcome("stop", confidence),
            ParseOutcome::Stop,
            "confidence {confidence:?}"
        );
    }
}

#[test]
fn every_stop_lexeme_fires() {
    for lexeme in ["stop", "halt", "cancel", "safeword"] {
        assert_eq!(outcome(lexeme, Some(0.99)), ParseOutcome::Stop, "{lexeme}");
    }
}

#[test]
fn stop_beats_a_valid_command_in_the_remainder() {
    // Without priority, the tail of this utterance is a
    // complete, valid command; the leading stop must win.
    assert_eq!(
        outcome("stop inspect project rich", Some(0.99)),
        ParseOutcome::Stop
    );
    assert_eq!(
        outcome("safeword export rig project rich out prima.glb", Some(0.99)),
        ParseOutcome::Stop
    );
    assert_eq!(
        outcome("halt everything now", Some(0.99)),
        ParseOutcome::Stop
    );
}

#[test]
fn stop_must_lead_the_utterance() {
    // The rule is first-token: a stop word later in the
    // utterance is not a stop. "please stop" matches no
    // template either, so it refuses rather than stopping.
    let outcome = outcome("please stop", Some(0.99));
    assert!(
        matches!(outcome, ParseOutcome::Refusal(_)),
        "got {outcome:?}"
    );
}

#[test]
fn stop_report_is_the_stopped_status() {
    let report = torogao_voice::dispatch::stop_report();
    assert_eq!(report.status, torogao_voice::dispatch::ExecStatus::Stopped);
    assert_eq!(report.entry_id, "safety.stop");
}
