//! The clip witness core's conformance tests
//! (hand-animation programme Rung H1): the fixture is the H0
//! golden byte for byte, the Tier 1 chain is deterministic,
//! save/continue reproduces the uninterrupted chain, foreign
//! saves refuse, and the presentation wraps with the hosted
//! clip's loop.

use std::path::PathBuf;
use torogao_runtime::corpus::{CLIP_SAVE_AT, clip_corpus};
use torogao_runtime::host::clip_witness::ClipWitnessCore;
use torogao_runtime::presentation_digest;
use torogao_runtime::{GameCore, Session};

fn read(relative: &str) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(relative);
    std::fs::read(&path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()))
}

#[test]
fn the_witness_fixture_is_the_h0_golden_byte_for_byte() {
    let fixture = read("corpus/clip-witness.clip.json");
    let golden = read("../torogao-format/tests/golden/motion-clip-2/loop-hold.clip.json");
    assert_eq!(fixture, golden);
}

#[test]
fn the_chain_and_presentation_are_deterministic() {
    let corpus = clip_corpus();
    let mut first = Session::new(ClipWitnessCore::golden().expect("witness"));
    let mut second = Session::new(ClipWitnessCore::golden().expect("witness"));
    for commands in &corpus {
        first.step(commands);
        second.step(commands);
        assert_eq!(
            presentation_digest(&first.core().present()),
            presentation_digest(&second.core().present())
        );
    }
    assert_eq!(first.chain(), second.chain());
}

#[test]
fn save_and_continue_matches_the_uninterrupted_chain() {
    let corpus = clip_corpus();
    let mut uninterrupted = Session::new(ClipWitnessCore::golden().expect("witness"));
    for commands in &corpus {
        uninterrupted.step(commands);
    }
    let mut producer = Session::new(ClipWitnessCore::golden().expect("witness"));
    for commands in corpus.iter().take(CLIP_SAVE_AT) {
        producer.step(commands);
    }
    let text = producer.save().expect("checkpoint save");
    let mut continued = Session::new(ClipWitnessCore::golden().expect("witness"));
    continued.load(&text).expect("checkpoint load");
    for commands in corpus.iter().skip(CLIP_SAVE_AT) {
        continued.step(commands);
    }
    // The continued chain is the uninterrupted chain from
    // the checkpoint tip onward, seeded by the envelope.
    assert_eq!(
        continued.chain(),
        &uninterrupted.chain()[CLIP_SAVE_AT - 1..]
    );
}

#[test]
fn a_foreign_clips_save_is_refused_and_changes_nothing() {
    let fac_text = String::from_utf8(read(
        "../torogao-format/tests/golden/motion-clip-2/fac.clip.json",
    ))
    .expect("fac text");
    let foreign = Session::new(ClipWitnessCore::from_clip_text(&fac_text).expect("fac witness"));
    let foreign_save = foreign.save().expect("foreign save");
    // The session envelope binds the core id, so the refusal
    // is exercised at the core's own load: the blob binds the
    // fac clip's freeze hash, not the golden's.
    let blob = torogao_runtime::SaveEnvelope::from_text(&foreign_save)
        .expect("envelope")
        .core_blob;
    let mut witness = ClipWitnessCore::golden().expect("witness");
    let before = witness.state_hash();
    let refused = witness.load(&torogao_runtime::SaveBlob(blob));
    assert!(refused.is_err(), "foreign clip save was accepted");
    assert_eq!(witness.tick(), 0);
    assert_eq!(witness.state_hash(), before);
}

fn gauge_bits(session: &Session<ClipWitnessCore>) -> Vec<(String, u64)> {
    // The gauges alone (the presentation digest also covers
    // the tick, which never wraps — the loop lives in the
    // evaluated values).
    session
        .core()
        .present()
        .gauges
        .iter()
        .map(|gauge| (gauge.id.clone(), gauge.fraction.to_bits()))
        .collect()
}

#[test]
fn the_presentation_wraps_with_the_loop() {
    let mut session = Session::new(ClipWitnessCore::golden().expect("witness"));
    let genesis_gauges = gauge_bits(&session);
    let corpus = clip_corpus();
    for commands in corpus.iter().take(12) {
        session.step(commands);
    }
    let interior_gauges = gauge_bits(&session);
    for commands in corpus.iter().skip(12).take(37) {
        session.step(commands);
    }
    // Tick 49 wraps the 49-frame loop back to frame 0: the
    // evaluated gauges equal genesis's, and differ from an
    // interior frame's. (Frame 48 is not the contrast: a
    // closed loop's last frame equals its first by
    // construction — that is the loop-closure property.)
    assert_eq!(gauge_bits(&session), genesis_gauges);
    assert_ne!(interior_gauges, genesis_gauges);
}

#[test]
fn every_command_is_ignored_and_the_tick_advances() {
    let mut session = Session::new(ClipWitnessCore::golden().expect("witness"));
    let report = session.step(&clip_corpus()[0]);
    assert_eq!(report.tick, 1);
    assert_eq!(report.verdicts.len(), 1);
    assert!(matches!(
        report.verdicts[0],
        torogao_runtime::Verdict::Ignored { .. }
    ));
}
