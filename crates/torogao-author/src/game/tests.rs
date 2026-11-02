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
fn flow_verbs_are_live_not_deferred() {
    // The AUTH-G flow verbs landed in Stage 2B: they no longer
    // refuse by name. With no game section and empty inputs the
    // refusal is the ordinary gate one (no section / malformed
    // inputs), never the deferred-verb unknown-kind refusal.
    let root = test_root("flow-live");
    for name in [
        "choice_define",
        "ending_define",
        "gallery_entry_define",
        "game_save_schema_define",
        "schedule_define",
        "scene_bind_rig",
        "scene_exit_define",
    ] {
        let answer = verb(&root, name, json!({}));
        assert_eq!(answer.status, Status::Refused, "{name}");
        assert!(
            !answer.doc["message"]
                .as_str()
                .unwrap_or_default()
                .contains("deferred to a later stage"),
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
            participants: vec!["amy".to_string()],
            backdrop: Some("bg_intro".to_string()),
            entry_condition: None,
            on_enter: None,
            on_exit: None,
            thread_interlude: Some(ThreadInterludeDef {
                thread: "night_thread".to_string(),
                reply_delay_ms: 3000,
            }),
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
        appearances: vec![AppearanceDef {
            base: vec![
                AppearanceEntry {
                    slot: "body".to_string(),
                    value: AppearanceValue::Text("base_body".to_string()),
                },
                AppearanceEntry {
                    slot: "scale".to_string(),
                    value: AppearanceValue::Ints(vec![1000]),
                },
            ],
            character: "amy".to_string(),
            slots: vec![
                AppearanceSlot {
                    id: "body".to_string(),
                    kind: AppearanceKind::Text,
                },
                AppearanceSlot {
                    id: "scale".to_string(),
                    kind: AppearanceKind::Ints,
                },
            ],
        }],
        visual_states: vec![
            VisualStateDef {
                character: "amy".to_string(),
                delta: vec![AppearanceEntry {
                    slot: "body".to_string(),
                    value: AppearanceValue::Text("grown_body".to_string()),
                }],
                state: "Growth-1".to_string(),
            },
            VisualStateDef {
                character: "amy".to_string(),
                delta: vec![],
                state: "MindControl-1".to_string(),
            },
        ],
        streams: vec![RngStreamDef {
            id: "battle".to_string(),
            algorithm: RNG_ALGORITHM.to_string(),
        }],
        tables: vec![TableDef {
            id: "encounter".to_string(),
            stream: "battle".to_string(),
            outcomes: vec![
                TableOutcome {
                    id: "glimpse".to_string(),
                    weight: TableWeight::Expr(Expr::Add(
                        Box::new(Expr::Lit(12)),
                        Box::new(Expr::Stat {
                            character: None,
                            stat: "control_level".to_string(),
                        }),
                    )),
                    gate: Some("is_morning".to_string()),
                    effect_set: "apply_control".to_string(),
                },
                TableOutcome {
                    id: "calm".to_string(),
                    weight: TableWeight::Static(88),
                    gate: None,
                    effect_set: "apply_control".to_string(),
                },
            ],
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
        alter_ego_veto: Some("apply_control".to_string()),
        choices: vec![ChoiceDef {
            id: "first_choice".to_string(),
            scene: "intro".to_string(),
            prompt_line: Some("text_prompt".to_string()),
            options: vec![
                ChoiceOption {
                    id: "stay".to_string(),
                    label_ref: "text_stay".to_string(),
                    visible_if: None,
                    enabled_if: None,
                    sort: None,
                    origin: ChoiceOrigin::Player,
                    effect_set: Some("apply_control".to_string()),
                    target: ChoiceTarget::Line("line_1".to_string()),
                },
                ChoiceOption {
                    id: "leave".to_string(),
                    label_ref: "text_leave".to_string(),
                    visible_if: Some("is_morning".to_string()),
                    enabled_if: None,
                    sort: Some(Expr::Clock(ClockField::Day)),
                    origin: ChoiceOrigin::AlterEgo,
                    effect_set: None,
                    target: ChoiceTarget::EndingCheck,
                },
            ],
        }],
        endings: vec![EndingDef {
            id: "held_ending".to_string(),
            condition: "is_morning".to_string(),
            priority: 10,
            epilogue_scene: "intro".to_string(),
        }],
        gallery_entries: vec![GalleryEntryDef {
            id: "first_glimpse".to_string(),
            category: "scenes".to_string(),
            unlock: GalleryUnlock::Condition("is_morning".to_string()),
            replay_scene: "intro".to_string(),
            replay_line: "line_1".to_string(),
            thumbnail: None,
        }],
        schedules: vec![ScheduleDef {
            character: "amy".to_string(),
            slots: vec![
                ScheduleSlot {
                    slot: "morning".to_string(),
                    scene: "intro".to_string(),
                },
                ScheduleSlot {
                    slot: "night".to_string(),
                    scene: "intro".to_string(),
                },
            ],
            overrides: vec![ScheduleOverride {
                condition: "is_morning".to_string(),
                scene: "intro".to_string(),
            }],
        }],
        scene_bindings: vec![SceneBindingDef {
            scene: "intro".to_string(),
            participant: "amy".to_string(),
            rig_model: "amy_rig".to_string(),
            entry_expression: "neutral".to_string(),
            entry_motion: None,
        }],
        scene_exits: vec![SceneExitDef {
            scene: "intro".to_string(),
            id: "to_hall".to_string(),
            target: "intro".to_string(),
            condition: None,
            effect_set: Some("apply_control".to_string()),
        }],
        threads: vec![ThreadDef {
            id: "night_thread".to_string(),
            participants: vec!["amy".to_string()],
            messages: vec![
                ThreadMessage {
                    id: "msg_1".to_string(),
                    speaker: "amy".to_string(),
                    line: "text_msg_1".to_string(),
                    at: 0,
                },
                ThreadMessage {
                    id: "msg_2".to_string(),
                    speaker: "amy".to_string(),
                    line: "text_msg_2".to_string(),
                    at: 1,
                },
            ],
        }],
        save_schema: Some(SaveSchema {
            version: 2,
            slots: 8,
            slot_holds: SlotHolds::OneRun,
            slot_fields: vec![
                "clock".to_string(),
                "scene".to_string(),
                "stats".to_string(),
            ],
            globals_fields: vec![
                "ending".to_string(),
                "flags_globals".to_string(),
                "flags_run".to_string(),
                "gallery".to_string(),
                "rng_state".to_string(),
                "triggers".to_string(),
            ],
            hash_algorithm: GAME_HASH_ALGO.to_string(),
            policy: SaveContextPolicy::SafeScenesOnly(vec!["intro".to_string()]),
            migrations: vec![Migration {
                from: 1,
                name: "first".to_string(),
            }],
        }),
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
    let mut state = genesis(project, 7).expect("genesis");
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
    let mut state = genesis(project, 7).expect("genesis");
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
    let mut reference = genesis(project, 7).expect("genesis");
    let (_e, reference_chain) = run_to(project, &mut reference, &log, 56);
    // Interrupted run: save at tick 24 (mid-window), load,
    // continue across the tick-48 expiry.
    let mut state = genesis(project, 7).expect("genesis");
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
    let state = genesis(project, 7).expect("genesis");
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
    let state = genesis(project, 7).expect("genesis");
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
    let mut state = genesis(project, 7).expect("genesis");
    let key = StatKey {
        character: Some("amy".to_string()),
        stat: "affection".to_string(),
    };
    let (_e, _c) = run_to(project, &mut state, &log, 40);
    assert_eq!(state.stats[&key], 4, "no lifetime, no decay");
    assert!(state.lifetimes.is_empty());
}

// ---- P2-3: deferral semantics (D-464) --------------------------------

/// Authors the deferral fixture: Amy starts with affection 1,
/// so the gate condition `offer_ready` holds from genesis. The
/// `gift` trigger matches the afternoon_1 slot, is gated on
/// `offer_ready`, adds +1 affection when it fires, and carries
/// an offer expiring after 2 days.
fn author_offer_game(root: &Path) {
    let answer = script(
        root,
        vec![
            ("project_init", init_inputs()),
            (
                "game_define",
                json!({"id": "offer_fixture", "start_scene": "room"}),
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
                    "id": "affection", "scope": "character", "kind": {"kind": "int"},
                    "min": 0, "max": 10, "default": 0, "overflow": "clamp",
                }),
            ),
            (
                "character_define",
                json!({
                    "id": "amy", "name_ref": "name_amy", "name_text": "Amy",
                    "initials": [{"stat": "affection", "value": 1}],
                }),
            ),
            (
                "scene_define",
                json!({"id": "room", "presentation": "room"}),
            ),
            (
                "condition_define",
                json!({
                    "id": "offer_ready",
                    "expr": {
                        "kind": "cmp", "op": "ge",
                        "left": {"kind": "stat", "stat": "affection", "character": "amy"},
                        "right": {"kind": "lit", "value": 1},
                    },
                }),
            ),
            (
                "effect_set_define",
                json!({
                    "id": "gift_set",
                    "atoms": [{
                        "kind": "stat_add", "stat": "affection", "character": "amy",
                        "amount": {"kind": "lit", "value": 1},
                    }],
                }),
            ),
            (
                "trigger_define",
                json!({
                    "id": "gift",
                    "kind": {"kind": "on_tick", "slots": ["afternoon_1"]},
                    "effect_set": "gift_set",
                    "gate": "offer_ready",
                    "offer": {"expires_days": 2},
                }),
            ),
        ],
    );
    assert_eq!(answer.status, Status::Ok, "offer fixture: {}", answer.doc);
    assert!(
        validate_codes(root).is_empty(),
        "offer fixture validates: {:?}",
        validate_codes(root)
    );
}

fn offer_log(
    entries: &[(u64, &str, &str)],
    ticks: u64,
) -> Vec<Option<crate::game::simulate::LogEntry>> {
    let inputs: Vec<LogEntryInput> = entries
        .iter()
        .map(|(tick, id, option)| LogEntryInput {
            tick: *tick,
            kind: "offer".to_string(),
            id: (*id).to_string(),
            option: Some((*option).to_string()),
        })
        .collect();
    parse_log(&inputs, ticks).expect("log parses")
}

fn offered_count(events: &[String]) -> usize {
    events
        .iter()
        .filter(|event| event.contains("offered"))
        .count()
}

#[test]
fn decline_defers_for_the_day_and_reoffers_the_next_matching_day() {
    let root = test_root("offer-decline");
    author_offer_game(&root);
    let loaded = load_project(&root);
    let project = &loaded.project;
    let log = offer_log(&[(3, "gift", "decline"), (11, "gift", "accept")], 20);
    let mut state = genesis(project, 7).expect("genesis");
    let key = StatKey {
        character: Some("amy".to_string()),
        stat: "affection".to_string(),
    };
    // Tick 2 is day 1's afternoon_1: the trigger offers instead
    // of firing (affection unchanged).
    let (events, _c) = run_to(project, &mut state, &log, 2);
    assert_eq!(offered_count(&events), 1);
    assert!(state.triggers["gift"].offer.is_some());
    assert_eq!(
        state.stats[&key], 1,
        "an offer does not apply its effect set"
    );
    // Decline on day 1: no re-offer for the rest of day 1.
    let (events, _c) = run_to(project, &mut state, &log, 7);
    assert_eq!(offered_count(&events), 0, "no re-offer on the decline day");
    assert_eq!(state.triggers["gift"].deferred_day, Some(1));
    assert!(state.triggers["gift"].offer.is_none());
    // Day 2's afternoon_1 (tick 10) re-offers; accepting on
    // tick 11 applies the effect set.
    let (events, _c) = run_to(project, &mut state, &log, 10);
    assert!(
        events
            .iter()
            .any(|event| event.contains("tick 10") && event.contains("offered")),
        "re-offer on the next matching day: {events:?}"
    );
    let (events, _c) = run_to(project, &mut state, &log, 11);
    assert!(
        events
            .iter()
            .any(|event| event.contains("tick 11") && event.contains("accepted")),
        "{events:?}"
    );
    assert_eq!(state.stats[&key], 2, "acceptance applies the effect set");
    assert!(state.triggers["gift"].offer.is_none());
}

#[test]
fn decline_suppresses_even_an_every_tick_trigger_for_the_whole_day() {
    // The deferral's teeth: a trigger matching every tick is
    // offered once on day 1, declined, and stays silent for the
    // remaining ticks of day 1 — it re-offers on day 2's first
    // tick.
    let root = test_root("offer-deferral-teeth");
    let answer = script(
        &root,
        vec![
            ("project_init", init_inputs()),
            (
                "game_define",
                json!({"id": "teeth_fixture", "start_scene": "room"}),
            ),
            (
                "clock_define",
                json!({"slots": ["a", "b", "c", "d"], "days_per_week": 7}),
            ),
            (
                "stat_define",
                json!({
                    "id": "visits", "scope": "game", "kind": {"kind": "int"},
                    "min": 0, "max": 100, "default": 0, "overflow": "clamp",
                }),
            ),
            (
                "scene_define",
                json!({"id": "room", "presentation": "room"}),
            ),
            (
                "effect_set_define",
                json!({
                    "id": "visit_set",
                    "atoms": [{
                        "kind": "stat_add", "stat": "visits",
                        "amount": {"kind": "lit", "value": 1},
                    }],
                }),
            ),
            (
                "trigger_define",
                json!({
                    "id": "daily_visit",
                    "kind": {"kind": "on_tick"},
                    "effect_set": "visit_set",
                    "offer": {},
                }),
            ),
        ],
    );
    assert_eq!(answer.status, Status::Ok, "{}", answer.doc);
    let loaded = load_project(&root);
    let project = &loaded.project;
    let log = offer_log(&[(2, "daily_visit", "decline")], 8);
    let mut state = genesis(project, 7).expect("genesis");
    let (events, _c) = run_to(project, &mut state, &log, 2);
    assert_eq!(
        offered_count(&events),
        1,
        "offered on tick 1, declined on tick 2"
    );
    assert_eq!(state.triggers["daily_visit"].deferred_day, Some(1));
    // Tick 3 matches too (the pattern is every tick) but the
    // day-1 deferral suppresses it.
    let (events, _c) = run_to(project, &mut state, &log, 3);
    assert_eq!(
        offered_count(&events),
        0,
        "suppressed for the rest of day 1"
    );
    // Tick 4 rolls over to day 2: the deferral clears and the
    // trigger re-offers at once.
    let (events, _c) = run_to(project, &mut state, &log, 4);
    assert_eq!(offered_count(&events), 1, "re-offer on day 2's first tick");
    assert_eq!(state.day, 2);
}

#[test]
fn offer_expiry_fires_on_the_lifetime_clock_at_the_boundary_tick() {
    let root = test_root("offer-expiry");
    author_offer_game(&root);
    let loaded = load_project(&root);
    let project = &loaded.project;
    let log = parse_log(&[], 24).expect("empty log");
    let mut state = genesis(project, 7).expect("genesis");
    // Offered tick 2 (day 1, age 0). Rollover tick 8: age 1.
    // Rollover tick 16: age 2 reaches expires_days — the offer
    // dies at exactly that tick, through the rollover path.
    let (events, _c) = run_to(project, &mut state, &log, 15);
    assert_eq!(offered_count(&events), 1);
    assert!(
        state.triggers["gift"].offer.is_some(),
        "still pending the tick before expiry"
    );
    let (events, _c) = run_to(project, &mut state, &log, 16);
    assert!(
        events
            .iter()
            .any(|event| { event.contains("tick 16") && event.contains("expired unfired") }),
        "expiry at the boundary tick: {events:?}"
    );
    assert!(state.triggers["gift"].offer.is_none());
    // The settled trigger re-offers on the next matching day:
    // day 3's afternoon_1 is tick 18.
    let (events, _c) = run_to(project, &mut state, &log, 18);
    assert!(
        events
            .iter()
            .any(|event| event.contains("tick 18") && event.contains("offered")),
        "re-offer after expiry: {events:?}"
    );
}

#[test]
fn offer_inputs_without_a_pending_offer_refuse_the_run() {
    let root = test_root("offer-adversarial");
    author_offer_game(&root);
    let loaded = load_project(&root);
    let project = &loaded.project;
    // Deciding an offer on tick 1, before any offer exists.
    let log = offer_log(&[(1, "gift", "accept")], 4);
    let mut state = genesis(project, 7).expect("genesis");
    let mut events = Vec::new();
    let mut chain = Vec::new();
    let err = run_ticks(project, &mut state, &log, 4, &mut events, &mut chain)
        .expect_err("deciding a non-pending offer refuses");
    assert_eq!(err.code(), "TG-AUTH-CONFLICT", "{err}");
    // An unknown decision word, with an offer pending.
    let log = offer_log(&[(3, "gift", "maybe")], 4);
    let mut state = genesis(project, 7).expect("genesis");
    let mut events = Vec::new();
    let mut chain = Vec::new();
    let err = run_ticks(project, &mut state, &log, 4, &mut events, &mut chain)
        .expect_err("an unknown decision refuses");
    assert_eq!(err.code(), "TG-AUTH-MALFORMED", "{err}");
    // Deciding a trigger the game does not define.
    let log = offer_log(&[(3, "nope", "accept")], 4);
    let mut state = genesis(project, 7).expect("genesis");
    let mut events = Vec::new();
    let mut chain = Vec::new();
    let err = run_ticks(project, &mut state, &log, 4, &mut events, &mut chain)
        .expect_err("deciding an unknown trigger refuses");
    assert_eq!(err.code(), "TG-AUTH-CONFLICT", "{err}");
}

#[test]
fn offer_expiry_zero_days_is_refused_at_define() {
    let root = test_root("offer-bad-expiry");
    author_offer_game(&root);
    let bad = verb(
        &root,
        "trigger_define",
        json!({
            "id": "bad_offer",
            "kind": {"kind": "on_tick", "slots": ["morning_1"]},
            "effect_set": "gift_set",
            "offer": {"expires_days": 0},
        }),
    );
    assert_eq!(bad.doc["code"], "TG-AUTH-RANGE", "{}", bad.doc);
}

#[test]
fn deferral_simulation_hash_chain_is_identical_across_three_runs() {
    let root = test_root("offer-chain");
    author_offer_game(&root);
    let inputs = json!({
        "run_seed": 11,
        "ticks": 20,
        "choice_log": [
            {"tick": 3, "kind": "offer", "id": "gift", "option": "decline"},
            {"tick": 11, "kind": "offer", "id": "gift", "option": "accept"},
        ],
    });
    let first = verb(&root, "game_simulate", inputs.clone());
    assert_eq!(first.status, Status::Ok, "{}", first.doc);
    for _ in 0..2 {
        let again = verb(&root, "game_simulate", inputs.clone());
        assert_eq!(again.doc, first.doc, "run evidence is byte-identical");
    }
    assert_eq!(first.doc["chain"].as_array().expect("chain").len(), 20);
}

#[test]
fn same_tick_triggers_fire_in_priority_order() {
    // §G3.1 ordering is undisturbed by the offer machinery:
    // two plain triggers on one event fire priority-desc.
    let root = test_root("offer-ordering");
    let answer = script(
        &root,
        vec![
            ("project_init", init_inputs()),
            (
                "game_define",
                json!({"id": "order_fixture", "start_scene": "room"}),
            ),
            ("clock_define", json!({"slots": ["morning", "night"]})),
            (
                "scene_define",
                json!({"id": "room", "presentation": "room"}),
            ),
            (
                "effect_set_define",
                json!({"id": "set_low", "atoms": [{"kind": "emit_event", "event": "low_done"}]}),
            ),
            (
                "effect_set_define",
                json!({"id": "set_high", "atoms": [{"kind": "emit_event", "event": "high_done"}]}),
            ),
            (
                "trigger_define",
                json!({
                    "id": "low",
                    "kind": {"kind": "on_event", "event": "go"},
                    "effect_set": "set_low",
                    "priority": 1,
                }),
            ),
            (
                "trigger_define",
                json!({
                    "id": "high",
                    "kind": {"kind": "on_event", "event": "go"},
                    "effect_set": "set_high",
                    "priority": 5,
                }),
            ),
        ],
    );
    assert_eq!(answer.status, Status::Ok, "{}", answer.doc);
    let loaded = load_project(&root);
    let project = &loaded.project;
    let log = event_log(&[(1, "go")], 2);
    let mut state = genesis(project, 7).expect("genesis");
    let (events, _c) = run_to(project, &mut state, &log, 2);
    let high = events
        .iter()
        .position(|event| event.contains("'high' fired"))
        .expect("high fired");
    let low = events
        .iter()
        .position(|event| event.contains("'low' fired"))
        .expect("low fired");
    assert!(high < low, "priority order in the trace: {events:?}");
}

// ---- P2-4: expression table weights (D-469) --------------------------

/// Authors the MoonDeep §G5 stress fixture: glimpse odds
/// 12% + 1% per interval, spark 5% + 1% beyond the third
/// interval, the remainder calm — as expression weights over a
/// game-scope `intervals` stat, drawn on the `battle` stream.
/// With `expr_weights` false, the same table carries static
/// weights of 1 (the stream-consumption comparison project).
fn author_moondeep(root: &Path, expr_weights: bool) {
    let (glimpse_w, spark_w, calm_w) = if expr_weights {
        (
            json!({"kind": "add", "left": {"kind": "lit", "value": 12},
                   "right": {"kind": "stat", "stat": "intervals"}}),
            json!({"kind": "add", "left": {"kind": "lit", "value": 5},
                   "right": {"kind": "max",
                             "left": {"kind": "lit", "value": 0},
                             "right": {"kind": "sub",
                                       "left": {"kind": "stat", "stat": "intervals"},
                                       "right": {"kind": "lit", "value": 3}}}}),
            json!({"kind": "sub",
                   "left": {"kind": "sub", "left": {"kind": "lit", "value": 83},
                            "right": {"kind": "stat", "stat": "intervals"}},
                   "right": {"kind": "max",
                             "left": {"kind": "lit", "value": 0},
                             "right": {"kind": "sub",
                                       "left": {"kind": "stat", "stat": "intervals"},
                                       "right": {"kind": "lit", "value": 3}}}}),
        )
    } else {
        (json!(1), json!(1), json!(1))
    };
    let answer = script(
        root,
        vec![
            ("project_init", init_inputs()),
            (
                "game_define",
                json!({"id": "moondeep_fixture", "start_scene": "field"}),
            ),
            ("clock_define", json!({"slots": ["day"]})),
            (
                "rng_stream_define",
                json!({"id": "battle", "algorithm": "estate-rng/1"}),
            ),
            (
                "stat_define",
                json!({
                    "id": "intervals", "scope": "game", "kind": {"kind": "int"},
                    "min": 0, "max": 100, "default": 0, "overflow": "clamp",
                }),
            ),
            (
                "stat_define",
                json!({
                    "id": "sparks_count", "scope": "game", "kind": {"kind": "int"},
                    "min": 0, "max": 1000, "default": 0, "overflow": "clamp",
                }),
            ),
            (
                "flag_define",
                json!({"id": "glimpsed", "scope": "run", "default": false}),
            ),
            (
                "scene_define",
                json!({"id": "field", "presentation": "room"}),
            ),
            (
                "effect_set_define",
                json!({"id": "glimpse_set", "atoms": [{"kind": "flag_set", "flag": "glimpsed"}]}),
            ),
            (
                "effect_set_define",
                json!({
                    "id": "spark_set",
                    "atoms": [{
                        "kind": "stat_add", "stat": "sparks_count",
                        "amount": {"kind": "lit", "value": 1},
                    }],
                }),
            ),
            (
                "effect_set_define",
                json!({"id": "calm_set", "atoms": [{"kind": "emit_event", "event": "calm"}]}),
            ),
            (
                "table_define",
                json!({
                    "id": "encounter",
                    "stream": "battle",
                    "outcomes": [
                        {"id": "glimpse", "weight": glimpse_w, "effect_set": "glimpse_set"},
                        {"id": "spark", "weight": spark_w, "effect_set": "spark_set"},
                        {"id": "calm", "weight": calm_w, "effect_set": "calm_set"},
                    ],
                }),
            ),
            (
                "effect_set_define",
                json!({"id": "wander_set", "atoms": [{"kind": "roll_table", "table": "encounter"}]}),
            ),
            (
                "effect_set_define",
                json!({
                    "id": "surge_set",
                    "atoms": [{
                        "kind": "stat_set", "stat": "intervals",
                        "value": {"kind": "lit", "value": 90},
                    }],
                }),
            ),
            (
                "trigger_define",
                json!({
                    "id": "wander",
                    "kind": {"kind": "on_event", "event": "wander"},
                    "effect_set": "wander_set",
                }),
            ),
            (
                "trigger_define",
                json!({
                    "id": "surge",
                    "kind": {"kind": "on_event", "event": "surge"},
                    "effect_set": "surge_set",
                }),
            ),
        ],
    );
    assert_eq!(
        answer.status,
        Status::Ok,
        "moondeep fixture: {}",
        answer.doc
    );
    assert!(
        validate_codes(root).is_empty(),
        "moondeep fixture validates: {:?}",
        validate_codes(root)
    );
}

fn drawn_outcomes(events: &[String]) -> Vec<String> {
    events
        .iter()
        .filter_map(|event| {
            event
                .split("drew outcome '")
                .nth(1)
                .map(|rest| rest.trim_end_matches('\'').to_string())
        })
        .collect()
}

#[test]
fn moon_deep_expression_weights_evaluate_at_draw_time() {
    let root = test_root("moondeep-weights");
    author_moondeep(&root, true);
    let loaded = load_project(&root);
    let project = &loaded.project;
    // Surge to 90 intervals, then wander 20 times. At 90
    // intervals the weights are glimpse 102, spark 92, calm
    // negative — calm is ineligible at draw time, so every
    // draw lands glimpse or spark.
    let mut entries: Vec<(u64, &str)> = vec![(1, "surge")];
    for tick in 2..=21 {
        entries.push((tick, "wander"));
    }
    let log = event_log(&entries, 24);
    let mut state = genesis(project, 7).expect("genesis");
    let (events, _c) = run_to(project, &mut state, &log, 24);
    let drawn = drawn_outcomes(&events);
    assert_eq!(drawn.len(), 20, "every wander drew: {events:?}");
    assert!(
        drawn
            .iter()
            .all(|outcome| outcome == "glimpse" || outcome == "spark"),
        "calm is weight-ineligible at 90 intervals: {drawn:?}"
    );
    assert!(
        state.rng.contains_key("battle"),
        "the draw consumed the named stream"
    );
}

#[test]
fn stream_consumption_is_identical_regardless_of_weight_values() {
    // The stream-consumption fixture: the same seed and the
    // same number of draws leave the stream in the same state
    // whether the weights were expressions or static 1s — a
    // draw is one stream step, independent of weight values.
    let root_expr = test_root("moondeep-consume-expr");
    author_moondeep(&root_expr, true);
    let root_static = test_root("moondeep-consume-static");
    author_moondeep(&root_static, false);
    let entries: Vec<(u64, &str)> = (1..=12).map(|tick| (tick, "wander")).collect();
    let log = event_log(&entries, 12);
    let loaded_expr = load_project(&root_expr);
    let mut state_expr = genesis(&loaded_expr.project, 7).expect("genesis");
    let (events_expr, _c) = run_to(&loaded_expr.project, &mut state_expr, &log, 12);
    let loaded_static = load_project(&root_static);
    let mut state_static = genesis(&loaded_static.project, 7).expect("genesis");
    let (events_static, _c) = run_to(&loaded_static.project, &mut state_static, &log, 12);
    assert_eq!(drawn_outcomes(&events_expr).len(), 12);
    assert_eq!(drawn_outcomes(&events_static).len(), 12);
    assert_eq!(
        state_expr.rng, state_static.rng,
        "stream state after 12 draws is weight-independent"
    );
    assert_ne!(
        state_expr.rng,
        genesis(&loaded_expr.project, 7).expect("genesis").rng,
        "the stream actually advanced"
    );
}

#[test]
fn moon_deep_simulation_hash_chain_is_identical_across_three_runs() {
    let root = test_root("moondeep-chain");
    author_moondeep(&root, true);
    let inputs = json!({
        "run_seed": 7,
        "ticks": 12,
        "choice_log": (0..12)
            .map(|i| json!({"tick": i + 1, "kind": "event", "id": "wander"}))
            .collect::<Vec<_>>(),
    });
    let first = verb(&root, "game_simulate", inputs.clone());
    assert_eq!(first.status, Status::Ok, "{}", first.doc);
    for _ in 0..2 {
        let again = verb(&root, "game_simulate", inputs.clone());
        assert_eq!(again.doc, first.doc, "run evidence is byte-identical");
    }
}

#[test]
fn static_weights_serialize_as_bare_integers() {
    // The slice's negative gate: the static form is the plain
    // encoding — a bare integer weight, no expression
    // scaffolding — and static tables draw.
    let root = test_root("moondeep-static-form");
    author_moondeep(&root, false);
    let text = read(&root, PROJECT_FILE);
    assert!(text.contains("weight: 1"), "{text}");
    let table_at = text.find("Table(").expect("table record present");
    let table_text = &text[table_at..];
    assert!(
        !table_text.contains("Add("),
        "static table carries expression scaffolding: {table_text}"
    );
    let loaded = load_project(&root);
    let log = event_log(&[(1, "wander")], 2);
    let mut state = genesis(&loaded.project, 7).expect("genesis");
    let (events, _c) = run_to(&loaded.project, &mut state, &log, 2);
    assert_eq!(drawn_outcomes(&events).len(), 1);
}

#[test]
fn table_and_stream_gates_refuse_typed() {
    let root = test_root("table-gates");
    author_moondeep(&root, true);
    // An algorithm this build does not implement.
    let bad = verb(
        &root,
        "rng_stream_define",
        json!({"id": "other", "algorithm": "lcg/9"}),
    );
    assert_eq!(bad.doc["code"], "TG-AUTH-UNKNOWN-KIND", "{}", bad.doc);
    // A table on a stream that does not exist.
    let bad = verb(
        &root,
        "table_define",
        json!({
            "id": "t_nostream", "stream": "nope",
            "outcomes": [{"id": "x", "weight": 1, "effect_set": "glimpse_set"}],
        }),
    );
    assert_eq!(bad.doc["code"], "TG-GAME-TABLE", "{}", bad.doc);
    // A non-positive static weight.
    let bad = verb(
        &root,
        "table_define",
        json!({
            "id": "t_zero", "stream": "battle",
            "outcomes": [{"id": "x", "weight": 0, "effect_set": "glimpse_set"}],
        }),
    );
    assert_eq!(bad.doc["code"], "TG-GAME-TABLE", "{}", bad.doc);
    // A duplicate outcome id.
    let bad = verb(
        &root,
        "table_define",
        json!({
            "id": "t_dup", "stream": "battle",
            "outcomes": [
                {"id": "x", "weight": 1, "effect_set": "glimpse_set"},
                {"id": "x", "weight": 2, "effect_set": "spark_set"},
            ],
        }),
    );
    assert_eq!(bad.doc["code"], "TG-GAME-TABLE", "{}", bad.doc);
    // An ill-typed (boolean) weight expression.
    let bad = verb(
        &root,
        "table_define",
        json!({
            "id": "t_bool", "stream": "battle",
            "outcomes": [{
                "id": "x",
                "weight": {"kind": "bool_lit", "value": true},
                "effect_set": "glimpse_set",
            }],
        }),
    );
    assert_eq!(bad.doc["code"], "TG-GAME-EXPR", "{}", bad.doc);
    // A weight naming a value that does not exist yet: cyclic
    // or forward references are unrepresentable — the
    // reference is unknown at define time.
    let bad = verb(
        &root,
        "table_define",
        json!({
            "id": "t_cycle", "stream": "battle",
            "outcomes": [{
                "id": "x",
                "weight": {"kind": "value", "value": "future_weight"},
                "effect_set": "glimpse_set",
            }],
        }),
    );
    assert_eq!(bad.doc["code"], "TG-GAME-EXPR", "{}", bad.doc);
    // An outcome naming a missing effect set dangles.
    let bad = verb(
        &root,
        "table_define",
        json!({
            "id": "t_dangle", "stream": "battle",
            "outcomes": [{"id": "x", "weight": 1, "effect_set": "nope"}],
        }),
    );
    assert_eq!(bad.doc["code"], "TG-AUTH-DANGLING", "{}", bad.doc);
    // A roll_table atom naming a missing table dangles.
    let bad = verb(
        &root,
        "effect_set_define",
        json!({"id": "bad_roll", "atoms": [{"kind": "roll_table", "table": "nope"}]}),
    );
    assert_eq!(bad.doc["code"], "TG-AUTH-DANGLING", "{}", bad.doc);
}

// ---- P2-5: ledger day-stamps + activity counters (RJ318915) ----------
//
// The slice's hypothesis, proven here: the RJ318915 ledger
// shape (SaveData `...Day` / `...Conv` fields — per-activity
// last-day stamps and counters, read by personality gates) is
// a PATTERN over the existing surface, not a primitive:
//
//   * a counter is a plain stat, advanced by `stat_add`;
//   * a day-stamp is a plain stat, written by a named effect
//     atom `stat_set` whose value expression is the clock's
//     day leaf — the stamp is written from the clock, by data;
//   * "within n days" is a condition over
//     `clock.day - stamp`, composed with counter/level
//     comparisons by ordinary boolean logic.
//
// The fixture below authors the pattern with no new records,
// verbs, or atoms; the slice's commit touches tests and the
// report only. If the pattern had failed to express the gate,
// the slice would have stopped and named the primitive it
// needed (the plan's fork). It did not.

/// Authors the RJ318915 ledger fixture: Amy carries a `visits`
/// counter, a `last_visit_day` stamp, and a `bond_level`; the
/// personality gate is (visits >= 3 AND today - stamp <= 2 AND
/// bond_level >= 2) — the original's gate shape.
fn author_ledger(root: &Path) {
    let answer = script(
        root,
        vec![
            ("project_init", init_inputs()),
            (
                "game_define",
                json!({"id": "ledger_fixture", "start_scene": "room"}),
            ),
            (
                "clock_define",
                json!({"slots": ["morning", "night"], "days_per_week": 7}),
            ),
            (
                "stat_define",
                json!({
                    "id": "visits", "scope": "character", "kind": {"kind": "int"},
                    "min": 0, "max": 1000, "default": 0, "overflow": "clamp",
                }),
            ),
            (
                "stat_define",
                json!({
                    "id": "last_visit_day", "scope": "character", "kind": {"kind": "int"},
                    "min": 0, "max": 10000, "default": 0, "overflow": "clamp",
                }),
            ),
            (
                "stat_define",
                json!({
                    "id": "bond_level", "scope": "character", "kind": {"kind": "int"},
                    "min": 0, "max": 5, "default": 0, "overflow": "clamp",
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
                "flag_define",
                json!({"id": "gate_open", "scope": "run", "default": false}),
            ),
            (
                "effect_set_define",
                json!({
                    "id": "visit_set",
                    "atoms": [
                        {
                            "kind": "stat_add", "stat": "visits", "character": "amy",
                            "amount": {"kind": "lit", "value": 1},
                        },
                        {
                            "kind": "stat_set", "stat": "last_visit_day", "character": "amy",
                            "value": {"kind": "clock", "field": "day"},
                        },
                    ],
                }),
            ),
            (
                "effect_set_define",
                json!({
                    "id": "bond_set",
                    "atoms": [{
                        "kind": "stat_set", "stat": "bond_level", "character": "amy",
                        "value": {"kind": "lit", "value": 2},
                    }],
                }),
            ),
            (
                "condition_define",
                json!({
                    "id": "personality_gate",
                    "expr": {
                        "kind": "and",
                        "args": [
                            {
                                "kind": "cmp", "op": "ge",
                                "left": {"kind": "stat", "stat": "visits", "character": "amy"},
                                "right": {"kind": "lit", "value": 3},
                            },
                            {
                                "kind": "cmp", "op": "le",
                                "left": {
                                    "kind": "sub",
                                    "left": {"kind": "clock", "field": "day"},
                                    "right": {"kind": "stat", "stat": "last_visit_day", "character": "amy"},
                                },
                                "right": {"kind": "lit", "value": 2},
                            },
                            {
                                "kind": "cmp", "op": "ge",
                                "left": {"kind": "stat", "stat": "bond_level", "character": "amy"},
                                "right": {"kind": "lit", "value": 2},
                            },
                        ],
                    },
                }),
            ),
            (
                "effect_set_define",
                json!({"id": "check_set", "atoms": [{"kind": "flag_set", "flag": "gate_open"}]}),
            ),
            (
                "trigger_define",
                json!({
                    "id": "visit",
                    "kind": {"kind": "on_event", "event": "visit"},
                    "effect_set": "visit_set",
                }),
            ),
            (
                "trigger_define",
                json!({
                    "id": "bond",
                    "kind": {"kind": "on_event", "event": "bond"},
                    "effect_set": "bond_set",
                }),
            ),
            (
                "trigger_define",
                json!({
                    "id": "check",
                    "kind": {"kind": "on_event", "event": "check"},
                    "effect_set": "check_set",
                    "gate": "personality_gate",
                }),
            ),
        ],
    );
    assert_eq!(answer.status, Status::Ok, "ledger fixture: {}", answer.doc);
    assert!(
        validate_codes(root).is_empty(),
        "ledger fixture validates: {:?}",
        validate_codes(root)
    );
}

fn ledger_run(root: &Path, entries: &[(u64, &str)], target: u64) -> GameState {
    let loaded = load_project(root);
    let log = event_log(entries, target);
    let mut state = genesis(&loaded.project, 7).expect("genesis");
    let (_e, _c) = run_to(&loaded.project, &mut state, &log, target);
    state
}

#[test]
fn ledger_pattern_gate_opens_when_counter_stamp_and_level_align() {
    let root = test_root("ledger-open");
    author_ledger(&root);
    // Visits on days 1, 2, 2 (stamps written from the clock
    // leaf at each visit), bond raised on day 3, check on day
    // 3: counter 3, stamp age 1 day, level 2 — the gate opens.
    let state = ledger_run(
        &root,
        &[
            (1, "visit"),
            (2, "visit"),
            (3, "visit"),
            (4, "bond"),
            (5, "check"),
        ],
        5,
    );
    let visits = StatKey {
        character: Some("amy".to_string()),
        stat: "visits".to_string(),
    };
    let stamp = StatKey {
        character: Some("amy".to_string()),
        stat: "last_visit_day".to_string(),
    };
    assert_eq!(state.stats[&visits], 3);
    assert_eq!(
        state.stats[&stamp], 2,
        "the stamp holds the last visit's day"
    );
    assert!(state.flags_run["gate_open"], "the personality gate opened");
}

#[test]
fn ledger_pattern_gate_closes_when_the_stamp_goes_stale_or_the_counter_is_short() {
    let root = test_root("ledger-closed");
    author_ledger(&root);
    // Same ledger, but the check lands on day 6 — four days
    // after the stamp: outside the 2-day window.
    let state = ledger_run(
        &root,
        &[
            (1, "visit"),
            (2, "visit"),
            (3, "visit"),
            (4, "bond"),
            (11, "check"),
        ],
        11,
    );
    assert!(
        !state.flags_run["gate_open"],
        "a stale stamp closes the gate"
    );
    // Same timing as the opening run, but only two visits:
    // the counter leg fails.
    let state = ledger_run(
        &root,
        &[(1, "visit"), (2, "visit"), (4, "bond"), (5, "check")],
        5,
    );
    assert!(
        !state.flags_run["gate_open"],
        "a short counter closes the gate"
    );
}

#[test]
fn ledger_pattern_evaluates_identically_across_three_replays() {
    let root = test_root("ledger-replay");
    author_ledger(&root);
    let inputs = json!({
        "run_seed": 7,
        "ticks": 5,
        "choice_log": [
            {"tick": 1, "kind": "event", "id": "visit"},
            {"tick": 2, "kind": "event", "id": "visit"},
            {"tick": 3, "kind": "event", "id": "visit"},
            {"tick": 4, "kind": "event", "id": "bond"},
            {"tick": 5, "kind": "event", "id": "check"},
        ],
    });
    let first = verb(&root, "game_simulate", inputs.clone());
    assert_eq!(first.status, Status::Ok, "{}", first.doc);
    assert!(
        first.doc["events"]
            .as_array()
            .expect("events")
            .iter()
            .any(|event| event.as_str().is_some_and(|e| e.contains("'check' fired"))),
        "the gated trigger fired in the replay: {}",
        first.doc
    );
    for _ in 0..2 {
        let again = verb(&root, "game_simulate", inputs.clone());
        assert_eq!(again.doc, first.doc, "replay evidence is byte-identical");
    }
}

// ---- P2-6: visual-state delta table (Modification App) ----------------
//
// The parity fixture is the pilot's measured MiaStates
// census: 39 rows — the `Default` baseline (which sets all
// seven slots) plus 38 path-stage states. The slot vocabulary
// {BODY, EXTRAS, FACE, SCHEDULE, POSITION, SCALE,
// FACE_POSITION}, the state key set, the per-state delta
// SHAPES (which slots each state touches), the empty
// MindControl deltas, and the baseline SCHEDULE vector
// [5.0, 4.0, 2.0, 1.0] are measured; the part-name tokens are
// representative (the study reports shapes, not part names).
// The one float vector enters as fixed-point integers
// ([5000, 4000, 2000, 1000]) — floats never enter the format.

/// The measured path census: (path, stage count, delta shape).
/// Shapes: "body" touches BODY; "late-face" adds FACE at the
/// final stages; "transform" adds POSITION and SCALE;
/// "empty" is the MindControl shape — no entries at all.
const MIA_PATHS: [(&str, u32, &str); 10] = [
    ("AssExpansion", 4, "body"),
    ("Bimbo", 4, "late-face"),
    ("BreastExpansion", 4, "body"),
    ("Goth", 3, "late-face"),
    ("Growth", 5, "transform"),
    ("Milk", 4, "body"),
    ("MindControl", 4, "empty"),
    ("Pet", 4, "body"),
    ("Shrink", 5, "transform"),
    ("Vampire", 1, "body"),
];

fn mia_delta_json(path: &str, stage: u32, count: u32, shape: &str) -> Value {
    let lower = path.to_lowercase();
    let mut delta = vec![json!({
        "slot": "body",
        "value": format!("body_{lower}_{stage}"),
    })];
    match shape {
        "body" => {}
        "empty" => return json!([]),
        "late-face" => {
            if stage >= 3 || stage == count {
                delta.push(json!({
                    "slot": "face",
                    "value": format!("face_{lower}_{stage}"),
                }));
            }
        }
        "transform" => {
            let scale = if path == "Growth" {
                1000 + 100 * i64::from(stage)
            } else {
                1000 - 80 * i64::from(stage)
            };
            delta.push(json!({"slot": "position", "value": [10 * i64::from(stage), 0]}));
            delta.push(json!({"slot": "scale", "value": [scale]}));
        }
        _ => unreachable!("fixture shapes are fixed"),
    }
    Value::Array(delta)
}

/// Authors the MiaStates fixture: Mia's appearance table
/// (seven slots, total baseline) plus all 38 path-stage
/// visual states.
fn author_mia(root: &Path) {
    let mut ops: Vec<(&'static str, Value)> = vec![
        ("project_init", init_inputs()),
        (
            "game_define",
            json!({"id": "mia_fixture", "start_scene": "room"}),
        ),
        (
            "character_define",
            json!({"id": "mia", "name_ref": "name_mia", "name_text": "Mia"}),
        ),
        (
            "scene_define",
            json!({"id": "room", "presentation": "room"}),
        ),
        (
            "appearance_define",
            json!({
                "character": "mia",
                "slots": [
                    {"id": "body", "kind": "text"},
                    {"id": "extras", "kind": "text"},
                    {"id": "face", "kind": "text"},
                    {"id": "face_position", "kind": "ints"},
                    {"id": "position", "kind": "ints"},
                    {"id": "scale", "kind": "ints"},
                    {"id": "schedule", "kind": "ints"},
                ],
                "base": [
                    {"slot": "body", "value": "mia_default"},
                    {"slot": "extras", "value": "none"},
                    {"slot": "face", "value": "default_face"},
                    {"slot": "face_position", "value": [0, 0]},
                    {"slot": "position", "value": [0, 0]},
                    {"slot": "scale", "value": [1000]},
                    {"slot": "schedule", "value": [5000, 4000, 2000, 1000]},
                ],
            }),
        ),
    ];
    for (path, count, shape) in MIA_PATHS {
        for stage in 1..=count {
            ops.push((
                "visual_state_define",
                json!({
                    "character": "mia",
                    "state": format!("{path}-{stage}"),
                    "delta": mia_delta_json(path, stage, count, shape),
                }),
            ));
        }
    }
    let answer = script(root, ops);
    assert_eq!(answer.status, Status::Ok, "mia fixture: {}", answer.doc);
    assert!(
        validate_codes(root).is_empty(),
        "mia fixture validates: {:?}",
        validate_codes(root)
    );
}

#[test]
fn mia_states_fixture_validates_and_every_state_resolves_to_its_measured_delta() {
    use crate::game::model::AppearanceValue;
    use crate::game::visual::resolve_appearance;
    let root = test_root("mia-states");
    author_mia(&root);
    let loaded = load_project(&root);
    let game = loaded.project.game.as_ref().expect("game section");
    assert_eq!(
        game.visual_states.len(),
        38,
        "38 path-stage rows plus the baseline"
    );
    // The baseline: all seven slots at their Default values.
    let base = resolve_appearance(game, "mia", None).expect("baseline resolves");
    assert_eq!(base.len(), 7);
    assert!(
        base.contains(&(
            "schedule".to_string(),
            AppearanceValue::Ints(vec![5000, 4000, 2000, 1000])
        )),
        "the measured SCHEDULE vector resolves: {base:?}"
    );
    // Every state resolves to exactly its measured sparse
    // override set: the slots whose resolved value differs
    // from the baseline are the delta's slots, no more.
    for (path, count, shape) in MIA_PATHS {
        for stage in 1..=count {
            let name = format!("{path}-{stage}");
            let resolved = resolve_appearance(game, "mia", Some(&name))
                .unwrap_or_else(|err| panic!("{name} resolves: {err:?}"));
            let overridden: Vec<&str> = resolved
                .iter()
                .filter(|(slot, value)| {
                    base.iter().find(|(s, _)| s == slot).map(|(_, v)| v) != Some(value)
                })
                .map(|(slot, _)| slot.as_str())
                .collect();
            let expected: Vec<&str> = match shape {
                "body" => vec!["body"],
                "empty" => vec![],
                "late-face" if stage >= 3 || stage == count => vec!["body", "face"],
                "late-face" => vec!["body"],
                "transform" => vec!["body", "position", "scale"],
                _ => unreachable!(),
            };
            assert_eq!(overridden, expected, "{name} overrides exactly its delta");
            assert_eq!(resolved.len(), 7, "{name} resolves all seven slots");
        }
    }
    // The measured negative space: every MindControl stage
    // resolves to EXACTLY the baseline — the empty delta is a
    // row that changes nothing, auditable in the data.
    for stage in 1..=4 {
        let name = format!("MindControl-{stage}");
        let resolved = resolve_appearance(game, "mia", Some(&name)).expect("resolves");
        assert_eq!(resolved, base, "{name} is the empty delta made visible");
    }
    // A state the table does not carry refuses, never a
    // silent baseline.
    assert!(resolve_appearance(game, "mia", Some("Nope-1")).is_err());
    assert!(resolve_appearance(game, "nobody", None).is_err());
}

#[test]
fn visual_state_gates_refuse_typed() {
    let root = test_root("visual-gates");
    author_mia(&root);
    // A delta naming an undeclared slot dangles.
    let bad = verb(
        &root,
        "visual_state_define",
        json!({"character": "mia", "state": "Winged-1", "delta": [{"slot": "wings", "value": "big"}]}),
    );
    assert_eq!(bad.doc["code"], "TG-AUTH-DANGLING", "{}", bad.doc);
    // A value whose form mismatches the slot's kind is
    // ill-typed (the expression class, as for table weights).
    let bad = verb(
        &root,
        "visual_state_define",
        json!({"character": "mia", "state": "Bad-1", "delta": [{"slot": "body", "value": [1, 2]}]}),
    );
    assert_eq!(bad.doc["code"], "TG-GAME-EXPR", "{}", bad.doc);
    // One slot assigned twice in a delta is malformed.
    let bad = verb(
        &root,
        "visual_state_define",
        json!({
            "character": "mia", "state": "Bad-2",
            "delta": [
                {"slot": "body", "value": "a"},
                {"slot": "body", "value": "b"},
            ],
        }),
    );
    assert_eq!(bad.doc["code"], "TG-AUTH-MALFORMED", "{}", bad.doc);
    // A state for a character with no appearance table.
    let ok = verb(
        &root,
        "character_define",
        json!({"id": "bob", "name_ref": "name_bob", "name_text": "Bob"}),
    );
    assert_eq!(ok.status, Status::Ok, "{}", ok.doc);
    let bad = verb(
        &root,
        "visual_state_define",
        json!({"character": "bob", "state": "Any-1", "delta": []}),
    );
    assert_eq!(bad.doc["code"], "TG-AUTH-DANGLING", "{}", bad.doc);
    // An appearance table for a character that does not exist.
    let bad = verb(
        &root,
        "appearance_define",
        json!({
            "character": "ghost",
            "slots": [{"id": "body", "kind": "text"}],
            "base": [{"slot": "body", "value": "x"}],
        }),
    );
    assert_eq!(bad.doc["code"], "TG-AUTH-DANGLING", "{}", bad.doc);
    // A baseline that omits a declared slot is malformed —
    // the baseline is total by construction.
    let bad = verb(
        &root,
        "appearance_define",
        json!({
            "character": "bob",
            "slots": [{"id": "body", "kind": "text"}, {"id": "face", "kind": "text"}],
            "base": [{"slot": "body", "value": "x"}],
        }),
    );
    assert_eq!(bad.doc["code"], "TG-AUTH-MALFORMED", "{}", bad.doc);
    // The upsert law: identical redefinition is a no-op, a
    // differing one conflicts.
    let same = verb(
        &root,
        "visual_state_define",
        json!({"character": "mia", "state": "MindControl-1", "delta": []}),
    );
    assert_eq!(same.status, Status::Ok, "{}", same.doc);
    let bad = verb(
        &root,
        "visual_state_define",
        json!({"character": "mia", "state": "MindControl-1", "delta": [{"slot": "body", "value": "other"}]}),
    );
    assert_eq!(bad.doc["code"], "TG-AUTH-CONFLICT", "{}", bad.doc);
}
