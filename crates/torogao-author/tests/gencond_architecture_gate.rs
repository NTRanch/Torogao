//! The conditioning seam's architecture gate (DESIGN §3.2):
//! the dependency rule is normative, so it is a test, not a
//! convention.
//!
//! 1. In the workspace dependency graph, the only crates that
//!    may depend on `torogao-gencond` are `torogao-author`
//!    (behind its `gen-conditioning` feature) and the
//!    `torogao` CLI (behind its own). No core crate — engine,
//!    format, session, and the rest — ever does.
//! 2. Every such edge is optional (feature-gated) in the
//!    declaring manifest.
//! 3. `torogao-gencond` itself depends on no workspace crate
//!    at all: the seam is a leaf, so removing it cannot strand
//!    a dependent inside the estate.
//! 4. Behaviour matches the build: feature-on exposes the
//!    two-verb conditioning manifest; feature-off refuses
//!    both verbs by family name (the inert-data contract,
//!    DESIGN §3.3.4).

mod common;

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::process::Command;
#[cfg(not(feature = "gen-conditioning"))]
use torogao_author::{Status, run_verb};

fn metadata() -> serde_json::Value {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let output = Command::new(cargo)
        .args(["metadata", "--format-version", "1"])
        .current_dir(root)
        .output()
        .expect("cargo metadata runs");
    assert!(output.status.success(), "cargo metadata failed");
    serde_json::from_slice(&output.stdout).expect("metadata is JSON")
}

fn package<'a>(metadata: &'a serde_json::Value, name: &str) -> &'a serde_json::Value {
    metadata["packages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|pkg| pkg["name"] == name)
        .unwrap_or_else(|| panic!("package {name} in metadata"))
}

#[test]
fn gencond_dependents_are_only_the_feature_gated_faces() {
    let metadata = metadata();
    // Declared dependents: every manifest in the workspace
    // that names torogao-gencond at all, any dependency kind.
    // (cargo metadata's resolve graph prunes inactive optional
    // edges, so the manifest census is the stable surface for
    // the rule; the resolve check below covers the active
    // graph of the build under test.)
    let mut declared: BTreeSet<String> = BTreeSet::new();
    for pkg in metadata["packages"].as_array().unwrap() {
        let names_gencond = pkg["dependencies"]
            .as_array()
            .unwrap()
            .iter()
            .any(|dep| dep["name"] == "torogao-gencond");
        if names_gencond {
            declared.insert(pkg["name"].as_str().unwrap().to_string());
        }
    }
    let allowed: BTreeSet<String> = ["torogao".to_string(), "torogao-author".to_string()]
        .into_iter()
        .collect();
    assert!(
        declared.is_subset(&allowed),
        "torogao-gencond declared dependents {declared:?} exceed the allowed faces {allowed:?}"
    );
    assert!(
        declared.contains("torogao-author"),
        "the author face must declare the dependency"
    );
    // Every declaring edge is optional in its manifest.
    for name in &declared {
        let pkg = package(&metadata, name);
        for edge in pkg["dependencies"].as_array().unwrap() {
            if edge["name"] == "torogao-gencond" {
                assert_eq!(
                    edge["optional"], true,
                    "{name}'s gencond dependency must be optional (feature-gated)"
                );
            }
        }
    }
    // The resolved graph of this build never exceeds the
    // declared set.
    let gencond_id = package(&metadata, "torogao-gencond")["id"].clone();
    for node in metadata["resolve"]["nodes"].as_array().unwrap() {
        let depends_on_gencond = node["deps"]
            .as_array()
            .unwrap()
            .iter()
            .any(|dep| dep["pkg"] == gencond_id);
        if depends_on_gencond {
            let name = metadata["packages"]
                .as_array()
                .unwrap()
                .iter()
                .find(|pkg| pkg["id"] == node["id"])
                .map(|pkg| pkg["name"].as_str().unwrap().to_string())
                .unwrap();
            assert!(
                declared.contains(&name),
                "resolved dependent {name} is not a declared dependent"
            );
        }
    }
    // The author's feature wires the dependency in.
    let author = package(&metadata, "torogao-author");
    let feature = &author["features"]["gen-conditioning"];
    assert!(
        feature
            .as_array()
            .is_some_and(|items| items.iter().any(|item| item == "dep:torogao-gencond")),
        "gen-conditioning feature wires dep:torogao-gencond: {feature}"
    );
}

#[test]
fn gencond_depends_on_no_workspace_crate() {
    let metadata = metadata();
    let gencond = package(&metadata, "torogao-gencond");
    let workspace_deps: Vec<&str> = gencond["dependencies"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|dep| dep["source"].is_null())
        .map(|dep| dep["name"].as_str().unwrap())
        .collect();
    assert_eq!(workspace_deps, Vec::<&str>::new());
}

#[test]
fn behaviour_matches_the_build() {
    assert_eq!(
        torogao_author::GENCOND_ENABLED,
        cfg!(feature = "gen-conditioning")
    );
    #[cfg(feature = "gen-conditioning")]
    {
        let manifest = torogao_author::conditioning::CONDITIONING_MANIFEST;
        assert_eq!(manifest.len(), 2);
        assert_eq!(manifest[0].name, "conditioning_compile");
        assert!(!manifest[0].mutating);
        assert_eq!(manifest[1].name, "conditioning_set");
        assert!(manifest[1].mutating);
    }
    #[cfg(not(feature = "gen-conditioning"))]
    {
        let root = common::fresh("architecture");
        let resolve =
            |name: &str| -> Result<PathBuf, torogao_author::Refusal> { Ok(PathBuf::from(name)) };
        for verb_name in ["conditioning_compile", "conditioning_set"] {
            let answer = run_verb(&root, &root, verb_name, &serde_json::json!({}), &resolve);
            assert_eq!(
                answer.status,
                Status::Refused,
                "{verb_name}: {}",
                answer.doc
            );
            assert_eq!(answer.doc["code"], "TG-AUTH-UNKNOWN-KIND");
            assert!(
                answer.doc["message"]
                    .as_str()
                    .unwrap()
                    .contains("torogao-gencond"),
                "the refusal names the family: {}",
                answer.doc
            );
        }
    }
}
