//! The Tier 1 gate: this crate's own sources scan clean, and
//! the scanner demonstrably catches planted violations
//! (validation + adversarial halves).

use std::path::PathBuf;
use torogao_runtime::tier1::scan_sources;

fn crate_src() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src")
}

#[test]
fn runtime_sources_are_tier1_clean() {
    let violations = scan_sources(&crate_src());
    assert!(
        violations.is_empty(),
        "Tier 1 violations in the runtime crate: {violations:?}"
    );
}

#[test]
fn planted_violations_are_caught() {
    let root = std::env::temp_dir().join(format!(
        "torogao-runtime-tier1-plant-{}",
        std::process::id()
    ));
    let _removed = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("scratch dir");
    // A float in a step path: banned.
    std::fs::write(
        root.join("core.rs"),
        "pub fn step(x: f64) -> f64 { x * 2.0 }\n",
    )
    .expect("plant float");
    // A hash map: banned everywhere.
    std::fs::write(
        root.join("state.rs"),
        "use std::collections::HashMap;\npub type M = HashMap<String, i64>;\n",
    )
    .expect("plant map");
    // A float inside the presentation module: allowed there.
    std::fs::write(
        root.join("present.rs"),
        "pub fn gauge(x: f64) -> f64 { x }\n",
    )
    .expect("plant present");
    // A float inside the Tier 2 measurement module: allowed
    // there too (Rung R2 widened the exemption by name).
    std::fs::write(root.join("tier2.rs"), "pub fn band(x: f64) -> f64 { x }\n")
        .expect("plant tier2");
    // A clean file: no findings.
    std::fs::write(root.join("clean.rs"), "pub fn tick() -> u64 { 1 }\n").expect("plant clean");

    let violations = scan_sources(&root);
    let _removed = std::fs::remove_dir_all(&root);
    let tokens: Vec<&str> = violations.iter().map(|v| v.token.as_str()).collect();
    assert!(
        tokens.contains(&"f64"),
        "the planted float is caught: {violations:?}"
    );
    assert!(
        tokens.contains(&"HashMap"),
        "the planted hash map is caught: {violations:?}"
    );
    assert!(
        violations.iter().all(|v| v.file != "present.rs"),
        "the presentation module's float is not flagged: {violations:?}"
    );
    assert!(
        violations.iter().all(|v| v.file != "tier2.rs"),
        "the Tier 2 module's float is not flagged: {violations:?}"
    );
    assert!(
        violations.iter().all(|v| v.file != "clean.rs"),
        "the clean file is not flagged: {violations:?}"
    );
}
