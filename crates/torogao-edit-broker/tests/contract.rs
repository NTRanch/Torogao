//! End-to-end contract tests for the broker binary, run
//! against a stub driver that mimics `edit-local.py`'s CLI
//! and output discipline. No GPU and no ComfyUI are
//! involved: what is under test is the broker — the contract
//! surface, the FIFO, the cancel semantics, and the refusal
//! behaviour. The live-fire run against the real edit route
//! is a separate, GPU-gated act (see the crate README).
//!
//! Stub driver behaviour, keyed by tokens in the prompt:
//! - `FAIL` → exit 3 with a stderr message, no outputs.
//! - `LONG` → sleep 20 s, then write outputs.
//! - `SLOWWRITE` → write the raw output, then sleep 20 s.
//! - `SLOW` → sleep 6 s, then write outputs.
//! - otherwise → write outputs immediately.
//!
//! Outputs follow the real driver's names: `<prefix>_00001_.png`
//! always, plus `<prefix>_comp.png` and `<prefix>_mask.png`
//! when a mask flag is present. Every run appends its prefix
//! to `runs.log` and its argv to `argv-<prefix>.txt` in the
//! route directory, so tests can assert what ran, in what
//! order, with what flags.

#![forbid(unsafe_code)]

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use torogao_edit_broker::{base64_decode, base64_encode, sha256_hex};

const STUB_DRIVER: &str = r#"
import sys, time
from pathlib import Path

HERE = Path(__file__).resolve().parent
PNG = bytes([0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A])

def main():
    args = sys.argv[1:]
    assert args[0] == "edit", args
    prompt = Path(args[2]).read_text()
    prefix = args[3]
    out = HERE / "output"
    out.mkdir(exist_ok=True)
    with open(HERE / "runs.log", "a") as f:
        f.write(prefix + "\n")
    (HERE / f"argv-{prefix}.txt").write_text(" ".join(args))
    if "FAIL" in prompt:
        print("stub driver failure, as requested", file=sys.stderr)
        return 3
    def write_outputs():
        (out / f"{prefix}_00001_.png").write_bytes(PNG + b"raw-" + prefix.encode())
        if "--mask" in args or "--auto-mask" in args:
            (out / f"{prefix}_comp.png").write_bytes(PNG + b"comp-" + prefix.encode())
            (out / f"{prefix}_mask.png").write_bytes(PNG + b"mask-" + prefix.encode())
    if "SLOWWRITE" in prompt:
        write_outputs()
        time.sleep(20)
    elif "LONG" in prompt:
        time.sleep(20)
        write_outputs()
    elif "SLOW" in prompt:
        time.sleep(6)
        write_outputs()
    else:
        write_outputs()
    return 0

sys.exit(main())
"#;

/// A running broker plus the directories it was pointed at.
struct Fixture {
    child: Child,
    port: u16,
    route_dir: PathBuf,
    work_dir: PathBuf,
    root: PathBuf,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn free_port() -> u16 {
    std::net::TcpListener::bind(("127.0.0.1", 0))
        .expect("bind ephemeral")
        .local_addr()
        .expect("local addr")
        .port()
}

fn start_fixture(test_name: &str) -> Fixture {
    let root = std::env::temp_dir().join(format!(
        "torogao-edit-broker-contract-{test_name}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    let route_dir = root.join("route");
    let work_dir = root.join("work");
    std::fs::create_dir_all(route_dir.join("output")).expect("route dir");
    std::fs::create_dir_all(&work_dir).expect("work dir");
    let driver = route_dir.join("edit-local.py");
    std::fs::write(&driver, STUB_DRIVER).expect("stub driver");
    let port = free_port();
    // Port 1 is the discard port: nothing answers, so the
    // broker's ComfyUI probe reports not-alive and its
    // interrupt calls fail best-effort, as designed.
    let stderr_path = root.join("broker.stderr.log");
    let stderr_file = std::fs::File::create(&stderr_path).expect("stderr log");
    let mut child = Command::new(env!("CARGO_BIN_EXE_torogao-edit-broker"))
        .args([
            "--port",
            &port.to_string(),
            "--driver",
            driver.to_str().expect("driver path utf8"),
            "--work-dir",
            work_dir.to_str().expect("work path utf8"),
            "--comfy-url",
            "http://127.0.0.1:1",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::from(stderr_file))
        .spawn()
        .expect("broker spawns");
    // Wait for the surface to answer. A broker that exited
    // instead is a startup failure, reported with its own
    // stderr — never mistaken for a slow start.
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if let Ok(Some(status)) = child.try_wait() {
            let stderr = std::fs::read_to_string(&stderr_path).unwrap_or_default();
            panic!("broker exited {status} before serving: {stderr}");
        }
        if let Ok((200, _)) = request(port, "GET", "/health", None) {
            return Fixture {
                child,
                port,
                route_dir,
                work_dir,
                root,
            };
        }
        assert!(Instant::now() < deadline, "broker did not come up");
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// One HTTP round trip against the broker; returns the
/// status code and the parsed JSON body.
fn request(
    port: u16,
    method: &str,
    path: &str,
    body: Option<&Value>,
) -> Result<(u16, Value), String> {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).map_err(|e| e.to_string())?;
    let payload = body.map(Value::to_string).unwrap_or_default();
    let request = format!(
        "{method} {path} HTTP/1.1\r\nhost: 127.0.0.1\r\ncontent-type: application/json\r\n\
         content-length: {}\r\nconnection: close\r\n\r\n{payload}",
        payload.len()
    );
    stream
        .write_all(request.as_bytes())
        .map_err(|e| e.to_string())?;
    let mut raw = String::new();
    stream.read_to_string(&mut raw).map_err(|e| e.to_string())?;
    let status: u16 = raw
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse().ok())
        .ok_or_else(|| format!("unparseable response: {raw:.100}"))?;
    let body_text = raw.split("\r\n\r\n").nth(1).unwrap_or("");
    let parsed = serde_json::from_str(body_text).map_err(|e| format!("bad json body: {e}"))?;
    Ok((status, parsed))
}

fn png(tag: &[u8]) -> Vec<u8> {
    let mut bytes = vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
    bytes.extend_from_slice(tag);
    bytes
}

fn job_body(prompt: &str) -> Value {
    json!({
        "canvas_png_base64": base64_encode(&png(b"canvas")),
        "prompt": prompt,
    })
}

fn submit(port: u16, body: &Value) -> (u16, Value) {
    request(port, "POST", "/jobs", Some(body)).expect("submit round trip")
}

fn poll(port: u16, id: &str) -> Value {
    let (status, body) = request(port, "GET", &format!("/jobs/{id}"), None).expect("poll");
    assert_eq!(status, 200, "poll {id}: {body}");
    body
}

fn wait_state(port: u16, id: &str, wanted: &str, deadline_secs: u64) -> Value {
    let deadline = Instant::now() + Duration::from_secs(deadline_secs);
    loop {
        let body = poll(port, id);
        if body["state"] == wanted {
            return body;
        }
        assert!(
            Instant::now() < deadline,
            "job {id} did not reach {wanted}: {body}"
        );
        std::thread::sleep(Duration::from_millis(150));
    }
}

fn runs_log(fixture: &Fixture) -> Vec<String> {
    std::fs::read_to_string(fixture.route_dir.join("runs.log"))
        .unwrap_or_default()
        .lines()
        .map(str::to_string)
        .collect()
}

/// Wait until the stub driver for `id` has actually begun
/// executing (its first act is appending to runs.log). A job
/// can be `running` in the broker while its python is still
/// in interpreter startup — under load that gap is seconds,
/// and a cancel landing inside it kills the child before
/// the script body runs. Tests that assert on runs.log
/// synchronize on the script, not on the broker's state.
fn wait_driver_started(fixture: &Fixture, id: &str) {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if runs_log(fixture).iter().any(|ran| ran == id) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "stub driver for {id} did not start"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn argv_of(fixture: &Fixture, prefix: &str) -> String {
    std::fs::read_to_string(fixture.route_dir.join(format!("argv-{prefix}.txt")))
        .expect("argv record")
}

// -- The happy paths ----------------------------------------------------------

#[test]
fn health_reports_the_engine_contract() {
    let fixture = start_fixture("health");
    let (status, body) = request(fixture.port, "GET", "/health", None).expect("health");
    assert_eq!(status, 200);
    assert_eq!(body["engine"], "torogao-edit-broker");
    // The driver is in place (its auto-start is this engine's
    // load path); ComfyUI is not running (dead probe port).
    assert_eq!(body["model_loaded"], true);
    assert_eq!(body["driver_present"], true);
    assert_eq!(body["comfyui_alive"], false);
    assert_eq!(body["queue_depth"], 0);
}

#[test]
fn a_faithful_job_runs_and_reports_its_artefact() {
    let fixture = start_fixture("faithful");
    let (status, body) = submit(fixture.port, &job_body("widen the straps"));
    assert_eq!(status, 202, "{body}");
    let id = body["id"].as_str().expect("job id").to_string();
    assert_eq!(id, "job-1");

    let done = wait_state(fixture.port, &id, "done", 15);
    assert_eq!(done["config"], "faithful");
    assert_eq!(done["seed"], 2701);
    assert_eq!(done["delivered"], "raw_edit");
    let artefacts = done["artefacts"].as_array().expect("artefacts");
    assert_eq!(artefacts.len(), 1);
    assert_eq!(artefacts[0]["kind"], "raw_edit");

    let raw_path = fixture.route_dir.join("output/job-1_00001_.png");
    let raw_bytes = std::fs::read(&raw_path).expect("raw output on disk");
    assert_eq!(artefacts[0]["sha256"], sha256_hex(&raw_bytes));
    assert_eq!(
        base64_decode(artefacts[0]["image_png_base64"].as_str().expect("b64")).expect("decodes"),
        raw_bytes
    );
    assert_eq!(done["image_png_base64"], artefacts[0]["image_png_base64"]);
    let argv = argv_of(&fixture, "job-1");
    assert!(
        !argv.contains("--turbo") && !argv.contains("--omni"),
        "{argv}"
    );
}

#[test]
fn a_masked_job_delivers_the_composited_frame() {
    let fixture = start_fixture("masked");
    let mut body = job_body("widen the straps, this region only");
    body["mask"] = json!({
        "kind": "explicit",
        "mask_png_base64": base64_encode(&png(b"mask")),
        "feather": 14,
    });
    let (status, submitted) = submit(fixture.port, &body);
    assert_eq!(status, 202, "{submitted}");
    let id = submitted["id"].as_str().expect("job id").to_string();

    let done = wait_state(fixture.port, &id, "done", 15);
    assert_eq!(done["delivered"], "composited");
    let kinds: Vec<&str> = done["artefacts"]
        .as_array()
        .expect("artefacts")
        .iter()
        .map(|a| a["kind"].as_str().expect("kind"))
        .collect();
    assert_eq!(kinds, ["raw_edit", "composited", "mask_alpha"]);
    let comp_bytes = std::fs::read(fixture.route_dir.join(format!("output/{id}_comp.png")))
        .expect("comp on disk");
    assert_eq!(
        base64_decode(done["image_png_base64"].as_str().expect("b64")).expect("decodes"),
        comp_bytes
    );
    // The broker staged the caller's mask and named it in argv.
    assert!(fixture.work_dir.join(format!("{id}/mask.png")).is_file());
    let argv = argv_of(&fixture, &id);
    assert!(argv.contains("--mask"), "{argv}");
    assert!(argv.contains("--feather 14"), "{argv}");
}

#[test]
fn turbo_is_labelled_wherever_it_is_reported() {
    let fixture = start_fixture("turbo");
    let mut body = job_body("a quick nudge");
    body["config"] = json!("turbo");
    let (status, submitted) = submit(fixture.port, &body);
    assert_eq!(status, 202, "{submitted}");
    let id = submitted["id"].as_str().expect("job id").to_string();
    let done = wait_state(fixture.port, &id, "done", 15);
    assert_eq!(done["config"], "turbo");
    let label = done["config_label"].as_str().expect("label");
    assert!(label.contains("NOT chain-safe"), "{label}");
    assert_eq!(argv_of(&fixture, &id).matches("--turbo").count(), 1);
}

// -- The FIFO and the cancel semantics ----------------------------------------

#[test]
fn jobs_run_in_submission_order() {
    let fixture = start_fixture("fifo");
    let (_, first) = submit(fixture.port, &job_body("SLOW first"));
    let first_id = first["id"].as_str().expect("id").to_string();
    let (_, second) = submit(fixture.port, &job_body("fast second"));
    let second_id = second["id"].as_str().expect("id").to_string();

    // While the first runs, the second is honestly queued.
    let first_poll = wait_state(fixture.port, &first_id, "running", 10);
    assert_eq!(first_poll["state"], "running");
    let second_poll = poll(fixture.port, &second_id);
    assert_eq!(second_poll["state"], "queued");
    assert_eq!(second_poll["queue_position"], 0);

    wait_state(fixture.port, &first_id, "done", 20);
    wait_state(fixture.port, &second_id, "done", 20);
    assert_eq!(runs_log(&fixture), [first_id, second_id]);
}

#[test]
fn a_cancelled_queued_job_never_runs() {
    let fixture = start_fixture("cancel-queued");
    let (_, first) = submit(fixture.port, &job_body("LONG first"));
    let first_id = first["id"].as_str().expect("id").to_string();
    let (_, second) = submit(fixture.port, &job_body("queued second"));
    let second_id = second["id"].as_str().expect("id").to_string();
    wait_state(fixture.port, &first_id, "running", 10);
    wait_driver_started(&fixture, &first_id);

    let (status, _) = request(
        fixture.port,
        "POST",
        &format!("/jobs/{second_id}/interrupt"),
        None,
    )
    .expect("interrupt queued");
    assert_eq!(status, 200);
    // Free the worker so a wrongly-run second job would show.
    let (status, _) = request(
        fixture.port,
        "POST",
        &format!("/jobs/{first_id}/interrupt"),
        None,
    )
    .expect("interrupt running");
    assert_eq!(status, 200);
    wait_state(fixture.port, &first_id, "cancelled", 10);
    let second_poll = wait_state(fixture.port, &second_id, "cancelled", 10);
    assert_eq!(second_poll["state"], "cancelled");
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(runs_log(&fixture), [first_id]);
}

#[test]
fn a_cancelled_running_job_is_killed_and_its_outputs_discarded() {
    let fixture = start_fixture("cancel-running");
    let (_, submitted) = submit(fixture.port, &job_body("SLOWWRITE partial"));
    let id = submitted["id"].as_str().expect("id").to_string();
    wait_state(fixture.port, &id, "running", 10);
    // The stub writes its raw output before sleeping; wait
    // for it to be on disk (however long interpreter startup
    // takes under load), then cancel.
    let raw = fixture.route_dir.join(format!("output/{id}_00001_.png"));
    let deadline = Instant::now() + Duration::from_secs(20);
    while !raw.is_file() {
        assert!(Instant::now() < deadline, "stub wrote no partial output");
        std::thread::sleep(Duration::from_millis(100));
    }

    let started = Instant::now();
    let (status, _) = request(fixture.port, "POST", &format!("/jobs/{id}/interrupt"), None)
        .expect("interrupt running");
    assert_eq!(status, 200);
    wait_state(fixture.port, &id, "cancelled", 10);
    assert!(
        started.elapsed() < Duration::from_secs(15),
        "the cancel killed the driver instead of waiting out its 20 s sleep"
    );
    assert!(!raw.exists(), "partial outputs are discarded on cancel");
}

// -- Failures and refusals ------------------------------------------------------

#[test]
fn a_driver_failure_surfaces_as_an_error_state() {
    let fixture = start_fixture("failure");
    let (_, submitted) = submit(fixture.port, &job_body("FAIL this one"));
    let id = submitted["id"].as_str().expect("id").to_string();
    let errored = wait_state(fixture.port, &id, "error", 15);
    let message = errored["error"].as_str().expect("error message");
    assert!(message.contains("driver exited"), "{message}");
    assert!(message.contains("stub driver failure"), "{message}");
}

#[test]
fn omni_is_refused_at_submit() {
    let fixture = start_fixture("omni");
    let mut body = job_body("widen the straps");
    body["config"] = json!("omni");
    let (status, refused) = submit(fixture.port, &body);
    assert_eq!(status, 400, "{refused}");
    let message = refused["error"].as_str().expect("error");
    assert!(message.contains("not offered"), "{message}");
    // And no job was created: the table is empty.
    let (_, jobs) = request(fixture.port, "GET", "/jobs", None).expect("jobs");
    assert_eq!(jobs["counts"], json!({}));
}

#[test]
fn malformed_submissions_are_refused() {
    let fixture = start_fixture("malformed");
    // Unknown field.
    let mut body = job_body("widen the straps");
    body["denoise"] = json!(0.5);
    let (status, _) = submit(fixture.port, &body);
    assert_eq!(status, 400);
    // Canvas that is not base64.
    let body = json!({ "canvas_png_base64": "!!!", "prompt": "x" });
    let (status, _) = submit(fixture.port, &body);
    assert_eq!(status, 400);
    // Missing prompt.
    let body = json!({ "canvas_png_base64": base64_encode(&png(b"c")) });
    let (status, _) = submit(fixture.port, &body);
    assert_eq!(status, 400);
    // Not JSON at all.
    let mut stream = TcpStream::connect(("127.0.0.1", fixture.port)).expect("connect");
    stream
        .write_all(b"POST /jobs HTTP/1.1\r\nhost: x\r\ncontent-length: 4\r\n\r\nnope")
        .expect("write");
    let mut raw = String::new();
    stream.read_to_string(&mut raw).expect("read");
    assert!(raw.starts_with("HTTP/1.1 400"), "{raw:.60}");
}

#[test]
fn unknown_jobs_are_not_found() {
    let fixture = start_fixture("unknown");
    let (status, _) = request(fixture.port, "GET", "/jobs/job-99", None).expect("poll");
    assert_eq!(status, 404);
    let (status, _) =
        request(fixture.port, "POST", "/jobs/job-99/interrupt", None).expect("interrupt");
    assert_eq!(status, 404);
}
