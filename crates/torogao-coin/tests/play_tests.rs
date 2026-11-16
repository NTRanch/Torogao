//! Gate tests for the playable day loop (`play` module).
//! Half validation (a full day can be played end to end:
//! prepare, swing, level up, reach the leaving minute,
//! save), half adversarial (refused saves, unknown or
//! malformed commands, and in-attempt preparation all leave
//! the session untouched).

use torogao_coin::measured_dynamics;
use torogao_coin::play::{PlaySession, run_play_script};
use torogao_coin::trajectory::fixture_samples;

fn swing_line() -> String {
    let samples: Vec<String> = fixture_samples().iter().map(|x| format!("{x}")).collect();
    format!("swing {}", samples.join(" "))
}

// ---------- validation ----------

#[test]
fn full_day_playthrough_swings_banks_levels_and_saves() {
    let mut commands: Vec<String> = vec!["prep 13".to_string(), "prep 8".to_string(), swing_line()];
    // Preparation alone carries the clock to the leaving
    // minute (action 8 costs 35 minutes, as in the shell's
    // day-frame script).
    for _ in 0..16 {
        commands.push("prep 8".to_string());
    }
    commands.push("save".to_string());
    let refs: Vec<&str> = commands.iter().map(String::as_str).collect();
    let replies = run_play_script(measured_dynamics().unwrap(), &refs).unwrap();
    assert!(
        replies[2].contains("swing win=true"),
        "swing reply: {}",
        replies[2]
    );
    assert!(replies[2].contains("level=1"), "{}", replies[2]);
    let save = replies.last().expect("save reply present");
    assert!(save.contains("saved=true"), "save reply: {save}");
}

#[test]
fn play_script_is_deterministic_across_runs() {
    let commands = ["prep 13", "status", "prep 8", "status"];
    let first = run_play_script(measured_dynamics().unwrap(), &commands).unwrap();
    let second = run_play_script(measured_dynamics().unwrap(), &commands).unwrap();
    assert_eq!(first, second);
    assert!(first[1].contains("clock=560"), "{}", first[1]);
    assert!(first[3].contains("clock=595"), "{}", first[3]);
}

#[test]
fn message_command_applies_the_positive_message_path() {
    let replies = run_play_script(
        measured_dynamics().unwrap(),
        &["message 100 200 50", "status"],
    )
    .unwrap();
    // 100/100*0.65, 200/100*0.65, 50/100*0.65.
    assert!(
        replies[1].contains("prep=0.6500/1.3000/0.3250"),
        "status: {}",
        replies[1]
    );
}

#[test]
fn an_unresolved_swing_stays_open_for_the_next_line() {
    let mut session = PlaySession::new(measured_dynamics().unwrap()).unwrap();
    let first = session.handle_line("swing 0 1 2 3");
    assert!(first.contains("swing open"), "{first}");
    let second = session.handle_line("status");
    assert!(second.contains("level=0"), "{second}");
}

// ---------- adversarial ----------

#[test]
fn save_before_day_end_is_refused_and_changes_nothing() {
    let mut session = PlaySession::new(measured_dynamics().unwrap()).unwrap();
    let before = session.state_hash();
    let reply = session.handle_line("save");
    assert!(reply.contains("saved=false"), "{reply}");
    assert!(reply.contains("reason=day-not-ended"), "{reply}");
    assert_eq!(session.state_hash(), before);
}

#[test]
fn unknown_and_malformed_commands_change_nothing() {
    let mut session = PlaySession::new(measured_dynamics().unwrap()).unwrap();
    let before = session.state_hash();
    for line in [
        "dance",
        "prep",
        "prep nope",
        "prep 99",
        "message 1 2",
        "message nan 0 0",
        "swing",
        "swing 1 nope",
        "swing nan",
    ] {
        let reply = session.handle_line(line);
        assert!(reply.contains("error="), "line {line:?} reply {reply}");
        assert_eq!(session.state_hash(), before, "line {line:?}");
    }
}

#[test]
fn preparation_during_an_open_attempt_is_refused() {
    let mut session = PlaySession::new(measured_dynamics().unwrap()).unwrap();
    let open = session.handle_line("swing 0 1 2 3");
    assert!(open.contains("swing open"), "{open}");
    let before = session.state_hash();
    let reply = session.handle_line("prep 0");
    assert!(reply.contains("error="), "{reply}");
    assert_eq!(session.state_hash(), before);
}

#[test]
fn commands_after_quit_are_refused() {
    let mut session = PlaySession::new(measured_dynamics().unwrap()).unwrap();
    let quit = session.handle_line("quit");
    assert!(quit.contains("quit=true"), "{quit}");
    assert!(session.finished());
    let reply = session.handle_line("status");
    assert!(reply.contains("error="), "{reply}");
}
