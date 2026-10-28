//! Determinism gate (design §8.2): a fixed utterance corpus
//! parses to byte-identical serialized outcomes across
//! repeated runs and across shuffled grammar-entry order;
//! grammar generation is byte-stable.

use torogao_voice::generate::build_grammar;
use torogao_voice::grammar::GrammarDoc;
use torogao_voice::parse::{Transcript, parse};
use torogao_voice::{EMBEDDED_GRAMMAR_JSON, embedded_grammar};

/// The corpus: one utterance per interesting shape —
/// read-only, export with flag, list slot, number slot,
/// author verb, app command, CLI mode, refusal shapes.
const CORPUS: &[&str] = &[
    "inspect project rich",
    "inspect rich",
    "validate project rich",
    "scene validate project rich scene lobby",
    "export rig project rich out prima.glb",
    "audio bake wav take.wav recipe bake.json out curve.json force",
    "instantiate template template base-humanoid out new.inx presets sway drift",
    "apply state project rich track track.json state smile variant stage",
    "writer draft character prima kink gentle lane house out draft.json pack core stage two",
    "author keyform set",
    "author param define width minimum zero maximum ten",
    "author game simulate",
    "open settings",
    "open playroom",
    "toggle nerd mode",
    "dump registry",
    "show paths",
    "inspect",
    "flibberty gibbet",
    "apply fixes project rich",
];

/// Serializes one parse of every corpus utterance.
fn corpus_outcomes(grammar: &GrammarDoc) -> Vec<String> {
    CORPUS
        .iter()
        .map(|utterance| {
            let outcome = parse(
                grammar,
                &Transcript {
                    text: utterance.to_string(),
                    confidence: Some(0.95),
                },
            );
            serde_json::to_string(&outcome).expect("outcome serializes")
        })
        .collect()
}

#[test]
fn parse_is_byte_identical_across_repeats() {
    let grammar = embedded_grammar().expect("embedded grammar parses");
    let first = corpus_outcomes(&grammar);
    for _ in 0..25 {
        assert_eq!(corpus_outcomes(&grammar), first);
    }
}

#[test]
fn parse_is_invariant_under_grammar_shuffle() {
    let mut shuffled = build_grammar();
    shuffled.entries.reverse();
    for entry in &mut shuffled.entries {
        entry.templates.reverse();
        entry.slots.reverse();
    }
    let grammar = embedded_grammar().expect("embedded grammar parses");
    assert_eq!(corpus_outcomes(&shuffled), corpus_outcomes(&grammar));
}

#[test]
fn generation_is_byte_stable() {
    let first = build_grammar().to_canonical_json();
    for _ in 0..10 {
        assert_eq!(build_grammar().to_canonical_json(), first);
    }
    assert_eq!(first, EMBEDDED_GRAMMAR_JSON);
}
