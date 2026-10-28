//! Phase F repertoire parity: the authored cast
//! repertoire (Greeting + Daydream for Prima, Yui, and
//! Alma, written through the INP data format from each
//! rig's own EXT named-state recipes) plays identically
//! through the app path and the engine-only path — the
//! play_parity protocol (fresh evaluation -> play
//! looping -> 45 steps -> pause -> seek 60 -> 1 step),
//! three times per performance, hashes equal and stable.
//!
//! Two more claims are gated here: a performance actually
//! moves (its evaluated hashes across a full span take
//! many distinct values — a frozen rig would pass parity
//! trivially), and each performance is seamless at the
//! data level (every lane samples identically at frame 0
//! and at its length, so the loop closes).
//!
//! Fixtures resolve through TOROGAO_PHASE_F_FIXTURES and
//! are skipped when the gate environment does not
//! provide them, the play_parity pattern.

use std::collections::BTreeSet;
use std::path::PathBuf;

use torogao_app::player::PlayHost;
use torogao_app::scene::{STEP_SECONDS, Scene};
use torogao_app::state_hash::evaluated_state_hash;
use torogao_engine::{AnimationPlayer, Puppet};
use torogao_format::container::read_inp;
use torogao_format::payload::decode_payload;

/// FNV-1a digest over the evaluated state (the
/// play_parity harness's independent re-implementation).
fn engine_side_hash(puppet: &mut Puppet) -> u64 {
    fn mix(state: &mut u64, value: f32) {
        for byte in value.to_bits().to_le_bytes() {
            *state ^= u64::from(byte);
            *state = state.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    let mut state: u64 = 0xcbf2_9ce4_8422_2325;
    for param in &puppet.params {
        mix(&mut state, param.last_internal.x);
        mix(&mut state, param.last_internal.y);
    }
    for index in puppet.nodes.preorder(puppet.nodes.root) {
        let global = puppet.nodes.transform(index);
        for value in [
            global.translation.x,
            global.translation.y,
            global.translation.z,
            global.rotation.x,
            global.rotation.y,
            global.rotation.z,
            global.scale.x,
            global.scale.y,
        ] {
            mix(&mut state, value);
        }
        for element in global.trs.m {
            mix(&mut state, element);
        }
        for entry in &puppet.nodes.nodes[index].deformation {
            mix(&mut state, entry.x);
            mix(&mut state, entry.y);
        }
    }
    state
}

fn engine_side_fresh(path: &std::path::Path) -> Puppet {
    let bytes = std::fs::read(path).expect("fixture is readable");
    let inp = read_inp(&bytes).expect("fixture container parses");
    let doc = decode_payload(&inp.payload).expect("fixture payload decodes");
    let mut puppet = Puppet::load(&doc, &inp).expect("fixture loads");
    puppet.clock.now = puppet.clock.last;
    puppet.clock.tick();
    puppet.reset_drivers();
    puppet.set_drivers_fixed_step(true);
    puppet.clock.now += STEP_SECONDS;
    puppet.clock.tick();
    puppet.update().expect("fixture evaluates");
    puppet
}

fn engine_side_step(puppet: &mut Puppet, player: &mut AnimationPlayer) {
    puppet.clock.now += STEP_SECONDS;
    puppet.clock.tick();
    player.update(STEP_SECONDS as f32, puppet);
    puppet.update().expect("step evaluates");
}

fn engine_path_hash(path: &std::path::Path, animation: &str) -> u64 {
    let mut puppet = engine_side_fresh(path);
    let mut player = AnimationPlayer::new();
    player
        .create_or_get(&puppet, animation)
        .expect("animation exists");
    player
        .get_mut(animation)
        .expect("playback exists")
        .play(true, true);
    for _ in 0..45 {
        engine_side_step(&mut puppet, &mut player);
    }
    player.get_mut(animation).expect("playback exists").pause();
    let anim = puppet
        .animations
        .iter()
        .find(|(name, _)| name == animation)
        .map(|(_, anim)| anim)
        .expect("animation record");
    player
        .get_mut(animation)
        .expect("playback exists")
        .seek(anim, 60);
    engine_side_step(&mut puppet, &mut player);
    engine_side_hash(&mut puppet)
}

fn app_path_hash(path: &std::path::Path, animation: &str) -> u64 {
    let mut scene = Scene::load_file(path).expect("scene loads");
    scene.evaluate_fresh(&[]).expect("fresh evaluation");
    let mut play = PlayHost::new();
    play.play(&scene, Some(animation), true, None)
        .expect("play starts");
    play.step(&mut scene, 45).expect("45 steps");
    play.pause();
    play.seek(&scene, 60).expect("seek");
    play.step(&mut scene, 1).expect("one step");
    evaluated_state_hash(&mut scene.puppet)
}

fn phase_f_fixture(name: &str) -> Option<PathBuf> {
    let dir = std::env::var_os("TOROGAO_PHASE_F_FIXTURES")?;
    let path = PathBuf::from(dir).join(name);
    path.exists().then_some(path)
}

const REPERTOIRE: [(&str, &str); 6] = [
    ("prima-repertoire.inx", "Greeting"),
    ("prima-repertoire.inx", "Daydream"),
    ("yui-repertoire.inx", "Greeting"),
    ("yui-repertoire.inx", "Daydream"),
    ("alma-repertoire.inx", "Greeting"),
    ("alma-repertoire.inx", "Daydream"),
];

#[test]
fn repertoire_plays_identically_on_both_paths_x3() {
    let mut ran = 0;
    for (fixture, animation) in REPERTOIRE {
        let Some(path) = phase_f_fixture(fixture) else {
            continue;
        };
        ran += 1;
        let mut hashes = Vec::new();
        for _ in 0..3 {
            let app_hash = app_path_hash(&path, animation);
            let engine_hash = engine_path_hash(&path, animation);
            assert_eq!(
                app_hash, engine_hash,
                "app path != engine path for {animation} on {fixture}"
            );
            hashes.push(app_hash);
        }
        assert!(
            hashes.windows(2).all(|pair| pair[0] == pair[1]),
            "x3 runs disagree for {animation} on {fixture}: {hashes:?}"
        );
    }
    if ran == 0 {
        eprintln!("skipping: TOROGAO_PHASE_F_FIXTURES not provided");
    }
}

#[test]
fn repertoire_performances_move_and_close_seamlessly() {
    for (fixture, animation) in REPERTOIRE {
        let Some(path) = phase_f_fixture(fixture) else {
            continue;
        };
        // Data-level seamlessness: every lane samples the
        // same at frame 0 and at the animation's length.
        let puppet = engine_side_fresh(&path);
        let record = puppet
            .animations
            .iter()
            .find(|(name, _)| name == animation)
            .map(|(_, anim)| anim)
            .unwrap_or_else(|| panic!("{animation} exists on {fixture}"));
        assert!(!record.lanes.is_empty(), "{animation} has lanes");
        for lane in &record.lanes {
            let start = lane.sample(0.0, false);
            let end = lane.sample(record.length as f32, false);
            assert!(
                (start - end).abs() <= 1e-6,
                "{animation} on {fixture}: lane {} axis {} opens at {start} but closes at {end}",
                lane.param_uuid,
                lane.axis
            );
        }
        // The performance moves: stepping through it, the
        // evaluated state takes many distinct hashes.
        let mut scene = Scene::load_file(&path).expect("scene loads");
        scene.evaluate_fresh(&[]).expect("fresh evaluation");
        let mut play = PlayHost::new();
        play.play(&scene, Some(animation), true, None)
            .expect("play starts");
        let mut hashes: BTreeSet<u64> = BTreeSet::new();
        let steps = record.length.max(1) as u64;
        for _ in 0..steps {
            play.step(&mut scene, 1).expect("step");
            hashes.insert(evaluated_state_hash(&mut scene.puppet));
        }
        assert!(
            hashes.len() >= 20,
            "{animation} on {fixture}: only {} distinct evaluated states across {steps} steps — the performance barely moves",
            hashes.len()
        );
    }
}
