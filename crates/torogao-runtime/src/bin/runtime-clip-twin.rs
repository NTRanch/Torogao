//! runtime-clip-twin: the cross-architecture twin-gate
//! harness, clip witness edition (hand-animation programme
//! Rung H1).
//!
//! The clip witness is the runtime's first evaluator-bearing
//! Tier 2 witness: its Tier 1 state is an integer tick, and
//! its presentation carries the evaluated channels of a
//! frozen `torogao-motion-clip/2` document (the H0 golden)
//! as Tier 2 gauges. This harness runs the witness corpus
//! from its file form and prints the same line-based report
//! as `runtime-twin`, so the twin gate's comparisons apply
//! unchanged:
//!
//! - `t1` lines: the per-tick Tier 1 hash chain —
//!   byte-identical on every target, no exceptions.
//! - `t2` lines: the per-tick Tier 2 presentation digest.
//! - `t2v` lines: every gauge's raw bit pattern, the numbers
//!   the Tier 2 band judges across architectures.
//! - `save-sha`: the checkpoint save envelope's SHA-256 —
//!   Tier 1, byte-identical everywhere.
//!
//! Modes mirror `runtime-twin`: `emit`, `save`, `continue`,
//! and `dump-corpus clip`.
//!
//! Exit codes: 0 = ran clean; 1 = replay diverged or a load
//! verification failed; 2 = usage, file, or fixture failure.

use torogao_runtime::corpus::{CorpusDoc, clip_corpus_doc};
use torogao_runtime::host::clip_witness::ClipWitnessCore;
use torogao_runtime::{GameCore, Hash, Session};

fn fail(message: &str) -> ! {
    eprintln!("runtime-clip-twin: {message}");
    std::process::exit(2);
}

fn read_corpus(path: &str) -> CorpusDoc {
    let text = std::fs::read_to_string(path)
        .unwrap_or_else(|error| fail(&format!("cannot read corpus {path}: {error}")));
    CorpusDoc::from_text(&text)
        .unwrap_or_else(|error| fail(&format!("corpus {path} refused: {error}")))
}

fn witness_session() -> Session<ClipWitnessCore> {
    match ClipWitnessCore::golden() {
        Ok(core) => Session::new(core),
        Err(error) => fail(&format!("clip witness failed: {error}")),
    }
}

fn check_core_id(doc: &CorpusDoc, session: &Session<ClipWitnessCore>) {
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
    let mut session = witness_session();
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
    let replay_core = match ClipWitnessCore::golden() {
        Ok(core) => core,
        Err(error) => fail(&format!("clip witness failed on replay: {error}")),
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
    let mut session = witness_session();
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
    let mut session = witness_session();
    check_core_id(&doc, &session);
    if let Err(error) = session.load(&text) {
        eprintln!("runtime-clip-twin: load refused: {error}");
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
        "clip" => clip_corpus_doc(),
        _ => fail("dump-corpus expects 'clip'"),
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
        _ => fail("usage: runtime-clip-twin <emit|save|continue|dump-corpus> ..."),
    }
}
