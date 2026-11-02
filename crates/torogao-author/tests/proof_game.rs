//! The Stage 7b acceptance harness: the proof game "The
//! Listening Room" (spec:
//! `~/workspace/ntranch-game-re-20261101/PROOF-GAME-SPEC.md`),
//! authored end-to-end through the author crate's verb/session
//! API — the same decide()/apply() path the TUI and MCP drive —
//! and gated on the spec §9 acceptance evidence:
//!
//! 1. Records alone: the op log (`ops.jsonl` in the game dir)
//!    is the provenance; `project_replay` reproduces the shipped
//!    `project.ron` byte-identically x3; zero hand-edits.
//! 2. Validate clean: `project_validate` reports no findings,
//!    plus a record-level graph sweep (this crate's validate
//!    carries no scene-reachability/dead-end sweep — see
//!    SURFACE-GAPS.md in the game dir) proving every scene is
//!    reachable, no scene is a dead end, and both endings are
//!    graph-reachable.
//! 3. Determinism x3: the three scripted playthroughs (engaged
//!    -> Ending A, gentle -> Ending B, week -> Day-5 lifetime
//!    expiry reset -> Ending B) reproduce identical state-hash
//!    chains x3, with sway total + derived tier asserted at
//!    every checkpoint.
//! 4. Save round-trip at the S3 save point (authored save
//!    schema, policy `safe_scenes_only [s3]`): save -> day tick
//!    -> load -> state equality and chain continuation;
//!    adversarial saves (mutated schema version, unknown field,
//!    bit-flipped state) refuse typed.
//! 5. Voice (D-483/D-481): the three voice-marked lines
//!    (s1_06, s2_08, s3_06 — spec ids s1.06, s2.08, s3.06) would
//!    bind identity `voice.cast.noor-1`; no authoring verb
//!    creates a voice binding row (SURFACE-GAPS.md), so the
//!    refusal evidence is produced by running the shippable
//!    sweep (`validate_shippable`) over an in-memory voiced
//!    variant of the shipped project built through the codec,
//!    and the shipped edition is the silent one (zero bindings,
//!    sweep clean).
//!
//! Encoding notes (also in the game dir's SURFACE-GAPS.md):
//! - Spec line ids carry dots (`s0.01`); the author surface's
//!   id grammar is `[A-Za-z0-9_-]`, so record ids and content
//!   keys transcribe `.` -> `_` (`s0_01`), a 1:1 mapping.
//! - Choice label prose has no standalone content-entry verb;
//!   per the engine's own gate fixtures (gameflow_gate,
//!   thread_gate), label keys are carried by dialogue lines —
//!   here, unchained carrier lines appended after each scene's
//!   script chain (speaker narrator). The 42 spec lines are
//!   exact; the 12 carriers are enumerated in the evidence.
//! - Scene boundaries with no spec choice (S0->S1, S1->T1,
//!   interim->S3, S3->S4) are driven by `on_event` triggers
//!   named in the playthrough logs (the §G3.1 log's event
//!   kind); every graph edge is ALSO recorded as a
//!   `scene_exit_define` record. C5's two options both target
//!   the between-days `interim` scene: the appointment choice's
//!   effect is the S3 entry day stamp (spec §5), realized by
//!   when the log fires `s3_begin` — Day 4 on the tomorrow
//!   path, Day 8 on the week path, with the lifetime reset on
//!   the Day-5 rollover between them.
//! - TC1's options also set the run flag `thread_done`
//!   (gallery bookkeeping for the T1 entry's unlock condition);
//!   the sway gains are exactly the spec's (+1 / +0).

mod common;

use common::{project_sha, query, request, verb};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use torogao_author::codec::emit_project;
use torogao_author::game::save::{load_game, save_game};
use torogao_author::game::simulate::{genesis, parse_log, run_ticks};
use torogao_author::game::state::{GameState, StatKey};
use torogao_author::game::visual::resolve_appearance;
use torogao_author::model::{AudioBinding, AudioBindingKind, DeliveryRef, VoiceBinding};
use torogao_author::voice::{VoiceRegistrySnapshot, validate_shippable};
use torogao_author::{RefusalClass, Status, run_requests};

/// A 1x1 transparent PNG (the backdrop asset's source bytes;
/// the intake checks the header, not the pixels).
const PNG_1X1: &[u8] = &[
    137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1, 8, 6, 0,
    0, 0, 31, 21, 196, 137, 0, 0, 0, 13, 73, 68, 65, 84, 120, 218, 99, 100, 248, 207, 80, 15, 0, 3,
    134, 1, 128, 90, 52, 125, 107, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
];

/// The game directory: `LISTENING_ROOM_DIR` when set, else the
/// commissioned location beside the integration tree.
fn game_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("LISTENING_ROOM_DIR") {
        return PathBuf::from(dir);
    }
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../listening-room")
}

fn assert_ok(answer: &torogao_author::Answer, what: &str) {
    assert_eq!(answer.status, Status::Ok, "{what}: {}", answer.doc);
}

fn batch(root: &Path, requests: Vec<torogao_author::store::Request>, what: &str) {
    let answer = run_requests(root, root, &requests);
    assert_ok(&answer, what);
}

/// The proof game's own init (adult lane: the spec's register
/// is suggestive adult; consent per the estate schema).
fn init_lr() -> torogao_author::store::Request {
    request(
        "project_init",
        json!({
            "name": "the-listening-room",
            "stamp": {
                "basis": "PROOF-GAME-SPEC.md D-486 criterion 5",
                "lane": "adult",
                "pack": "ntranch-proof",
                "rating": "adult",
            },
            "consent": {
                "adult": true, "schema": "torogao-consent/1",
                "wants_selected_scenes": true,
            },
        }),
    )
}

fn line(
    id: &str,
    scene: &str,
    ordinal: u32,
    speaker: &str,
    text: &str,
    next: Option<&str>,
    hook: Option<&str>,
) -> Value {
    let mut row = json!({
        "id": id, "scene": scene, "ordinal": ordinal, "speaker": speaker,
        "text_ref": id, "text": text,
    });
    if let Some(next) = next {
        row["next"] = json!(next);
    }
    if let Some(hook) = hook {
        row["hook_expression"] = json!(hook);
    }
    row
}

/// Authors one scene's script chain plus its label carriers:
/// `script` rows are (id, speaker, text, hook) in spec order,
/// chained by `next`; `carriers` are (id, label text) appended
/// unchained after the chain (see the module header).
fn scene_lines(
    scene: &str,
    script: &[(&str, &str, &str, Option<&str>)],
    carriers: &[(&str, &str)],
) -> Vec<Value> {
    let mut rows = Vec::new();
    for (index, (id, speaker, text, hook)) in script.iter().enumerate() {
        let next = script.get(index + 1).map(|row| row.0);
        rows.push(line(
            id,
            scene,
            u32::try_from(index).unwrap_or(0),
            speaker,
            text,
            next,
            *hook,
        ));
    }
    for (offset, (id, text)) in carriers.iter().enumerate() {
        rows.push(line(
            id,
            scene,
            u32::try_from(script.len() + offset).unwrap_or(0),
            "narrator",
            text,
            None,
            None,
        ));
    }
    rows
}

/// The whole authoring script, in batches (each batch is one
/// transactional `run_requests` over the same session API).
fn author_game(root: &Path) {
    std::fs::write(root.join("backdrop.png"), PNG_1X1).expect("write backdrop png");

    // -- Batch 1: rig anchors, game core, stat/tiers, cast. --
    let mut b1 = vec![
        init_lr(),
        request(
            "param_define",
            json!({"id": "gaze-y", "class": "angle", "min": -30.0, "default": 0.0, "max": 30.0}),
        ),
        request("part_create", json!({"id": "body"})),
    ];
    for (index, name) in [
        "neutral", "guarded", "softened", "lidded", "devoted", "uneasy",
    ]
    .iter()
    .enumerate()
    {
        b1.push(request(
            "expression_define",
            json!({
                "id": name, "blend": "additive",
                "values": [{"parameter": "gaze-y", "value": index as f64}],
            }),
        ));
    }
    b1.extend([
        request(
            "intake_image",
            json!({
                "id": "bg_listening_room", "path": "backdrop.png",
                "license": "proof-game build fixture (self-authored placeholder)",
                "origin": "proof game build",
            }),
        ),
        request(
            "game_define",
            json!({"id": "the-listening-room", "start_scene": "s0"}),
        ),
        request(
            "clock_define",
            json!({
                "slots": ["dawn", "morning", "afternoon", "evening", "night", "late"],
                "days_per_week": 7,
            }),
        ),
        request(
            "stat_define",
            json!({
                "id": "sway", "scope": "character", "kind": {"kind": "int"},
                "min": 0, "max": 12, "default": 0, "overflow": "clamp",
                "lifetime_days": 3,
            }),
        ),
        request(
            "stat_tier_define",
            json!({
                "stat": "sway", "thresholds": [3, 6, 9],
                "names": ["T0 Guarded", "T1 Softened", "T2 Yielding", "T3 Devoted"],
            }),
        ),
        request(
            "flag_define",
            json!({"id": "at_finale", "scope": "run", "default": false}),
        ),
        request(
            "flag_define",
            json!({"id": "thread_done", "scope": "run", "default": false}),
        ),
        request(
            "character_define",
            json!({"id": "noor", "name_ref": "name_noor", "name_text": "Noor Ellison"}),
        ),
    ]);
    batch(root, b1, "batch 1 (core)");

    // -- Batch 2: conditions, effect sets, flow triggers. --
    let tier_of_sway = || json!({"kind": "tier_of", "stat": "sway", "character": "noor"});
    let mut b2 = vec![
        request(
            "condition_define",
            json!({
                "id": "sway_t2_plus",
                "expr": {"kind": "cmp", "op": "ge", "left": tier_of_sway(), "right": {"kind": "lit", "value": 2}},
            }),
        ),
        request(
            "condition_define",
            json!({
                "id": "finale_a",
                "expr": {"kind": "and", "args": [
                    {"kind": "flag", "flag": "at_finale"},
                    {"kind": "cmp", "op": "ge", "left": tier_of_sway(), "right": {"kind": "lit", "value": 2}},
                ]},
            }),
        ),
        request(
            "condition_define",
            json!({
                "id": "finale_b",
                "expr": {"kind": "and", "args": [
                    {"kind": "flag", "flag": "at_finale"},
                    {"kind": "cmp", "op": "lt", "left": tier_of_sway(), "right": {"kind": "lit", "value": 2}},
                ]},
            }),
        ),
        request(
            "condition_define",
            json!({"id": "thread_done_cond", "expr": {"kind": "flag", "flag": "thread_done"}}),
        ),
    ];
    for (id, amount) in [("sway_p1", 1), ("sway_p2", 2), ("sway_p3", 3)] {
        b2.push(request(
            "effect_set_define",
            json!({
                "id": id,
                "atoms": [{
                    "kind": "stat_add", "stat": "sway", "character": "noor",
                    "amount": {"kind": "lit", "value": amount},
                }],
            }),
        ));
    }
    b2.extend([
        request(
            "effect_set_define",
            json!({
                "id": "tc1_stay",
                "atoms": [
                    {"kind": "stat_add", "stat": "sway", "character": "noor", "amount": {"kind": "lit", "value": 1}},
                    {"kind": "flag_set", "flag": "thread_done"},
                ],
            }),
        ),
        request(
            "effect_set_define",
            json!({"id": "tc1_fade", "atoms": [{"kind": "flag_set", "flag": "thread_done"}]}),
        ),
        request(
            "effect_set_define",
            json!({"id": "finale_mark", "atoms": [{"kind": "flag_set", "flag": "at_finale"}]}),
        ),
    ]);
    batch(root, b2, "batch 2 (state machinery)");

    // -- Batch 3: thread, scenes, appearance table + deltas. --
    let mut b3 = vec![request(
        "thread_create",
        json!({"id": "night_thread", "participants": ["noor"]}),
    )];
    for scene in ["s0", "s1", "s2", "s3", "ea", "eb"] {
        b3.push(request(
            "scene_define",
            json!({
                "id": scene, "presentation": "vn", "participants": ["noor"],
                "backdrop": "bg_listening_room",
            }),
        ));
    }
    b3.extend([
        request(
            "scene_define",
            json!({
                "id": "t1", "presentation": "event", "participants": ["noor"],
                "thread_interlude": {"thread": "night_thread", "reply_delay_ms": 3000},
            }),
        ),
        request(
            "scene_define",
            json!({"id": "interim", "presentation": "event"}),
        ),
        request(
            "scene_define",
            json!({
                "id": "s4", "presentation": "vn", "participants": ["noor"],
                "backdrop": "bg_listening_room", "on_enter": "finale_mark",
            }),
        ),
    ]);
    // The flow sets/triggers land after the scenes exist:
    // an `enter_scene` atom resolves its scene at define time.
    for (id, event, scene) in [
        ("flow_s0_close", "s0_close", "s1"),
        ("flow_s1_close", "s1_close", "t1"),
        ("flow_s3_begin", "s3_begin", "s3"),
        ("flow_s3_close", "s3_close", "s4"),
    ] {
        b3.push(request(
            "effect_set_define",
            json!({"id": format!("to_{scene}"), "atoms": [{"kind": "enter_scene", "scene": scene}]}),
        ));
        b3.push(request(
            "trigger_define",
            json!({
                "id": id, "kind": {"kind": "on_event", "event": event},
                "effect_set": format!("to_{scene}"), "priority": 0,
                "repetition": "once", "cooldown_ticks": 0,
            }),
        ));
    }
    b3.extend([request(
        "appearance_define",
        json!({
            "character": "noor",
            "slots": [{"id": "face", "kind": "text"}],
            "base": [{"slot": "face", "value": "guarded"}],
        }),
    )]);
    for (state, face) in [
        ("sway-t0", None),
        ("sway-t1", Some("softened")),
        ("sway-t2", Some("lidded")),
        ("sway-t3", Some("devoted")),
        ("sway-reset", Some("uneasy")),
    ] {
        let delta = face.map_or_else(Vec::new, |face| {
            vec![json!({"slot": "face", "value": face})]
        });
        b3.push(request(
            "visual_state_define",
            json!({"character": "noor", "state": state, "delta": delta}),
        ));
    }
    batch(root, b3, "batch 3 (scenes)");

    // -- Batch 4: the 42 spec lines + 12 label carriers. --
    let mut b4: Vec<torogao_author::store::Request> = Vec::new();
    let mut push_scene = |rows: Vec<Value>| {
        for row in rows {
            b4.push(request("dialogue_line_define", row));
        }
    };
    push_scene(scene_lines(
        "s0",
        &[
            (
                "s0_01",
                "narrator",
                "The listening room keeps one rule: nothing here is taken. Everything is offered, and everything can be refused.",
                None,
            ),
            (
                "s0_02",
                "narrator",
                "She arrives at eleven, coat still damp from the rain, holding the appointment card like it might change its mind.",
                None,
            ),
            (
                "s0_03",
                "noor",
                "You're the one they call when sleep won't come. That's what the card says, anyway.",
                Some("guarded"),
            ),
            (
                "s0_04",
                "narrator",
                "Noor Ellison. Archivist. Awake for the better part of three days, by her own cheerful confession.",
                None,
            ),
            (
                "s0_05",
                "noor",
                "I don't want pills. I don't want advice. I just want one hour where my head is quiet.",
                None,
            ),
            ("s0_06", "noor", "Can you do that? Just... quiet?", None),
            (
                "s0_07",
                "noor",
                "Huh. Your voice does something strange to the room.",
                None,
            ),
        ],
        &[
            ("c1_opt_sit", "Sit. Breathe with me."),
            ("c1_opt_door", "Quiet is a door. You open it."),
        ],
    ));
    push_scene(scene_lines(
        "s1",
        &[
            (
                "s1_01",
                "narrator",
                "The lamps go down to amber. The metronome on the shelf keeps its slow, patient count.",
                None,
            ),
            (
                "s1_02",
                "noor",
                "Sixty beats a minute. You set it to a heartbeat, didn't you.",
                None,
            ),
            (
                "s1_03",
                "noor",
                "Mine's been racing for days. It's... slowing down to match. That's rude of it.",
                None,
            ),
            (
                "s1_04",
                "narrator",
                "Her shoulders drop a fraction — the first honest thing her body has said all night.",
                None,
            ),
            (
                "s1_05",
                "noor",
                "When you count, the numbers stop arguing. Did you know they argue? Mine do.",
                None,
            ),
            (
                "s1_06",
                "noor",
                "Down is easy. Down is... oh. Oh, that's the quiet.",
                None,
            ),
            (
                "s1_07",
                "noor",
                "I almost fell asleep sitting up. Nobody's managed that since I was small.",
                None,
            ),
            (
                "s1_08",
                "narrator",
                "Session's end. She blinks like someone surfacing from warm water.",
                None,
            ),
        ],
        &[
            ("c2_opt_follow", "Follow the count down."),
            ("c2_opt_letgo", "Let the numbers go."),
        ],
    ));
    push_scene(scene_lines(
        "t1",
        &[
            (
                "t1_01",
                "noor",
                "awake. obviously. but it's a different awake",
                None,
            ),
            (
                "t1_02",
                "noor",
                "the quiet followed me home. is it supposed to do that",
                None,
            ),
            (
                "t1_03",
                "noor",
                "then i'm letting it. don't tell my insomnia, she'll be furious",
                None,
            ),
            (
                "t1_04",
                "noor",
                "tomorrow. same time. i'm booking before i can argue with myself",
                None,
            ),
        ],
        &[
            ("tc1_opt_stay", "It's yours now. It stays if you let it."),
            ("tc1_opt_fade", "It fades. That's normal."),
        ],
    ));
    push_scene(scene_lines(
        "s2",
        &[
            (
                "s2_01",
                "narrator",
                "She comes early. No coat this time, no card. She knows where the chair is.",
                None,
            ),
            (
                "s2_02",
                "noor",
                "I slept four hours. Four. I nearly wrote you a thank-you letter at dawn.",
                None,
            ),
            (
                "s2_03",
                "noor",
                "Instead I brought questions. When you count down, why do I stop wanting to argue?",
                None,
            ),
            (
                "s2_04",
                "narrator",
                "Her gaze keeps drifting to the metronome, the way other people's drift to a window.",
                None,
            ),
            (
                "s2_05",
                "noor",
                "Careful. If the quiet is mine, I might start keeping it in your room on purpose.",
                None,
            ),
            (
                "s2_06",
                "noor",
                "Count, then. Let's see how far down goes today.",
                None,
            ),
            (
                "s2_07",
                "narrator",
                "Deeper this time — her answers come slower, wrapped in that amber calm.",
                None,
            ),
            (
                "s2_08",
                "noor",
                "If you told me to stay in this chair all night, I think part of me would just... agree.",
                None,
            ),
            (
                "s2_09",
                "narrator",
                "She says it lightly. Neither of you believes it was light.",
                None,
            ),
        ],
        &[
            (
                "c3_opt_quiet",
                "Because arguing is loud. You prefer the quiet.",
            ),
            ("c3_opt_decided", "Because you decided to."),
            ("c5_opt_tomorrow", "Tomorrow, then."),
            ("c5_opt_week", "Give yourself a week."),
        ],
    ));
    push_scene(scene_lines(
        "s3",
        &[
            (
                "s3_01",
                "narrator",
                "Time apart has thinned the quiet — she arrives with her arguing head back on.",
                None,
            ),
            (
                "s3_02",
                "noor",
                "The loud days were awful. I hated knowing the difference.",
                None,
            ),
            (
                "s3_03",
                "noor",
                "So fix it. Please. Whatever you did, do the longer version.",
                None,
            ),
            (
                "s3_04",
                "noor",
                "All the way sounds... yes. Yes, that's the one my shoulders picked.",
                None,
            ),
            (
                "s3_05",
                "narrator",
                "The count takes her down past arguing, past bargaining, to the warm floor of it.",
                None,
            ),
            (
                "s3_06",
                "noor",
                "You could ask me anything right now. That's not a complaint. It's barely even a warning.",
                None,
            ),
        ],
        &[
            ("c4_opt_deep", "Then we go all the way down."),
            ("c4_opt_pace", "We go at your pace."),
        ],
    ));
    push_scene(scene_lines(
        "s4",
        &[(
            "s4_01",
            "narrator",
            "The hour ends. The metronome stops. The quiet stays — the question is whether she will.",
            None,
        )],
        &[],
    ));
    push_scene(scene_lines(
        "ea",
        &[
            ("ea_01", "noor", "I'm not booking next week.", None),
            ("ea_02", "narrator", "A pause, weighted.", None),
            (
                "ea_03",
                "noor",
                "I'm asking if I can just... keep coming. Without the card. Without pretending it's about sleep.",
                None,
            ),
            ("ea_04", "narrator", "Ending A — The Open Door.", None),
        ],
        &[],
    ));
    push_scene(scene_lines(
        "eb",
        &[
            (
                "eb_01",
                "noor",
                "I slept last night. On my own. Whatever you did, it... let go properly.",
                None,
            ),
            (
                "eb_02",
                "noor",
                "That's what I paid for, isn't it? So why does goodbye feel like the loud option?",
                None,
            ),
            (
                "eb_03",
                "narrator",
                "She leaves with her quiet in her own keeping. Ending B — The Quiet Goodbye.",
                None,
            ),
        ],
        &[],
    ));
    batch(root, b4, "batch 4 (lines)");

    // -- Batch 5: thread posts, exits, choices, schema, endings, gallery, rig bindings. --
    let mut b5 = Vec::new();
    for (index, key) in ["t1_01", "t1_02", "t1_03", "t1_04"].iter().enumerate() {
        b5.push(request(
            "thread_post",
            json!({
                "thread": "night_thread", "id": format!("msg_{}", index + 1),
                "speaker": "noor", "line": key, "at": index,
            }),
        ));
    }
    for (scene, id, target) in [
        ("s0", "to-s1", "s1"),
        ("s1", "to-t1", "t1"),
        ("t1", "to-s2", "s2"),
        ("s2", "to-interim", "interim"),
        ("interim", "to-s3", "s3"),
        ("s3", "to-s4", "s4"),
        ("s4", "to-ea", "ea"),
        ("s4", "to-eb", "eb"),
    ] {
        b5.push(request(
            "scene_exit_define",
            json!({"scene": scene, "id": id, "target": target}),
        ));
    }
    let opt = |id: &str, label: &str, set: Option<&str>, target: Value| {
        let mut row = json!({"id": id, "label_ref": label, "origin": "player", "target": target});
        if let Some(set) = set {
            row["effect_set"] = json!(set);
        }
        row
    };
    b5.extend([
        request(
            "choice_define",
            json!({
                "id": "C1", "scene": "s0",
                "options": [
                    opt("sit", "c1_opt_sit", Some("sway_p1"), json!({"kind": "line", "id": "s0_07"})),
                    opt("door", "c1_opt_door", Some("sway_p2"), json!({"kind": "line", "id": "s0_07"})),
                ],
            }),
        ),
        request(
            "choice_define",
            json!({
                "id": "C2", "scene": "s1",
                "options": [
                    opt("follow", "c2_opt_follow", Some("sway_p2"), json!({"kind": "line", "id": "s1_06"})),
                    opt("letgo", "c2_opt_letgo", Some("sway_p1"), json!({"kind": "line", "id": "s1_06"})),
                ],
            }),
        ),
        request(
            "choice_define",
            json!({
                "id": "TC1", "scene": "t1",
                "options": [
                    opt("stay", "tc1_opt_stay", Some("tc1_stay"), json!({"kind": "exit", "id": "to-s2"})),
                    opt("fade", "tc1_opt_fade", Some("tc1_fade"), json!({"kind": "exit", "id": "to-s2"})),
                ],
            }),
        ),
        request(
            "choice_define",
            json!({
                "id": "C3", "scene": "s2",
                "options": [
                    opt("quiet", "c3_opt_quiet", Some("sway_p2"), json!({"kind": "line", "id": "s2_05"})),
                    opt("decided", "c3_opt_decided", Some("sway_p1"), json!({"kind": "line", "id": "s2_05"})),
                ],
            }),
        ),
        request(
            "choice_define",
            json!({
                "id": "C5", "scene": "s2",
                "options": [
                    opt("tomorrow", "c5_opt_tomorrow", None, json!({"kind": "exit", "id": "to-interim"})),
                    opt("week", "c5_opt_week", None, json!({"kind": "exit", "id": "to-interim"})),
                ],
            }),
        ),
        request(
            "choice_define",
            json!({
                "id": "C4", "scene": "s3",
                "options": [
                    opt("deep", "c4_opt_deep", Some("sway_p3"), json!({"kind": "line", "id": "s3_04"})),
                    opt("pace", "c4_opt_pace", Some("sway_p1"), json!({"kind": "line", "id": "s3_04"})),
                ],
            }),
        ),
        request(
            "game_save_schema_define",
            json!({
                "version": 1, "slots": 8, "slot_holds": "one_run",
                "slot_fields": ["clock", "scene", "stats"],
                "globals_fields": ["ending", "flags_globals", "flags_run", "gallery", "rng_state", "triggers"],
                "hash_algorithm": "sha256/1",
                "policy": {"kind": "safe_scenes_only", "scenes": ["s3"]},
                "migrations": [],
            }),
        ),
        request(
            "ending_define",
            json!({"id": "ending_a", "condition": "finale_a", "priority": 10, "epilogue_scene": "ea"}),
        ),
        request(
            "ending_define",
            json!({"id": "ending_b", "condition": "finale_b", "priority": 20, "epilogue_scene": "eb"}),
        ),
        request(
            "gallery_entry_define",
            json!({
                "id": "gallery_ending_a", "category": "endings",
                "unlock": {"kind": "condition", "id": "finale_a"},
                "replay_scene": "ea", "replay_line": "ea_01", "thumbnail": "bg_listening_room",
            }),
        ),
        request(
            "gallery_entry_define",
            json!({
                "id": "gallery_ending_b", "category": "endings",
                "unlock": {"kind": "condition", "id": "finale_b"},
                "replay_scene": "eb", "replay_line": "eb_01", "thumbnail": "bg_listening_room",
            }),
        ),
        request(
            "gallery_entry_define",
            json!({
                "id": "gallery_thread", "category": "scenes",
                "unlock": {"kind": "condition", "id": "thread_done_cond"},
                "replay_scene": "t1", "replay_line": "t1_01", "thumbnail": "bg_listening_room",
            }),
        ),
    ]);
    for (scene, expression) in [
        ("s0", "guarded"),
        ("s1", "neutral"),
        ("s2", "neutral"),
        ("s3", "neutral"),
        ("ea", "devoted"),
        ("eb", "uneasy"),
    ] {
        b5.push(request(
            "scene_bind_rig",
            json!({
                "scene": scene, "participant": "noor", "rig_model": "vn-repertoire",
                "entry_expression": expression,
            }),
        ));
    }
    batch(root, b5, "batch 5 (flow records)");
}

// ---------------------------------------------------------------------
// Playthroughs: §G3.1 choice logs + checkpoint assertions.
// ---------------------------------------------------------------------

fn entry(tick: u64, kind: &str, id: &str, option: Option<&str>) -> Value {
    let mut row = json!({"tick": tick, "kind": kind, "id": id});
    if let Some(option) = option {
        row["option"] = json!(option);
    }
    row
}

/// One checkpoint assertion: (label, tick, sway, tier band,
/// scene, day, lifetime days left — None when disarmed).
type Checkpoint = (
    &'static str,
    u64,
    i64,
    usize,
    &'static str,
    i64,
    Option<u32>,
);

struct Playthrough {
    name: &'static str,
    ticks: u64,
    log: Vec<Value>,
    ending: &'static str,
    checkpoints: Vec<Checkpoint>,
}

fn engaged() -> Playthrough {
    Playthrough {
        name: "engaged",
        ticks: 20,
        log: vec![
            entry(1, "choice", "C1", Some("door")),
            entry(2, "event", "s0_close", None),
            entry(3, "choice", "C2", Some("follow")),
            entry(4, "event", "s1_close", None),
            entry(8, "choice", "TC1", Some("stay")),
            entry(9, "choice", "C3", Some("quiet")),
            entry(10, "choice", "C5", Some("tomorrow")),
            entry(18, "event", "s3_begin", None),
            entry(19, "choice", "C4", Some("deep")),
            entry(20, "event", "s3_close", None),
        ],
        ending: "ending_a",
        checkpoints: vec![
            ("C1 'door' (+2)", 1, 2, 0, "s0", 1, Some(3)),
            ("C2 'follow' (+2)", 3, 4, 1, "s1", 1, Some(3)),
            (
                "T1 entry: batch delivered, 3000 ms delay armed",
                4,
                4,
                1,
                "t1",
                1,
                Some(3),
            ),
            ("TC1 'stay' (+1)", 8, 5, 1, "s2", 2, Some(3)),
            ("C3 'quiet' (+2)", 9, 7, 2, "s2", 2, Some(3)),
            ("C5 'tomorrow' -> interim", 10, 7, 2, "interim", 2, Some(3)),
            ("S3 entry - SAVE POINT (Day 4)", 18, 7, 2, "s3", 4, Some(1)),
            ("C4 'deep' (+3)", 19, 10, 3, "s3", 4, Some(3)),
            ("finale: ending check", 20, 10, 3, "s4", 4, Some(3)),
        ],
    }
}

fn gentle() -> Playthrough {
    Playthrough {
        name: "gentle",
        ticks: 20,
        log: vec![
            entry(1, "choice", "C1", Some("sit")),
            entry(2, "event", "s0_close", None),
            entry(3, "choice", "C2", Some("letgo")),
            entry(4, "event", "s1_close", None),
            entry(8, "choice", "TC1", Some("fade")),
            entry(9, "choice", "C3", Some("decided")),
            entry(10, "choice", "C5", Some("tomorrow")),
            entry(18, "event", "s3_begin", None),
            entry(19, "choice", "C4", Some("pace")),
            entry(20, "event", "s3_close", None),
        ],
        ending: "ending_b",
        checkpoints: vec![
            ("C1 'sit' (+1)", 1, 1, 0, "s0", 1, Some(3)),
            ("C2 'letgo' (+1)", 3, 2, 0, "s1", 1, Some(3)),
            ("TC1 'fade' (+0)", 8, 2, 0, "s2", 2, Some(2)),
            ("C3 'decided' (+1)", 9, 3, 1, "s2", 2, Some(3)),
            ("S3 entry - SAVE POINT (Day 4)", 18, 3, 1, "s3", 4, Some(1)),
            ("C4 'pace' (+1)", 19, 4, 1, "s3", 4, Some(3)),
            ("finale: ending check", 20, 4, 1, "s4", 4, Some(3)),
        ],
    }
}

fn week() -> Playthrough {
    Playthrough {
        name: "week",
        ticks: 44,
        log: vec![
            entry(1, "choice", "C1", Some("door")),
            entry(2, "event", "s0_close", None),
            entry(3, "choice", "C2", Some("follow")),
            entry(4, "event", "s1_close", None),
            entry(8, "choice", "TC1", Some("stay")),
            entry(9, "choice", "C3", Some("quiet")),
            entry(10, "choice", "C5", Some("week")),
            entry(42, "event", "s3_begin", None),
            entry(43, "choice", "C4", Some("deep")),
            entry(44, "event", "s3_close", None),
        ],
        ending: "ending_b",
        checkpoints: vec![
            (
                "C3 'quiet' (+2): sway 7 by Day 2",
                9,
                7,
                2,
                "s2",
                2,
                Some(3),
            ),
            ("C5 'in a week' -> interim", 10, 7, 2, "interim", 2, Some(3)),
            (
                "Day-5 rollover: lifetime expiry hard-resets sway",
                24,
                0,
                0,
                "interim",
                5,
                None,
            ),
            ("S3 entry - SAVE POINT (Day 8)", 42, 0, 0, "s3", 8, None),
            ("C4 'deep' (+3)", 43, 3, 1, "s3", 8, Some(3)),
            ("finale: ending check", 44, 3, 1, "s4", 8, Some(3)),
        ],
    }
}

fn sway_key() -> StatKey {
    StatKey {
        character: Some("noor".to_string()),
        stat: "sway".to_string(),
    }
}

/// Runs one playthrough directly over the interpreter in
/// checkpoint segments; returns (final state, chain, events,
/// checkpoint trace rows).
fn run_checkpointed(
    project: &torogao_author::model::Project,
    play: &Playthrough,
) -> (GameState, Vec<String>, Vec<String>, Vec<String>) {
    let inputs: Vec<torogao_author::game::op::LogEntryInput> =
        serde_json::from_value(json!(play.log)).expect("log inputs parse");
    let log = parse_log(&inputs, play.ticks).expect("log parses");
    let mut state = genesis(project, 7).expect("genesis");
    let mut events = Vec::new();
    let mut chain = Vec::new();
    let mut rows = Vec::new();
    let game = project.game.as_ref().expect("game section");
    let tier = game.tier("sway").expect("tier bands");
    for (label, tick, want_sway, want_band, want_scene, want_day, want_lifetime) in
        &play.checkpoints
    {
        run_ticks(project, &mut state, &log, *tick, &mut events, &mut chain)
            .unwrap_or_else(|err| panic!("{} to tick {tick}: {err}", play.name));
        let sway = state.stats.get(&sway_key()).copied().unwrap_or(-1);
        let band = tier.band_of(sway);
        let lifetime = state.lifetimes.get(&sway_key()).copied();
        assert_eq!(sway, *want_sway, "{} @{tick} sway", play.name);
        assert_eq!(band, *want_band, "{} @{tick} tier band", play.name);
        assert_eq!(
            state.current_scene.as_deref(),
            Some(*want_scene),
            "{} @{tick} scene",
            play.name
        );
        assert_eq!(state.day, *want_day, "{} @{tick} day", play.name);
        assert_eq!(lifetime, *want_lifetime, "{} @{tick} lifetime", play.name);
        rows.push(format!(
            "tick {tick:>2} | day {want_day} | {label:<52} | sway {sway:>2} | {} | scene {:<7} | lifetime {}",
            tier.names[band],
            want_scene,
            lifetime.map_or("disarmed".to_string(), |days| format!("{days} day(s) left")),
        ));
    }
    run_ticks(
        project,
        &mut state,
        &log,
        play.ticks,
        &mut events,
        &mut chain,
    )
    .unwrap_or_else(|err| panic!("{} to end: {err}", play.name));
    assert_eq!(
        state.ending.as_deref(),
        Some(play.ending),
        "{} ending",
        play.name
    );
    (state, chain, events, rows)
}

/// Removes only the files this harness generates, so a re-run
/// rebuilds the game without touching SURFACE-GAPS.md or any
/// other resident file.
fn clean_game_dir(root: &Path) {
    std::fs::create_dir_all(root).expect("create game dir");
    for name in [
        "project.ron",
        "content.ron",
        "ops.jsonl",
        "authoring-ops.jsonl",
        ".lock",
        "backdrop.png",
        "validation.txt",
        "determinism.txt",
        "save-roundtrip.txt",
        "voice-refusal.txt",
        "asset-manifest.json",
    ] {
        let path = root.join(name);
        if path.exists() {
            std::fs::remove_file(&path).expect("clean generated file");
        }
    }
    let plays = root.join("playthroughs");
    if plays.exists() {
        std::fs::remove_dir_all(&plays).expect("clean playthroughs");
    }
}

#[test]
fn listening_room_acceptance() {
    let root = game_dir();
    clean_game_dir(&root);
    author_game(&root);

    // The authoring op log is the provenance (spec S9 gate 1):
    // the session's own log, copied under the evidence name.
    std::fs::copy(root.join("ops.jsonl"), root.join("authoring-ops.jsonl")).expect("copy op log");

    let loaded = torogao_author::store::load(&root)
        .expect("load runs")
        .expect("project exists");
    let project = &loaded.project;
    let game = project.game.as_ref().expect("game section");

    // ---- Counts: the spec's inventory, exactly. ----
    let carrier_ids = [
        "c1_opt_sit",
        "c1_opt_door",
        "c2_opt_follow",
        "c2_opt_letgo",
        "tc1_opt_stay",
        "tc1_opt_fade",
        "c3_opt_quiet",
        "c3_opt_decided",
        "c5_opt_tomorrow",
        "c5_opt_week",
        "c4_opt_deep",
        "c4_opt_pace",
    ];
    let spec_lines = game
        .dialogue
        .iter()
        .filter(|row| !carrier_ids.contains(&row.id.as_str()))
        .count();
    assert_eq!(spec_lines, 42, "spec dialogue/message lines");
    assert_eq!(game.dialogue.len(), 54, "42 spec lines + 12 label carriers");
    assert_eq!(game.choices.len(), 6, "C1, C2, TC1, C3, C5, C4");
    assert_eq!(project.content.len(), 55, "name + 42 lines + 12 labels");
    assert_eq!(game.scenes.len(), 9);
    assert_eq!(game.endings.len(), 2);
    assert_eq!(game.gallery_entries.len(), 3);

    // ---- Gate 2: project_validate clean + graph sweep. ----
    let validated = query(&root, "project_validate");
    assert_eq!(validated.status, Status::Ok, "{}", validated.doc);
    assert_eq!(validated.doc["result"], "clean", "{}", validated.doc);

    // Record-level reachability / dead-end sweep (the crate's
    // validate carries neither; see SURFACE-GAPS.md). Edges:
    // scene exits, choice scene/exit targets, ending epilogues.
    let mut edges: Vec<(&str, &str)> = Vec::new();
    for exit in &game.scene_exits {
        edges.push((exit.scene.as_str(), exit.target.as_str()));
    }
    for choice in &game.choices {
        for option in &choice.options {
            match &option.target {
                torogao_author::game::model::ChoiceTarget::Scene(id) => {
                    edges.push((choice.scene.as_str(), id.as_str()));
                }
                torogao_author::game::model::ChoiceTarget::Exit(id) => {
                    let exit = game.scene_exit(&choice.scene, id).expect("exit exists");
                    edges.push((choice.scene.as_str(), exit.target.as_str()));
                }
                _ => {}
            }
        }
    }
    for ending in &game.endings {
        edges.push(("s4", ending.epilogue_scene.as_str()));
    }
    let mut reachable: Vec<&str> = vec!["s0"];
    let mut frontier = vec!["s0"];
    while let Some(scene) = frontier.pop() {
        for (from, to) in &edges {
            if *from == scene && !reachable.contains(to) {
                reachable.push(to);
                frontier.push(to);
            }
        }
    }
    let mut unreachable: Vec<&str> = game
        .scenes
        .iter()
        .map(|scene| scene.id.as_str())
        .filter(|id| !reachable.contains(id))
        .collect();
    unreachable.sort_unstable();
    assert!(
        unreachable.is_empty(),
        "unreachable scenes: {unreachable:?}"
    );
    let epilogues: Vec<&str> = game
        .endings
        .iter()
        .map(|e| e.epilogue_scene.as_str())
        .collect();
    let dead_ends: Vec<&str> = game
        .scenes
        .iter()
        .map(|scene| scene.id.as_str())
        .filter(|id| !epilogues.contains(id) && !edges.iter().any(|(from, _)| from == id))
        .collect();
    assert!(dead_ends.is_empty(), "dead-end scenes: {dead_ends:?}");
    let priorities: Vec<i64> = game.endings.iter().map(|e| e.priority).collect();
    assert_eq!(priorities, vec![10, 20], "unique ending priorities");

    // ---- Appearance deltas resolve per tier (P2-6). ----
    let face_of = |state: Option<&str>| {
        resolve_appearance(game, "noor", state)
            .expect("appearance resolves")
            .into_iter()
            .find(|(slot, _)| slot == "face")
            .map(|(_, value)| value)
    };
    use torogao_author::game::model::AppearanceValue;
    assert_eq!(
        face_of(None),
        Some(AppearanceValue::Text("guarded".to_string()))
    );
    assert_eq!(
        face_of(Some("sway-t1")),
        Some(AppearanceValue::Text("softened".to_string()))
    );
    assert_eq!(
        face_of(Some("sway-t2")),
        Some(AppearanceValue::Text("lidded".to_string()))
    );
    assert_eq!(
        face_of(Some("sway-t3")),
        Some(AppearanceValue::Text("devoted".to_string()))
    );
    assert_eq!(
        face_of(Some("sway-reset")),
        Some(AppearanceValue::Text("uneasy".to_string()))
    );

    // ---- Thread readback (P2-7). ----
    let read = verb(&root, "thread_read", json!({"thread": "night_thread"}));
    assert_ok(&read, "thread_read");
    let messages = read.doc["messages"].as_array().expect("messages");
    assert_eq!(messages.len(), 4);
    assert_eq!(messages[0]["line"], "t1_01");
    assert_eq!(messages[3]["line"], "t1_04");

    // ---- Gate 1: op-log replay reproduces the bytes x3. ----
    let on_disk = project_sha(&root);
    for run in 1..=3 {
        let replayed = query(&root, "project_replay");
        assert_eq!(
            replayed.status,
            Status::Ok,
            "replay {run}: {}",
            replayed.doc
        );
        assert_eq!(replayed.doc["sha256"], on_disk.as_str(), "replay {run}");
    }

    // ---- Gates 3+5: playthroughs, checkpoints, determinism x3. ----
    let plays = [engaged(), gentle(), week()];
    let plays_dir = root.join("playthroughs");
    std::fs::create_dir_all(&plays_dir).expect("playthroughs dir");
    let mut determinism = String::from(
        "Determinism evidence - The Listening Room (stage 7b)\n\
         Each playthrough: direct interpreter run (checkpointed) + game_simulate x3.\n\n",
    );
    for play in &plays {
        let script = json!({
            "run_seed": 7, "ticks": play.ticks, "choice_log": play.log,
        });
        std::fs::write(
            plays_dir.join(format!("{}.json", play.name)),
            serde_json::to_string_pretty(&script).expect("script json"),
        )
        .expect("write playthrough script");

        let (_state, chain, events, rows) = run_checkpointed(project, play);
        // The verb's run reproduces the direct run's chain.
        let simulated = verb(&root, "game_simulate", script.clone());
        assert_ok(&simulated, "game_simulate");
        assert_eq!(
            simulated.doc["ending"], play.ending,
            "{} verb ending",
            play.name
        );
        let verb_chain: Vec<String> = simulated.doc["chain"]
            .as_array()
            .expect("chain")
            .iter()
            .map(|hash| hash.as_str().expect("hash").to_string())
            .collect();
        assert_eq!(
            verb_chain, chain,
            "{} verb chain == direct chain",
            play.name
        );
        // Determinism x3 through the verb surface.
        for run in 2..=3 {
            let again = verb(&root, "game_simulate", script.clone());
            assert_eq!(
                again.doc, simulated.doc,
                "{} run {run} identical",
                play.name
            );
        }
        determinism.push_str(&format!(
            "{}: ticks {}, chain {} hashes, x3 identical: true\n  first {}\n  last  {}\n",
            play.name,
            play.ticks,
            chain.len(),
            chain.first().map_or("-", String::as_str),
            chain.last().map_or("-", String::as_str),
        ));
        let mut trace = format!(
            "Playthrough '{}' - checkpoint trace (spec: PROOF-GAME-SPEC.md S5)\n\
             run_seed 7, {} ticks, ending {}\n\n",
            play.name, play.ticks, play.ending
        );
        for row in &rows {
            trace.push_str(row);
            trace.push('\n');
        }
        trace.push_str("\nEvents:\n");
        for event in &events {
            trace.push_str(&format!("  {event}\n"));
        }
        trace.push_str("\nState-hash chain:\n");
        for (index, hash) in chain.iter().enumerate() {
            trace.push_str(&format!("  tick {:>2}: {hash}\n", index + 1));
        }
        std::fs::write(plays_dir.join(format!("{}.trace.txt", play.name)), trace)
            .expect("write trace");
    }
    determinism.push_str(&format!(
        "\nOp-log replay (project_replay) x3: sha256 {} == on-disk project.ron: true\n",
        on_disk
    ));
    std::fs::write(root.join("determinism.txt"), determinism).expect("write determinism");

    // ---- Interpreter negative: TC1 during the reply delay refuses. ----
    let mut early = engaged();
    early.log = vec![
        entry(1, "choice", "C1", Some("door")),
        entry(2, "event", "s0_close", None),
        entry(3, "choice", "C2", Some("follow")),
        entry(4, "event", "s1_close", None),
        entry(6, "choice", "TC1", Some("stay")),
    ];
    early.ticks = 6;
    let inputs: Vec<torogao_author::game::op::LogEntryInput> =
        serde_json::from_value(json!(early.log)).expect("log parses");
    let log = parse_log(&inputs, early.ticks).expect("log parses");
    let mut state = genesis(project, 7).expect("genesis");
    let mut events = Vec::new();
    let mut chain = Vec::new();
    let refused = run_ticks(
        project,
        &mut state,
        &log,
        early.ticks,
        &mut events,
        &mut chain,
    )
    .expect_err("TC1 at tick 6 must refuse (delay standing)");
    assert_eq!(refused.class, RefusalClass::GameGraph);
    assert!(
        refused.message.contains("1000 ms left"),
        "refusal: {}",
        refused.message
    );

    // ---- Gate 4: save round-trip at the S3 save point. ----
    let play = engaged();
    let inputs: Vec<torogao_author::game::op::LogEntryInput> =
        serde_json::from_value(json!(play.log)).expect("log parses");
    let log = parse_log(&inputs, play.ticks).expect("log parses");
    // Uninterrupted reference.
    let mut reference = genesis(project, 7).expect("genesis");
    let mut ref_events = Vec::new();
    let mut ref_chain = Vec::new();
    run_ticks(
        project,
        &mut reference,
        &log,
        play.ticks,
        &mut ref_events,
        &mut ref_chain,
    )
    .expect("reference run");
    // Interrupted at the S3 save point (tick 18).
    let mut at_save = genesis(project, 7).expect("genesis");
    let mut save_events = Vec::new();
    let mut save_chain = Vec::new();
    run_ticks(
        project,
        &mut at_save,
        &log,
        18,
        &mut save_events,
        &mut save_chain,
    )
    .expect("run to save point");
    assert_eq!(
        at_save.current_scene.as_deref(),
        Some("s3"),
        "save policy scene"
    );
    assert_eq!(at_save.day, 4);
    let save_text = save_game(project, &loaded.sha256, &at_save, 7).expect("save at S3");
    let loaded_save = load_game(project, &loaded.sha256, &save_text).expect("save loads");
    assert_eq!(loaded_save.state, at_save, "loaded state == saved state");
    assert!(loaded_save.migrations_applied.is_empty());
    // Continue the loaded state: the chain tail and final state
    // reproduce the uninterrupted run.
    let mut resumed = loaded_save.state.clone();
    let mut res_events = Vec::new();
    let mut res_chain = Vec::new();
    run_ticks(
        project,
        &mut resumed,
        &log,
        play.ticks,
        &mut res_events,
        &mut res_chain,
    )
    .expect("resumed run");
    save_chain.extend(res_chain);
    assert_eq!(save_chain, ref_chain, "chain survives save/load");
    assert_eq!(resumed, reference, "final state survives save/load");
    // Day-tick leg: from the save point, six empty ticks cross
    // the Day-5 rollover; the lifetime counter (1 day left)
    // expires there. Load again and re-run: identical state.
    let empty_inputs: Vec<torogao_author::game::op::LogEntryInput> = Vec::new();
    let empty_log = parse_log(&empty_inputs, 24).expect("empty log parses");
    let mut aged = loaded_save.state.clone();
    let mut aged_events = Vec::new();
    let mut aged_chain = Vec::new();
    run_ticks(
        project,
        &mut aged,
        &empty_log,
        24,
        &mut aged_events,
        &mut aged_chain,
    )
    .expect("aged run");
    assert_eq!(aged.day, 5);
    assert_eq!(
        aged.stats.get(&sway_key()),
        Some(&0),
        "expiry fired across the save"
    );
    let reloaded = load_game(project, &loaded.sha256, &save_text).expect("save loads again");
    let mut aged2 = reloaded.state;
    let mut aged2_events = Vec::new();
    let mut aged2_chain = Vec::new();
    run_ticks(
        project,
        &mut aged2,
        &empty_log,
        24,
        &mut aged2_events,
        &mut aged2_chain,
    )
    .expect("aged run 2");
    assert_eq!(aged2, aged, "post-load day-tick state equality");
    assert_eq!(aged2_chain, aged_chain, "post-load day-tick chain equality");
    // Adversarial saves refuse typed and load nothing.
    let newer = save_text.replacen("schema_version: 1", "schema_version: 9", 1);
    assert_ne!(newer, save_text);
    let err = load_game(project, &loaded.sha256, &newer).expect_err("newer schema refuses");
    assert_eq!(err.code(), "TG-GAME-SAVE-SCHEMA", "{err}");
    let unknown_field = save_text.replacen("Save(\n", "Save(\n    bogus_field: 1,\n", 1);
    assert_ne!(unknown_field, save_text);
    let err2 =
        load_game(project, &loaded.sha256, &unknown_field).expect_err("unknown field refuses");
    let flipped = save_text.replacen("day: 4", "day: 5", 1);
    assert_ne!(flipped, save_text);
    let err3 = load_game(project, &loaded.sha256, &flipped).expect_err("bit-flip refuses");
    assert_eq!(err3.code(), "TG-GAME-SAVE-SCHEMA", "{err3}");
    std::fs::write(
        root.join("save-roundtrip.txt"),
        format!(
            "Save round-trip - The Listening Room (stage 7b)\n\n\
             Save point: S3 entry, engaged path, tick 18 (Day 4), sway 7 (T2 Yielding), lifetime 1 day left.\n\
             Authored schema: version 1, policy safe_scenes_only [s3]; the save was taken in s3.\n\
             save -> load: state equality: true; migrations applied: none.\n\
             Resumed chain == uninterrupted chain ({} hashes): true; final states equal: true.\n\
             Day-tick leg: save point + 6 ticks crosses the Day-5 rollover; lifetime expiry fires (sway 7 -> 0);\n\
             load + identical re-run reproduces the state and chain exactly: true.\n\n\
             Adversarial saves (all refused typed, nothing loaded):\n\
             - schema_version mutated 1 -> 9: {} ({})\n\
             - unknown field 'bogus_field' added to the Save record: {} ({})\n\
             - state bit-flip (day 4 -> 5, hash mismatch): {} ({})\n",
            ref_chain.len(),
            err.code(),
            err.message,
            err2.code(),
            err2.message,
            err3.code(),
            err3.message,
        ),
    )
    .expect("write save evidence");

    // ---- Voice: the sweep refuses the voiced variant; the ship is silent. ----
    assert!(
        project.audio_bindings.is_empty(),
        "silent edition: no bindings authored"
    );
    assert!(
        validate_shippable(project, None).is_empty(),
        "silent edition ships under no snapshot"
    );
    let mut voiced = project.clone();
    for (id, record) in [
        ("voice_noor_s1_06", "cond.noor.s1_06.1"),
        ("voice_noor_s2_08", "cond.noor.s2_08.1"),
        ("voice_noor_s3_06", "cond.noor.s3_06.1"),
    ] {
        voiced.audio_bindings.push(AudioBinding {
            id: id.to_string(),
            kind: AudioBindingKind::Voice(VoiceBinding {
                character: "noor".to_string(),
                delivery: DeliveryRef {
                    identity: "voice.cast.noor-1".to_string(),
                    record: record.to_string(),
                },
            }),
        });
    }
    // The voiced variant round-trips the codec (it is a lawful
    // project document; only its shippability is refused).
    let voiced_text = emit_project(&voiced);
    let voiced_parsed = torogao_author::codec::parse_project(&voiced_text).expect("voiced parses");
    assert_eq!(voiced_parsed.audio_bindings.len(), 3);
    let snapshot = VoiceRegistrySnapshot::parse(
        "VoiceRegistrySnapshot(\n    identities: [\n        VoiceIdentityState(\n            identity: \"voice.cast.noor-1\",\n            lock_state: Designed,\n        ),\n    ],\n)\n",
    )
    .expect("snapshot parses");
    let findings = validate_shippable(&voiced_parsed, Some(&snapshot));
    assert_eq!(
        findings.len(),
        3,
        "one refusal per voiced line: {findings:?}"
    );
    for finding in &findings {
        assert_eq!(finding.class, RefusalClass::VoiceUnadmitted);
        assert_eq!(finding.code(), "TG-AUTH-VOICE-UNADMITTED");
        assert!(
            finding.message.contains("voice.cast.noor-1"),
            "{}",
            finding.message
        );
    }
    let findings_no_snapshot = validate_shippable(&voiced_parsed, None);
    assert_eq!(findings_no_snapshot.len(), 3);
    let mut refusal_text = String::from(
        "Voice refusal evidence - The Listening Room (stage 7b, D-483/D-481)\n\n\
         Lines s1_06 (spec s1.06), s2_08 (spec s2.08), s3_06 (spec s3.06) bind identity\n\
         'voice.cast.noor-1' in the voiced variant. No voice was generated (D-481).\n\
         The identity is below EarAdmitted (snapshot states it Designed; it is equally\n\
         refused with no snapshot at all - the sweep refuses closed).\n\n\
         Shippable sweep over the voiced variant, snapshot lock_state = Designed:\n",
    );
    for finding in &findings {
        refusal_text.push_str(&format!("- {}: {}\n", finding.code(), finding.message));
    }
    refusal_text.push_str(&format!(
        "\nShippable sweep over the voiced variant, no snapshot: {} findings, all TG-AUTH-VOICE-UNADMITTED.\n\
         Shipped edition: the silent project (0 voice bindings); sweep clean under no snapshot.\n\
         NOTE: no authoring verb creates a voice binding row - see SURFACE-GAPS.md gap 1.\n",
        findings_no_snapshot.len()
    ));
    std::fs::write(root.join("voice-refusal.txt"), refusal_text).expect("write voice evidence");

    // ---- Export manifest (the export family's manifest output). ----
    let manifest = verb(&root, "export_manifest", json!({}));
    assert_ok(&manifest, "export_manifest");
    std::fs::write(
        root.join("asset-manifest.json"),
        serde_json::to_string_pretty(&manifest.doc).expect("manifest json"),
    )
    .expect("write manifest");

    // ---- validation.txt ----
    std::fs::write(
        root.join("validation.txt"),
        format!(
            "Validation - The Listening Room (stage 7b)\n\n\
             project_validate (author session verb):\n{}\n\n\
             Counts: 42 spec dialogue/message lines + 12 choice-label carrier lines = {} dialogue records;\n\
             6 choices (C1, C2, TC1, C3, C5, C4); {} content entries (1 name + 42 lines + 12 labels);\n\
             {} scenes, {} endings (priorities 10/20, unique), {} gallery entries, 1 thread (4 messages).\n\n\
             Graph sweep (record level; the crate's validate carries no reachability/dead-end\n\
             sweep - SURFACE-GAPS.md gap 3):\n\
             - scenes reachable from s0: {}/{} (unreachable: none)\n\
             - dead-end scenes (no outgoing edge, not an ending epilogue): none\n\
             - both endings graph-reachable via s4: ending_a -> ea, ending_b -> eb\n\
             - ending reachability by state rule is proven by the playthroughs (engaged -> A,\n\
               gentle -> B, week -> B over identical project bytes).\n\n\
             Appearance (P2-6): baseline face 'guarded'; sway-t1 'softened'; sway-t2 'lidded';\n\
             sway-t3 'devoted'; sway-reset 'uneasy' - all resolve.\n\n\
             Interpreter negative: TC1 attempted at tick 6 with the thread reply delay standing\n\
             refuses {}: {}\n",
            serde_json::to_string_pretty(&validated.doc).expect("validate json"),
            game.dialogue.len(),
            project.content.len(),
            game.scenes.len(),
            game.endings.len(),
            game.gallery_entries.len(),
            reachable.len(),
            game.scenes.len(),
            refused.code(),
            refused.message,
        ),
    )
    .expect("write validation");
}

#[test]
fn op_script_rebuild_is_byte_identical() {
    // The same authoring script, run into two fresh dirs,
    // produces byte-identical project.ron and content.ron —
    // the authoring route is deterministic, not just replayable.
    let a = common::fresh("proof-rebuild-a");
    let b = common::fresh("proof-rebuild-b");
    author_game(&a);
    author_game(&b);
    assert_eq!(project_sha(&a), project_sha(&b));
    let content_a = std::fs::read(a.join("content.ron")).expect("content a");
    let content_b = std::fs::read(b.join("content.ron")).expect("content b");
    assert_eq!(content_a, content_b);
    let validated = query(&a, "project_validate");
    assert_eq!(validated.status, Status::Ok, "{}", validated.doc);
    assert_eq!(validated.doc["result"], "clean");
}
