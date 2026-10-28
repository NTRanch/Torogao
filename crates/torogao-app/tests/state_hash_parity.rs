//! State-hash parity: the app's evaluated state hash must
//! equal a headless engine-only evaluation of the same
//! parameter set. The engine side is computed here from the
//! engine and format crates directly — no app scene or
//! renderer code — following the evaluation-parity probe
//! protocol, then hashed with an independent implementation
//! of the canonical digest (same coverage, re-derived from
//! the digest's documentation in `state_hash`).
//!
//! Fixtures come from TOROGAO_GATE_FIXTURES (a directory
//! holding .inx files) when set; otherwise the workspace's
//! example Aka model is used when present. Tests with no
//! fixture available are skipped with a printed reason, so
//! the workspace suite stays green on machines without the
//! cast rigs.

use torogao_engine::{Puppet, Vec2};
use torogao_format::container::read_inp;
use torogao_format::payload::decode_payload;

use torogao_app::scene::{NamedAssignment, Scene};
use torogao_app::state_hash::evaluated_state_hash;

/// FNV-1a digest over the probe-dump values, re-implemented
/// for this harness.
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

/// Loads and evaluates a puppet with the probe protocol:
/// scripted clock origin, driver reset, fixed-step drivers,
/// assign, advance 1/60, one update.
fn engine_side_evaluation(path: &std::path::Path, assignments: &[(usize, Vec2)]) -> Puppet {
    let bytes = std::fs::read(path).expect("fixture is readable");
    let inp = read_inp(&bytes).expect("fixture container parses");
    let doc = decode_payload(&inp.payload).expect("fixture payload decodes");
    let mut puppet = Puppet::load(&doc, &inp).expect("fixture loads");
    puppet.clock.now = puppet.clock.last;
    puppet.clock.tick();
    puppet.reset_drivers();
    puppet.set_drivers_fixed_step(true);
    for (index, value) in assignments {
        puppet.params[*index].value = *value;
    }
    puppet.clock.now += 1.0 / 60.0;
    puppet.clock.tick();
    puppet.update().expect("fixture evaluates");
    puppet
}

fn fixture_paths() -> Vec<std::path::PathBuf> {
    if let Ok(dir) = std::env::var("TOROGAO_GATE_FIXTURES") {
        let mut paths: Vec<std::path::PathBuf> = std::fs::read_dir(&dir)
            .expect("gate fixture dir is readable")
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .filter(|path| path.extension().map(|ext| ext == "inx").unwrap_or(false))
            .collect();
        paths.sort();
        return paths;
    }
    let aka = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/example-models/aka/Aka.inx");
    if aka.exists() { vec![aka] } else { Vec::new() }
}

#[test]
fn app_state_hash_matches_engine_only_evaluation() {
    let fixtures = fixture_paths();
    if fixtures.is_empty() {
        eprintln!("no gate fixtures available; parity test skipped");
        return;
    }
    for path in fixtures {
        // Defaults first.
        let mut app_scene = Scene::load_file(&path).expect("app loads fixture");
        app_scene
            .evaluate_fresh(&[])
            .expect("app evaluates fixture");
        let app_hash = evaluated_state_hash(&mut app_scene.puppet);
        let mut engine_puppet = engine_side_evaluation(&path, &[]);
        let engine_hash = engine_side_hash(&mut engine_puppet);
        assert_eq!(
            app_hash,
            engine_hash,
            "default-parameter hash mismatch on {}",
            path.display()
        );

        // Then every parameter pushed to its range midpoint
        // (a set no fixture ships as defaults).
        let named: Vec<NamedAssignment> = app_scene
            .puppet
            .params
            .iter()
            .map(|param| NamedAssignment {
                name: param.name.clone(),
                x: (param.min.x + param.max.x) / 2.0,
                y: Some((param.min.y + param.max.y) / 2.0),
            })
            .collect();
        let assignments = app_scene
            .resolve_assignments(&named)
            .expect("midpoints resolve");
        let mut app_scene = Scene::load_file(&path).expect("app reloads fixture");
        app_scene
            .evaluate_fresh(&assignments)
            .expect("app evaluates midpoint set");
        let app_hash = evaluated_state_hash(&mut app_scene.puppet);
        let mut engine_puppet = engine_side_evaluation(&path, &assignments);
        let engine_hash = engine_side_hash(&mut engine_puppet);
        assert_eq!(
            app_hash,
            engine_hash,
            "midpoint-parameter hash mismatch on {}",
            path.display()
        );
    }
}

#[test]
fn state_hash_is_stable_across_repeated_evaluation() {
    let fixtures = fixture_paths();
    if fixtures.is_empty() {
        eprintln!("no gate fixtures available; stability test skipped");
        return;
    }
    let path = &fixtures[0];
    let mut hashes = Vec::new();
    for _ in 0..3 {
        let mut scene = Scene::load_file(path).expect("app loads fixture");
        scene.evaluate_fresh(&[]).expect("app evaluates fixture");
        hashes.push(evaluated_state_hash(&mut scene.puppet));
    }
    assert_eq!(hashes[0], hashes[1]);
    assert_eq!(hashes[1], hashes[2]);
}
