//! runtime-replay: the headless replay harness (the D-925
//! TUI-first validation surface), interpreter witness edition.
//!
//! Runs the shared interpreter corpus against the Changr
//! fixture core (AUTH-G seed 7) in a fresh session, verifies
//! the recorded op log replays to the identical chain, and
//! prints the genesis hash and the per-tick chain, one hash
//! per line. The output is the cross-architecture spot gate's
//! comparison artefact: the same binary built for
//! aarch64-unknown-linux-gnu and run under qemu must print
//! byte-identical lines.
//!
//! Exit codes: 0 = corpus ran and replay matched; 1 = replay
//! diverged; 2 = the fixture or corpus failed to run.

use torogao_runtime::Session;
use torogao_runtime::corpus::interpreter_corpus;
use torogao_runtime::host::author_game::changr_fixture_core;

fn main() {
    let core = match changr_fixture_core(7) {
        Ok(core) => core,
        Err(error) => {
            eprintln!("runtime-replay: fixture failed: {error}");
            std::process::exit(2);
        }
    };
    let corpus = interpreter_corpus();
    let mut session = Session::new(core);
    println!("genesis {}", session.genesis_hash());
    for commands in &corpus {
        let report = session.step(commands);
        let hash = session
            .chain()
            .last()
            .cloned()
            .unwrap_or_else(|| session.genesis_hash().clone());
        println!("tick {} {}", report.tick, hash);
    }
    let replay_core = match changr_fixture_core(7) {
        Ok(core) => core,
        Err(error) => {
            eprintln!("runtime-replay: fixture failed on replay: {error}");
            std::process::exit(2);
        }
    };
    let replay = Session::replay(replay_core, session.log());
    if replay.matches {
        println!("replay ok");
    } else {
        println!(
            "replay DIVERGED at tick {}",
            replay.first_divergence.unwrap_or(0)
        );
        std::process::exit(1);
    }
}
