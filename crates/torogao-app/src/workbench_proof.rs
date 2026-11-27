// SPDX-License-Identifier: Apache-2.0 OR MIT
// Copyright (c) 2026 Qompass AI

//! The headless end-to-end proof (`--workbench-proof`).
//!
//! This drives the workbench's own code path — the same
//! core and client functions the window's buttons call —
//! through one full art loop:
//!
//! 1. originate one candidate for a character through the
//!    facade's governed contract and register it with full
//!    provenance;
//! 2. wait for the shared edit queue to read idle (the
//!    queue-honesty rule: never fire into a busy queue);
//! 3. land one faithful masked edit on that candidate,
//!    with the mask rasterized by the same rasterizer the
//!    UI paints with, and register the strike as a second
//!    candidate whose provenance names its canvas;
//! 4. verify both artefacts and their provenance on disk.
//!
//! Everything lands CANDIDATE class. Nothing is admitted —
//! admission is the Owner's act, in the window.

use std::path::PathBuf;
use std::time::Duration;

use crate::workbench::{
    CLASS_CANDIDATE, ROUTE_EDIT_FAITHFUL, ROUTE_ORIGINATE, RecipeSettings, RgbaImage, Store,
};
use crate::workbench_client::{
    EditConfig, EditQueue, EditSpec, FacadeClient, JobPoll, SeatHealth, WbConfig, probe_edit_queue,
    run_edit,
};

/// Options for one proof run.
pub struct ProofOptions {
    /// Character id to originate for.
    pub character: String,
    /// Origination seed.
    pub seed: u64,
    /// Scene prompt for the origination.
    pub scene_prompt: String,
    /// Edit instruction for the strike.
    pub edit_prompt: String,
    /// Store root (default when `None`).
    pub store_root: Option<PathBuf>,
}

/// Run the proof, returning human-readable log lines. Any
/// failure is an `Err` carrying the failing step's name.
pub fn run_proof(opts: &ProofOptions) -> Result<Vec<String>, String> {
    let mut log: Vec<String> = Vec::new();
    let root = opts.store_root.clone().unwrap_or_else(Store::default_root);
    let store = Store::open(&root).map_err(|e| format!("store: {e}"))?;
    log.push(format!("store: {}", store.root().display()));
    let mut record = store
        .load(&opts.character)
        .map_err(|e| format!("load character: {e}"))?;

    let cfg = WbConfig::from_env();
    let client = FacadeClient::new(cfg.clone());

    // -- Step 1: seat health, wake if the seat is absent. --
    match client.health() {
        SeatHealth::Up { engine, seat } => {
            log.push(format!("facade health: engine={engine} seat={seat}"));
            if seat != "ready" {
                log.push("seat not ready; waking".to_string());
                client.wake().map_err(|e| format!("wake: {e}"))?;
            }
        }
        SeatHealth::Down(why) => return Err(format!("facade health: {why}")),
    }
    if !cfg.has_token() {
        return Err("facade token: not held — submissions would refuse".to_string());
    }

    // -- Step 2: originate through the governed contract. --
    let recipe = RecipeSettings::of_record();
    let prompt_id = client
        .submit(&record, &opts.scene_prompt, opts.seed, &recipe)
        .map_err(|e| format!("originate submit: {e}"))?;
    log.push(format!("originated: prompt_id={prompt_id}"));
    store
        .bank_prompt(&mut record, "originate", &opts.scene_prompt)
        .map_err(|e| format!("bank prompt: {e}"))?;

    let filename = {
        let deadline = std::time::Instant::now() + Duration::from_secs(420);
        loop {
            match client.poll(&prompt_id).map_err(|e| format!("poll: {e}"))? {
                JobPoll::Done(name) => break name,
                JobPoll::Failed(why) => return Err(format!("originate: {why}")),
                JobPoll::Pending => {
                    if std::time::Instant::now() > deadline {
                        return Err("originate: timed out waiting for the seat".to_string());
                    }
                    std::thread::sleep(Duration::from_secs(3));
                }
            }
        }
    };
    log.push(format!("seat produced: {filename}"));
    let png = client
        .fetch_view(&filename)
        .map_err(|e| format!("fetch view: {e}"))?;
    let decoded = crate::workbench::decode_png_bytes(&png)
        .map_err(|e| format!("decode originated png: {e}"))?;
    log.push(format!(
        "originated png: {}x{}",
        decoded.width, decoded.height
    ));

    let job_dir = store.root().join("jobs").join(format!(
        "proof-{}-{}",
        record.id,
        crate::workbench::now_iso().replace(':', "")
    ));
    std::fs::create_dir_all(&job_dir).map_err(|e| format!("job dir: {e}"))?;
    let origin_path = job_dir.join("originated.png");
    std::fs::write(&origin_path, &png).map_err(|e| format!("write originated: {e}"))?;

    let composed = crate::workbench::compose_positive(&record, &opts.scene_prompt);
    let origin_config = serde_json::json!({
        "steps": recipe.steps, "cfg": recipe.cfg,
        "width": recipe.width, "height": recipe.height,
        "sampler": crate::workbench::SAMPLER,
        "scheduler": crate::workbench::SCHEDULER,
        "checkpoint": crate::workbench::CHECKPOINT,
        "prompt_id": prompt_id,
        "seat_file": filename,
    });
    let origin_cand = store
        .add_candidate(
            &mut record,
            crate::workbench::CandidateSpec {
                source_file: origin_path,
                route: ROUTE_ORIGINATE.to_string(),
                prompt: composed,
                seed: Some(opts.seed),
                config: origin_config,
                canvas: None,
            },
        )
        .map_err(|e| format!("register origination: {e}"))?;
    log.push(format!(
        "registered {} ({}) sha256={}",
        origin_cand.id, origin_cand.class, origin_cand.sha256
    ));

    // -- Step 3: queue honesty — wait for an idle edit queue. --
    {
        let deadline = std::time::Instant::now() + Duration::from_secs(900);
        loop {
            match probe_edit_queue(&cfg) {
                EditQueue::Idle => {
                    log.push("edit queue: idle — firing is honest".to_string());
                    break;
                }
                EditQueue::Busy { running, pending } => {
                    log.push(format!(
                        "edit queue: busy (running={running} pending={pending}) — waiting"
                    ));
                }
                EditQueue::Unknown => {
                    log.push("edit queue: probe unanswered — waiting".to_string());
                }
            }
            if std::time::Instant::now() > deadline {
                return Err("edit queue: never read idle within the proof budget".to_string());
            }
            std::thread::sleep(Duration::from_secs(10));
        }
    }

    // -- Step 4: the faithful masked edit. The mask covers a
    // centred band, rasterized by the UI's own rasterizer. --
    let canvas_path = store.candidate_path(&origin_cand);
    let (cw, ch) = (decoded.width, decoded.height);
    let mut mask = crate::workbench::MaskCanvas::new(cw, ch);
    mask.strokes.push(crate::workbench::Stroke {
        points: vec![
            (cw as f32 * 0.30, ch as f32 * 0.42),
            (cw as f32 * 0.70, ch as f32 * 0.42),
        ],
        radius: ch as f32 * 0.06,
    });
    let coverage = mask.coverage();
    let mask_path = job_dir.join("mask.png");
    mask.write_png(&mask_path)
        .map_err(|e| format!("write mask: {e}"))?;
    log.push(format!("mask: {}x{} coverage {:.3}", cw, ch, coverage));

    let canvas_sha =
        crate::workbench::sha256_file(&canvas_path).map_err(|e| format!("canvas hash: {e}"))?;
    let edit_seed: i64 = 20261127;
    let spec = EditSpec {
        canvas: canvas_path,
        prompt: opts.edit_prompt.clone(),
        out_prefix: job_dir.join("edit"),
        config: EditConfig::Faithful,
        seed: Some(edit_seed),
        mask: Some(mask_path),
        feather: 16,
    };
    let outcome = run_edit(&cfg, &spec, Duration::from_secs(900))
        .map_err(|e| format!("faithful edit: {e}"))?;
    log.push(format!("edit delivered: {}", outcome.delivered.display()));
    store
        .bank_prompt(&mut record, "edit", &opts.edit_prompt)
        .map_err(|e| format!("bank edit prompt: {e}"))?;

    let edit_config = serde_json::json!({
        "driver": "edit-local.py edit",
        "flags": ["--mask", "--feather 16"],
        "feather": 16,
        "mask_coverage": coverage,
        "raw_output": outcome.raw.display().to_string(),
    });
    let edit_cand = store
        .add_candidate(
            &mut record,
            crate::workbench::CandidateSpec {
                source_file: outcome.delivered.clone(),
                route: ROUTE_EDIT_FAITHFUL.to_string(),
                prompt: opts.edit_prompt.clone(),
                seed: Some(edit_seed as u64),
                config: edit_config,
                canvas: Some((origin_cand.id.clone(), canvas_sha)),
            },
        )
        .map_err(|e| format!("register edit: {e}"))?;
    log.push(format!(
        "registered {} ({}) sha256={} canvas={} canvas_sha256={}",
        edit_cand.id,
        edit_cand.class,
        edit_cand.sha256,
        edit_cand.canvas_candidate.as_deref().unwrap_or("-"),
        edit_cand.canvas_sha256.as_deref().unwrap_or("-")
    ));

    // -- Step 5: verify on disk, fresh from the store. --
    let reloaded = store
        .load(&opts.character)
        .map_err(|e| format!("reload record: {e}"))?;
    for cand in [&origin_cand, &edit_cand] {
        let stored = reloaded
            .candidate(&cand.id)
            .ok_or_else(|| format!("verify: {} missing after reload", cand.id))?;
        if stored.class != CLASS_CANDIDATE {
            return Err(format!(
                "verify: {} has class {} — the proof must never admit",
                cand.id, stored.class
            ));
        }
        let path = store.candidate_path(stored);
        let disk_sha = crate::workbench::sha256_file(&path)
            .map_err(|e| format!("verify hash {}: {e}", cand.id))?;
        if disk_sha != stored.sha256 {
            return Err(format!(
                "verify: {} on-disk hash {disk_sha} != recorded {}",
                cand.id, stored.sha256
            ));
        }
        let img: RgbaImage = crate::workbench::decode_png_file(&path)
            .map_err(|e| format!("verify decode {}: {e}", cand.id))?;
        log.push(format!(
            "verified {}: {}x{} class={} route={} prompt={:?}",
            cand.id,
            img.width,
            img.height,
            stored.class,
            stored.route,
            truncate(&stored.prompt, 60)
        ));
    }
    log.push("PROOF OK: originate + faithful masked edit landed as candidates".to_string());
    Ok(log)
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let cut: String = s.chars().take(max).collect();
        format!("{cut}…")
    }
}
