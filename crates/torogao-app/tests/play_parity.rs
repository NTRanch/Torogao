//! Phase D exit-gate parity: the hash play_state reports
//! for a played scene equals a headless engine-only
//! evaluation of the same puppet through the same player
//! sequence at the same step count.
//!
//! Both sides run the identical sequence: fresh evaluation
//! at the default parameters (the probe protocol) -> play
//! the named animation (looping) -> 45 fixed steps ->
//! pause -> seek to frame 60 -> 1 step. The engine side
//! uses the engine + format crates directly with an
//! independently written digest (the `state_hash_parity`
//! twin); the app side uses Scene + PlayHost, the exact
//! calls the session surface and the window make. Prima's
//! live pendulum drivers make the evaluated state a
//! function of the whole step history, so equality at
//! equal step count is the determinism claim.
//!
//! Fixtures: rich.inx ships in the workspace corpus; the
//! Phase D performance fixture (prima-greeting.inx) is
//! resolved through TOROGAO_PHASE_D_FIXTURES and skipped
//! when the gate environment does not provide it.

use std::path::PathBuf;

use torogao_app::player::PlayHost;
use torogao_app::scene::{STEP_SECONDS, Scene};
use torogao_app::state_hash::evaluated_state_hash;
use torogao_engine::{AnimationPlayer, Puppet, Vec2};
use torogao_format::container::read_inp;
use torogao_format::payload::decode_payload;

/// FNV-1a digest over the evaluated state, re-implemented
/// for this harness (same coverage as `state_hash_parity`).
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

/// Loads a puppet and evaluates it fresh with the probe
/// protocol (scripted clock origin, driver reset,
/// fixed-step drivers, advance 1/60, one update).
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

/// One fixed step on the engine side, mirroring the play
/// host: advance the clock, tick, the player renders its
/// frame, the puppet updates.
fn engine_side_step(puppet: &mut Puppet, player: &mut AnimationPlayer) {
    puppet.clock.now += STEP_SECONDS;
    puppet.clock.tick();
    player.update(STEP_SECONDS as f32, puppet);
    puppet.update().expect("step evaluates");
}

/// The shared play sequence on the engine side.
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

/// The shared play sequence on the app side: Scene +
/// PlayHost, the session surface's own calls.
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

fn assert_parity_x3(path: &std::path::Path, animation: &str) {
    let mut hashes = Vec::new();
    for _ in 0..3 {
        let app_hash = app_path_hash(path, animation);
        let engine_hash = engine_path_hash(path, animation);
        assert_eq!(
            app_hash,
            engine_hash,
            "app path != engine path for {animation} on {}",
            path.display()
        );
        hashes.push(app_hash);
    }
    assert!(
        hashes.windows(2).all(|pair| pair[0] == pair[1]),
        "x3 runs disagree for {animation}: {hashes:?}"
    );
}

fn workspace_rich() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("tests")
        .join("fixtures")
        .join("p2")
        .join("fixtures")
        .join("rich.inx")
}

fn phase_d_fixture(name: &str) -> Option<PathBuf> {
    let dir = std::env::var_os("TOROGAO_PHASE_D_FIXTURES")?;
    let path = PathBuf::from(dir).join(name);
    path.exists().then_some(path)
}

#[test]
fn play_state_hash_matches_engine_evaluation_rich_blink_x3() {
    assert_parity_x3(&workspace_rich(), "Blink");
}

#[test]
fn play_state_hash_matches_engine_evaluation_greeting_x3() {
    let Some(path) = phase_d_fixture("prima-greeting.inx") else {
        eprintln!("skipping: TOROGAO_PHASE_D_FIXTURES not provided");
        return;
    };
    assert_parity_x3(&path, "Greeting");
}

/// The smoothness trace behind the gate report's numbers:
/// stepping the Greeting performance through a full loop,
/// the per-step parameter motion is continuous — it moves,
/// no step spikes, and the loop seam sits inside the step
/// family (no seam spike).
#[test]
fn greeting_motion_is_continuous_across_the_loop_seam() {
    let Some(path) = phase_d_fixture("prima-greeting.inx") else {
        eprintln!("skipping: TOROGAO_PHASE_D_FIXTURES not provided");
        return;
    };
    let mut scene = Scene::load_file(&path).expect("scene loads");
    scene.evaluate_fresh(&[]).expect("fresh evaluation");
    let mut play = PlayHost::new();
    play.play(&scene, Some("Greeting"), true, None)
        .expect("play starts");
    let snapshot = |scene: &Scene| -> Vec<Vec2> {
        scene
            .puppet
            .params
            .iter()
            .map(|param| param.last_internal)
            .collect()
    };
    let mut previous = snapshot(&scene);
    let mut deltas: Vec<f32> = Vec::new();
    for _ in 0..121 {
        play.step(&mut scene, 1).expect("step");
        let current = snapshot(&scene);
        let delta: f32 = previous
            .iter()
            .zip(&current)
            .map(|(a, b)| (a.x - b.x).abs() + (a.y - b.y).abs())
            .sum();
        deltas.push(delta);
        previous = current;
    }
    let mut sorted = deltas.clone();
    sorted.sort_by(f32::total_cmp);
    let median = sorted[sorted.len() / 2];
    let max = sorted[sorted.len() - 1];
    let seam = deltas[119]; // the step that wraps frame 120 -> 0
    let mean: f32 = deltas.iter().sum::<f32>() / deltas.len() as f32;
    eprintln!(
        "greeting trace: mean={mean:.5} median={median:.5} max={max:.5} seam={seam:.5} min={:.5}",
        sorted[0]
    );
    assert!(median > 0.0, "the performance must move");
    assert!(
        max <= median * 8.0,
        "no step may spike: max {max} vs median {median}"
    );
    assert!(
        seam <= median * 4.0,
        "loop seam must sit in the step family: seam {seam} vs median {median}"
    );
}
