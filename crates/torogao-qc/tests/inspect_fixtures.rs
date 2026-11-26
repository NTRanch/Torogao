//! Inspect-pass fixtures: one planted defect per spec, each
//! producing exactly its contract code and no other — the
//! planted-error positive control. Plus adversarial inputs:
//! malformed, wrong-typed, escaping, and oversized specs must
//! fail closed or stay bounded, never panic.

use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use torogao_qc::inspect::inspect;
use torogao_qc::options::builtin_set;
use torogao_qc::report::Report;

fn tempdir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("torogao-qc-{}-{}", std::process::id(), tag));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create tempdir");
    dir
}

fn write(dir: &Path, name: &str, contents: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, contents).expect("write fixture");
    path
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn codes(report: &Report) -> Vec<&str> {
    report.findings().iter().map(|f| f.code).collect()
}

fn run(spec: &Path) -> Report {
    inspect(spec, None, &builtin_set()).expect("inspect should succeed")
}

#[test]
fn clean_spec_passes_with_zero_findings() {
    let dir = tempdir("clean");
    std::fs::write(dir.join("plate.png"), b"plate-bytes").expect("asset");
    let hash = sha256_hex(b"plate-bytes");
    let spec = write(
        &dir,
        "scene.json",
        &format!(
            r#"{{"name":"clean","fps":60,
               "assets":[{{"id":"plate","path":"plate.png","sha256":"{hash}"}}],
               "options":{{"fps":60,"seed":7}},
               "frame":{{"width":100,"height":100}},
               "elements":[
                 {{"id":"panel","bounds":{{"x":0,"y":0,"w":60,"h":60}}}},
                 {{"id":"caption","kind":"text","panel":"panel","text":"hi",
                   "bounds":{{"x":5,"y":5,"w":40,"h":10}}}},
                 {{"id":"actor","bounds":{{"x":65,"y":65,"w":20,"h":20}}}}
               ],
               "cues":[{{"id":"hit","frame":60,"ms":1000}}],
               "characters":[{{"id":"prima","identity":{{"hair":"black"}}}}]}}"#
        ),
    );
    let report = run(&spec);
    assert_eq!(codes(&report), Vec::<&str>::new());
    assert!(!report.failed());
    let json = report.to_json();
    assert_eq!(json["summary"]["pass"], serde_json::json!(true));
}

#[test]
fn missing_asset_is_exactly_missing_media() {
    let dir = tempdir("missing");
    let spec = write(
        &dir,
        "scene.json",
        r#"{"name":"t","assets":[{"id":"a","path":"nope.png"}]}"#,
    );
    assert_eq!(codes(&run(&spec)), ["QC_MISSING_MEDIA"]);
}

#[test]
fn hash_mismatch_is_exactly_missing_media() {
    let dir = tempdir("hash");
    std::fs::write(dir.join("data.bin"), b"actual").expect("asset");
    let wrong = sha256_hex(b"different");
    let spec = write(
        &dir,
        "scene.json",
        &format!(r#"{{"name":"t","assets":[{{"id":"d","path":"data.bin","sha256":"{wrong}"}}]}}"#),
    );
    assert_eq!(codes(&run(&spec)), ["QC_MISSING_MEDIA"]);
}

#[test]
fn nonfinite_number_is_exactly_nan_value() {
    let dir = tempdir("nan");
    let spec = write(
        &dir,
        "scene.json",
        r#"{"name":"t","curve":[0.0, 1e999, 2.0]}"#,
    );
    let report = run(&spec);
    assert_eq!(codes(&report), ["QC_NAN_VALUE"]);
    assert_eq!(report.findings()[0].spec_ref, "/curve/1");
}

#[test]
fn nan_literal_and_nested_infinity_are_nan_value() {
    let dir = tempdir("nanlit");
    let spec = write(&dir, "a.json", r#"{"name":"t","v":NaN}"#);
    let report = run(&spec);
    assert_eq!(codes(&report), ["QC_NAN_VALUE"]);
    assert_eq!(report.findings()[0].spec_ref, "/v");
    let spec = write(&dir, "b.json", r#"{"name":"t","a":{"b":-Infinity}}"#);
    let report = run(&spec);
    assert_eq!(codes(&report), ["QC_NAN_VALUE"]);
    assert_eq!(report.findings()[0].spec_ref, "/a/b");
}

#[test]
fn nan_inside_strings_is_not_a_finding() {
    let dir = tempdir("nanstr");
    let spec = write(
        &dir,
        "scene.json",
        r#"{"name":"t","note":"NaN and Infinity are words here","curve":[1.0,2.0]}"#,
    );
    assert_eq!(codes(&run(&spec)), Vec::<&str>::new());
}

#[test]
fn underflow_exponent_is_accepted_as_zero() {
    let dir = tempdir("underflow");
    let spec = write(&dir, "scene.json", r#"{"name":"t","v":1e-999}"#);
    assert_eq!(codes(&run(&spec)), Vec::<&str>::new());
}

#[test]
fn dead_option_is_exactly_unused_option() {
    let dir = tempdir("option");
    let spec = write(
        &dir,
        "scene.json",
        r#"{"name":"t","options":{"fps":60,"bogus_option":true}}"#,
    );
    assert_eq!(codes(&run(&spec)), ["QC_UNUSED_OPTION"]);
}

#[test]
fn clipped_text_is_exactly_text_clipped() {
    let dir = tempdir("clipped");
    let spec = write(
        &dir,
        "scene.json",
        r#"{"name":"t","frame":{"width":100,"height":100},
           "elements":[
             {"id":"panel","bounds":{"x":0,"y":0,"w":50,"h":50}},
             {"id":"caption","kind":"text","panel":"panel","text":"hi",
              "bounds":{"x":40,"y":40,"w":30,"h":30}}
           ]}"#,
    );
    assert_eq!(codes(&run(&spec)), ["QC_TEXT_CLIPPED"]);
}

#[test]
fn overlapping_pair_is_exactly_text_overlap_and_whitelist_clears_it() {
    let dir = tempdir("overlap");
    let body = r#""frame":{"width":100,"height":100},
           "elements":[
             {"id":"caption","kind":"text","text":"hi","bounds":{"x":0,"y":0,"w":40,"h":20}},
             {"id":"badge","bounds":{"x":20,"y":10,"w":40,"h":20}}
           ]"#;
    let spec = write(&dir, "a.json", &format!(r#"{{"name":"t",{body}}}"#));
    assert_eq!(codes(&run(&spec)), ["QC_TEXT_OVERLAP"]);
    let spec = write(
        &dir,
        "b.json",
        &format!(r#"{{"name":"t",{body},"overlap_whitelist":[["badge","caption"]]}}"#),
    );
    assert_eq!(codes(&run(&spec)), Vec::<&str>::new());
}

#[test]
fn off_frame_is_exactly_off_frame_and_exit_clears_it() {
    let dir = tempdir("offframe");
    let spec = write(
        &dir,
        "a.json",
        r#"{"name":"t","frame":{"width":100,"height":100},
           "elements":[{"id":"actor","bounds":{"x":90,"y":90,"w":30,"h":30}}]}"#,
    );
    assert_eq!(codes(&run(&spec)), ["QC_OFF_FRAME"]);
    let spec = write(
        &dir,
        "b.json",
        r#"{"name":"t","frame":{"width":100,"height":100},
           "elements":[{"id":"actor","exit":true,"bounds":{"x":90,"y":90,"w":30,"h":30}}]}"#,
    );
    assert_eq!(codes(&run(&spec)), Vec::<&str>::new());
}

#[test]
fn drifted_cue_is_exactly_sync_drift() {
    let dir = tempdir("drift");
    let spec = write(
        &dir,
        "scene.json",
        r#"{"name":"t","fps":60,"cues":[{"id":"hit","frame":60,"ms":1200}]}"#,
    );
    let report = run(&spec);
    assert_eq!(codes(&report), ["QC_SYNC_DRIFT"]);
    assert_eq!(report.findings()[0].frame, Some(60.0));
}

#[test]
fn identity_drift_against_lock_and_match_is_clean() {
    let dir = tempdir("identity");
    let spec = write(
        &dir,
        "scene.json",
        r#"{"name":"t","characters":[{"id":"prima","identity":{"hair":"black","eyes":"plum"}}]}"#,
    );
    write(
        &dir,
        "scene.lock.json",
        r#"{"characters":{"prima":{"hair":"black","eyes":"violet"}}}"#,
    );
    assert_eq!(codes(&run(&spec)), ["QC_IDENTITY_DRIFT"]);
    let spec = write(
        &dir,
        "match.json",
        r#"{"name":"t","characters":[{"id":"prima","identity":{"hair":"black","eyes":"violet"}}]}"#,
    );
    write(
        &dir,
        "match.lock.json",
        r#"{"characters":{"prima":{"hair":"black","eyes":"violet"}}}"#,
    );
    assert_eq!(codes(&run(&spec)), Vec::<&str>::new());
}

#[test]
fn manifest_supplies_refs_and_hashes() {
    let dir = tempdir("manifest");
    std::fs::write(dir.join("tex.png"), b"tex").expect("asset");
    let spec = write(&dir, "scene.json", r#"{"name":"t"}"#);
    let wrong = sha256_hex(b"not-tex");
    let manifest = write(
        &dir,
        "manifest.json",
        &format!(r#"{{"assets":{{"tex":{{"path":"tex.png","sha256":"{wrong}"}}}}}}"#),
    );
    let report = inspect(&spec, Some(&manifest), &builtin_set()).expect("inspect");
    assert_eq!(codes(&report), ["QC_MISSING_MEDIA"]);
}

#[test]
fn manifest_path_escape_fails_closed() {
    let dir = tempdir("escape");
    let spec = write(&dir, "scene.json", r#"{"name":"t"}"#);
    let manifest = write(
        &dir,
        "manifest.json",
        r#"{"assets":{"x":{"path":"../outside.png"}}}"#,
    );
    let err = inspect(&spec, Some(&manifest), &builtin_set()).expect_err("must fail");
    assert_eq!(err.code(), "QC-E-PATH-ESCAPE");
}

#[test]
fn spec_asset_path_escape_fails_closed() {
    let dir = tempdir("escape2");
    let spec = write(
        &dir,
        "scene.json",
        r#"{"name":"t","assets":[{"id":"x","path":"../outside.png"}]}"#,
    );
    let err = inspect(&spec, None, &builtin_set()).expect_err("must fail");
    assert_eq!(err.code(), "QC-E-PATH-ESCAPE");
}

#[test]
fn malformed_json_is_a_typed_error() {
    let dir = tempdir("malformed");
    let spec = write(&dir, "scene.json", "{not json");
    let err = inspect(&spec, None, &builtin_set()).expect_err("must fail");
    assert_eq!(err.code(), "QC-E-SPEC-PARSE");
}

#[test]
fn non_object_root_is_a_typed_error() {
    let dir = tempdir("rootarray");
    let spec = write(&dir, "scene.json", "[1,2,3]");
    let err = inspect(&spec, None, &builtin_set()).expect_err("must fail");
    assert_eq!(err.code(), "QC-E-SPEC-SHAPE");
}

#[test]
fn wrong_typed_sections_are_skipped_not_panicked() {
    let dir = tempdir("wrongtypes");
    let spec = write(
        &dir,
        "scene.json",
        r#"{"name":"t","elements":"nope","cues":42,"options":[1],"characters":{}}"#,
    );
    let report = run(&spec);
    assert_eq!(codes(&report), Vec::<&str>::new());
}

#[test]
fn empty_spec_passes() {
    let dir = tempdir("empty");
    let spec = write(&dir, "scene.json", "{}");
    let report = run(&spec);
    assert!(!report.failed());
    assert_eq!(codes(&report), Vec::<&str>::new());
}

#[test]
fn too_many_assets_is_a_typed_error() {
    let dir = tempdir("manyassets");
    let mut body = String::from(r#"{"name":"t","assets":["#);
    for i in 0..4_097 {
        if i > 0 {
            body.push(',');
        }
        body.push_str(&format!(r#"{{"id":"a{i}","path":"f{i}.png"}}"#));
    }
    body.push_str("]}");
    let spec = write(&dir, "scene.json", &body);
    let err = inspect(&spec, None, &builtin_set()).expect_err("must fail");
    assert_eq!(err.code(), "QC-E-TOO-MANY-ASSETS");
}

#[test]
fn findings_are_capped_and_truncation_is_reported() {
    let dir = tempdir("cap");
    let mut body = String::from(r#"{"name":"t","frame":{"width":10,"height":10},"elements":["#);
    for i in 0..10_001 {
        if i > 0 {
            body.push(',');
        }
        body.push_str(&format!(
            r#"{{"id":"e{i}","bounds":{{"x":50,"y":50,"w":5,"h":5}}}}"#
        ));
    }
    body.push_str("]}");
    let spec = write(&dir, "scene.json", &body);
    let report = run(&spec);
    assert_eq!(report.findings().len(), 10_000);
    assert_eq!(
        report.to_json()["summary"]["truncated"],
        serde_json::json!(true)
    );
}

#[test]
fn report_is_byte_identical_across_runs() {
    let dir = tempdir("determinism");
    let spec = write(
        &dir,
        "scene.json",
        r#"{"name":"t","fps":60,
           "assets":[{"id":"a","path":"nope.png"}],
           "options":{"bogus_option":1},
           "cues":[{"id":"c","frame":60,"ms":1200}]}"#,
    );
    let first = run(&spec).to_json_string();
    let second = run(&spec).to_json_string();
    assert_eq!(first, second);
    assert_eq!(
        codes(&run(&spec)),
        ["QC_MISSING_MEDIA", "QC_SYNC_DRIFT", "QC_UNUSED_OPTION"]
    );
}
