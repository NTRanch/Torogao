//! The playable day loop: torogao-coin as a game a person
//! can actually play, not only a fixture a gate can run.
//!
//! `PlaySession` drives one `CoinShell` through the whole
//! measured loop — preparation actions on the day clock,
//! timed swing attempts fed sample by sample, the day-end
//! save gate — behind a small line protocol. The terminal
//! binary (`coin-play`) is a thin stdin/stdout adapter over
//! this module, so a human at a keyboard and a scripted
//! smoke run drive exactly the same code.
//!
//! Protocol (one command per line, one reply line each):
//!
//! - `status` — level, clock, preparation stats, save gate.
//! - `prep <index>` — one preparation action (day minutes).
//! - `message <hp> <mp> <sp>` — the message-driven stat path.
//! - `swing <x> [<x> ...]` — feed pointer-x samples (world
//!   units) into the open attempt, opening one if none is
//!   open. An unresolved attempt stays open for the next
//!   `swing` line; a resolving sample closes it and the
//!   reply carries the verdict.
//! - `save` — succeeds only at or past the leaving minute.
//! - `help` — the command list, as one line.
//! - `quit` — ends the session.
//!
//! Every refusal (unknown command, malformed number, an
//! input the shell refuses) is a reply line beginning
//! `coin-play: error=` and leaves the session state
//! untouched — the shell binding already guarantees the
//! second half; this module guarantees the first.
//! Determinism: identical command streams produce identical
//! reply streams, bit for bit (the state hash is in the
//! status/save replies).

use crate::dynamics::{CoinError, Dynamics, StatTriple};
use crate::shell::CoinShell;
use crate::trajectory::FIXTURE_DT_SECONDS;

/// Day-clock start for a play session (09:00, the measured
/// day frame's morning, as in the canonical fixture).
pub const PLAY_DAY_START_MINUTE: u32 = 9 * 60;
/// Bound on whitespace-separated tokens in one command line
/// (a `swing` line carries at most one attempt's samples).
pub const TOKENS_PER_LINE_MAX: usize = 1300;
/// Bound on commands in one play session.
pub const COMMANDS_PER_SESSION_MAX: usize = 4096;

/// One playable session: a bound shell plus the protocol's
/// own bookkeeping (whether `quit` was seen).
pub struct PlaySession {
    shell: CoinShell,
    finished: bool,
    commands_seen: usize,
}

impl PlaySession {
    /// A fresh playable session on `dynamics`, at the
    /// fixture's day start.
    pub fn new(dynamics: Dynamics) -> Result<PlaySession, CoinError> {
        Ok(PlaySession {
            shell: CoinShell::new(dynamics, PLAY_DAY_START_MINUTE)?,
            finished: false,
            commands_seen: 0,
        })
    }

    /// Whether `quit` has ended the session.
    pub fn finished(&self) -> bool {
        self.finished
    }

    /// The bound shell's state hash (test/smoke surface).
    pub fn state_hash(&self) -> u64 {
        self.shell.state_hash()
    }

    fn status_line(&self) -> String {
        let prep = self.shell.prep();
        format!(
            "coin-play: level={} clock={} prep={:.4}/{:.4}/{:.4} \
             can_save={} state_hash={:016x}",
            self.shell.level(),
            self.shell.clock_minute(),
            prep.hp,
            prep.mp,
            prep.sp,
            self.shell.can_save(),
            self.shell.state_hash()
        )
    }

    /// Handle one protocol line and return its reply line.
    /// Empty lines and `#` comments reply with the status
    /// line, so a script can checkpoint anywhere.
    pub fn handle_line(&mut self, line: &str) -> String {
        if self.finished {
            return "coin-play: error=session already finished".to_string();
        }
        self.commands_seen += 1;
        if self.commands_seen > COMMANDS_PER_SESSION_MAX {
            self.finished = true;
            return "coin-play: error=command bound exceeded; session finished".to_string();
        }
        let tokens: Vec<&str> = line.split_whitespace().collect();
        if tokens.len() > TOKENS_PER_LINE_MAX {
            return "coin-play: error=line exceeds the token bound".to_string();
        }
        let Some(&command) = tokens.first() else {
            return self.status_line();
        };
        if command.starts_with('#') {
            return self.status_line();
        }
        match command {
            "status" => self.status_line(),
            "help" => "coin-play: commands=status|prep <index>|message <hp> <mp> <sp>|\
                       swing <x>...|save|help|quit"
                .to_string(),
            "quit" => {
                self.finished = true;
                format!("coin-play: quit=true {}", self.status_line())
            }
            "prep" => self.handle_prep(&tokens),
            "message" => self.handle_message(&tokens),
            "swing" => self.handle_swing(&tokens),
            "save" => self.handle_save(),
            other => format!("coin-play: error=unknown command {other}"),
        }
    }

    fn handle_prep(&mut self, tokens: &[&str]) -> String {
        let Some(index) = parse_index(tokens) else {
            return "coin-play: error=prep needs one action index".to_string();
        };
        match self.shell.prepare(index) {
            Ok(minutes) => format!(
                "coin-play: prep action={} minutes={} clock={} state_hash={:016x}",
                index,
                minutes,
                self.shell.clock_minute(),
                self.shell.state_hash()
            ),
            Err(e) => format!("coin-play: error={e}"),
        }
    }

    fn handle_message(&mut self, tokens: &[&str]) -> String {
        let Some(raw) = parse_triple(tokens) else {
            return "coin-play: error=message needs three finite numbers".to_string();
        };
        match self.shell.message_stats(raw) {
            Ok(()) => format!("coin-play: message applied {}", self.status_line()),
            Err(e) => format!("coin-play: error={e}"),
        }
    }

    fn handle_swing(&mut self, tokens: &[&str]) -> String {
        let Some(samples) = parse_samples(tokens) else {
            return "coin-play: error=swing needs one or more finite samples".to_string();
        };
        if !self.shell.attempt_active() {
            if let Err(e) = self.shell.begin_attempt(FIXTURE_DT_SECONDS) {
                return format!("coin-play: error={e}");
            }
        }
        for x in samples {
            match self.shell.pointer_sample(x) {
                Ok(Some(verdict)) => {
                    return format!(
                        "coin-play: swing win={} gauge={:.4} points={:.4} level={} \
                         trajectory_hash={:016x}",
                        verdict.win,
                        verdict.outcome.gauge,
                        verdict.outcome.points,
                        verdict.outcome.level_after,
                        verdict.trajectory_hash
                    );
                }
                Ok(None) => {}
                Err(e) => return format!("coin-play: error={e}"),
            }
        }
        format!(
            "coin-play: swing open gauge={:.4} {}",
            self.shell.attempt_gauge().unwrap_or(0.0),
            self.status_line()
        )
    }

    fn handle_save(&mut self) -> String {
        if self.shell.can_save() {
            format!(
                "coin-play: saved=true clock={} state_hash={:016x}",
                self.shell.clock_minute(),
                self.shell.state_hash()
            )
        } else {
            format!(
                "coin-play: saved=false reason=day-not-ended clock={}",
                self.shell.clock_minute()
            )
        }
    }
}

fn parse_index(tokens: &[&str]) -> Option<usize> {
    if tokens.len() != 2 {
        return None;
    }
    tokens[1].parse::<usize>().ok()
}

fn parse_triple(tokens: &[&str]) -> Option<StatTriple> {
    if tokens.len() != 4 {
        return None;
    }
    let hp = tokens[1].parse::<f64>().ok()?;
    let mp = tokens[2].parse::<f64>().ok()?;
    let sp = tokens[3].parse::<f64>().ok()?;
    if ![hp, mp, sp].iter().all(|v| v.is_finite()) {
        return None;
    }
    Some(StatTriple { hp, mp, sp })
}

fn parse_samples(tokens: &[&str]) -> Option<Vec<f64>> {
    if tokens.len() < 2 {
        return None;
    }
    let mut samples = Vec::with_capacity(tokens.len() - 1);
    for token in &tokens[1..] {
        let value = token.parse::<f64>().ok()?;
        if !value.is_finite() {
            return None;
        }
        samples.push(value);
    }
    Some(samples)
}

/// Run a whole command script through a fresh play session
/// and collect the reply lines — the scripted-smoke surface
/// (the same stream a human would type, replayed).
pub fn run_play_script(dynamics: Dynamics, commands: &[&str]) -> Result<Vec<String>, CoinError> {
    if commands.len() > COMMANDS_PER_SESSION_MAX {
        return Err(CoinError::InvalidStep {
            reason: "play script exceeds the command bound",
        });
    }
    let mut session = PlaySession::new(dynamics)?;
    let mut replies = Vec::with_capacity(commands.len());
    for command in commands {
        replies.push(session.handle_line(command));
        if session.finished() {
            break;
        }
    }
    Ok(replies)
}
