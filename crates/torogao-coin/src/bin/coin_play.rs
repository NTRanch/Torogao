//! coin-play: the playable Magic Coin day loop on Linux.
//!
//! A thin stdin/stdout adapter over
//! `torogao_coin::play::PlaySession`: type (or pipe) the
//! protocol lines documented there — `prep`, `swing`,
//! `message`, `status`, `save`, `quit` — and play one full
//! day: prepare, swing the coin, raise the level, reach
//! the leaving minute, and save. No graphics, no audio;
//! the systems loop is the game at this pilot stage.
//! Exit 0 on a completed session, 1 on a systems refusal
//! before the session starts.

use std::io::{BufRead, Write};

use torogao_coin::measured_dynamics;
use torogao_coin::play::{COMMANDS_PER_SESSION_MAX, PlaySession};

fn main() {
    std::process::exit(run());
}

fn run() -> i32 {
    let dynamics = match measured_dynamics() {
        Ok(dynamics) => dynamics,
        Err(e) => {
            eprintln!("coin-play: {e}");
            return 1;
        }
    };
    let mut session = match PlaySession::new(dynamics) {
        Ok(session) => session,
        Err(e) => {
            eprintln!("coin-play: {e}");
            return 1;
        }
    };
    println!("coin-play: Magic Coin — prepare, swing, reach 18:00, save.");
    println!("coin-play: type `help` for commands.");
    let stdin = std::io::stdin();
    let mut lines = 0_usize;
    for line in stdin.lock().lines() {
        lines += 1;
        if lines > COMMANDS_PER_SESSION_MAX {
            println!("coin-play: error=command bound exceeded; session finished");
            break;
        }
        let Ok(line) = line else {
            eprintln!("coin-play: stdin read failed");
            return 1;
        };
        println!("{}", session.handle_line(&line));
        let _ = std::io::stdout().flush();
        if session.finished() {
            break;
        }
    }
    0
}
