//! Slice tests for the AUTH-G game layer, driven through the
//! face-neutral session API against real project directories.
//! Half validation (the authored behaviour works), half
//! adversarial (corrupt references, ordinal attacks, prose
//! conflicts, deferred verbs refused by name).

use crate::game::codec::{emit_content, parse_content};
use crate::game::model::ContentEntry;
use crate::refusal::{Refusal, RefusalClass};
use crate::session::{Answer, Status, run_requests, run_verb};
use crate::store::{CONTENT_FILE, PROJECT_FILE, Request};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

static COUNTER: AtomicUsize = AtomicUsize::new(0);

fn test_root(name: &str) -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let root = std::env::temp_dir().join(format!(
        "torogao-game-test-{}-{name}-{n}",
        std::process::id()
    ));
    let _removed = std::fs::remove_dir_all(&root);
    root
}

fn resolve_none(_name: &str) -> Result<PathBuf, Refusal> {
    Err(Refusal::new(
        RefusalClass::UnknownTarget,
        "no other projects in tests",
    ))
}

fn verb(root: &Path, name: &str, inputs: Value) -> Answer {
    run_verb(root, root, name, &inputs, &resolve_none)
}

fn script(root: &Path, ops: Vec<(&str, Value)>) -> Answer {
    let requests: Vec<Request> = ops
        .into_iter()
        .map(|(verb, inputs)| Request {
            verb: verb.to_string(),
            inputs,
        })
        .collect();
    run_requests(root, root, &requests)
}

fn init_inputs() -> Value {
    json!({
        "name": "vn_fixture",
        "stamp": {"basis": "test fixture", "lane": "all-ages", "pack": "test", "rating": "E"},
        "consent": {"adult": true, "schema": "torogao-consent/1", "wants_selected_scenes": true},
    })
}

/// Authors the plain-VN fixture: a game, one character, one
/// scene, two chained lines. No lifetime, no deferral, no
/// sidecar prose beyond its lines.
fn author_plain_vn(root: &Path) {
    let answer = script(
        root,
        vec![
            ("project_init", init_inputs()),
            (
                "game_define",
                json!({"id": "vn_fixture", "start_scene": "intro"}),
            ),
            (
                "character_define",
                json!({"id": "amy", "name_ref": "name_amy", "name_text": "Amy"}),
            ),
            ("scene_define", json!({"id": "intro", "presentation": "vn"})),
            (
                "dialogue_line_define",
                json!({
                    "id": "line_1", "scene": "intro", "ordinal": 0, "speaker": "amy",
                    "text_ref": "text_line_1", "text": "Hello. The train is late again.",
                    "next": "line_2",
                }),
            ),
            (
                "dialogue_line_define",
                json!({
                    "id": "line_2", "scene": "intro", "ordinal": 1, "speaker": "narrator",
                    "text_ref": "text_line_2", "text": "She watched the platform clock.",
                }),
            ),
        ],
    );
    assert_eq!(answer.status, Status::Ok, "fixture script: {}", answer.doc);
}

fn read(root: &Path, name: &str) -> String {
    std::fs::read_to_string(root.join(name)).expect("file exists")
}

fn validate_codes(root: &Path) -> Vec<String> {
    let answer = verb(root, "project_validate", json!({}));
    answer.doc["findings"]
        .as_array()
        .expect("findings array")
        .iter()
        .map(|finding| finding["code"].as_str().expect("code").to_string())
        .collect()
}

#[test]
fn sidecar_holds_prose_and_project_holds_references_only() {
    let root = test_root("sidecar");
    author_plain_vn(&root);
    let project_text = read(&root, PROJECT_FILE);
    assert!(project_text.contains("game: Some(Game("), "{project_text}");
    assert!(project_text.contains("text_ref: \"text_line_1\""));
    assert!(
        !project_text.contains("The train is late"),
        "prose leaked into project.ron"
    );
    let content_text = read(&root, CONTENT_FILE);
    let doc = parse_content(&content_text).expect("sidecar parses");
    assert_eq!(doc.locale, "en", "canonical authored locale is en (D-472)");
    assert_eq!(doc.entries.len(), 3);
    assert!(
        doc.entries
            .iter()
            .any(|e| e.id == "text_line_1" && e.text.contains("The train is late"))
    );
    assert_eq!(validate_codes(&root), Vec::<String>::new());
}

#[test]
fn prose_only_edit_leaves_project_bytes_and_replay_identical() {
    // The P2-1 parity gate (D-468): edit the prose in the
    // sidecar; project.ron must not move a byte, validation
    // stays clean, and replay (x3) still reproduces the project.
    let root = test_root("prose-edit");
    author_plain_vn(&root);
    let before = read(&root, PROJECT_FILE);
    let sha_before = verb(&root, "project_inspect", json!({})).doc["sha256"].clone();
    let mut doc = parse_content(&read(&root, CONTENT_FILE)).expect("sidecar parses");
    for entry in &mut doc.entries {
        if entry.id == "text_line_1" {
            entry.text = "Hello. The train is cancelled, actually.".to_string();
        }
    }
    let edited = emit_content(&doc.entries, &doc.locale);
    std::fs::write(root.join(CONTENT_FILE), edited).expect("rewrite sidecar");
    assert_eq!(read(&root, PROJECT_FILE), before, "project.ron churned");
    let sha_after = verb(&root, "project_inspect", json!({})).doc["sha256"].clone();
    assert_eq!(sha_before, sha_after, "project hash churned");
    assert_eq!(validate_codes(&root), Vec::<String>::new());
    for _ in 0..3 {
        let replay = verb(&root, "project_replay", json!({}));
        assert_eq!(replay.status, Status::Ok, "replay: {}", replay.doc);
        assert_eq!(replay.doc["result"], "identical");
    }
}

#[test]
fn missing_sidecar_key_and_missing_sidecar_refuse_dangling() {
    let root = test_root("dangling");
    author_plain_vn(&root);
    // Corrupt the sidecar: drop one referenced key.
    let doc = parse_content(&read(&root, CONTENT_FILE)).expect("sidecar parses");
    let kept: Vec<ContentEntry> = doc
        .entries
        .into_iter()
        .filter(|entry| entry.id != "text_line_2")
        .collect();
    std::fs::write(root.join(CONTENT_FILE), emit_content(&kept, "en")).expect("rewrite");
    let codes = validate_codes(&root);
    assert!(
        codes.iter().any(|code| code == "TG-AUTH-DANGLING"),
        "expected TG-AUTH-DANGLING, got {codes:?}"
    );
    // And with the whole sidecar gone.
    std::fs::remove_file(root.join(CONTENT_FILE)).expect("remove sidecar");
    let codes = validate_codes(&root);
    assert!(
        codes.iter().any(|code| code == "TG-AUTH-DANGLING"),
        "expected TG-AUTH-DANGLING, got {codes:?}"
    );
}

#[test]
fn locale_swap_validates_and_is_preserved_by_later_ops() {
    // The per-locale swap gate: a translation sidecar (same
    // keys, another locale) validates without touching
    // project.ron, and later authoring ops preserve its locale.
    let root = test_root("locale");
    author_plain_vn(&root);
    let before = read(&root, PROJECT_FILE);
    let doc = parse_content(&read(&root, CONTENT_FILE)).expect("sidecar parses");
    let translated: Vec<ContentEntry> = doc
        .entries
        .iter()
        .map(|entry| ContentEntry {
            id: entry.id.clone(),
            text: format!("[fr] {}", entry.text),
        })
        .collect();
    std::fs::write(root.join(CONTENT_FILE), emit_content(&translated, "fr")).expect("swap");
    assert_eq!(read(&root, PROJECT_FILE), before);
    assert_eq!(validate_codes(&root), Vec::<String>::new());
    let answer = verb(
        &root,
        "dialogue_line_define",
        json!({
            "id": "line_3", "scene": "intro", "ordinal": 2, "speaker": "amy",
            "text_ref": "text_line_3", "text": "[fr] Au revoir.",
        }),
    );
    assert_eq!(answer.status, Status::Ok, "{}", answer.doc);
    let after = parse_content(&read(&root, CONTENT_FILE)).expect("sidecar parses");
    assert_eq!(after.locale, "fr", "loaded locale was rewritten");
}

#[test]
fn rig_only_project_emits_no_sidecar_and_no_game_field() {
    // The negative gate: a rig-only project is byte-identical
    // in shape to before the slice — no game field, no
    // content.ron, replay identical.
    let root = test_root("rig-only");
    let answer = script(
        &root,
        vec![
            ("project_init", init_inputs()),
            ("part_create", json!({"id": "body"})),
        ],
    );
    assert_eq!(answer.status, Status::Ok, "{}", answer.doc);
    let project_text = read(&root, PROJECT_FILE);
    assert!(!project_text.contains("game"), "{project_text}");
    assert!(!root.join(CONTENT_FILE).exists(), "sidecar emitted");
    let replay = verb(&root, "project_replay", json!({}));
    assert_eq!(replay.status, Status::Ok);
}

#[test]
fn ordinal_gap_and_taken_ordinal_refuse_game_graph() {
    let root = test_root("ordinals");
    let answer = script(
        &root,
        vec![
            ("project_init", init_inputs()),
            ("game_define", json!({"id": "g"})),
            ("scene_define", json!({"id": "s", "presentation": "vn"})),
        ],
    );
    assert_eq!(answer.status, Status::Ok, "{}", answer.doc);
    let gap = verb(
        &root,
        "dialogue_line_define",
        json!({
            "id": "l_gap", "scene": "s", "ordinal": 1, "speaker": "narrator",
            "text_ref": "t_gap", "text": "Skipped zero.",
        }),
    );
    assert_eq!(gap.status, Status::Refused);
    assert_eq!(gap.doc["code"], "TG-GAME-GRAPH", "{}", gap.doc);
    let first = verb(
        &root,
        "dialogue_line_define",
        json!({
            "id": "l0", "scene": "s", "ordinal": 0, "speaker": "narrator",
            "text_ref": "t0", "text": "First.",
        }),
    );
    assert_eq!(first.status, Status::Ok, "{}", first.doc);
    let taken = verb(
        &root,
        "dialogue_line_define",
        json!({
            "id": "l_taken", "scene": "s", "ordinal": 0, "speaker": "narrator",
            "text_ref": "t_taken", "text": "Also first.",
        }),
    );
    assert_eq!(taken.status, Status::Refused);
    assert_eq!(taken.doc["code"], "TG-GAME-GRAPH", "{}", taken.doc);
}

#[test]
fn prose_redefinition_conflicts_and_identical_redefinition_is_noop() {
    let root = test_root("prose-conflict");
    author_plain_vn(&root);
    let changed = verb(
        &root,
        "dialogue_line_define",
        json!({
            "id": "line_1", "scene": "intro", "ordinal": 0, "speaker": "amy",
            "text_ref": "text_line_1", "text": "Rewritten by redefinition.",
            "next": "line_2",
        }),
    );
    assert_eq!(changed.status, Status::Refused);
    assert_eq!(changed.doc["code"], "TG-AUTH-CONFLICT", "{}", changed.doc);
    let identical = verb(
        &root,
        "dialogue_line_define",
        json!({
            "id": "line_1", "scene": "intro", "ordinal": 0, "speaker": "amy",
            "text_ref": "text_line_1", "text": "Hello. The train is late again.",
            "next": "line_2",
        }),
    );
    assert_eq!(identical.status, Status::Ok, "{}", identical.doc);
    assert_eq!(identical.doc["result"], "no-op");
}

#[test]
fn dialogue_references_to_missing_scene_and_speaker_refuse_dangling() {
    let root = test_root("dialogue-refs");
    let answer = script(
        &root,
        vec![
            ("project_init", init_inputs()),
            ("game_define", json!({"id": "g"})),
            ("scene_define", json!({"id": "s", "presentation": "vn"})),
        ],
    );
    assert_eq!(answer.status, Status::Ok, "{}", answer.doc);
    let no_scene = verb(
        &root,
        "dialogue_line_define",
        json!({
            "id": "l1", "scene": "nowhere", "ordinal": 0, "speaker": "narrator",
            "text_ref": "t1", "text": "Lost.",
        }),
    );
    assert_eq!(no_scene.doc["code"], "TG-AUTH-DANGLING", "{}", no_scene.doc);
    let no_speaker = verb(
        &root,
        "dialogue_line_define",
        json!({
            "id": "l1", "scene": "s", "ordinal": 0, "speaker": "ghost",
            "text_ref": "t1", "text": "Boo.",
        }),
    );
    assert_eq!(
        no_speaker.doc["code"], "TG-AUTH-DANGLING",
        "{}",
        no_speaker.doc
    );
}

#[test]
fn deferred_game_verbs_refuse_by_name() {
    let root = test_root("deferred");
    for name in ["choice_define", "ending_define", "table_define"] {
        let answer = verb(&root, name, json!({}));
        assert_eq!(answer.status, Status::Refused, "{name}");
        assert_eq!(
            answer.doc["code"], "TG-AUTH-UNKNOWN-KIND",
            "{name}: {}",
            answer.doc
        );
    }
}

#[test]
fn orphan_sidecar_entry_is_a_dangling_finding() {
    let root = test_root("orphan");
    author_plain_vn(&root);
    let mut doc = parse_content(&read(&root, CONTENT_FILE)).expect("sidecar parses");
    doc.entries.push(ContentEntry {
        id: "text_orphan".to_string(),
        text: "Nobody says this.".to_string(),
    });
    doc.entries.sort_by(|a, b| a.id.cmp(&b.id));
    std::fs::write(root.join(CONTENT_FILE), emit_content(&doc.entries, "en")).expect("rewrite");
    let answer = verb(&root, "project_validate", json!({}));
    let findings = answer.doc["findings"].as_array().expect("findings");
    assert!(
        findings.iter().any(|finding| {
            finding["code"] == "TG-AUTH-DANGLING"
                && finding["message"]
                    .as_str()
                    .is_some_and(|m| m.contains("text_orphan"))
        }),
        "findings: {}",
        answer.doc
    );
}

#[test]
fn chain_cycle_and_cross_scene_chain_are_graph_findings() {
    let root = test_root("cycle");
    let answer = script(
        &root,
        vec![
            ("project_init", init_inputs()),
            ("game_define", json!({"id": "g", "start_scene": "s"})),
            ("scene_define", json!({"id": "s", "presentation": "vn"})),
            ("scene_define", json!({"id": "t", "presentation": "vn"})),
            (
                "dialogue_line_define",
                json!({
                    "id": "a", "scene": "s", "ordinal": 0, "speaker": "narrator",
                    "text_ref": "ta", "text": "A.", "next": "b",
                }),
            ),
            (
                "dialogue_line_define",
                json!({
                    "id": "b", "scene": "s", "ordinal": 1, "speaker": "narrator",
                    "text_ref": "tb", "text": "B.", "next": "a",
                }),
            ),
            (
                "dialogue_line_define",
                json!({
                    "id": "c", "scene": "t", "ordinal": 0, "speaker": "narrator",
                    "text_ref": "tc", "text": "C.", "next": "a",
                }),
            ),
        ],
    );
    assert_eq!(answer.status, Status::Ok, "{}", answer.doc);
    let answer = verb(&root, "project_validate", json!({}));
    let findings = answer.doc["findings"].as_array().expect("findings");
    let graph = findings
        .iter()
        .filter(|finding| finding["code"] == "TG-GAME-GRAPH")
        .count();
    assert_eq!(graph, 2, "cycle + cross-scene findings: {}", answer.doc);
}

#[test]
fn hook_expression_resolves_against_rig_expressions_at_validate() {
    let root = test_root("hook");
    let answer = script(
        &root,
        vec![
            ("project_init", init_inputs()),
            ("game_define", json!({"id": "g"})),
            ("scene_define", json!({"id": "s", "presentation": "vn"})),
            (
                "dialogue_line_define",
                json!({
                    "id": "l1", "scene": "s", "ordinal": 0, "speaker": "narrator",
                    "text_ref": "t1", "text": "She smiled.", "hook_expression": "smile",
                }),
            ),
        ],
    );
    assert_eq!(answer.status, Status::Ok, "{}", answer.doc);
    let codes = validate_codes(&root);
    assert!(
        codes.iter().any(|code| code == "TG-AUTH-DANGLING"),
        "missing hook should dangle: {codes:?}"
    );
    let answer = verb(
        &root,
        "param_define",
        json!({"id": "smile_amount", "class": "face", "min": 0.0, "default": 0.0, "max": 1.0}),
    );
    assert_eq!(answer.status, Status::Ok, "{}", answer.doc);
    let answer = verb(
        &root,
        "expression_define",
        json!({
            "id": "smile", "blend": "additive",
            "values": [{"parameter": "smile_amount", "value": 1.0}],
        }),
    );
    assert_eq!(answer.status, Status::Ok, "{}", answer.doc);
    assert_eq!(validate_codes(&root), Vec::<String>::new());
}

#[test]
fn game_section_codec_round_trips_every_record_shape() {
    // The record layout is landed whole in P2-1 (slices add
    // verbs, not shapes): a section carrying every record kind
    // — including the P2-2/P2-3 shapes — must round-trip
    // byte-identically through the one serializer.
    use crate::codec::{emit_project, parse_project};
    use crate::game::model::*;
    let game = Game {
        id: "shapes".to_string(),
        format_version: GAME_FORMAT_VERSION,
        save_schema_version: 1,
        start_scene: Some("intro".to_string()),
        clock: Some(Clock {
            slots: vec!["morning".to_string(), "night".to_string()],
            days_per_week: Some(7),
            day_limit: Some(30),
        }),
        stats: vec![StatDef {
            id: "control_level".to_string(),
            scope: StatScope::Character,
            kind: StatKind::Enum(vec!["free".to_string(), "held".to_string()]),
            min: 0,
            max: 1,
            default: 0,
            overflow: Overflow::Clamp,
            lifetime: Some(Lifetime { days: 6 }),
        }],
        tiers: vec![TierDef {
            stat: "control_level".to_string(),
            thresholds: vec![1],
            names: vec!["free".to_string(), "held".to_string()],
        }],
        flags: vec![FlagDef {
            id: "met_amy".to_string(),
            scope: FlagScope::Run,
            default: false,
        }],
        values: vec![ValueDef {
            id: "ready".to_string(),
            expr: Expr::Cmp(
                CmpOp::Ge,
                Box::new(Expr::Stat {
                    character: Some("amy".to_string()),
                    stat: "control_level".to_string(),
                }),
                Box::new(Expr::Lit(1)),
            ),
            value_type: ExprType::Bool,
        }],
        conditions: vec![ConditionDef {
            id: "is_morning".to_string(),
            expr: Expr::Cmp(
                CmpOp::Eq,
                Box::new(Expr::Clock(ClockField::Slot)),
                Box::new(Expr::Lit(0)),
            ),
        }],
        effect_sets: vec![EffectSetDef {
            id: "apply_control".to_string(),
            atoms: vec![
                Atom::StatSet {
                    character: Some("amy".to_string()),
                    stat: "control_level".to_string(),
                    value: Box::new(Expr::Lit(1)),
                },
                Atom::FlagSet {
                    flag: "met_amy".to_string(),
                },
            ],
        }],
        characters: vec![CharacterDef {
            id: "amy".to_string(),
            name_ref: "name_amy".to_string(),
            initials: vec![],
        }],
        scenes: vec![SceneDef {
            id: "intro".to_string(),
            presentation: Presentation::Vn,
        }],
        dialogue: vec![DialogueLine {
            id: "line_1".to_string(),
            scene: "intro".to_string(),
            ordinal: 0,
            speaker: "amy".to_string(),
            text_ref: "text_line_1".to_string(),
            next: None,
            hook_expression: None,
        }],
        triggers: vec![
            TriggerDef {
                id: "level_watch".to_string(),
                kind: TriggerKind::OnThreshold {
                    character: Some("amy".to_string()),
                    stat: "control_level".to_string(),
                    threshold: 1,
                    direction: Direction::Rising,
                },
                gate: None,
                effect_set: "apply_control".to_string(),
                priority: 0,
                repetition: Repetition::Once,
                cooldown_ticks: 3,
                offer: None,
            },
            TriggerDef {
                id: "offer_tea".to_string(),
                kind: TriggerKind::OnTick {
                    slots: Some(vec!["morning".to_string()]),
                    weekdays: None,
                    day_modulo: None,
                    exact_day: None,
                },
                gate: Some("is_morning".to_string()),
                effect_set: "apply_control".to_string(),
                priority: 5,
                repetition: Repetition::Repeat,
                cooldown_ticks: 0,
                offer: Some(OfferDef {
                    expires_days: Some(2),
                }),
            },
        ],
    };
    let project = crate::model::Project {
        game: Some(game),
        ..crate::model::Project::default()
    };
    let text = emit_project(&project);
    let parsed = parse_project(&text).expect("section parses");
    assert_eq!(parsed, project);
    assert_eq!(emit_project(&parsed), text, "emission is byte-stable");
}

// ---- P2-2: state lifetime primitive (D-463) --------------------------

use crate::game::model::GAME_FORMAT_VERSION;
use crate::game::op::LogEntryInput;
use crate::game::save::{load_game, save_game};
use crate::game::simulate::{genesis, parse_log, run_ticks};
use crate::game::state::{GameState, StatKey};

/// Authors the Changr parity fixture: an 8-slot clock, a
/// character-scoped `control_level` stat (0..=5, default 0)
/// carrying a 6-day lifetime, and an `apply_control` event whose
/// effect set raises the level to 1 — the pilot's measured
/// pairing (level 1, `reset_count = 6`).
fn author_changr(root: &Path) {
    let answer = script(
        root,
        vec![
            ("project_init", init_inputs()),
            (
                "game_define",
                json!({"id": "changr_fixture", "start_scene": "room"}),
            ),
            (
                "clock_define",
                json!({
                    "slots": ["morning_1", "morning_2", "afternoon_1", "afternoon_2",
                              "evening_1", "evening_2", "night_1", "night_2"],
                    "days_per_week": 7,
                }),
            ),
            (
                "stat_define",
                json!({
                    "id": "control_level", "scope": "character", "kind": {"kind": "int"},
                    "min": 0, "max": 5, "default": 0, "overflow": "clamp",
                    "lifetime_days": 6,
                }),
            ),
            (
                "character_define",
                json!({"id": "amy", "name_ref": "name_amy", "name_text": "Amy"}),
            ),
            (
                "scene_define",
                json!({"id": "room", "presentation": "room"}),
            ),
            (
                "effect_set_define",
                json!({
                    "id": "apply_control_set",
                    "atoms": [{
                        "kind": "stat_set", "stat": "control_level", "character": "amy",
                        "value": {"kind": "lit", "value": 1},
                    }],
                }),
            ),
            (
                "trigger_define",
                json!({
                    "id": "apply_control",
                    "kind": {"kind": "on_event", "event": "apply_control"},
                    "effect_set": "apply_control_set",
                }),
            ),
        ],
    );
    assert_eq!(answer.status, Status::Ok, "changr fixture: {}", answer.doc);
    assert!(
        validate_codes(root).is_empty(),
        "changr fixture validates clean: {:?}",
        validate_codes(root)
    );
}

fn control_key() -> StatKey {
    StatKey {
        character: Some("amy".to_string()),
        stat: "control_level".to_string(),
    }
}

fn load_project(root: &Path) -> crate::store::Loaded {
    crate::store::load(root)
        .expect("project loads")
        .expect("project exists")
}

fn event_log(entries: &[(u64, &str)], ticks: u64) -> Vec<Option<crate::game::simulate::LogEntry>> {
    let inputs: Vec<LogEntryInput> = entries
        .iter()
        .map(|(tick, id)| LogEntryInput {
            tick: *tick,
            kind: "event".to_string(),
            id: (*id).to_string(),
            option: None,
        })
        .collect();
    parse_log(&inputs, ticks).expect("log parses")
}

fn run_to(
    project: &crate::model::Project,
    state: &mut GameState,
    log: &[Option<crate::game::simulate::LogEntry>],
    target: u64,
) -> (Vec<String>, Vec<String>) {
    let mut events = Vec::new();
    let mut chain = Vec::new();
    run_ticks(project, state, log, target, &mut events, &mut chain).expect("run ticks");
    (events, chain)
}

#[test]
fn changr_decay_holds_five_rollovers_and_resets_on_the_sixth() {
    let root = test_root("changr-decay");
    author_changr(&root);
    let loaded = load_project(&root);
    let project = &loaded.project;
    let log = event_log(&[(1, "apply_control")], 56);
    let mut state = genesis(project).expect("genesis");
    let key = control_key();
    // Applied on tick 1 (day 1): level 1, counter armed at 6.
    let (_events, _chain) = run_to(project, &mut state, &log, 1);
    assert_eq!(state.stats[&key], 1);
    assert_eq!(state.lifetimes[&key], 6);
    // Five day rollovers (ticks 8..40) decrement to 1; the level
    // holds through day 6 (tick 47 is the last tick of day 6).
    let (_events, _chain) = run_to(project, &mut state, &log, 47);
    assert_eq!(state.day, 6);
    assert_eq!(state.stats[&key], 1, "level holds on day 6");
    assert_eq!(state.lifetimes[&key], 1);
    // The sixth rollover (tick 48, the first tick of day 7)
    // hard-resets the stat to its default and disarms.
    let (events, _chain) = run_to(project, &mut state, &log, 48);
    assert_eq!(state.day, 7);
    assert_eq!(state.stats[&key], 0, "level resets on day 7");
    assert!(!state.lifetimes.contains_key(&key));
    assert!(
        events
            .iter()
            .any(|event| event.contains("tick 48") && event.contains("lifetime expired")),
        "expiry is a trace event at the boundary tick: {events:?}"
    );
}

#[test]
fn changr_refresh_rearms_the_full_window() {
    let root = test_root("changr-refresh");
    author_changr(&root);
    let loaded = load_project(&root);
    let project = &loaded.project;
    // Re-application on day 3 (tick 20) re-arms the counter.
    let log = event_log(&[(1, "apply_control"), (20, "apply_control")], 72);
    let mut state = genesis(project).expect("genesis");
    let key = control_key();
    let (_e, _c) = run_to(project, &mut state, &log, 20);
    assert_eq!(state.stats[&key], 1);
    assert_eq!(
        state.lifetimes[&key], 6,
        "refresh re-arms to the full window"
    );
    // Without the refresh the level would have died at tick 48;
    // it now holds to tick 63 and resets at the tick-64 rollover.
    let (_e, _c) = run_to(project, &mut state, &log, 63);
    assert_eq!(state.stats[&key], 1);
    let (_e, _c) = run_to(project, &mut state, &log, 64);
    assert_eq!(state.stats[&key], 0);
}

#[test]
fn changr_simulation_hash_chain_is_identical_across_three_runs() {
    let root = test_root("changr-chain");
    author_changr(&root);
    let inputs = json!({
        "run_seed": 7,
        "ticks": 56,
        "choice_log": [{"tick": 1, "kind": "event", "id": "apply_control"}],
    });
    let first = verb(&root, "game_simulate", inputs.clone());
    assert_eq!(first.status, Status::Ok, "{}", first.doc);
    for _ in 0..2 {
        let again = verb(&root, "game_simulate", inputs.clone());
        assert_eq!(again.doc, first.doc, "run evidence is byte-identical");
    }
    let chain = first.doc["chain"].as_array().expect("chain");
    assert_eq!(chain.len(), 56);
    assert_eq!(first.doc["final"]["day"], 8);
}

#[test]
fn save_round_trip_across_the_expiry_tick_reproduces_the_chain() {
    let root = test_root("changr-save");
    author_changr(&root);
    let loaded = load_project(&root);
    let project = &loaded.project;
    let log = event_log(&[(1, "apply_control")], 56);
    // Uninterrupted reference run.
    let mut reference = genesis(project).expect("genesis");
    let (_e, reference_chain) = run_to(project, &mut reference, &log, 56);
    // Interrupted run: save at tick 24 (mid-window), load,
    // continue across the tick-48 expiry.
    let mut state = genesis(project).expect("genesis");
    let (_e, mut chain) = run_to(project, &mut state, &log, 24);
    let text = save_game(project, &loaded.sha256, &state, 7).expect("save");
    let saved = load_game(project, &loaded.sha256, &text).expect("load");
    assert_eq!(saved.state, state, "the loaded state is the saved state");
    assert_eq!(saved.run_seed, 7);
    let mut resumed = saved.state;
    let (_e, tail) = run_to(project, &mut resumed, &log, 56);
    chain.extend(tail);
    assert_eq!(chain, reference_chain, "the chain survives save/load");
    assert_eq!(resumed, reference);
}

#[test]
fn v1_save_loads_through_the_migration_step() {
    let root = test_root("save-v1");
    author_changr(&root);
    let loaded = load_project(&root);
    let project = &loaded.project;
    let state = genesis(project).expect("genesis");
    let text = save_game(project, &loaded.sha256, &state, 3).expect("save");
    assert!(text.contains("format_version: 2"), "{text}");
    // A v1 save of the same genesis state (no lifetimes armed,
    // no offers — v1 could not carry either) loads through the
    // named migration and yields the same state.
    let v1 = text.replacen("format_version: 2", "format_version: 1", 1);
    let saved = load_game(project, &loaded.sha256, &v1).expect("v1 save loads");
    assert_eq!(saved.state, state);
}

#[test]
fn v1_project_migrates_to_v2_on_the_first_game_op() {
    let root = test_root("project-v1");
    author_changr(&root);
    let text = read(&root, PROJECT_FILE);
    assert!(text.contains("format_version: 2"), "{text}");
    let v1 = text.replacen("format_version: 2", "format_version: 1", 1);
    std::fs::write(root.join(PROJECT_FILE), &v1).expect("rewrite as v1");
    // The v1 text still validates: the sweep rebuilds it
    // byte-identically (migration state is not op-authored).
    assert!(
        validate_codes(&root).is_empty(),
        "v1 project validates clean: {:?}",
        validate_codes(&root)
    );
    // The first game op to land migrates the stamp to the
    // current version.
    let answer = verb(
        &root,
        "flag_define",
        json!({"id": "met_amy", "scope": "run"}),
    );
    assert_eq!(answer.status, Status::Ok, "{}", answer.doc);
    let after = read(&root, PROJECT_FILE);
    assert!(after.contains(&format!("format_version: {GAME_FORMAT_VERSION}")));
    assert!(after.contains("met_amy"));
    assert!(validate_codes(&root).is_empty());
}

#[test]
fn adversarial_saves_refuse_typed_and_load_nothing() {
    let root = test_root("save-adversarial");
    author_changr(&root);
    let loaded = load_project(&root);
    let project = &loaded.project;
    let state = genesis(project).expect("genesis");
    let text = save_game(project, &loaded.sha256, &state, 7).expect("save");
    // Newer format version.
    let newer = text.replacen("format_version: 2", "format_version: 3", 1);
    let err = load_game(project, &loaded.sha256, &newer).expect_err("newer format refuses");
    assert_eq!(err.code(), "TG-GAME-SAVE-SCHEMA", "{err}");
    // Newer authored schema version.
    let schema = text.replacen("schema_version: 1", "schema_version: 2", 1);
    let err = load_game(project, &loaded.sha256, &schema).expect_err("newer schema refuses");
    assert_eq!(err.code(), "TG-GAME-SAVE-SCHEMA", "{err}");
    // Bit-flipped state (day 1 -> day 2) breaks the state hash.
    let flipped = text.replacen("day: 1", "day: 2", 1);
    assert_ne!(flipped, text);
    let err = load_game(project, &loaded.sha256, &flipped).expect_err("corrupt state refuses");
    assert_eq!(err.code(), "TG-GAME-SAVE-SCHEMA", "{err}");
    // A save does not cross projects.
    let other_root = test_root("save-adversarial-other");
    author_plain_vn(&other_root);
    let other = load_project(&other_root);
    let err = load_game(&other.project, &other.sha256, &text).expect_err("foreign save refuses");
    assert_eq!(err.code(), "TG-AUTH-CONFLICT", "{err}");
}

#[test]
fn tier_and_expression_gates_refuse_typed() {
    let root = test_root("tier-expr-gates");
    author_changr(&root);
    // A well-formed tier definition lands.
    let ok = verb(
        &root,
        "stat_tier_define",
        json!({"stat": "control_level", "thresholds": [1, 3], "names": ["none", "light", "deep"]}),
    );
    assert_eq!(ok.status, Status::Ok, "{}", ok.doc);
    // Non-ascending thresholds.
    let bad = verb(
        &root,
        "stat_tier_define",
        json!({"stat": "control_level", "thresholds": [3, 3], "names": ["a", "b", "c"]}),
    );
    assert_eq!(bad.doc["code"], "TG-GAME-TIER", "{}", bad.doc);
    // Wrong band-name count.
    let bad = verb(
        &root,
        "stat_tier_define",
        json!({"stat": "control_level", "thresholds": [2], "names": ["only"]}),
    );
    assert_eq!(bad.doc["code"], "TG-GAME-TIER", "{}", bad.doc);
    // Threshold outside (min, max].
    let bad = verb(
        &root,
        "stat_tier_define",
        json!({"stat": "control_level", "thresholds": [6], "names": ["a", "b"]}),
    );
    assert_eq!(bad.doc["code"], "TG-GAME-TIER", "{}", bad.doc);
    // Tier over an unknown stat dangles.
    let bad = verb(
        &root,
        "stat_tier_define",
        json!({"stat": "nope", "thresholds": [1], "names": ["a", "b"]}),
    );
    assert_eq!(bad.doc["code"], "TG-AUTH-DANGLING", "{}", bad.doc);
    // An expression naming an unknown stat.
    let bad = verb(
        &root,
        "value_define",
        json!({"id": "v_bad", "expr": {"kind": "stat", "stat": "nope"}}),
    );
    assert_eq!(bad.doc["code"], "TG-GAME-EXPR", "{}", bad.doc);
    // A value naming itself: unknown at its own define time —
    // reference cycles are unrepresentable.
    let bad = verb(
        &root,
        "value_define",
        json!({"id": "v_loop", "expr": {"kind": "value", "value": "v_loop"}}),
    );
    assert_eq!(bad.doc["code"], "TG-GAME-EXPR", "{}", bad.doc);
    // A condition must be boolean.
    let bad = verb(
        &root,
        "condition_define",
        json!({"id": "c_bad", "expr": {"kind": "lit", "value": 1}}),
    );
    assert_eq!(bad.doc["code"], "TG-GAME-EXPR", "{}", bad.doc);
    // A literal stat_set outside the stat's range refuses at
    // define time.
    let bad = verb(
        &root,
        "effect_set_define",
        json!({
            "id": "too_far",
            "atoms": [{
                "kind": "stat_set", "stat": "control_level", "character": "amy",
                "value": {"kind": "lit", "value": 9},
            }],
        }),
    );
    assert_eq!(bad.doc["code"], "TG-AUTH-RANGE", "{}", bad.doc);
    // An on_tick trigger naming a slot the clock does not have.
    let bad = verb(
        &root,
        "trigger_define",
        json!({
            "id": "t_bad_slot",
            "kind": {"kind": "on_tick", "slots": ["brunch"]},
            "effect_set": "apply_control_set",
        }),
    );
    assert_eq!(bad.doc["code"], "TG-AUTH-DANGLING", "{}", bad.doc);
}

#[test]
fn enum_stat_shape_is_checked_at_define() {
    let root = test_root("enum-shape");
    let answer = script(
        &root,
        vec![
            ("project_init", init_inputs()),
            ("game_define", json!({"id": "enum_fixture"})),
            (
                "stat_define",
                json!({
                    "id": "mood", "scope": "game",
                    "kind": {"kind": "enum", "values": ["calm", "tense", "wild"]},
                    "min": 0, "max": 5, "default": 0, "overflow": "clamp",
                }),
            ),
        ],
    );
    assert_eq!(answer.status, Status::Refused, "{}", answer.doc);
    assert_eq!(answer.doc["code"], "TG-AUTH-RANGE", "{}", answer.doc);
}

#[test]
fn stats_without_a_lifetime_never_decay() {
    // The negative gate at the interpreter level: a stat with
    // no lifetime field evaluates exactly as before the
    // primitive — nothing arms, nothing decays, over any span.
    let root = test_root("no-lifetime");
    let answer = script(
        &root,
        vec![
            ("project_init", init_inputs()),
            (
                "game_define",
                json!({"id": "plain_fixture", "start_scene": "room"}),
            ),
            (
                "clock_define",
                json!({"slots": ["morning", "night"], "days_per_week": 7}),
            ),
            (
                "stat_define",
                json!({
                    "id": "affection", "scope": "character", "kind": {"kind": "int"},
                    "min": 0, "max": 10, "default": 0, "overflow": "clamp",
                }),
            ),
            (
                "character_define",
                json!({"id": "amy", "name_ref": "name_amy", "name_text": "Amy"}),
            ),
            (
                "scene_define",
                json!({"id": "room", "presentation": "room"}),
            ),
            (
                "effect_set_define",
                json!({
                    "id": "warm_set",
                    "atoms": [{
                        "kind": "stat_set", "stat": "affection", "character": "amy",
                        "value": {"kind": "lit", "value": 4},
                    }],
                }),
            ),
            (
                "trigger_define",
                json!({
                    "id": "warm",
                    "kind": {"kind": "on_event", "event": "warm"},
                    "effect_set": "warm_set",
                }),
            ),
        ],
    );
    assert_eq!(answer.status, Status::Ok, "{}", answer.doc);
    let loaded = load_project(&root);
    let project = &loaded.project;
    let log = event_log(&[(1, "warm")], 40);
    let mut state = genesis(project).expect("genesis");
    let key = StatKey {
        character: Some("amy".to_string()),
        stat: "affection".to_string(),
    };
    let (_e, _c) = run_to(project, &mut state, &log, 40);
    assert_eq!(state.stats[&key], 4, "no lifetime, no decay");
    assert!(state.lifetimes.is_empty());
}
