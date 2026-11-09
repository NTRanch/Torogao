//! coin-smoke: a deterministic smoke run of the coin dynamic
//! on the measured table — the canonical fixture from the
//! crate's `trajectory` module (two preparation actions, then
//! a synthetic swing stream against the level-0 gauge).
//! Prints the outcome and the fixture's trajectory hash;
//! exit 0 on a completed run, 1 on refusal. This is the
//! pilot's Linux-native proof artifact, not a game. The
//! Android spike shell runs the same fixture from the same
//! module; the printed hash is the S7 cross-platform
//! comparison value.

use torogao_coin::run_measured_fixture;

fn main() {
    let code = run();
    std::process::exit(code);
}

fn run() -> i32 {
    match run_measured_fixture() {
        Ok(run) => {
            let outcome = run.outcome;
            println!(
                "coin-smoke: success={} gauge={:.4} points={:.4} elapsed={:.2}s level={} cap={} clock={} can_save={} hash={:016x}",
                outcome.success,
                outcome.gauge,
                outcome.points,
                outcome.elapsed_seconds,
                outcome.level_after,
                outcome.at_measured_cap,
                run.clock_minute,
                run.can_save,
                run.trajectory_hash
            );
            0
        }
        Err(e) => {
            eprintln!("coin-smoke: {e}");
            1
        }
    }
}
