//! The coin host: torogao-coin bound into this shell.
//!
//! `--coin-sessions` runs the TF Jam skeleton sessions
//! (a winning swing, a timed-out swing, the day frame)
//! through the coin crate's shell binding
//! (`torogao_coin::shell`) — the same input surface the
//! windowed shell would drive per pointer event — and
//! prints one line per session with its verdict and state
//! hash. This is the workspace smoke pattern applied to the
//! bound crate: headless, deterministic, hash-bearing. The
//! graphical session itself is S5 content work and is not
//! claimed here; what this host proves is that the shell
//! and the systems crate compose.

use torogao_coin::measured_dynamics;
use torogao_coin::shell::{SkeletonSession, run_skeleton_sessions};

use crate::error::{AppError, AppResult};

fn win_label(win: Option<bool>) -> &'static str {
    match win {
        Some(true) => "true",
        Some(false) => "false",
        None => "none",
    }
}

fn hash_label(hash: Option<u64>) -> String {
    match hash {
        Some(value) => format!("{value:016x}"),
        None => "none".to_string(),
    }
}

fn session_line(session: &SkeletonSession) -> String {
    format!(
        "coin-session: name={} win={} level={} clock={} can_save={} state_hash={:016x} trajectory_hash={}",
        session.name,
        win_label(session.win),
        session.level_after,
        session.clock_minute,
        session.can_save,
        session.state_hash,
        hash_label(session.trajectory_hash)
    )
}

/// Runs the skeleton sessions on the bound coin crate and
/// prints their lines. Exit 0 when every session completes;
/// a systems refusal is a typed `AppError`, never a panic.
pub fn run_coin_sessions() -> AppResult<i32> {
    let dynamics = measured_dynamics().map_err(|e| AppError::Format(e.to_string()))?;
    let sessions = run_skeleton_sessions(dynamics).map_err(|e| AppError::Format(e.to_string()))?;
    for session in &sessions {
        println!("{}", session_line(session));
    }
    println!("coin-sessions: success=true sessions={}", sessions.len());
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skeleton_sessions_run_through_the_binding() {
        let dynamics = measured_dynamics().expect("measured dynamics load");
        let sessions = run_skeleton_sessions(dynamics).expect("skeleton sessions run");
        assert_eq!(sessions.len(), 3);
        assert_eq!(sessions[0].win, Some(true));
        assert_eq!(sessions[1].win, Some(false));
        assert_eq!(sessions[2].win, None);
        assert!(sessions[2].can_save);
        let line = session_line(&sessions[0]);
        assert!(
            line.starts_with("coin-session: name=fixture-swing win=true"),
            "{line}"
        );
    }
}
