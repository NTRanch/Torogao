//! Shared helpers for the integration tests.

#![allow(dead_code)]

use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use torogao_author::store::Request;
use torogao_author::{Answer, Status, parse_script, run_requests, run_verb};

/// A fresh, empty directory unique to this test process and name.
pub fn fresh(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("torogao-author-{}-{name}", std::process::id()));
    if dir.exists() {
        std::fs::remove_dir_all(&dir).expect("clear stale test dir");
    }
    std::fs::create_dir_all(&dir).expect("create test dir");
    dir
}

/// The fixture script's requests.
pub fn fixture_requests() -> Vec<Request> {
    parse_script(include_str!("../fixtures/census-a1/script.jsonl")).expect("fixture parses")
}

/// Builds the census fixture in `root` and asserts the run is admitted.
pub fn build_fixture(root: &Path) -> Answer {
    let answer = run_requests(root, root, &fixture_requests());
    assert_eq!(answer.status, Status::Ok, "fixture run: {}", answer.doc);
    answer
}

fn no_projects(name: &str) -> Result<PathBuf, torogao_author::Refusal> {
    Err(torogao_author::Refusal::new(
        torogao_author::RefusalClass::UnknownTarget,
        format!("no project named {name}"),
    ))
}

/// Runs one verb with empty-object inputs unless given.
pub fn verb(root: &Path, name: &str, inputs: Value) -> Answer {
    run_verb(root, root, name, &inputs, &no_projects)
}

/// Runs one verb with `{}` inputs.
pub fn query(root: &Path, name: &str) -> Answer {
    verb(root, name, json!({}))
}

/// sha256 of a file's bytes.
pub fn file_sha(path: &Path) -> String {
    torogao_author::intake::sha256_hex(&std::fs::read(path).expect("read file"))
}

/// The project file's sha256.
pub fn project_sha(root: &Path) -> String {
    file_sha(&root.join("project.ron"))
}

/// The op log's records.
pub fn log(root: &Path) -> Vec<Value> {
    torogao_author::store::read_log(root).expect("read log")
}

/// A minimal valid init request for `name`.
pub fn init(name: &str) -> Request {
    Request {
        verb: "project_init".to_string(),
        inputs: json!({
            "name": name,
            "stamp": {"basis": "test", "lane": "all-ages", "pack": "all-ages", "rating": "all-ages"},
            "consent": {"adult": true, "schema": "torogao-consent/1", "wants_selected_scenes": true},
        }),
    }
}

/// A request from a verb and inputs.
pub fn request(verb: &str, inputs: Value) -> Request {
    Request {
        verb: verb.to_string(),
        inputs,
    }
}
