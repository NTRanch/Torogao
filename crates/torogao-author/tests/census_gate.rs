//! The A1 slice gate: a fixture rig authored from text alone passes
//! project_validate and project_inspect, and inspect reproduces the
//! census fields the fixture was shaped from. Replay and rebuild
//! determinism are measured here too (×3 each).

mod common;

use common::{build_fixture, fixture_requests, fresh, log, project_sha, query};
use serde_json::{Map, Value};
use torogao_author::{Status, run_requests};

fn source() -> Value {
    serde_json::from_str(include_str!("fixtures/census-a1/census-source.json")).expect("source")
}

/// Keeps only `keys` of every object in an array.
fn pick(items: &Value, keys: &[&str]) -> Vec<Value> {
    items
        .as_array()
        .expect("array")
        .iter()
        .map(|item| {
            let mut out = Map::new();
            for key in keys {
                out.insert((*key).to_string(), item[*key].clone());
            }
            Value::Object(out)
        })
        .collect()
}

#[test]
fn fixture_validates_clean_and_inspect_reproduces_the_census() {
    let root = fresh("census-gate");
    build_fixture(&root);
    let validated = query(&root, "project_validate");
    assert_eq!(validated.status, Status::Ok, "{}", validated.doc);
    assert_eq!(validated.doc["result"], "clean");

    let inspected = query(&root, "project_inspect");
    assert_eq!(inspected.status, Status::Ok, "{}", inspected.doc);
    let census = &inspected.doc["census"];
    let want = source();

    let part_keys = ["id", "parent", "deformer", "drawable", "visible"];
    assert_eq!(
        pick(&census["parts"]["tree"], &part_keys),
        pick(&want["parts"], &part_keys)
    );
    assert_eq!(census["parts"]["depth_max"], want["part_depth_max"]);

    let deformer_keys = ["id", "kind", "parent", "keyed_on"];
    assert_eq!(
        pick(&census["deformers"]["tree"], &deformer_keys),
        pick(&want["deformers"], &deformer_keys)
    );
    assert_eq!(
        census["deformers"]["chain_depth_max"],
        want["deformer_chain_depth_max"]
    );
    let glue_keys = ["id", "a", "b", "pairs"];
    assert_eq!(
        pick(&census["deformers"]["glues"], &glue_keys),
        pick(&want["glues"], &glue_keys)
    );

    let parameter_keys = [
        "id",
        "class",
        "min",
        "default",
        "max",
        "neutral",
        "grid_partners",
    ];
    assert_eq!(
        pick(&census["parameters"]["table"], &parameter_keys),
        pick(&want["parameters"], &parameter_keys)
    );
    assert_eq!(census["parameters"]["grids"], want["grids"]);

    assert_eq!(census["draw"]["bands"], want["draw_bands"]);
    assert_eq!(
        census["masks"],
        Value::Array(pick(&want["masks"], &["inverted", "part", "source"]))
    );
    let expression_keys = ["id", "blend", "parameters", "part_swaps"];
    assert_eq!(
        pick(&census["expressions"]["table"], &expression_keys),
        pick(&want["expressions"], &expression_keys)
    );

    let density = &census["mesh_density"];
    assert_eq!(
        density["total_vertices"],
        want["mesh_density"]["total_vertices"]
    );
    assert_eq!(
        density["total_triangles"],
        want["mesh_density"]["total_triangles"]
    );
    assert_eq!(
        density["parts"].as_array().expect("parts").len() as u64,
        want["mesh_density"]["drawables"]
            .as_u64()
            .expect("drawables")
    );
    let lattices = census["lattices"].as_array().expect("lattices");
    assert_eq!(
        lattices.len() as u64,
        want["lattices"]["count"].as_u64().expect("count")
    );
    assert!(lattices.iter().all(|lattice| lattice["complete"] == true));
}

#[test]
fn replay_reproduces_project_bytes_three_times() {
    let root = fresh("census-replay");
    build_fixture(&root);
    let on_disk = project_sha(&root);
    for run in 0..3 {
        let replayed = query(&root, "project_replay");
        assert_eq!(replayed.status, Status::Ok, "run {run}: {}", replayed.doc);
        assert_eq!(replayed.doc["sha256"], on_disk.as_str(), "run {run}");
    }
    assert_eq!(project_sha(&root), on_disk, "replay is read-only");
}

#[test]
fn independent_builds_are_byte_identical_three_times() {
    let shas: Vec<String> = (0..3)
        .map(|run| {
            let root = fresh(&format!("census-build-{run}"));
            build_fixture(&root);
            project_sha(&root)
        })
        .collect();
    assert_eq!(shas[0], shas[1]);
    assert_eq!(shas[1], shas[2]);
}

#[test]
fn rerunning_the_whole_script_is_a_no_op() {
    let root = fresh("census-rerun");
    build_fixture(&root);
    let before = project_sha(&root);
    let records = log(&root).len();
    let again = run_requests(&root, &root, &fixture_requests());
    assert_eq!(again.status, Status::Ok, "{}", again.doc);
    assert_eq!(again.doc["result"], "no-op");
    assert_eq!(project_sha(&root), before);
    assert_eq!(
        log(&root).len(),
        records * 2,
        "no-ops are logged as admitted ops"
    );
    let replayed = query(&root, "project_replay");
    assert_eq!(replayed.status, Status::Ok, "{}", replayed.doc);
}
