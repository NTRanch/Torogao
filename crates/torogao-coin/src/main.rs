//! coin-smoke: a deterministic smoke run of the coin dynamic
//! on the measured table — two preparation actions, then a
//! synthetic swing stream against the level-0 gauge. Prints
//! the outcome; exit 0 on a completed run, 1 on refusal.
//! This is the pilot's Linux-native proof artifact, not a game.

use torogao_coin::{Session, measured_dynamics};

fn main() {
    let code = run();
    std::process::exit(code);
}

fn run() -> i32 {
    let dynamics = match measured_dynamics() {
        Ok(d) => d,
        Err(e) => {
            eprintln!("coin-smoke: {e}");
            return 1;
        }
    };
    let mut session = match Session::new(dynamics, 9 * 60) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("coin-smoke: {e}");
            return 1;
        }
    };
    // Preparation: the two broadest measured actions
    // (indices 13 and 8 in the source switch table).
    for index in [13_usize, 8] {
        if let Err(e) = session.apply_prep_action(index) {
            eprintln!("coin-smoke: {e}");
            return 1;
        }
    }
    // Swing stream: a triangle wave, amplitude 10 world units,
    // one sample per 1/60 s step, up to 20 s of samples.
    let mut samples: Vec<f64> = Vec::new();
    let steps = 20 * 60;
    for i in 0..steps {
        // Triangle wave centred on the start point (x = 0 at
        // i = 0), swinging +-10 world units either side.
        let phase = ((i + 10) % 40) as f64;
        let x = if phase < 20.0 {
            phase - 10.0
        } else {
            30.0 - phase
        };
        samples.push(x);
    }
    match session.run_attempt(&samples, 1.0 / 60.0) {
        Ok(outcome) => {
            println!(
                "coin-smoke: success={} gauge={:.4} points={:.4} elapsed={:.2}s level={} cap={} clock={} can_save={}",
                outcome.success,
                outcome.gauge,
                outcome.points,
                outcome.elapsed_seconds,
                outcome.level_after,
                outcome.at_measured_cap,
                session.clock_minute(),
                session.can_save()
            );
            0
        }
        Err(e) => {
            eprintln!("coin-smoke: {e}");
            1
        }
    }
}
