//! Stage 3.3 gate: the governed census against the real
//! cast models, end-to-end latency on the apply loop,
//! tracking-loss hold, mimicry record/replay determinism,
//! and viseme timing against the baked record.
//!
//! Fixtures: the bundled example models (Aka, Midori)
//! under the workspace `fixtures/example-models/`.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde_json::json;

use torogao_app::scene::Scene;
use torogao_app::vtuber::{LiveTracking, VisemeDriver, replay_mimicry};
use torogao_cli::viseme::{VisemeContract, bake_visemes};
use torogao_vmc::arkit::{GOVERNED_PARAMETERS, governed_values};
use torogao_vmc::vmc::{BonePose, VmcFrame};

fn model_path(folder: &str, file: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/example-models")
        .join(folder)
        .join(file)
}

fn load_primed(folder: &str, file: &str) -> Scene {
    let mut scene = Scene::load_file(&model_path(folder, file)).expect("model loads");
    scene
        .evaluate_fresh(&[])
        .expect("the shared evaluation protocol primes the scene");
    scene
}

fn param_value(scene: &Scene, name: &str) -> [f32; 2] {
    let index = scene
        .puppet
        .param_index(name)
        .unwrap_or_else(|| panic!("model carries {name}"));
    let value = scene.puppet.params[index].value;
    [value.x, value.y]
}

/// A scripted performance frame: both blinks, jaw, and a
/// yawed head — the channels every latency/mimicry script
/// below shares.
fn scripted_frame(step: u32) -> VmcFrame {
    let mut frame = VmcFrame {
        blends: vec![
            ("eyeBlinkLeft".to_string(), (step % 30) as f32 / 30.0),
            ("eyeBlinkRight".to_string(), (step % 30) as f32 / 30.0),
            ("jawOpen".to_string(), (step % 45) as f32 / 45.0),
        ],
        ..VmcFrame::default()
    };
    let half_yaw = (step as f32 * 3.0).to_radians() / 2.0;
    frame.bones.push(BonePose {
        name: "Head".to_string(),
        position: [0.0, 1.6, 0.0],
        rotation: [0.0, half_yaw.sin(), 0.0, half_yaw.cos()],
    });
    frame
}

/// One frame carrying every mapped ARKit channel plus
/// both governed bones.
fn full_frame() -> VmcFrame {
    let mut frame = VmcFrame::default();
    for (name, value) in [
        ("eyeBlinkLeft", 0.11),
        ("eyeBlinkRight", 0.22),
        ("eyeLookDownLeft", 0.0),
        ("eyeLookDownRight", 0.0),
        ("eyeLookInLeft", 0.0),
        ("eyeLookInRight", 0.0),
        ("eyeLookOutLeft", 0.33),
        ("eyeLookOutRight", 0.0),
        ("eyeLookUpLeft", 0.44),
        ("eyeLookUpRight", 0.0),
        ("jawOpen", 0.55),
        ("mouthFrownLeft", 0.0),
        ("mouthFrownRight", 0.0),
        ("mouthSmileLeft", 0.66),
        ("mouthSmileRight", 0.66),
    ] {
        frame.blends.push((name.to_string(), value));
    }
    for bone in ["Head", "Chest"] {
        frame.bones.push(BonePose {
            name: bone.to_string(),
            position: [0.0, 1.5, 0.0],
            rotation: [0.0, 0.0, 0.0, 1.0],
        });
    }
    frame
}

#[test]
fn census_governed_parameters_exist_on_the_cast_models() {
    for (folder, file) in [("aka", "Aka.inx"), ("midori", "Midori.inx")] {
        let scene = load_primed(folder, file);
        for name in GOVERNED_PARAMETERS {
            assert!(
                scene.puppet.param_index(name).is_some(),
                "{file} carries the governed parameter {name}"
            );
        }
    }
    // One fully-populated frame governs the whole table.
    let values = governed_values(&full_frame());
    assert_eq!(
        values.keys().map(String::as_str).collect::<Vec<_>>(),
        GOVERNED_PARAMETERS,
        "all nine governed parameters are covered, none beyond them"
    );
    assert_eq!(values["Eye:: Left:: Blink"].x, Some(0.11));
    assert_eq!(values["Eye:: Left:: Move"].x, Some(0.33));
    assert_eq!(values["Eye:: Left:: Move"].y, Some(0.44));
    assert_eq!(values["Mouth:: Shape"].x, Some(0.55));
}

#[test]
fn latency_stays_within_two_frames_over_240_frames() {
    const FRAMES: u64 = 240;
    let mut scene = load_primed("aka", "Aka.inx");
    let mut live = LiveTracking::new();
    // Delivery schedule: frame s arrives at tick s + (s % 2)
    // — up to two frames land per tick, the loop applies one.
    let mut delivered: Vec<bool> = vec![false; FRAMES as usize];
    let mut applied = 0_u64;
    let mut max_latency = 0_u64;
    let mut now = 0_u64;
    while applied < FRAMES {
        for stamp in 0..FRAMES {
            if !delivered[stamp as usize] && stamp + (stamp % 2) == now {
                live.ingest(stamp, scripted_frame(stamp as u32));
                delivered[stamp as usize] = true;
            }
        }
        if let Some(frame) = live.tick(&mut scene, now).expect("tick applies") {
            applied += 1;
            assert!(
                frame.latency_frames <= 2,
                "frame {} applied at {now} is {} frames late",
                frame.stamp,
                frame.latency_frames
            );
            max_latency = max_latency.max(frame.latency_frames);
        }
        now += 1;
        assert!(now < 4 * FRAMES, "the schedule drains in bounded ticks");
    }
    assert_eq!(applied, FRAMES);
    assert_eq!(live.driver().dropped_frames(), 0, "nothing was dropped");
    eprintln!("latency gate: {FRAMES} frames applied, max latency {max_latency} frame(s)");
}

#[test]
fn tracking_loss_holds_the_last_governed_state() {
    let mut scene = load_primed("aka", "Aka.inx");
    let mut live = LiveTracking::new();
    for stamp in 0..30_u64 {
        live.ingest(stamp, scripted_frame(stamp as u32));
        live.tick(&mut scene, stamp).expect("tick applies");
    }
    let held_state = live.driver().state().clone();
    let held_values: BTreeMap<String, [f32; 2]> = GOVERNED_PARAMETERS
        .iter()
        .map(|name| (name.to_string(), param_value(&scene, name)))
        .collect();
    for now in 30..120_u64 {
        assert!(
            live.tick(&mut scene, now).expect("quiet tick").is_none(),
            "no frame applies while the feed is down"
        );
    }
    assert_eq!(live.driver().state(), &held_state, "driver state holds");
    for (name, value) in &held_values {
        assert_eq!(
            &param_value(&scene, name),
            value,
            "the puppet's {name} never moved during the outage"
        );
    }
    assert!(
        live.driver().holding(120, 30),
        "the driver reports the hold"
    );
}

#[test]
fn mimicry_record_replay_is_deterministic() {
    let mut scene = load_primed("aka", "Aka.inx");
    let mut live = LiveTracking::new();
    for stamp in 0..120_u64 {
        live.ingest(stamp, scripted_frame(stamp as u32));
        live.tick(&mut scene, stamp).expect("tick applies");
    }
    let take = live.take();
    assert_eq!(take.frames.len(), 120, "every applied frame was recorded");
    assert_eq!(
        take.frames[0].len(),
        5,
        "the script governs both blinks, jaw, and the head pair per frame"
    );
    let json = take.to_json().expect("take serializes");
    let parsed = torogao_app::vtuber::MimicryTake::from_json(&json).expect("take parses");
    assert_eq!(parsed, take, "the take survives its JSON round-trip");
    let mut hashes = Vec::new();
    for _ in 0..3 {
        let mut replay_scene = load_primed("aka", "Aka.inx");
        hashes.push(replay_mimicry(&parsed, &mut replay_scene).expect("replay runs"));
    }
    assert_eq!(hashes[0], hashes[1], "replays are bit-identical");
    assert_eq!(hashes[1], hashes[2], "replays are bit-identical");
    eprintln!("mimicry gate: 120-frame sequence hash {}", hashes[0]);
}

#[test]
fn viseme_bake_drives_the_mouth_on_the_recorded_frames() {
    let mut targets: BTreeMap<String, BTreeMap<String, f64>> = BTreeMap::new();
    targets.insert(
        "sil".to_string(),
        BTreeMap::from([("Mouth:: Shape".to_string(), 0.0)]),
    );
    targets.insert(
        "a".to_string(),
        BTreeMap::from([("Mouth:: Shape".to_string(), 1.0)]),
    );
    let contract = VisemeContract {
        set_name: "gate".to_string(),
        parameters: vec!["Mouth:: Shape".to_string()],
        targets,
        path: String::new(),
    };
    let track = json!({
        "format": "torogao-viseme-track/1",
        "fps": 60,
        "durationSeconds": 2.0,
        "blendSeconds": 0.0,
        "events": [
            { "viseme": "a", "start": 0.5, "end": 1.5 },
            { "viseme": "sil", "start": 1.5, "end": 2.0 }
        ]
    });
    let curves = bake_visemes(&track, &contract).expect("the track bakes");
    assert_eq!(curves.steps(), 120);
    let series = &curves.series[0];
    assert_eq!(series.values[29], 0.0, "silence holds before the event");
    assert_eq!(series.values[30], 1.0, "the vowel lands on its frame");
    assert_eq!(series.values[89], 1.0, "the vowel holds");
    assert_eq!(series.values[90], 0.0, "silence resumes on its frame");

    let mut scene = load_primed("aka", "Aka.inx");
    let mut driver = VisemeDriver::new(&scene, curves).expect("driver resolves");
    let mut seen: Vec<f32> = Vec::new();
    for _ in 0..120 {
        assert!(
            driver.tick(&mut scene).expect("tick applies"),
            "curve applies"
        );
        seen.push(param_value(&scene, "Mouth:: Shape")[0]);
    }
    assert!(
        !driver.tick(&mut scene).expect("exhausted"),
        "the curve ends"
    );
    // The puppet's mouth x at each step equals the baked
    // record's value at that step: timing error 0 frames.
    assert_eq!(seen[29], 0.0);
    assert_eq!(seen[30], 1.0);
    assert_eq!(seen[89], 1.0);
    assert_eq!(seen[90], 0.0);
}
