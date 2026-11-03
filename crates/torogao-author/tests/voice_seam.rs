//! The voice seam gate (voice plan §3 + §5, ruling D-483).
//!
//! Proves, on the fixture project under `fixtures/voice-seam/`:
//! the `kind: voice` audio binding round-trips the project
//! codec deterministically; the §3 chain resolves end-to-end
//! (dialogue line → content.ron prose, voice binding → torori
//! delivery record, delivered asset + manifest hash) with no
//! torori call and no generated audio — the dialogue, content,
//! delivery, and timeline files are fixture stubs, marked as
//! such; the §5 subtitle cues derive from sidecar prose +
//! delivered durations and land in a take-manifest-shaped
//! document, never in project.ron; and the shippable sweep
//! refuses voice bindings whose torori identity is not
//! ear-admitted in a caller-supplied registry snapshot.
//!
//! Half validation, half adversarial, per the house test rule.

mod common;

use torogao_author::codec::{emit_project, parse_project};
use torogao_author::intake::sha256_hex;
use torogao_author::model::{AudioBindingKind, Project};
use torogao_author::ron::{Ron, emit_document, parse_document};
use torogao_author::validate::validate;
use torogao_author::voice::{VoiceLockState, VoiceRegistrySnapshot, validate_shippable};
use torogao_author::{Refusal, RefusalClass};

const PROJECT_RON: &str = include_str!("fixtures/voice-seam/project.ron");
const CONTENT_RON: &str = include_str!("fixtures/voice-seam/content.ron");
const DIALOGUE_RON: &str = include_str!("fixtures/voice-seam/dialogue.ron");
const DELIVERY_RON: &str = include_str!("fixtures/voice-seam/delivery.ron");
const TIMELINE_RON: &str = include_str!("fixtures/voice-seam/timeline.ron");
const REGISTRY_ADMITTED: &str = include_str!("fixtures/voice-seam/registry-ear-admitted.ron");
const REGISTRY_ANCHORED: &str = include_str!("fixtures/voice-seam/registry-anchored.ron");
const REGISTRY_DESIGNED: &str = include_str!("fixtures/voice-seam/registry-designed.ron");

const IDENTITY: &str = "voice.trial.prima-1";
const EDITED_LINE_ONE: &str = "The rain has stopped. The pier is ours.";

// ---- fixture stub records -------------------------------------
//
// The dialogue/content/delivery/timeline dialects below are
// fixture-local stubs (their landed formats belong to the
// game-layer slices). Extraction is strict: a malformed stub
// is a broken fixture, and the tests say so with expect().

#[derive(Clone, Debug, PartialEq)]
struct ContentLine {
    id: String,
    text: String,
}

#[derive(Clone, Debug, PartialEq)]
struct DialogueLine {
    audio_binding: String,
    id: String,
    speaker: String,
    text_ref: String,
}

#[derive(Clone, Debug, PartialEq)]
struct DeliveredLine {
    asset_id: String,
    duration_ms: i64,
    line_id: String,
    manifest_sha256: String,
}

#[derive(Clone, Debug, PartialEq)]
struct TimelineEntry {
    line_id: String,
    start_ms: i64,
}

fn fields_of<'a>(value: &'a Ron, expected: &str) -> &'a [(String, Ron)] {
    match value {
        Ron::Struct(name, fields) if name == expected => fields,
        other => panic!("expected a {expected} record, found {other:?}"),
    }
}

fn field<'a>(fields: &'a [(String, Ron)], key: &str) -> &'a Ron {
    fields
        .iter()
        .find(|(name, _)| name == key)
        .map(|(_, value)| value)
        .unwrap_or_else(|| panic!("record lacks field '{key}'"))
}

fn as_text(value: &Ron) -> &str {
    match value {
        Ron::Str(text) => text,
        other => panic!("expected a string, found {other:?}"),
    }
}

fn as_int(value: &Ron) -> i64 {
    match value {
        Ron::Int(number) => *number,
        other => panic!("expected an integer, found {other:?}"),
    }
}

fn as_list(value: &Ron) -> &[Ron] {
    match value {
        Ron::List(items) => items,
        other => panic!("expected a list, found {other:?}"),
    }
}

fn parse_content(text: &str) -> Vec<ContentLine> {
    let root = parse_document(text).expect("content stub parses");
    let fields = fields_of(&root, "Content");
    assert_eq!(as_text(field(fields, "locale")), "en", "authored locale");
    as_list(field(fields, "lines"))
        .iter()
        .map(|item| {
            let fields = fields_of(item, "ContentLine");
            ContentLine {
                id: as_text(field(fields, "id")).to_string(),
                text: as_text(field(fields, "text")).to_string(),
            }
        })
        .collect()
}

fn parse_dialogue(text: &str) -> Vec<DialogueLine> {
    let root = parse_document(text).expect("dialogue stub parses");
    let fields = fields_of(&root, "Dialogue");
    as_list(field(fields, "lines"))
        .iter()
        .map(|item| {
            let fields = fields_of(item, "DialogueLine");
            DialogueLine {
                audio_binding: as_text(field(fields, "audio_binding")).to_string(),
                id: as_text(field(fields, "id")).to_string(),
                speaker: as_text(field(fields, "speaker")).to_string(),
                text_ref: as_text(field(fields, "text_ref")).to_string(),
            }
        })
        .collect()
}

fn parse_delivery(text: &str) -> Vec<DeliveredLine> {
    let root = parse_document(text).expect("delivery stub parses");
    let fields = fields_of(&root, "Delivery");
    as_list(field(fields, "records"))
        .iter()
        .map(|item| {
            let fields = fields_of(item, "DeliveredLine");
            DeliveredLine {
                asset_id: as_text(field(fields, "asset_id")).to_string(),
                duration_ms: as_int(field(fields, "duration_ms")),
                line_id: as_text(field(fields, "line_id")).to_string(),
                manifest_sha256: as_text(field(fields, "manifest_sha256")).to_string(),
            }
        })
        .collect()
}

fn parse_timeline(text: &str) -> Vec<TimelineEntry> {
    let root = parse_document(text).expect("timeline stub parses");
    let fields = fields_of(&root, "Timeline");
    as_list(field(fields, "entries"))
        .iter()
        .map(|item| {
            let fields = fields_of(item, "TimelineEntry");
            TimelineEntry {
                line_id: as_text(field(fields, "line_id")).to_string(),
                start_ms: as_int(field(fields, "start_ms")),
            }
        })
        .collect()
}

// ---- the §3 chain ----------------------------------------------

/// One dialogue line with every §3 link resolved.
#[derive(Clone, Debug, PartialEq)]
struct ResolvedLine {
    asset_id: String,
    character: String,
    delivery_identity: String,
    delivery_record: String,
    duration_ms: i64,
    line_id: String,
    manifest_sha256: String,
    text: String,
    text_ref: String,
}

fn dangling(message: String) -> Refusal {
    Refusal::new(RefusalClass::Dangling, message)
}

/// Resolves each dialogue line, in dialogue order:
/// line → (content.ron text via text_ref, voice binding →
/// delivery record, delivered asset + manifest hash).
fn resolve_chain(
    project: &Project,
    dialogue: &[DialogueLine],
    content: &[ContentLine],
    delivery: &[DeliveredLine],
) -> Result<Vec<ResolvedLine>, Refusal> {
    let mut resolved = Vec::new();
    for line in dialogue {
        let binding = project.audio_binding(&line.audio_binding).ok_or_else(|| {
            dangling(format!(
                "dialogue line '{}' names missing audio binding '{}'",
                line.id, line.audio_binding
            ))
        })?;
        let AudioBindingKind::Voice(voice) = &binding.kind else {
            panic!("voice fixture binding");
        };
        if voice.character != line.speaker {
            return Err(dangling(format!(
                "dialogue line '{}' speaker '{}' disagrees with binding '{}' character '{}'",
                line.id, line.speaker, binding.id, voice.character
            )));
        }
        let prose = content
            .iter()
            .find(|entry| entry.id == line.text_ref)
            .ok_or_else(|| {
                dangling(format!(
                    "dialogue line '{}' text_ref '{}' is absent from the content sidecar",
                    line.id, line.text_ref
                ))
            })?;
        let delivered = delivery
            .iter()
            .find(|record| record.line_id == line.id)
            .ok_or_else(|| {
                dangling(format!(
                    "no delivered asset recorded for dialogue line '{}'",
                    line.id
                ))
            })?;
        resolved.push(ResolvedLine {
            asset_id: delivered.asset_id.clone(),
            character: voice.character.clone(),
            delivery_identity: voice.delivery.identity.clone(),
            delivery_record: voice.delivery.record.clone(),
            duration_ms: delivered.duration_ms,
            line_id: line.id.clone(),
            manifest_sha256: delivered.manifest_sha256.clone(),
            text: prose.text.clone(),
            text_ref: line.text_ref.clone(),
        });
    }
    Ok(resolved)
}

// ---- the §5 cue derivation --------------------------------------

/// One subtitle cue: text is the sidecar prose verbatim
/// (resolved at derivation time), timing is the take timeline
/// start plus the delivered manifest's measured duration.
#[derive(Clone, Debug, PartialEq)]
struct Cue {
    end_ms: i64,
    line_id: String,
    start_ms: i64,
    text: String,
    text_ref: String,
}

fn derive_cues(resolved: &[ResolvedLine], timeline: &[TimelineEntry]) -> Result<Vec<Cue>, Refusal> {
    let mut cues = Vec::new();
    for line in resolved {
        let entry = timeline
            .iter()
            .find(|entry| entry.line_id == line.line_id)
            .ok_or_else(|| {
                dangling(format!(
                    "take timeline has no start for line '{}'",
                    line.line_id
                ))
            })?;
        cues.push(Cue {
            end_ms: entry.start_ms + line.duration_ms,
            line_id: line.line_id.clone(),
            start_ms: entry.start_ms,
            text: line.text.clone(),
            text_ref: line.text_ref.clone(),
        });
    }
    Ok(cues)
}

/// The take-manifest-shaped document the cues ship in
/// (voice plan §5): timing + text_ref rows. The prose itself
/// never enters the manifest — it resolves from the sidecar.
fn cues_to_ron(project_name: &str, cues: &[Cue]) -> Ron {
    Ron::Struct(
        "TakeManifest".to_string(),
        vec![
            (
                "cues".to_string(),
                Ron::List(
                    cues.iter()
                        .map(|cue| {
                            Ron::Struct(
                                "SubtitleCue".to_string(),
                                vec![
                                    ("end_ms".to_string(), Ron::Int(cue.end_ms)),
                                    ("line_id".to_string(), Ron::Str(cue.line_id.clone())),
                                    ("start_ms".to_string(), Ron::Int(cue.start_ms)),
                                    ("text_ref".to_string(), Ron::Str(cue.text_ref.clone())),
                                ],
                            )
                        })
                        .collect(),
                ),
            ),
            ("project".to_string(), Ron::Str(project_name.to_string())),
        ],
    )
}

fn fixture_project() -> Project {
    parse_project(PROJECT_RON).expect("fixture project parses")
}

fn fixture_chain() -> Vec<ResolvedLine> {
    resolve_chain(
        &fixture_project(),
        &parse_dialogue(DIALOGUE_RON),
        &parse_content(CONTENT_RON),
        &parse_delivery(DELIVERY_RON),
    )
    .expect("fixture chain resolves")
}

// ---- validation -------------------------------------------------

#[test]
fn binding_codec_round_trip_and_determinism() {
    let project = fixture_project();
    assert_eq!(project.audio_bindings.len(), 2);
    let first = project
        .audio_binding("voice-prima-001")
        .expect("first binding");
    let AudioBindingKind::Voice(voice) = &first.kind else {
        panic!("voice fixture binding");
    };
    assert_eq!(voice.character, "prima");
    assert_eq!(voice.delivery.identity, IDENTITY);
    assert_eq!(voice.delivery.record, "cond.prima.greet.1");
    // Round-trip ×3, the tree's determinism idiom: emission is
    // byte-stable and parse(emit(v)) == v.
    let mut emitted = String::new();
    for _ in 0..3 {
        let text = emit_project(&project);
        assert!(emitted.is_empty() || emitted == text, "emission drifted");
        emitted = text;
        let reparsed = parse_project(&emitted).expect("emitted project re-parses");
        assert_eq!(reparsed, project, "round trip changed the project");
    }
}

#[test]
fn absent_section_parses_empty_and_emits_absent() {
    // A project without bindings carries no audio_bindings
    // section at all (Amendment 2 §Item G4: absent sections
    // are absent), and a pre-seam file's bytes are untouched
    // by the seam: clearing the section re-emits exactly the
    // section-free document.
    let mut bare = fixture_project();
    bare.audio_bindings.clear();
    let text = emit_project(&bare);
    assert!(
        !text.contains("audio_bindings"),
        "empty section leaked into the document"
    );
    let reparsed = parse_project(&text).expect("section-free project parses");
    assert!(reparsed.audio_bindings.is_empty());
    assert_eq!(emit_project(&reparsed), text, "bytes moved");
}

#[test]
fn authoring_validate_accepts_unadmitted_bindings() {
    // The fixture identity is Designed in torori's registry:
    // legal while authoring. project_validate (which never
    // runs the shippable sweep) is clean on the canonical
    // document.
    let project = fixture_project();
    let text = emit_project(&project);
    let root = common::fresh("voice-seam-authoring");
    let findings = validate(&project, &text, &root);
    assert!(findings.is_empty(), "authoring findings: {findings:?}");
}

#[test]
fn shippable_passes_when_ear_admitted() {
    let snapshot = VoiceRegistrySnapshot::parse(REGISTRY_ADMITTED).expect("snapshot parses");
    assert_eq!(
        snapshot.lock_state(IDENTITY),
        Some(VoiceLockState::EarAdmitted)
    );
    let findings = validate_shippable(&fixture_project(), Some(&snapshot));
    assert!(findings.is_empty(), "shippable findings: {findings:?}");
}

#[test]
fn chain_resolves_end_to_end() {
    let resolved = fixture_chain();
    assert_eq!(resolved.len(), 2);
    let greet = &resolved[0];
    assert_eq!(greet.line_id, "line.001");
    assert_eq!(greet.text, "The rain has stopped. Shall we go?");
    assert_eq!(greet.text_ref, "line.001");
    assert_eq!(greet.character, "prima");
    assert_eq!(greet.delivery_identity, IDENTITY);
    assert_eq!(greet.delivery_record, "cond.prima.greet.1");
    assert_eq!(greet.asset_id, "asset.stub.prima-001");
    assert_eq!(
        greet.manifest_sha256,
        "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
    );
    let train = &resolved[1];
    assert_eq!(train.text, "Wait. Listen — the night train is coming.");
    assert_eq!(train.delivery_record, "cond.prima.train.1");
    assert_eq!(train.asset_id, "asset.stub.prima-002");
}

#[test]
fn subtitle_cues_derive_from_sidecar_and_manifest() {
    let project = fixture_project();
    let cues = derive_cues(&fixture_chain(), &parse_timeline(TIMELINE_RON)).expect("cues derive");
    assert_eq!(cues.len(), 2);
    // Cue text is the sidecar prose verbatim; timing is the
    // timeline start plus the delivered duration.
    assert_eq!(cues[0].text, "The rain has stopped. Shall we go?");
    assert_eq!((cues[0].start_ms, cues[0].end_ms), (0, 4200));
    assert_eq!(cues[1].text, "Wait. Listen — the night train is coming.");
    assert_eq!((cues[1].start_ms, cues[1].end_ms), (5000, 8600));
    // The rows ship as a take-manifest document: ids, timings,
    // text_refs — and no prose anywhere in it.
    let manifest = emit_document(&cues_to_ron(&project.header.name, &cues));
    let expected = "\
TakeManifest(
    cues: [
        SubtitleCue(
            end_ms: 4200,
            line_id: \"line.001\",
            start_ms: 0,
            text_ref: \"line.001\",
        ),
        SubtitleCue(
            end_ms: 8600,
            line_id: \"line.002\",
            start_ms: 5000,
            text_ref: \"line.002\",
        ),
    ],
    project: \"voice-seam-fixture\",
)
";
    assert_eq!(manifest, expected);
    assert!(
        !manifest.contains("night train"),
        "prose leaked into the manifest"
    );
    // And no prose in project.ron either (ruling D-468).
    let project_text = emit_project(&project);
    assert!(!project_text.contains("night train"));
    assert!(!project_text.contains("rain has stopped"));
}

#[test]
fn prose_edit_leaves_binding_chain_intact() {
    let project = fixture_project();
    let before_text = emit_project(&project);
    let before_hash = sha256_hex(before_text.as_bytes());
    // Edit the sidecar prose for line.001, nothing else.
    let edited = CONTENT_RON.replace("The rain has stopped. Shall we go?", EDITED_LINE_ONE);
    assert_ne!(edited, CONTENT_RON, "the edit did not land");
    // The project document and its hash do not move (D-468).
    let after_text = emit_project(&project);
    assert_eq!(after_text, before_text);
    assert_eq!(sha256_hex(after_text.as_bytes()), before_hash);
    // The chain still resolves; only the prose follows.
    let resolved = resolve_chain(
        &project,
        &parse_dialogue(DIALOGUE_RON),
        &parse_content(&edited),
        &parse_delivery(DELIVERY_RON),
    )
    .expect("chain resolves after the prose edit");
    assert_eq!(resolved[0].text, EDITED_LINE_ONE);
    assert_eq!(resolved[0].delivery_record, "cond.prima.greet.1");
    assert_eq!(resolved[0].asset_id, "asset.stub.prima-001");
    assert_eq!(resolved[1], fixture_chain()[1], "the other line moved");
    // Cues follow the prose, timings unchanged.
    let cues = derive_cues(&resolved, &parse_timeline(TIMELINE_RON)).expect("cues derive");
    assert_eq!(cues[0].text, EDITED_LINE_ONE);
    assert_eq!((cues[0].start_ms, cues[0].end_ms), (0, 4200));
}

// ---- adversarial -------------------------------------------------

#[test]
fn shippable_refuses_below_ear_admitted() {
    for (name, text) in [
        ("anchored", REGISTRY_ANCHORED),
        ("designed", REGISTRY_DESIGNED),
    ] {
        let snapshot = VoiceRegistrySnapshot::parse(text).expect("snapshot parses");
        let findings = validate_shippable(&fixture_project(), Some(&snapshot));
        assert_eq!(findings.len(), 2, "{name}: one finding per binding");
        for finding in &findings {
            assert_eq!(finding.class, RefusalClass::VoiceUnadmitted);
            assert_eq!(finding.code(), "TG-AUTH-VOICE-UNADMITTED");
            assert!(
                finding.message.contains(IDENTITY),
                "{name}: finding names the identity: {}",
                finding.message
            );
        }
    }
}

#[test]
fn shippable_refuses_closed_without_snapshot() {
    // No snapshot at all: every voice binding refuses.
    let findings = validate_shippable(&fixture_project(), None);
    assert_eq!(findings.len(), 2);
    assert!(
        findings
            .iter()
            .all(|finding| finding.class == RefusalClass::VoiceUnadmitted)
    );
    // But a project with no voice bindings ships under no
    // snapshot: refuse-closed must not invent findings.
    let mut bare = fixture_project();
    bare.audio_bindings.clear();
    assert!(validate_shippable(&bare, None).is_empty());
}

#[test]
fn shippable_refuses_identity_absent_from_snapshot() {
    let text = "\
VoiceRegistrySnapshot(
    identities: [
        VoiceIdentityState(
            identity: \"voice.someone.else\",
            lock_state: EarAdmitted,
        ),
    ],
)
";
    let snapshot = VoiceRegistrySnapshot::parse(text).expect("snapshot parses");
    let findings = validate_shippable(&fixture_project(), Some(&snapshot));
    assert_eq!(findings.len(), 2);
    assert!(
        findings
            .iter()
            .all(|finding| finding.message.contains("absent")),
        "findings name the absence: {findings:?}"
    );
}

#[test]
fn dangling_audio_binding_caught() {
    let mut dialogue = parse_dialogue(DIALOGUE_RON);
    dialogue[1].audio_binding = "voice-missing".to_string();
    let refusal = resolve_chain(
        &fixture_project(),
        &dialogue,
        &parse_content(CONTENT_RON),
        &parse_delivery(DELIVERY_RON),
    )
    .expect_err("a missing binding must refuse");
    assert_eq!(refusal.class, RefusalClass::Dangling);
    assert!(refusal.message.contains("voice-missing"));
}

#[test]
fn dangling_delivery_ref_caught() {
    // The delivery stub without line.002's record: the chain
    // from dialogue to a delivered asset is broken, and the
    // resolver says which line dangles.
    let mut delivery = parse_delivery(DELIVERY_RON);
    delivery.retain(|record| record.line_id != "line.002");
    let refusal = resolve_chain(
        &fixture_project(),
        &parse_dialogue(DIALOGUE_RON),
        &parse_content(CONTENT_RON),
        &delivery,
    )
    .expect_err("a missing delivery record must refuse");
    assert_eq!(refusal.class, RefusalClass::Dangling);
    assert!(refusal.message.contains("line.002"));
}

#[test]
fn unknown_binding_kind_refused() {
    let text = PROJECT_RON.replace("kind: Voice", "kind: Music");
    let refusal = parse_project(&text).expect_err("an unknown kind must refuse");
    assert_eq!(refusal.class, RefusalClass::UnknownKind);
    assert_eq!(refusal.code(), "TG-AUTH-UNKNOWN-KIND");
}

#[test]
fn empty_delivery_record_refused_by_sweep() {
    // The codec carries the row (a reference is just text);
    // the rebuild sweep refuses an empty delivery reference,
    // so the project cannot validate.
    let mut project = fixture_project();
    let AudioBindingKind::Voice(voice) = &mut project.audio_bindings[0].kind else {
        panic!("voice fixture binding");
    };
    voice.delivery.record = String::new();
    let text = emit_project(&project);
    let root = common::fresh("voice-seam-empty-record");
    let findings = validate(&project, &text, &root);
    assert!(
        findings
            .iter()
            .any(|finding| finding.class == RefusalClass::Malformed),
        "expected a Malformed finding, found: {findings:?}"
    );
}

#[test]
fn duplicate_binding_ids_refused() {
    let text = PROJECT_RON.replace("voice-prima-002", "voice-prima-001");
    let refusal = parse_project(&text).expect_err("duplicate ids must refuse");
    assert_eq!(refusal.class, RefusalClass::Malformed);
}

#[test]
fn explicit_empty_section_is_not_canonical() {
    // audio_bindings: [] written out by hand parses to the
    // empty section — and validate refuses the document as
    // non-canonical, because the one emitter omits it.
    let mut bare = fixture_project();
    bare.audio_bindings.clear();
    let canonical = emit_project(&bare);
    let hand_written = canonical.replace(
        "    assets: [],\n",
        "    assets: [],\n    audio_bindings: [],\n",
    );
    assert_ne!(hand_written, canonical, "the splice did not land");
    let parsed = parse_project(&hand_written).expect("hand-written form parses");
    let root = common::fresh("voice-seam-empty-section");
    let findings = validate(&parsed, &hand_written, &root);
    assert!(
        findings
            .iter()
            .any(|finding| finding.class == RefusalClass::NonCanonical),
        "expected a NonCanonical finding, found: {findings:?}"
    );
}

#[test]
fn snapshot_refuses_duplicates_and_unknown_states() {
    let duplicate = "\
VoiceRegistrySnapshot(
    identities: [
        VoiceIdentityState(
            identity: \"voice.trial.prima-1\",
            lock_state: Designed,
        ),
        VoiceIdentityState(
            identity: \"voice.trial.prima-1\",
            lock_state: EarAdmitted,
        ),
    ],
)
";
    let refusal = VoiceRegistrySnapshot::parse(duplicate)
        .expect_err("a snapshot naming an identity twice must refuse");
    assert_eq!(refusal.class, RefusalClass::Malformed);
    let unknown = REGISTRY_ADMITTED.replace("EarAdmitted", "Shipped");
    let refusal =
        VoiceRegistrySnapshot::parse(&unknown).expect_err("an unknown lock state must refuse");
    assert_eq!(refusal.class, RefusalClass::UnknownKind);
}
