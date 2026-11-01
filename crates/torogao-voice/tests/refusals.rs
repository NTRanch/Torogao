//! Typed-refusal gate (design §8.3): ambiguous,
//! low-confidence, no-match, missing-argument, and
//! wrong-type inputs each refuse with their `TG-VOICE-*`
//! code and the required payload. Nothing executes on a
//! refusal — these tests only ever see `ParseOutcome::
//! Refusal`.

use torogao_voice::embedded_grammar;
use torogao_voice::grammar::{ConfirmClass, GRAMMAR_VERSION, GrammarDoc, GrammarEntry, Target};
use torogao_voice::parse::{ParseOutcome, Transcript, VoiceRefusal, parse};

/// Parses expecting a refusal; returns it.
fn expect_refusal(grammar: &GrammarDoc, text: &str, confidence: Option<f32>) -> VoiceRefusal {
    match parse(
        grammar,
        &Transcript {
            text: text.to_string(),
            confidence,
        },
    ) {
        ParseOutcome::Refusal(refusal) => refusal,
        other => panic!("expected refusal for {text:?}, got {other:?}"),
    }
}

/// A hand-built grammar with a genuine template collision:
/// two entries claiming the same spoken form.
fn colliding_grammar() -> GrammarDoc {
    let entry = |id: &str, name: &str| GrammarEntry {
        id: id.to_string(),
        target: Target::RegistryTool {
            name: name.to_string(),
        },
        posture: "read-only".to_string(),
        confirm: ConfirmClass::None,
        templates: vec!["check the thing".to_string()],
        slots: Vec::new(),
    };
    GrammarDoc {
        version: GRAMMAR_VERSION.to_string(),
        entries: vec![
            entry("tool.alpha-check", "alpha"),
            entry("tool.beta-check", "beta"),
        ],
    }
}

#[test]
fn ambiguous_input_refuses_with_all_candidates() {
    let grammar = colliding_grammar();
    let refusal = expect_refusal(&grammar, "check the thing", Some(0.99));
    assert_eq!(refusal.code, "TG-VOICE-AMBIGUOUS");
    assert_eq!(
        refusal.candidates,
        vec![
            "tool.alpha-check".to_string(),
            "tool.beta-check".to_string()
        ],
        "candidates are every matching entry, sorted"
    );
}

#[test]
fn low_confidence_refuses_and_names_the_closest_parse() {
    let grammar = embedded_grammar().expect("embedded grammar parses");
    let refusal = expect_refusal(&grammar, "inspect project rich", Some(0.30));
    assert_eq!(refusal.code, "TG-VOICE-LOW-CONFIDENCE");
    assert_eq!(refusal.candidates, vec!["tool.inspect".to_string()]);
}

#[test]
fn confidence_at_the_floor_parses() {
    let grammar = embedded_grammar().expect("embedded grammar parses");
    let outcome = parse(
        &grammar,
        &Transcript {
            text: "inspect project rich".to_string(),
            confidence: Some(torogao_voice::parse::CONFIDENCE_MIN),
        },
    );
    assert!(matches!(outcome, ParseOutcome::Command(_)));
}

#[test]
fn unknown_input_refuses_no_match() {
    let grammar = embedded_grammar().expect("embedded grammar parses");
    let refusal = expect_refusal(&grammar, "flibberty gibbet", Some(0.99));
    assert_eq!(refusal.code, "TG-VOICE-NO-MATCH");
    assert_eq!(refusal.transcript, "flibberty gibbet");
}

#[test]
fn missing_required_argument_is_named() {
    let grammar = embedded_grammar().expect("embedded grammar parses");
    let refusal = expect_refusal(&grammar, "inspect", Some(0.99));
    assert_eq!(refusal.code, "TG-VOICE-MISSING-ARG");
    assert!(
        refusal.message.contains("'project'"),
        "the missing slot is named: {}",
        refusal.message
    );
    let refusal2 = expect_refusal(&grammar, "scene validate", Some(0.99));
    assert_eq!(refusal2.code, "TG-VOICE-MISSING-ARG");
    assert!(refusal2.message.contains("'scene'"));
}

#[test]
fn wrong_typed_argument_refuses_arg_type() {
    let grammar = embedded_grammar().expect("embedded grammar parses");
    // writer-draft's `stage` is a required number slot.
    let refusal = expect_refusal(
        &grammar,
        "writer draft character prima kink gentle lane house out draft.json pack core stage banana",
        Some(0.99),
    );
    assert_eq!(refusal.code, "TG-VOICE-ARG-TYPE");
    assert!(refusal.message.contains("'stage'"));
    assert!(refusal.message.contains("banana"));
}

#[test]
fn empty_utterance_refuses() {
    let grammar = embedded_grammar().expect("embedded grammar parses");
    let refusal = expect_refusal(&grammar, "   ", Some(0.99));
    assert_eq!(refusal.code, "TG-VOICE-NO-MATCH");
}
