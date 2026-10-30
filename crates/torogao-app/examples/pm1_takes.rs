//! PM-G13 eye-sheet generator (BRIEF-PM1 P16): one
//! full-progression take per girl + one REC-skin take,
//! delivered as files for the Owner's eye — NOT
//! self-approved. Each take is replay-verified before
//! it is written (its hashes must reproduce from a
//! fresh entry).
//!
//! Usage: `PM1_EVIDENCE_DIR=<dir> cargo run -p
//! torogao-app --example pm1_takes`

use std::path::PathBuf;

use torogao_app::playtime::PlayroomPlaytime;
use torogao_session::playmode::{ModeTickInput, PlaytimeDefinition, PlaytimeMode, replay_take};
use torogao_session::{ArousalHost, Phase};

/// The gate-canonical script (same state-triggered
/// shape as tests/playmode_gates.rs): ramp over 7200
/// ticks, a paced plateau in Building until body +
/// fixture are wet, touch from t=1200, Hold at Q ≥ 800
/// for 900 ticks, release to Culmination, then Taper →
/// Afterglow → Refractory.
fn full_progression_take(character: &str) -> torogao_session::playmode::TakeRecord {
    let definition = PlaytimeDefinition::blossom();
    let mut host = ArousalHost::new();
    let mut mode = PlaytimeMode::new(definition);
    mode.enter(&mut host, character).unwrap();
    mode.set_rec_skin(&mut host, true).unwrap();
    mode.start_take(&mut host).unwrap();
    let mut plateau_active = false;
    let mut plateau_done = false;
    let mut plateau_amp: u32 = 0;
    let mut plateau_start = 0u64;
    let mut last_q = 0u32;
    let mut hold_spent = false;
    let mut hold_release_at: Option<u64> = None;
    for t in 0..120_000u64 {
        if plateau_active {
            if t % 15 == 0 {
                let next = if last_q > 760 {
                    plateau_amp.saturating_sub(30)
                } else if last_q < 650 {
                    (plateau_amp + 30).min(1000)
                } else {
                    plateau_amp
                };
                if next != plateau_amp {
                    plateau_amp = next;
                    mode.set_amplitude(&mut host, plateau_amp).unwrap();
                }
            }
        } else if t % 60 == 0 && t <= 7200 {
            let amp = ((t / 60) * 10).min(1000) as u32;
            mode.set_amplitude(&mut host, amp).unwrap();
        }
        if let Some(rt) = hold_release_at {
            if t == rt {
                mode.set_hold(&mut host, false).unwrap();
                hold_release_at = None;
            }
        }
        let mut input = ModeTickInput::default();
        if t >= 1200 && t % 45 == 0 {
            input.touch = Some(("pelvis".to_string(), 400));
        }
        if t % 900 == 450 {
            input.spray_rate = Some(600);
            input.face_contact = true;
        }
        let out = mode.step(&mut host, &input).unwrap();
        let v = host.get(character).unwrap().vector();
        last_q = v.pleasure_charge;
        if !plateau_done && !plateau_active && v.pleasure_charge >= 600 {
            plateau_active = true;
            plateau_start = t;
            plateau_amp = mode.user_amplitude();
        }
        if plateau_active {
            let wet = mode.wetness();
            if (wet.permille("body.lower") > 0 && wet.permille("fixture.seat") > 0)
                || t - plateau_start > 10_800
            {
                plateau_active = false;
                plateau_done = true;
            }
        }
        if !hold_spent && plateau_done && v.pleasure_charge >= 800 {
            mode.set_hold(&mut host, true).unwrap();
            hold_release_at = Some(t + 900);
            hold_spent = true;
        }
        let _ = out;
        if v.phase == Phase::Refractory {
            for _ in 0..30 {
                mode.step(&mut host, &ModeTickInput::default()).unwrap();
            }
            break;
        }
    }
    mode.stop_take(&mut host).unwrap()
}

fn main() {
    let dir = PathBuf::from(
        std::env::var("PM1_EVIDENCE_DIR").unwrap_or_else(|_| "evidence-pm1".to_string()),
    );
    std::fs::create_dir_all(&dir).expect("create evidence dir");
    let definition = PlaytimeDefinition::blossom();
    for character in ["prima", "yui", "alma"] {
        let take = full_progression_take(character);
        let outcome = replay_take(&definition, &take).expect("take replays");
        assert_eq!(outcome.trajectory_hash, take.trajectory_hash);
        assert_eq!(outcome.cue_track_hash, take.cue_track_hash);
        let path = dir.join(format!("take-{character}-full-progression.json"));
        take.save_to(&path).unwrap();
        println!(
            "{}: {} ticks, trajectory {}, cue track {} -> {}",
            character,
            take.ticks(),
            take.trajectory_hash,
            take.cue_track_hash,
            path.display()
        );
    }
    // The REC-skin take: driven through the Playroom
    // panel binding so the skin lines rendered below
    // are the surface's own. A compact progression
    // (constant full amplitude) suffices — this take
    // exists for the skin.
    let mut panel = PlayroomPlaytime::new("prima").unwrap();
    panel.enter_mode().unwrap();
    panel.set_rec_skin(true).unwrap();
    panel.start_take().unwrap();
    panel.set_amplitude(1000).unwrap();
    let mut input = ModeTickInput::default();
    let mut skin_mid: Vec<String> = Vec::new();
    for t in 0..120_000u64 {
        if t % 45 == 0 {
            input.touch = Some(("pelvis".to_string(), 400));
        } else {
            input.touch = None;
        }
        panel.step(&input).unwrap();
        if t == 2400 {
            skin_mid = panel.rec_skin_lines();
        }
        if panel.view().vector().map(|v| v.phase) == Some(Phase::Refractory) {
            for _ in 0..30 {
                panel.step(&ModeTickInput::default()).unwrap();
            }
            break;
        }
    }
    let take = panel.stop_take().unwrap();
    let outcome = replay_take(&definition, &take).expect("REC take replays");
    assert_eq!(outcome.trajectory_hash, take.trajectory_hash);
    let path = dir.join("take-prima-rec-skin.json");
    take.save_to(&path).unwrap();
    let skin_path = dir.join("rec-skin-mid-take.txt");
    std::fs::write(
        &skin_path,
        format!(
            "REC skin as rendered by the Playroom panel binding mid-take (tick 2400):\n\n{}\n",
            skin_mid.join("\n")
        ),
    )
    .unwrap();
    println!(
        "rec-skin take: {} ticks, trajectory {} -> {} (+ {})",
        take.ticks(),
        take.trajectory_hash,
        path.display(),
        skin_path.display()
    );
}
