//! Coverage gate (design §8.1): every registry tool, every
//! authoring verb, and every CLI mode has a grammar entry;
//! the checked-in grammar file is byte-identical to a fresh
//! generation; no excluded target appears; no template is
//! claimed by two entries.

use torogao_voice::audit::coverage_report;
use torogao_voice::generate::build_grammar;
use torogao_voice::grammar::{GrammarDoc, Target};
use torogao_voice::{EMBEDDED_GRAMMAR_JSON, embedded_grammar};

/// The checked-in grammar file equals a fresh generation
/// from the sources of truth, byte for byte — the file
/// cannot drift from the registry silently.
#[test]
fn checked_in_grammar_is_byte_identical_to_fresh_generation() {
    let fresh = build_grammar().to_canonical_json();
    assert_eq!(
        fresh, EMBEDDED_GRAMMAR_JSON,
        "grammar file drifted from its sources; regenerate with `torogao-voice gen-grammar`"
    );
    let from_disk = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/grammar/torogao-voice-grammar.json"
    ))
    .expect("checked-in grammar file readable");
    assert_eq!(from_disk, fresh);
}

/// The embedded grammar parses and is complete.
#[test]
fn coverage_is_complete() {
    let grammar = embedded_grammar().expect("embedded grammar parses");
    let report = coverage_report(&grammar);
    assert_eq!(report.tools_total, 57, "registry census changed");
    assert_eq!(
        report.tools_covered, 57,
        "missing tools: {:?}",
        report.missing_tools
    );
    assert_eq!(report.verbs_total, 69, "verb manifest census changed");
    assert_eq!(
        report.verbs_covered, 69,
        "missing verbs: {:?}",
        report.missing_verbs
    );
    assert_eq!(report.modes_covered, report.modes_total);
    assert!(
        report.exclusions_present.is_empty(),
        "excluded targets present: {:?}",
        report.exclusions_present
    );
    assert!(
        report.duplicate_templates.is_empty(),
        "templates claimed by two entries: {:?}",
        report.duplicate_templates
    );
    assert!(report.ok);
}

/// The gate must be able to fail: removing one tool's entry
/// makes the report name it. (A coverage test that cannot
/// fail proves nothing.)
#[test]
fn coverage_fails_when_a_tool_loses_its_entry() {
    let mut grammar = build_grammar();
    grammar.entries.retain(|entry| {
        entry.target
            != (Target::RegistryTool {
                name: "inspect".to_string(),
            })
    });
    let report = coverage_report(&grammar);
    assert!(!report.ok);
    assert_eq!(report.missing_tools, vec!["inspect".to_string()]);
}

/// Every live registry tool name appears as a target.
#[test]
fn every_live_registry_tool_is_a_target() {
    let grammar = embedded_grammar().expect("embedded grammar parses");
    for tool in torogao_cli::registry::list_tools() {
        assert!(
            grammar.entries.iter().any(|entry| {
                entry.target
                    == (Target::RegistryTool {
                        name: tool.name.to_string(),
                    })
            }),
            "tool {} has no grammar entry",
            tool.name
        );
    }
}

/// The grammar document round-trips its canonical form.
#[test]
fn grammar_doc_round_trips() {
    let doc = build_grammar();
    let text = doc.to_canonical_json();
    let back = GrammarDoc::from_json(&text).expect("canonical grammar re-parses");
    assert_eq!(back.entries.len(), doc.entries.len());
    assert_eq!(back.to_canonical_json(), text);
}
