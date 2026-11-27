//! Corpus file format gates (runtime plan Rung R2): the
//! committed corpus documents parse to exactly the built-in
//! corpora the R1 conformance suite runs, and the format's
//! canonical text round-trips byte-stably — the twin gate's
//! corpus is a file, and the file is the same on every target.

use torogao_runtime::corpus::{CORPUS_FORMAT, CorpusDoc, interpreter_corpus_doc, loop_corpus_doc};

fn committed(name: &str) -> String {
    let path = format!("{}/corpus/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("read {path}: {error}"))
}

#[test]
fn committed_corpus_files_match_the_built_in_corpora() {
    let interpreter = CorpusDoc::from_text(&committed("interpreter.corpus.json"))
        .expect("interpreter corpus parses");
    assert_eq!(interpreter, interpreter_corpus_doc());
    let loop_doc =
        CorpusDoc::from_text(&committed("loop.corpus.json")).expect("loop corpus parses");
    assert_eq!(loop_doc, loop_corpus_doc());
}

#[test]
fn corpus_text_round_trips_byte_stably() {
    for doc in [interpreter_corpus_doc(), loop_corpus_doc()] {
        let text = doc.to_text();
        let parsed = CorpusDoc::from_text(&text).expect("own text parses");
        assert_eq!(parsed, doc);
        assert_eq!(parsed.to_text(), text, "canonical text is a fixed point");
    }
}

#[test]
fn corpus_format_stamp_is_enforced() {
    let mut doc = interpreter_corpus_doc();
    doc.format = "torogao-runtime-corpus/0".to_string();
    let refused = CorpusDoc::from_text(&doc.to_text());
    assert!(refused.is_err(), "a foreign format stamp must refuse");
    assert_eq!(interpreter_corpus_doc().format, CORPUS_FORMAT);
}
