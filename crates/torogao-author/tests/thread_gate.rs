//! The Stage 7a gate (P2-7 message-thread family, ruling
//! D-461): `thread_create` / `thread_post` / `thread_read`
//! live in the game manifest, thread records round-trip
//! through the one codec as a conditional section (projects
//! without threads keep their exact bytes), the decide()
//! refusals fire typed, and the interpreter delivers a
//! scene's thread interlude as one instant batch at entry,
//! consumes the authored reply delay on the step clock
//! (never wall time), refuses a choice while the delay
//! remains, applies a thread choice's effect set through the
//! ordinary choice machinery, and never re-delivers on
//! re-entry (fire-once).

mod common;

use common::{fresh, init, log, project_sha, query, request, verb};
use serde_json::{Value, json};
use std::path::Path;
use torogao_author::game::GAME_MANIFEST;
use torogao_author::game::simulate::{genesis, parse_log, run_ticks};
use torogao_author::game::state::StatKey;
use torogao_author::{RefusalClass, Status, run_requests};

/// Asserts a verb answer is a refusal carrying `code`.
fn assert_refused(answer: &torogao_author::Answer, code: &str) {
    assert_eq!(answer.status, Status::Refused, "answer: {}", answer.doc);
    assert_eq!(answer.doc["code"], code, "answer: {}", answer.doc);
}

/// Asserts a verb answer was admitted.
fn assert_ok(answer: &torogao_author::Answer) {
    assert_eq!(answer.status, Status::Ok, "answer: {}", answer.doc);
}

/// One dialogue line authoring a prose key in the room scene
/// (the C-7 route by which message prose enters the sidecar).
fn room_line(id: &str, ordinal: u32, speaker: &str, text_ref: &str, text: &str) -> Value {
    json!({
        "id": id, "scene": "room", "ordinal": ordinal, "speaker": speaker,
        "text_ref": text_ref, "text": text,
    })
}

/// Authors the thread fixture: a two-character cast, a `sway`
/// stat with a +1 effect set, the room's dialogue lines (which
/// carry every content key the thread and choices name), the
/// four-message thread of the proof game's T1 interlude, the
/// interlude scene naming it with the spec's 3,000 ms authored
/// delay, and the two choices (go to the thread; the thread
/// choice itself).
fn author_thread(root: &Path) {
    let mut requests = vec![
        init("thread-gate"),
        request(
            "game_define",
            json!({"id": "thread-game", "start_scene": "room"}),
        ),
        request("clock_define", json!({"slots": ["morning", "night"]})),
        request(
            "stat_define",
            json!({
                "id": "sway", "scope": "game", "kind": {"kind": "int"},
                "min": 0, "max": 12, "default": 0, "overflow": "clamp",
            }),
        ),
        request(
            "effect_set_define",
            json!({
                "id": "sway_up",
                "atoms": [{
                    "kind": "stat_add", "stat": "sway",
                    "amount": {"kind": "lit", "value": 1},
                }],
            }),
        ),
        request(
            "character_define",
            json!({"id": "noor", "name_ref": "name_noor", "name_text": "Noor"}),
        ),
        request(
            "character_define",
            json!({"id": "guide", "name_ref": "name_guide", "name_text": "Guide"}),
        ),
        request(
            "character_define",
            json!({"id": "outsider", "name_ref": "name_outsider", "name_text": "Outsider"}),
        ),
        request(
            "scene_define",
            json!({"id": "room", "presentation": "vn", "participants": ["noor"]}),
        ),
    ];
    for line in [
        room_line(
            "room_line",
            0,
            "narrator",
            "text_room",
            "The listening room.",
        ),
        room_line("m1", 1, "noor", "t_msg_1", "awake. obviously."),
        room_line("m2", 2, "noor", "t_msg_2", "the quiet followed me home."),
        room_line("m3", 3, "noor", "t_msg_3", "then i'm letting it."),
        room_line("m4", 4, "noor", "t_msg_4", "tomorrow. same time."),
        room_line("l_stay", 5, "narrator", "t_stay", "It's yours now."),
        room_line("l_fade", 6, "narrator", "t_fade", "It fades."),
        room_line("l_go", 7, "narrator", "t_go", "Open the thread."),
    ] {
        requests.push(request("dialogue_line_define", line));
    }
    requests.extend([
        request(
            "thread_create",
            json!({"id": "night_thread", "participants": ["noor", "guide"]}),
        ),
        request(
            "thread_post",
            json!({"thread": "night_thread", "id": "msg_1", "speaker": "noor", "line": "t_msg_1", "at": 0}),
        ),
        request(
            "thread_post",
            json!({"thread": "night_thread", "id": "msg_2", "speaker": "noor", "line": "t_msg_2", "at": 1}),
        ),
        request(
            "thread_post",
            json!({"thread": "night_thread", "id": "msg_3", "speaker": "noor", "line": "t_msg_3", "at": 2}),
        ),
        request(
            "thread_post",
            json!({"thread": "night_thread", "id": "msg_4", "speaker": "guide", "line": "t_msg_4", "at": 3}),
        ),
        request(
            "scene_define",
            json!({
                "id": "thread_room", "presentation": "vn", "participants": ["noor"],
                "thread_interlude": {"thread": "night_thread", "reply_delay_ms": 3000},
            }),
        ),
        request(
            "choice_define",
            json!({
                "id": "go_choice", "scene": "room",
                "options": [{
                    "id": "go", "label_ref": "t_go", "origin": "player",
                    "target": {"kind": "scene", "id": "thread_room"},
                }],
            }),
        ),
        request(
            "choice_define",
            json!({
                "id": "thread_choice", "scene": "thread_room", "prompt_line": "t_msg_1",
                "options": [
                    {
                        "id": "stay", "label_ref": "t_stay", "origin": "player",
                        "effect_set": "sway_up",
                        "target": {"kind": "scene", "id": "room"},
                    },
                    {
                        "id": "fade", "label_ref": "t_fade", "origin": "player",
                        "target": {"kind": "scene", "id": "room"},
                    },
                ],
            }),
        ),
    ]);
    let answer = run_requests(root, root, &requests);
    assert_eq!(answer.status, Status::Ok, "thread fixture: {}", answer.doc);
}

#[test]
fn thread_records_author_validate_read_and_replay_three_times() {
    let root = fresh("thread-replay");
    author_thread(&root);

    let entries = log(&root);
    let verbs: Vec<&str> = entries
        .iter()
        .map(|entry| entry["verb"].as_str().expect("verb"))
        .collect();
    assert!(verbs.contains(&"thread_create"), "{verbs:?}");
    assert_eq!(
        verbs.iter().filter(|name| **name == "thread_post").count(),
        4,
        "{verbs:?}"
    );

    let bytes = std::fs::read_to_string(root.join("project.ron")).expect("project bytes");
    assert!(bytes.contains("threads"), "{bytes}");
    assert!(bytes.contains("ThreadMessage"), "{bytes}");
    assert!(bytes.contains("thread_interlude"), "{bytes}");

    // thread_read returns the messages in authored order,
    // with participants in canonical (sorted) order.
    let read = verb(&root, "thread_read", json!({"thread": "night_thread"}));
    assert_ok(&read);
    assert_eq!(read.doc["thread"], "night_thread");
    assert_eq!(read.doc["participants"], json!(["guide", "noor"]));
    let messages = read.doc["messages"].as_array().expect("messages");
    let ids: Vec<&str> = messages
        .iter()
        .map(|row| row["id"].as_str().expect("id"))
        .collect();
    assert_eq!(ids, ["msg_1", "msg_2", "msg_3", "msg_4"]);
    assert_eq!(messages[0]["speaker"], "noor");
    assert_eq!(messages[0]["line"], "t_msg_1");
    assert_eq!(messages[3]["speaker"], "guide");
    assert_eq!(messages[3]["at"], 3);

    let validated = query(&root, "project_validate");
    assert_eq!(validated.status, Status::Ok, "{}", validated.doc);
    assert_eq!(validated.doc["result"], "clean", "{}", validated.doc);

    let on_disk = project_sha(&root);
    for run in 1..=3 {
        let replayed = query(&root, "project_replay");
        assert_eq!(replayed.status, Status::Ok, "run {run}: {}", replayed.doc);
        assert_eq!(replayed.doc["sha256"], on_disk.as_str(), "run {run}");
    }
}

#[test]
fn thread_verbs_are_live_in_the_game_census() {
    assert_eq!(GAME_MANIFEST.len(), 27);
    let implemented = GAME_MANIFEST.iter().filter(|spec| spec.implemented).count();
    assert_eq!(implemented, 27);
    for (name, mutating) in [
        ("thread_create", true),
        ("thread_post", true),
        ("thread_read", false),
    ] {
        let spec = GAME_MANIFEST
            .iter()
            .find(|spec| spec.name == name)
            .expect("verb in manifest");
        assert!(spec.implemented, "{name} must be implemented");
        assert_eq!(spec.mutating, mutating, "{name}");
    }
}

#[test]
fn projects_without_threads_keep_their_pre_p27_bytes() {
    // The byte-identity negative gate: a project authored with
    // none of the thread machinery emits exactly the bytes the
    // pre-P2-7 codec emitted — no `threads` section, no
    // `thread_interlude` field — and a codec round-trip on
    // that pre-P2-7-shaped document reproduces it identically.
    let root = fresh("thread-plain");
    let requests = vec![
        init("thread-plain"),
        request("game_define", json!({"id": "vn", "start_scene": "room"})),
        request("clock_define", json!({"slots": ["morning", "night"]})),
        request(
            "character_define",
            json!({"id": "amy", "name_ref": "name_amy", "name_text": "Amy"}),
        ),
        request("scene_define", json!({"id": "room", "presentation": "vn"})),
        request(
            "dialogue_line_define",
            json!({
                "id": "l1", "scene": "room", "ordinal": 0, "speaker": "amy",
                "text_ref": "t1", "text": "Hello.",
            }),
        ),
    ];
    let answer = run_requests(&root, &root, &requests);
    assert_eq!(answer.status, Status::Ok, "plain project: {}", answer.doc);
    let bytes = std::fs::read_to_string(root.join("project.ron")).expect("project bytes");
    for absent in ["threads", "Thread", "thread_interlude", "ThreadInterlude"] {
        assert!(
            !bytes.contains(absent),
            "plain bytes carry {absent}: {bytes}"
        );
    }
    let parsed = torogao_author::codec::parse_project(&bytes).expect("pre-P2-7 shape parses");
    assert_eq!(
        torogao_author::codec::emit_project(&parsed),
        bytes,
        "the pre-P2-7-shaped document re-emits byte-identically"
    );
    assert!(parsed.game.is_some_and(|game| game.threads.is_empty()));
    let validated = query(&root, "project_validate");
    assert_eq!(validated.status, Status::Ok, "{}", validated.doc);
    assert_eq!(validated.doc["result"], "clean", "{}", validated.doc);
}

#[test]
fn thread_decide_refusals_are_typed() {
    let root = fresh("thread-refusals");
    author_thread(&root);

    // Create with a participant who is not a cast member.
    assert_refused(
        &verb(
            &root,
            "thread_create",
            json!({"id": "bad_cast", "participants": ["noor", "ghost"]}),
        ),
        "TG-AUTH-DANGLING",
    );
    // Post by a cast member who is not a thread participant.
    assert_refused(
        &verb(
            &root,
            "thread_post",
            json!({"thread": "night_thread", "id": "msg_x", "speaker": "outsider", "line": "t_msg_1", "at": 4}),
        ),
        "TG-GAME-GRAPH",
    );
    // Post naming a content key nobody authored.
    assert_refused(
        &verb(
            &root,
            "thread_post",
            json!({"thread": "night_thread", "id": "msg_x", "speaker": "noor", "line": "t_missing", "at": 4}),
        ),
        "TG-AUTH-DANGLING",
    );
    // Post carrying inline prose instead of a content key:
    // the C-7 law — the input shape has no text field at all.
    assert_refused(
        &verb(
            &root,
            "thread_post",
            json!({"thread": "night_thread", "id": "msg_x", "speaker": "noor", "line": "t_msg_1", "at": 4, "text": "inline prose"}),
        ),
        "TG-AUTH-MALFORMED",
    );
    // Post to a thread that does not exist.
    assert_refused(
        &verb(
            &root,
            "thread_post",
            json!({"thread": "nope", "id": "msg_x", "speaker": "noor", "line": "t_msg_1", "at": 0}),
        ),
        "TG-AUTH-DANGLING",
    );
    // A duplicate message id with different inputs conflicts;
    // the identical repost is a no-op.
    assert_refused(
        &verb(
            &root,
            "thread_post",
            json!({"thread": "night_thread", "id": "msg_1", "speaker": "guide", "line": "t_msg_2", "at": 0}),
        ),
        "TG-AUTH-CONFLICT",
    );
    let sha_before = project_sha(&root);
    assert_ok(&verb(
        &root,
        "thread_post",
        json!({"thread": "night_thread", "id": "msg_1", "speaker": "noor", "line": "t_msg_1", "at": 0}),
    ));
    assert_eq!(
        project_sha(&root),
        sha_before,
        "identical repost is a no-op"
    );
    // A position that would leave a gap.
    assert_refused(
        &verb(
            &root,
            "thread_post",
            json!({"thread": "night_thread", "id": "msg_x", "speaker": "noor", "line": "t_msg_1", "at": 9}),
        ),
        "TG-GAME-GRAPH",
    );
    // Read of an unknown thread.
    assert_refused(
        &verb(&root, "thread_read", json!({"thread": "nope"})),
        "TG-AUTH-UNKNOWN-TARGET",
    );
    // An interlude naming a thread that does not exist.
    assert_refused(
        &verb(
            &root,
            "scene_define",
            json!({
                "id": "bad_scene", "presentation": "vn",
                "thread_interlude": {"thread": "nope", "reply_delay_ms": 1},
            }),
        ),
        "TG-AUTH-DANGLING",
    );
}

#[test]
fn interlude_delivers_batch_delay_gates_choice_and_fires_once() {
    let root = fresh("thread-interpreter");
    author_thread(&root);
    let project = torogao_author::store::load(&root)
        .expect("load runs")
        .expect("project exists")
        .project;
    let run = |entries: Vec<Value>, ticks: u64| {
        let inputs: Vec<torogao_author::game::op::LogEntryInput> =
            serde_json::from_value(json!(entries)).expect("log inputs parse");
        let log = parse_log(&inputs, ticks).expect("log parses");
        let mut state = genesis(&project, 7).expect("genesis");
        let mut events = Vec::new();
        let mut chain = Vec::new();
        run_ticks(&project, &mut state, &log, ticks, &mut events, &mut chain)
            .map(|()| (state, events, chain))
    };
    let go = json!({"tick": 1, "kind": "choice", "id": "go_choice", "option": "go"});

    // (b) Too early: entering at tick 1 arms 3,000 ms; ticks 2
    // and 3 consume 2,000 of it, so a thread choice at tick 3
    // still has 1,000 ms standing and the run refuses.
    let refused = run(
        vec![
            go.clone(),
            json!({"tick": 3, "kind": "choice", "id": "thread_choice", "option": "stay"}),
        ],
        3,
    )
    .expect_err("choice during the delay refuses");
    assert_eq!(refused.class, RefusalClass::GameGraph);
    assert!(
        refused.message.contains("1000 ms left"),
        "refusal: {}",
        refused.message
    );

    // On time: the third step after entry (tick 4) consumes
    // the last 1,000 ms, and the thread choice fires through
    // the ordinary choice machinery.
    let (state, events, chain) = run(
        vec![
            go.clone(),
            json!({"tick": 4, "kind": "choice", "id": "thread_choice", "option": "stay"}),
        ],
        4,
    )
    .expect("on-time run");
    // (a) The batch delivered as one event, in authored order,
    // at entry (tick 1).
    assert!(
        events
            .iter()
            .any(|event| event
                == "tick 1: thread 'night_thread' delivered [msg_1, msg_2, msg_3, msg_4]"),
        "events: {events:?}"
    );
    assert!(
        events
            .iter()
            .any(|event| event == "tick 1: thread 'night_thread' reply delay 3000 ms pending"),
        "events: {events:?}"
    );
    // (b) The delay elapsed exactly at tick 4 — three step
    // clock steps after the tick-1 entry.
    assert!(
        events
            .iter()
            .any(|event| event == "tick 4: thread reply delay elapsed"),
        "events: {events:?}"
    );
    assert_eq!(state.tick, 4);
    assert_eq!(chain.len(), 4);
    // (d) The thread choice applied its effect set exactly
    // like a scene choice, and its target transitioned.
    assert_eq!(
        state.stats.get(&StatKey {
            character: None,
            stat: "sway".to_string(),
        }),
        Some(&1),
        "events: {events:?}"
    );
    assert!(
        events
            .iter()
            .any(|event| event.contains("choice 'thread_choice' presented [stay, fade]")),
        "events: {events:?}"
    );
    assert_eq!(state.current_scene.as_deref(), Some("room"));

    // (c) Re-entry does not re-deliver: go back at tick 5 and
    // the run to tick 6 carries exactly one delivery.
    let (state, events, _) = run(
        vec![
            go,
            json!({"tick": 4, "kind": "choice", "id": "thread_choice", "option": "fade"}),
            json!({"tick": 5, "kind": "choice", "id": "go_choice", "option": "go"}),
        ],
        6,
    )
    .expect("re-entry run");
    assert_eq!(state.current_scene.as_deref(), Some("thread_room"));
    assert_eq!(
        events
            .iter()
            .filter(|event| event.contains("delivered ["))
            .count(),
        1,
        "events: {events:?}"
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| event.contains("reply delay"))
            .count(),
        2, // one "3000 ms pending" (tick 1) + one "elapsed" (tick 4); none on re-entry
        "events: {events:?}"
    );

    // Through the verb: the same trace lands in the answer.
    let answer = verb(
        &root,
        "game_simulate",
        json!({
            "run_seed": 7, "ticks": 4,
            "choice_log": [
                {"tick": 1, "kind": "choice", "id": "go_choice", "option": "go"},
                {"tick": 4, "kind": "choice", "id": "thread_choice", "option": "stay"},
            ],
        }),
    );
    assert_ok(&answer);
    assert_eq!(answer.doc["final"]["scene"], "room");
    let events = answer.doc["events"].as_array().expect("events");
    assert!(
        events.iter().any(|event| event
            .as_str()
            .is_some_and(|e| e.contains("thread 'night_thread' delivered"))),
        "events: {}",
        answer.doc
    );
}
