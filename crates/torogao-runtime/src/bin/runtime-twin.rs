//! runtime-twin: the cross-architecture twin-gate harness
//! (runtime plan Rung R2), interpreter witness edition.
//!
//! Where `runtime-replay` prints only the Tier 1 chain, this
//! harness is the gate's full instrument. It runs a corpus
//! from its file form (`torogao-runtime-corpus/1`) and prints
//! a line-based report the gate script compares across
//! targets:
//!
//! - `t1` lines: the per-tick Tier 1 hash chain. Must be
//!   byte-identical on every target, no exceptions.
//! - `t2` lines: the per-tick Tier 2 presentation digest
//!   (bit-pattern hash of the derived view). Must be
//!   byte-identical between two runs on one architecture;
//!   across architectures it is the band that judges.
//! - `t2v` lines: every gauge's raw bit pattern, so the gate
//!   can measure the numeric cross-architecture distance the
//!   band is applied to.
//! - `save-sha`: the SHA-256 of the save envelope taken at
//!   the corpus's checkpoint step — Tier 1, byte-identical
//!   everywhere.
//!
//! Modes:
//! - `emit <corpus-file>`: the full report above, ending with
//!   the replay verdict.
//! - `save <corpus-file> <out-file>`: run to the checkpoint,
//!   write the save envelope text to `<out-file>`.
//! - `continue <corpus-file> <save-file>`: load the envelope
//!   into a fresh session and run the remaining steps,
//!   printing the seeded chain (`seed` + `c` lines). The
//!   cross-target save gate runs every producer/consumer
//!   target pair through `save`/`continue` and requires all
//!   continuations to be identical.
//! - `dump-corpus <interpreter|loop> <out-file>`: write a
//!   built-in corpus in its file form (how the committed
//!   corpus documents were produced).
//!
//! Exit codes: 0 = ran clean; 1 = replay diverged or a load
//! verification failed; 2 = usage, file, or fixture failure.

use torogao_runtime::corpus::{CorpusDoc, interpreter_corpus_doc, loop_corpus_doc};
use torogao_runtime::host::author_game::changr_fixture_core;
use torogao_runtime::{GameCore, Hash, Session};

fn fail(message: &str) -> ! {
    eprintln!("runtime-twin: {message}");
    std::process::exit(2);
}

fn read_corpus(path: &str) -> CorpusDoc {
    let text = std::fs::read_to_string(path)
        .unwrap_or_else(|error| fail(&format!("cannot read corpus {path}: {error}")));
    CorpusDoc::from_text(&text)
        .unwrap_or_else(|error| fail(&format!("corpus {path} refused: {error}")))
}

fn fixture_session() -> Session<torogao_runtime::host::author_game::InterpreterCore> {
    match changr_fixture_core(7) {
        Ok(core) => Session::new(core),
        Err(error) => fail(&format!("fixture failed: {error}")),
    }
}

fn check_core_id(
    doc: &CorpusDoc,
    session: &Session<torogao_runtime::host::author_game::InterpreterCore>,
) {
    let core_id = session.core().core_id().to_string();
    if doc.core_id != core_id {
        fail(&format!(
            "corpus names core '{}', this harness hosts '{core_id}'",
            doc.core_id
        ));
    }
}

fn emit(path: &str) -> ! {
    let doc = read_corpus(path);
    let bytes =
        std::fs::read(path).unwrap_or_else(|error| fail(&format!("cannot read {path}: {error}")));
    let mut session = fixture_session();
    check_core_id(&doc, &session);
    println!("core {}", doc.core_id);
    println!("corpus-sha {}", Hash::sha256(&bytes));
    println!("genesis {}", session.genesis_hash());
    let mut save_sha: Option<Hash> = None;
    for (index, commands) in doc.steps.iter().enumerate() {
        let report = session.step(commands);
        let hash = session
            .chain()
            .last()
            .cloned()
            .unwrap_or_else(|| session.genesis_hash().clone());
        println!("t1 {} {}", report.tick, hash);
        let view = session.core().present();
        println!(
            "t2 {} {}",
            report.tick,
            torogao_runtime::presentation_digest(&view)
        );
        for gauge in &view.gauges {
            println!(
                "t2v {} {} {:016x}",
                report.tick,
                gauge.id,
                gauge.fraction.to_bits()
            );
        }
        if index + 1 == doc.save_at {
            match session.save() {
                Ok(text) => save_sha = Some(Hash::sha256(text.as_bytes())),
                Err(error) => fail(&format!("checkpoint save refused: {error}")),
            }
        }
    }
    println!("save-at {}", doc.save_at);
    match save_sha {
        Some(sha) => println!("save-sha {sha}"),
        None => fail("corpus checkpoint was never reached"),
    }
    let replay_core = match changr_fixture_core(7) {
        Ok(core) => core,
        Err(error) => fail(&format!("fixture failed on replay: {error}")),
    };
    let replay = Session::replay(replay_core, session.log());
    if replay.matches {
        println!("replay ok");
        std::process::exit(0);
    }
    println!(
        "replay DIVERGED at tick {}",
        replay.first_divergence.unwrap_or(0)
    );
    std::process::exit(1);
}

fn save(path: &str, out: &str) -> ! {
    let doc = read_corpus(path);
    let mut session = fixture_session();
    check_core_id(&doc, &session);
    for commands in doc.steps.iter().take(doc.save_at) {
        session.step(commands);
    }
    match session.save() {
        Ok(text) => {
            if let Err(error) = std::fs::write(out, text) {
                fail(&format!("cannot write save {out}: {error}"));
            }
            println!("saved tick {}", session.tick());
            std::process::exit(0);
        }
        Err(error) => fail(&format!("save refused: {error}")),
    }
}

fn continue_run(path: &str, save_path: &str) -> ! {
    let doc = read_corpus(path);
    let text = std::fs::read_to_string(save_path)
        .unwrap_or_else(|error| fail(&format!("cannot read save {save_path}: {error}")));
    let mut session = fixture_session();
    check_core_id(&doc, &session);
    if let Err(error) = session.load(&text) {
        eprintln!("runtime-twin: load refused: {error}");
        std::process::exit(1);
    }
    println!("seed {}", session.genesis_hash());
    for commands in doc.steps.iter().skip(doc.save_at) {
        let report = session.step(commands);
        let hash = session
            .chain()
            .last()
            .cloned()
            .unwrap_or_else(|| session.genesis_hash().clone());
        println!("c {} {}", report.tick, hash);
    }
    std::process::exit(0);
}

fn dump_corpus(which: &str, out: &str) -> ! {
    let doc = match which {
        "interpreter" => interpreter_corpus_doc(),
        "loop" => loop_corpus_doc(),
        _ => fail("dump-corpus expects 'interpreter' or 'loop'"),
    };
    let mut text = doc.to_text();
    text.push('\n');
    if let Err(error) = std::fs::write(out, text) {
        fail(&format!("cannot write corpus {out}: {error}"));
    }
    println!("wrote {} steps for {}", doc.steps.len(), doc.core_id);
    std::process::exit(0);
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("emit") if args.len() == 2 => emit(&args[1]),
        Some("save") if args.len() == 3 => save(&args[1], &args[2]),
        Some("continue") if args.len() == 3 => continue_run(&args[1], &args[2]),
        Some("dump-corpus") if args.len() == 3 => dump_corpus(&args[1], &args[2]),
        _ => fail("usage: runtime-twin <emit|save|continue|dump-corpus> ..."),
    }
}
