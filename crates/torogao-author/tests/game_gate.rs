//! The AUTH-G G1 slice gate (Amendment 2 §G6, VERSION-ASSESSMENT
//! §8 G-01): the five game-core verbs author a game project
//! through decide() → apply → op log; op-log replay reproduces
//! `project.ron` byte-identically ×3; `game_simulate` reproduces
//! its per-tick hash chain ×3; and the G1 refusal battery fires
//! every class the slice makes live (`TG-GAME-SAVE-SCHEMA` and
//! `TG-GAME-GRAPH` through the verbs, `TG-GAME-EXPR`,
//! `TG-GAME-TIER`, and `TG-GAME-TABLE` through the checkers G2
//! will wire to its verbs).

mod common;

use common::{fresh, init, log, project_sha, query, request, verb};
use serde_json::{Value, json};
use std::path::Path;
use torogao_author::game::{Expr, TableOutcome, check_expression, check_table, check_tier_bands};
use torogao_author::{Status, run_requests};

const WEEKDAYS: [&str; 7] = [
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
    "Sunday",
];

fn clock_inputs() -> Value {
    json!({
        "slots": ["morning", "afternoon", "evening", "night"],
        "days_per_week": 7,
        "weekdays": WEEKDAYS,
        "day_limit": 3,
    })
}

fn schema_inputs() -> Value {
    json!({
        "version": 1,
        "slots": 4,
        "slot_holds": "one_run",
        "slot_fields": ["clock", "flags_run", "rng_state", "scene", "stats", "triggers"],
        "globals_fields": ["ending", "flags_globals", "gallery"],
        "hash_algorithm": "sha256/1",
        "policy": {"kind": "any_tick_boundary"},
        "migrations": [],
    })
}

/// Authors the G1 fixture: init, clock, one RNG stream, the
/// game, and the save schema — five game ops in the log.
fn build_game(root: &Path) {
    let requests = vec![
        init("game-gate"),
        request("clock_define", clock_inputs()),
        request(
            "rng_stream_define",
            json!({"id": "events", "algorithm": "torogao-rng/1"}),
        ),
        request(
            "game_define",
            json!({"id": "fixture-game", "start_scene": "start"}),
        ),
        request("game_save_schema_define", schema_inputs()),
    ];
    let answer = run_requests(root, root, &requests);
    assert_eq!(answer.status, Status::Ok, "game fixture: {}", answer.doc);
}

fn advance_log(ticks: u64) -> Vec<Value> {
    (1..=ticks)
        .map(|tick| json!({"tick": tick, "kind": "advance", "id": "wait"}))
        .collect()
}

fn simulate(root: &Path, ticks: u64) -> Value {
    let answer = verb(
        root,
        "game_simulate",
        json!({
            "project_sha256": project_sha(root),
            "seed": 42,
            "choice_log": advance_log(ticks),
        }),
    );
    assert_eq!(answer.status, Status::Ok, "simulate: {}", answer.doc);
    answer.doc
}

/// Asserts a verb answer is a refusal carrying `code`.
fn assert_refused(answer: &torogao_author::Answer, code: &str) {
    assert_eq!(answer.status, Status::Refused, "answer: {}", answer.doc);
    assert_eq!(answer.doc["code"], code, "answer: {}", answer.doc);
}

#[test]
fn game_verbs_author_and_replay_byte_identical_three_times() {
    let root = fresh("game-replay");
    build_game(&root);

    // Five game ops appended after project_init, in order.
    let entries = log(&root);
    let verbs: Vec<&str> = entries
        .iter()
        .map(|entry| entry["verb"].as_str().expect("verb"))
        .collect();
    assert_eq!(
        verbs,
        [
            "project_init",
            "clock_define",
            "rng_stream_define",
            "game_define",
            "game_save_schema_define",
        ]
    );

    let validated = query(&root, "project_validate");
    assert_eq!(validated.status, Status::Ok, "{}", validated.doc);
    assert_eq!(validated.doc["result"], "clean");

    let on_disk = project_sha(&root);
    for run in 1..=3 {
        let replayed = query(&root, "project_replay");
        assert_eq!(replayed.status, Status::Ok, "run {run}: {}", replayed.doc);
        assert_eq!(replayed.doc["sha256"], on_disk.as_str(), "run {run}");
    }
    assert_eq!(project_sha(&root), on_disk, "replay is read-only");
}

#[test]
fn simulate_hash_chain_reproduces_three_times() {
    let root = fresh("game-simulate");
    build_game(&root);

    let first = simulate(&root, 5);
    assert_eq!(first["ticks"], 5);
    assert_eq!(first["hashes"].as_array().expect("hashes").len(), 5);
    assert_eq!(first["ending"], Value::Null);
    assert_eq!(first["stopped"], Value::Null);
    // Five advances from day 1 / morning: day 2, afternoon.
    assert_eq!(first["day"], 2);
    assert_eq!(first["slot"], "afternoon");
    for run in 2..=3 {
        let again = simulate(&root, 5);
        assert_eq!(again["chain_sha256"], first["chain_sha256"], "run {run}");
        assert_eq!(again["hashes"], first["hashes"], "run {run}");
    }
}

#[test]
fn simulate_stops_at_the_day_limit() {
    let root = fresh("game-day-limit");
    build_game(&root);

    // From day 1 / morning, eleven advances stay inside a
    // 3-day limit; the twelfth would open day 4 and does not
    // execute.
    let doc = simulate(&root, 20);
    assert_eq!(doc["ticks"], 11);
    assert_eq!(doc["stopped"], "day_limit");
    assert_eq!(doc["day"], 3);
    assert_eq!(doc["slot"], "night");
}

#[test]
fn identical_redefinition_is_a_noop_and_a_different_one_conflicts() {
    let root = fresh("game-upsert");
    build_game(&root);
    let sha_before = project_sha(&root);
    let log_len = log(&root).len();

    let again = verb(&root, "clock_define", clock_inputs());
    assert_eq!(again.status, Status::Ok, "{}", again.doc);
    assert_eq!(project_sha(&root), sha_before, "a no-op writes nothing");
    // The op log records the admitted no-op (the brief's model:
    // replaying it is itself a no-op), with the unchanged hash.
    let entries = log(&root);
    assert_eq!(entries.len(), log_len + 1, "a no-op is logged once");
    assert_eq!(entries[log_len]["verb"], "clock_define");
    assert_eq!(
        entries[log_len]["project_sha256_after"],
        sha_before.as_str()
    );

    let mut different = clock_inputs();
    different["day_limit"] = json!(9);
    assert_refused(&verb(&root, "clock_define", different), "TG-AUTH-CONFLICT");
}

#[test]
fn clock_and_rng_refusals() {
    let root = fresh("game-clock-rng");
    let answer = run_requests(&root, &root, &[init("game-clock-rng")]);
    assert_eq!(answer.status, Status::Ok, "{}", answer.doc);

    assert_refused(
        &verb(&root, "clock_define", json!({"slots": []})),
        "TG-AUTH-MALFORMED",
    );
    assert_refused(
        &verb(
            &root,
            "clock_define",
            json!({"slots": ["morning", "morning"]}),
        ),
        "TG-AUTH-MALFORMED",
    );
    assert_refused(
        &verb(
            &root,
            "clock_define",
            json!({"slots": ["morning"], "day_limit": 0}),
        ),
        "TG-AUTH-RANGE",
    );
    assert_refused(
        &verb(
            &root,
            "clock_define",
            json!({"slots": ["morning"], "days_per_week": 7, "weekdays": ["Monday"]}),
        ),
        "TG-AUTH-MALFORMED",
    );
    assert_refused(
        &verb(
            &root,
            "rng_stream_define",
            json!({"id": "events", "algorithm": "lcg/9"}),
        ),
        "TG-AUTH-UNKNOWN-KIND",
    );
}

#[test]
fn game_define_refusals() {
    let root = fresh("game-define");
    let answer = run_requests(&root, &root, &[init("game-define")]);
    assert_eq!(answer.status, Status::Ok, "{}", answer.doc);

    // G1 authors no stat/flag definitions, so an initial
    // assignment is honestly dangling until G2.2.
    assert_refused(
        &verb(
            &root,
            "game_define",
            json!({
                "id": "g",
                "start_scene": "start",
                "initial": [{"target": "nerve", "value": 3}],
            }),
        ),
        "TG-AUTH-DANGLING",
    );
    // A G2 game verb is refused by name, with its slice.
    let refused = verb(&root, "stat_define", json!({}));
    assert_refused(&refused, "TG-AUTH-UNKNOWN-KIND");
    assert!(
        refused.doc["message"]
            .as_str()
            .expect("message")
            .contains("G2"),
        "{}",
        refused.doc
    );
}

#[test]
fn save_schema_refusal_battery() {
    let root = fresh("game-schema");
    let answer = run_requests(&root, &root, &[init("game-schema")]);
    assert_eq!(answer.status, Status::Ok, "{}", answer.doc);

    // A field in both partitions.
    let mut both = schema_inputs();
    both["globals_fields"] = json!(["clock", "ending", "flags_globals", "gallery"]);
    assert_refused(
        &verb(&root, "game_save_schema_define", both),
        "TG-GAME-SAVE-SCHEMA",
    );
    // A field in neither partition.
    let mut neither = schema_inputs();
    neither["slot_fields"] = json!(["clock", "flags_run", "rng_state", "scene", "stats"]);
    assert_refused(
        &verb(&root, "game_save_schema_define", neither),
        "TG-GAME-SAVE-SCHEMA",
    );
    // A field the family cannot produce (the cover stays exact,
    // so only the unknown name can refuse).
    let mut unknown = schema_inputs();
    unknown["globals_fields"] = json!(["ending", "flags_globals", "gallery", "weather"]);
    assert_refused(
        &verb(&root, "game_save_schema_define", unknown),
        "TG-GAME-SAVE-SCHEMA",
    );
    // Slot counts outside 1..=64.
    for slots in [0, 65] {
        let mut bad = schema_inputs();
        bad["slots"] = json!(slots);
        assert_refused(
            &verb(&root, "game_save_schema_define", bad),
            "TG-GAME-SAVE-SCHEMA",
        );
    }
    // A hash algorithm with no version named.
    let mut unversioned = schema_inputs();
    unversioned["hash_algorithm"] = json!("sha256");
    assert_refused(
        &verb(&root, "game_save_schema_define", unversioned),
        "TG-GAME-SAVE-SCHEMA",
    );
    // A versioned algorithm the interpreter does not implement.
    let mut foreign = schema_inputs();
    foreign["hash_algorithm"] = json!("md5/1");
    assert_refused(
        &verb(&root, "game_save_schema_define", foreign),
        "TG-AUTH-UNKNOWN-KIND",
    );
    // A migration chain with a gap (version 3, only a step from 1).
    let mut gapped = schema_inputs();
    gapped["version"] = json!(3);
    gapped["migrations"] = json!([{"from": 1, "name": "first"}]);
    assert_refused(
        &verb(&root, "game_save_schema_define", gapped),
        "TG-GAME-SAVE-SCHEMA",
    );
    // A valid v1, then a v2 whose chain does not start at v1,
    // then a v2 done properly.
    let ok = verb(&root, "game_save_schema_define", schema_inputs());
    assert_eq!(ok.status, Status::Ok, "{}", ok.doc);
    let mut orphaned = schema_inputs();
    orphaned["version"] = json!(2);
    orphaned["slots"] = json!(8);
    assert_refused(
        &verb(&root, "game_save_schema_define", orphaned),
        "TG-GAME-SAVE-SCHEMA",
    );
    let mut chained = schema_inputs();
    chained["version"] = json!(2);
    chained["slots"] = json!(8);
    chained["migrations"] = json!([{"from": 1, "name": "widen-slots"}]);
    let ok = verb(&root, "game_save_schema_define", chained);
    assert_eq!(ok.status, Status::Ok, "{}", ok.doc);
    // A same-version redefinition with different content.
    let mut same = schema_inputs();
    same["version"] = json!(2);
    same["slots"] = json!(16);
    same["migrations"] = json!([{"from": 1, "name": "widen-slots"}]);
    assert_refused(
        &verb(&root, "game_save_schema_define", same),
        "TG-GAME-SAVE-SCHEMA",
    );
}

#[test]
fn simulate_refusal_battery() {
    let root = fresh("game-simulate-refusals");
    build_game(&root);

    // A log authored against another project.
    assert_refused(
        &verb(
            &root,
            "game_simulate",
            json!({"project_sha256": "0".repeat(64), "seed": 1, "choice_log": []}),
        ),
        "TG-AUTH-CONFLICT",
    );
    // Non-consecutive ticks.
    assert_refused(
        &verb(
            &root,
            "game_simulate",
            json!({
                "project_sha256": project_sha(&root),
                "seed": 1,
                "choice_log": [{"tick": 2, "kind": "advance", "id": "wait"}],
            }),
        ),
        "TG-AUTH-MALFORMED",
    );
    // An input kind the interpreter does not consume.
    assert_refused(
        &verb(
            &root,
            "game_simulate",
            json!({
                "project_sha256": project_sha(&root),
                "seed": 1,
                "choice_log": [{"tick": 1, "kind": "teleport", "id": "x"}],
            }),
        ),
        "TG-AUTH-UNKNOWN-KIND",
    );
    // A choice input: no choices exist to honour in G1.
    assert_refused(
        &verb(
            &root,
            "game_simulate",
            json!({
                "project_sha256": project_sha(&root),
                "seed": 1,
                "choice_log": [{"tick": 1, "kind": "choice", "id": "door", "option": "open"}],
            }),
        ),
        "TG-GAME-GRAPH",
    );
}

#[test]
fn simulate_needs_a_game_and_a_clock() {
    let root = fresh("game-simulate-empty");
    let answer = run_requests(&root, &root, &[init("game-simulate-empty")]);
    assert_eq!(answer.status, Status::Ok, "{}", answer.doc);
    assert_refused(
        &verb(
            &root,
            "game_simulate",
            json!({"project_sha256": project_sha(&root), "seed": 1, "choice_log": []}),
        ),
        "TG-AUTH-UNKNOWN-TARGET",
    );
}

#[test]
fn expression_checker_refusals() {
    let stats = ["nerve"];
    let flags = ["met"];
    let cyc_a = Expr::ValueRef("b".to_string());
    let cyc_b = Expr::ValueRef("a".to_string());
    let values = [("a", &cyc_a), ("b", &cyc_b)];

    // A reference cycle among named values.
    let err = check_expression(&Expr::ValueRef("a".to_string()), &stats, &flags, &values)
        .expect_err("cycle refuses");
    assert_eq!(err.code(), "TG-GAME-EXPR");
    // An unknown stat.
    let err = check_expression(&Expr::Stat("luck".to_string()), &stats, &flags, &[])
        .expect_err("unknown stat refuses");
    assert_eq!(err.code(), "TG-GAME-EXPR");
    // A flag in an arithmetic position.
    let sum = Expr::Arith(
        torogao_author::game::Arith::Add,
        Box::new(Expr::Flag("met".to_string())),
        Box::new(Expr::Lit(1)),
    );
    let err = check_expression(&sum, &stats, &flags, &[]).expect_err("flag arithmetic refuses");
    assert_eq!(err.code(), "TG-GAME-EXPR");
    // An ordering comparison over flags.
    let ordered = Expr::Compare(
        torogao_author::game::Cmp::Lt,
        Box::new(Expr::Flag("met".to_string())),
        Box::new(Expr::BoolLit(true)),
    );
    let err = check_expression(&ordered, &stats, &flags, &[]).expect_err("flag ordering refuses");
    assert_eq!(err.code(), "TG-GAME-EXPR");
    // A well-typed tree passes: tier-of against a literal.
    let fine = Expr::Compare(
        torogao_author::game::Cmp::Ge,
        Box::new(Expr::TierOf("nerve".to_string())),
        Box::new(Expr::Lit(2)),
    );
    assert_eq!(
        check_expression(&fine, &stats, &flags, &[]).expect("well-typed"),
        torogao_author::game::ExprType::Bool
    );
}

#[test]
fn tier_and_table_checker_refusals() {
    // Thresholds not strictly ascending.
    let err = check_tier_bands(
        "nerve",
        0,
        100,
        &[50, 50],
        &["low".into(), "mid".into(), "high".into()],
    )
    .expect_err("plateau refuses");
    assert_eq!(err.code(), "TG-GAME-TIER");
    // A band outside the stat's range.
    let err = check_tier_bands("nerve", 0, 100, &[101], &["low".into(), "high".into()])
        .expect_err("out of range refuses");
    assert_eq!(err.code(), "TG-GAME-TIER");
    // A tier count that disagrees with the names.
    let err = check_tier_bands("nerve", 0, 100, &[50], &["only".into()])
        .expect_err("count mismatch refuses");
    assert_eq!(err.code(), "TG-GAME-TIER");
    // A sound band set passes.
    check_tier_bands(
        "nerve",
        0,
        100,
        &[34, 67],
        &["low".into(), "mid".into(), "high".into()],
    )
    .expect("sound bands");

    let outcome = |id: &str, weight: i64| TableOutcome {
        id: id.to_string(),
        weight,
    };
    // No named stream.
    let err = check_table(None, &["events"], &[outcome("a", 1)]).expect_err("no stream refuses");
    assert_eq!(err.code(), "TG-GAME-TABLE");
    // An undefined stream is dangling, not a table defect.
    let err = check_table(Some("battle"), &["events"], &[outcome("a", 1)])
        .expect_err("unknown stream refuses");
    assert_eq!(err.code(), "TG-AUTH-DANGLING");
    // A non-positive weight.
    let err = check_table(Some("events"), &["events"], &[outcome("a", 0)])
        .expect_err("zero weight refuses");
    assert_eq!(err.code(), "TG-GAME-TABLE");
    // A duplicate outcome id.
    let err = check_table(
        Some("events"),
        &["events"],
        &[outcome("a", 1), outcome("a", 2)],
    )
    .expect_err("duplicate refuses");
    assert_eq!(err.code(), "TG-GAME-TABLE");
    // A zero total (no outcomes at all).
    let err = check_table(Some("events"), &["events"], &[]).expect_err("empty refuses");
    assert_eq!(err.code(), "TG-GAME-TABLE");
    // A sound table passes.
    check_table(
        Some("events"),
        &["events"],
        &[outcome("a", 55), outcome("b", 45)],
    )
    .expect("sound table");
}
