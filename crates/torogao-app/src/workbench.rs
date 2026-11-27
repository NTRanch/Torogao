// SPDX-License-Identifier: Apache-2.0 OR MIT
// Copyright (c) 2026 Qompass AI

//! The desktop art workbench core: character records, the
//! governed origination workflow, the candidate gallery's
//! data model, and the edit-mask rasterizer.
//!
//! The workbench puts the character-art loop in the Owner's
//! hands on the desktop. This module is the UI-free half:
//! everything the window shows is a view over these types,
//! and the headless proof (`--workbench-proof`) drives the
//! same functions the buttons call.
//!
//! Contracts held here:
//!
//! - Origination speaks the facade's governed job contract
//!   exactly as `torogao-mobile-core` defines it (workflow
//!   document, canonical bytes, SHA-256 approval hash). The
//!   composition below mirrors that crate's `style` and
//!   `workflow` modules value-for-value — the facade
//!   re-validates seat-side (checkpoint of record, recipe
//!   bounds, prompt cap, content screen), so a drift here is
//!   a refusal there, never a silent second door to the seat.
//! - Every artefact the workbench produces lands as a
//!   CANDIDATE. Admission is the Owner's act: the only path
//!   to `class: admitted` is [`Store::set_admitted`], called
//!   from the Owner's button, and it records its timestamp.
//! - Character records use the shared `torogao-character/1`
//!   schema (the mobile architecture's record), additive
//!   fields only, so the phone and the desktop read the same
//!   girls.
//! - Provenance is complete per candidate: the verbatim
//!   prompt text, the seed, the route, the config, and the
//!   canvas hash for edits. The facade token is never part
//!   of any persisted structure.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::Digest;

// ---------------------------------------------------------------------
// The style record of record (mirrors torogao-mobile-core::style).
// ---------------------------------------------------------------------

/// The generator checkpoint of record (waiIllustriousSDXL
/// v170). Only this checkpoint may be requested; the facade
/// enforces the same name seat-side.
pub const CHECKPOINT: &str = "waiIllustriousSDXL_v170.safetensors";
/// Quality prefix prepended to every composed positive prompt.
pub const STYLE_PREFIX: &str = "masterpiece, best quality, amazing quality, very aesthetic, ";
/// Shared negative prompt; cast records append their own guards.
pub const DEFAULT_NEGATIVE: &str = "lowres, bad anatomy, bad hands, text, error, \
    missing fingers, extra digit, fewer digits, cropped, worst quality, low quality, \
    jpeg artifacts, signature, watermark, username, blurry";
/// Sampler name in the seat's vocabulary.
pub const SAMPLER: &str = "euler_ancestral";
/// Scheduler name in the seat's vocabulary.
pub const SCHEDULER: &str = "normal";
/// Sampling steps per still, recipe of record.
pub const STEPS: u32 = 28;
/// Classifier-free guidance scale, recipe of record.
pub const CFG: f32 = 6.0;
/// Portrait width in pixels, recipe of record.
pub const WIDTH: u32 = 832;
/// Portrait height in pixels, recipe of record.
pub const HEIGHT: u32 = 1344;
/// File-name prefix the seat saves results under.
pub const FILENAME_PREFIX: &str = "torogao_mobile";
/// Longest user prompt accepted, in bytes (facade cap).
pub const USER_PROMPT_BYTES_MAX: usize = 2000;

/// User-adjustable recipe settings, bounded by the recipe of
/// record. The facade re-checks the same bounds seat-side.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct RecipeSettings {
    /// Sampler steps.
    pub steps: u32,
    /// Classifier-free guidance scale.
    pub cfg: f32,
    /// Output width in pixels.
    pub width: u32,
    /// Output height in pixels.
    pub height: u32,
}

/// Lowest sampler steps the settings may choose.
pub const STEPS_MIN: u32 = 10;
/// Highest sampler steps the settings may choose.
pub const STEPS_MAX: u32 = 60;
/// Lowest CFG the settings may choose.
pub const CFG_MIN: f32 = 1.0;
/// Highest CFG the settings may choose.
pub const CFG_MAX: f32 = 12.0;
/// Smallest axis (either direction) the settings may choose.
pub const AXIS_MIN: u32 = 512;
/// Largest width the settings may choose.
pub const WIDTH_MAX: u32 = 1024;
/// Largest height the settings may choose.
pub const HEIGHT_MAX: u32 = 1536;

impl RecipeSettings {
    /// The recipe of record, as constants above.
    pub fn of_record() -> Self {
        Self {
            steps: STEPS,
            cfg: CFG,
            width: WIDTH,
            height: HEIGHT,
        }
    }

    /// Check the settings against the recipe bounds. Returns
    /// a presentable reason for the first bound violated.
    pub fn validate(&self) -> Result<(), String> {
        if !(STEPS_MIN..=STEPS_MAX).contains(&self.steps) {
            return Err(format!(
                "steps {} is outside {STEPS_MIN}..={STEPS_MAX}",
                self.steps
            ));
        }
        if !self.cfg.is_finite() || !(CFG_MIN..=CFG_MAX).contains(&self.cfg) {
            return Err(format!("cfg {} is outside {CFG_MIN}..={CFG_MAX}", self.cfg));
        }
        if self.width < AXIS_MIN || self.width > WIDTH_MAX || self.width % 8 != 0 {
            return Err(format!(
                "width {} is outside {AXIS_MIN}..={WIDTH_MAX} in steps of 8",
                self.width
            ));
        }
        if self.height < AXIS_MIN || self.height > HEIGHT_MAX || self.height % 8 != 0 {
            return Err(format!(
                "height {} is outside {AXIS_MIN}..={HEIGHT_MAX} in steps of 8",
                self.height
            ));
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------
// Workflow composition (mirrors torogao-mobile-core::workflow).
// ---------------------------------------------------------------------

/// Compose the positive prompt: the shared style prefix, the
/// character's identity and signature, then the user's scene
/// prompt. An empty user prompt is a canonical portrait.
pub fn compose_positive(record: &CharacterRecord, user_prompt: &str) -> String {
    let mut out = String::with_capacity(
        STYLE_PREFIX.len()
            + record.prompt_identity.identity.len()
            + record.prompt_identity.signature.len()
            + user_prompt.len()
            + 4,
    );
    out.push_str(STYLE_PREFIX);
    out.push_str(&record.prompt_identity.identity);
    out.push_str(", ");
    out.push_str(&record.prompt_identity.signature);
    let scene = user_prompt.trim();
    if !scene.is_empty() {
        out.push_str(", ");
        out.push_str(scene);
    }
    out
}

/// Compose the negative prompt: the shared negative plus the
/// character's own guards.
pub fn compose_negative(record: &CharacterRecord) -> String {
    if record.prompt_identity.negative_guards.is_empty() {
        DEFAULT_NEGATIVE.to_string()
    } else {
        format!(
            "{}, {}",
            DEFAULT_NEGATIVE, record.prompt_identity.negative_guards
        )
    }
}

/// Build the workflow document for one still with explicit
/// recipe settings. Node ids are the document's own wiring
/// and are stable: "1" is the checkpoint loader, "7" is the
/// save node the facade's history shape reports.
pub fn build_workflow(
    record: &CharacterRecord,
    user_prompt: &str,
    seed: u64,
    recipe: &RecipeSettings,
) -> Result<serde_json::Value, String> {
    recipe.validate()?;
    if user_prompt.len() > USER_PROMPT_BYTES_MAX {
        return Err(format!(
            "user prompt is {} bytes; the cap is {USER_PROMPT_BYTES_MAX}",
            user_prompt.len()
        ));
    }
    Ok(serde_json::json!({
        "1": {
            "class_type": "CheckpointLoaderSimple",
            "inputs": { "ckpt_name": CHECKPOINT }
        },
        "2": {
            "class_type": "CLIPTextEncode",
            "inputs": {
                "clip": ["1", 1],
                "text": compose_positive(record, user_prompt)
            }
        },
        "3": {
            "class_type": "CLIPTextEncode",
            "inputs": {
                "clip": ["1", 1],
                "text": compose_negative(record)
            }
        },
        "4": {
            "class_type": "EmptyLatentImage",
            "inputs": {
                "width": recipe.width,
                "height": recipe.height,
                "batch_size": 1
            }
        },
        "5": {
            "class_type": "KSampler",
            "inputs": {
                "model": ["1", 0],
                "seed": seed,
                "steps": recipe.steps,
                "cfg": recipe.cfg,
                "sampler_name": SAMPLER,
                "scheduler": SCHEDULER,
                "positive": ["2", 0],
                "negative": ["3", 0],
                "latent_image": ["4", 0],
                "denoise": 1.0
            }
        },
        "6": {
            "class_type": "VAEDecode",
            "inputs": { "samples": ["5", 0], "vae": ["1", 2] }
        },
        "7": {
            "class_type": "SaveImage",
            "inputs": {
                "images": ["6", 0],
                "filename_prefix": FILENAME_PREFIX
            }
        }
    }))
}

/// The canonical serialization of a workflow document — the
/// bytes the approval hash covers. Object keys are emitted
/// in sorted order at every depth, so the bytes are the same
/// on both sides of the wire regardless of map-ordering
/// feature flags in either build.
pub fn canonical_workflow_bytes(workflow: &serde_json::Value) -> Vec<u8> {
    let mut out = Vec::new();
    write_canonical(workflow, &mut out);
    out
}

fn write_canonical(value: &serde_json::Value, out: &mut Vec<u8>) {
    match value {
        serde_json::Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            out.push(b'{');
            for (i, key) in keys.iter().enumerate() {
                if i > 0 {
                    out.push(b',');
                }
                let key_json = serde_json::to_string(key).unwrap_or_else(|_| "\"\"".to_string());
                out.extend_from_slice(key_json.as_bytes());
                out.push(b':');
                write_canonical(&map[*key], out);
            }
            out.push(b'}');
        }
        serde_json::Value::Array(items) => {
            out.push(b'[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(b',');
                }
                write_canonical(item, out);
            }
            out.push(b']');
        }
        other => {
            let text = serde_json::to_string(other).unwrap_or_else(|_| "null".to_string());
            out.extend_from_slice(text.as_bytes());
        }
    }
}

/// SHA-256 of the canonical workflow bytes, lowercase hex.
/// The client sends this with the submission; the facade
/// recomputes it over the document it received and refuses
/// a mismatch.
pub fn workflow_sha256(workflow: &serde_json::Value) -> String {
    sha256_hex(&canonical_workflow_bytes(workflow))
}

/// SHA-256 of arbitrary bytes, lowercase hex.
pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = sha2::Sha256::digest(bytes);
    let mut out = String::with_capacity(64);
    for b in digest {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

/// SHA-256 of a file's bytes, lowercase hex.
pub fn sha256_file(path: &Path) -> Result<String, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    Ok(sha256_hex(&bytes))
}

// ---------------------------------------------------------------------
// Character records (schema torogao-character/1, shared with mobile).
// ---------------------------------------------------------------------

/// The record schema name, shared with the mobile lanes.
pub const RECORD_SCHEMA: &str = "torogao-character/1";
/// Candidate class label at birth. Every output lands here.
pub const CLASS_CANDIDATE: &str = "candidate";
/// Admitted class label. Reached only through the Owner's
/// admission act recorded by [`Store::set_admitted`].
pub const CLASS_ADMITTED: &str = "admitted";
/// Route label for facade originations.
pub const ROUTE_ORIGINATE: &str = "wai-facade";
/// Route label for faithful primo-local edits.
pub const ROUTE_EDIT_FAITHFUL: &str = "edit-faithful";
/// Route label for turbo primo-local edits (not chain-safe).
pub const ROUTE_EDIT_TURBO: &str = "edit-turbo";
/// Prompt-history entries kept per character (bounded).
pub const PROMPT_HISTORY_MAX: usize = 200;

/// A character's prompt identity, taken from her governing
/// lock or design record — never invented at a prompt box.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PromptIdentity {
    /// Positive identity tags (Illustrious tag style).
    pub identity: String,
    /// Signature items / canonical outfit line.
    pub signature: String,
    /// Per-character additions to the shared negative prompt.
    pub negative_guards: String,
    /// The lock / design record this identity was taken from.
    pub source: String,
}

/// One banked prompt-history entry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PromptHistoryEntry {
    /// ISO-8601 UTC timestamp of the firing.
    pub at: String,
    /// "originate" or "edit".
    pub kind: String,
    /// The user-authored text (scene prompt or instruction).
    pub text: String,
}

/// One gallery artefact with its full provenance.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Candidate {
    /// Stable per-character id ("cand-0001", ...).
    pub id: String,
    /// File path relative to the store root.
    pub file: String,
    /// SHA-256 of the file's bytes at registration.
    pub sha256: String,
    /// Producing route (ROUTE_* label).
    pub route: String,
    /// The verbatim prompt text used (the composed positive
    /// for originations; the instruction for edits).
    pub prompt: String,
    /// The seed, when the route takes one.
    pub seed: Option<u64>,
    /// Route config as JSON (recipe for originations; driver
    /// flags for edits).
    pub config: serde_json::Value,
    /// For edits: the canvas candidate's id.
    pub canvas_candidate: Option<String>,
    /// For edits: the canvas file's SHA-256 at firing time.
    pub canvas_sha256: Option<String>,
    /// CLASS_CANDIDATE or CLASS_ADMITTED.
    pub class: String,
    /// When the Owner admitted this candidate, if ever.
    pub admitted_at: Option<String>,
    /// ISO-8601 UTC registration timestamp.
    pub created_at: String,
}

/// The art state of one character record.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ArtState {
    /// All registered candidates, in registration order.
    pub candidates: Vec<Candidate>,
}

/// One character record — the shared torogao-character/1
/// document. Fields beyond the mobile schema's core are
/// additive (prompt history, cast grouping).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CharacterRecord {
    /// Schema name; must equal RECORD_SCHEMA.
    pub schema: String,
    /// Stable id used in requests, files, and gallery entries.
    pub id: String,
    /// Display name for the picker.
    pub name: String,
    /// Cast grouping label ("Torogao", "MoonDeep").
    pub cast: String,
    /// Prompt identity of record.
    pub prompt_identity: PromptIdentity,
    /// Banked prompt history, oldest first.
    #[serde(default)]
    pub prompt_history: Vec<PromptHistoryEntry>,
    /// Gallery state.
    #[serde(default)]
    pub art: ArtState,
    /// Monotonic revision, bumped on every saved change.
    pub revision: u64,
    /// ISO-8601 UTC timestamp of the last saved change.
    pub updated_at: String,
}

impl CharacterRecord {
    fn blank(id: &str, name: &str, cast: &str, prompt_identity: PromptIdentity) -> Self {
        Self {
            schema: RECORD_SCHEMA.to_string(),
            id: id.to_string(),
            name: name.to_string(),
            cast: cast.to_string(),
            prompt_identity,
            prompt_history: Vec::new(),
            art: ArtState::default(),
            revision: 0,
            updated_at: String::new(),
        }
    }

    /// Find a candidate by id.
    pub fn candidate(&self, id: &str) -> Option<&Candidate> {
        self.art.candidates.iter().find(|c| c.id == id)
    }
}

/// The provenance bundle for registering one candidate:
/// where the artefact is, which route produced it, the
/// verbatim prompt, the seed, the route config, and — for
/// edits — the canvas it struck over.
#[derive(Debug, Clone)]
pub struct CandidateSpec {
    /// The produced artefact file to copy into the store.
    pub source_file: PathBuf,
    /// Producing route (ROUTE_* label).
    pub route: String,
    /// The verbatim prompt text used.
    pub prompt: String,
    /// The seed, when the route takes one.
    pub seed: Option<u64>,
    /// Route config as JSON.
    pub config: serde_json::Value,
    /// For edits: (canvas candidate id, canvas sha256).
    pub canvas: Option<(String, String)>,
}

// ---------------------------------------------------------------------
// The store: records and artefact files under one root.
// ---------------------------------------------------------------------

/// The workbench store: one JSON record per character under
/// `characters/`, artefact files under `art/<id>/`, job
/// scratch under `jobs/`.
#[derive(Debug, Clone)]
pub struct Store {
    root: PathBuf,
}

impl Store {
    /// The default store root: `$TOROGAO_WB_HOME`, else
    /// `$HOME/.local/share/torogao-workbench`.
    pub fn default_root() -> PathBuf {
        if let Ok(home) = std::env::var("TOROGAO_WB_HOME") {
            if !home.is_empty() {
                return PathBuf::from(home);
            }
        }
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        PathBuf::from(home)
            .join(".local")
            .join("share")
            .join("torogao-workbench")
    }

    /// Open (creating if needed) the store at `root`. On a
    /// fresh store the seed cast is written so the picker is
    /// never empty; an existing store is never reseeded.
    pub fn open(root: &Path) -> Result<Store, String> {
        let store = Store {
            root: root.to_path_buf(),
        };
        std::fs::create_dir_all(store.characters_dir())
            .map_err(|e| format!("create characters dir: {e}"))?;
        std::fs::create_dir_all(store.root.join("art"))
            .map_err(|e| format!("create art dir: {e}"))?;
        std::fs::create_dir_all(store.root.join("jobs"))
            .map_err(|e| format!("create jobs dir: {e}"))?;
        if store.list_ids()?.is_empty() {
            for record in seeds::seed_records() {
                store.save(&record)?;
            }
        }
        Ok(store)
    }

    /// The store root path.
    pub fn root(&self) -> &Path {
        &self.root
    }

    fn characters_dir(&self) -> PathBuf {
        self.root.join("characters")
    }

    fn record_path(&self, id: &str) -> PathBuf {
        self.characters_dir().join(format!("{id}.json"))
    }

    /// All character ids on disk, sorted.
    pub fn list_ids(&self) -> Result<Vec<String>, String> {
        let mut ids = Vec::new();
        let entries = match std::fs::read_dir(self.characters_dir()) {
            Ok(e) => e,
            Err(e) => return Err(format!("list characters: {e}")),
        };
        for entry in entries {
            let entry = entry.map_err(|e| format!("list characters: {e}"))?;
            let name = entry.file_name().to_string_lossy().to_string();
            if let Some(id) = name.strip_suffix(".json") {
                ids.push(id.to_string());
            }
        }
        ids.sort();
        Ok(ids)
    }

    /// Load one record. A malformed record is a typed refusal
    /// — never a panic, never a silent overwrite.
    pub fn load(&self, id: &str) -> Result<CharacterRecord, String> {
        let path = self.record_path(id);
        let text =
            std::fs::read_to_string(&path).map_err(|e| format!("read {}: {e}", path.display()))?;
        let record: CharacterRecord =
            serde_json::from_str(&text).map_err(|e| format!("record {id} is malformed: {e}"))?;
        if record.schema != RECORD_SCHEMA {
            return Err(format!(
                "record {id} has schema {:?}; this workbench reads {RECORD_SCHEMA}",
                record.schema
            ));
        }
        if record.id != id {
            return Err(format!(
                "record file {id}.json carries id {:?}; refusing",
                record.id
            ));
        }
        Ok(record)
    }

    /// Load every record, in id order.
    pub fn load_all(&self) -> Result<Vec<CharacterRecord>, String> {
        let mut out = Vec::new();
        for id in self.list_ids()? {
            out.push(self.load(&id)?);
        }
        Ok(out)
    }

    /// Save a record (revision bumped, timestamp refreshed),
    /// written via a temp file + rename so a crash cannot
    /// leave a torn record.
    pub fn save(&self, record: &CharacterRecord) -> Result<(), String> {
        let mut record = record.clone();
        record.revision = record.revision.saturating_add(1);
        record.updated_at = now_iso();
        let path = self.record_path(&record.id);
        let tmp = path.with_extension("json.tmp");
        let text = serde_json::to_string_pretty(&record)
            .map_err(|e| format!("serialize record {}: {e}", record.id))?;
        std::fs::write(&tmp, text).map_err(|e| format!("write {}: {e}", tmp.display()))?;
        std::fs::rename(&tmp, &path).map_err(|e| format!("rename to {}: {e}", path.display()))?;
        Ok(())
    }

    /// Absolute path of a candidate's artefact file.
    pub fn candidate_path(&self, candidate: &Candidate) -> PathBuf {
        self.root.join(&candidate.file)
    }

    /// Register a produced artefact as a candidate of
    /// `record`. The artefact bytes are copied into the
    /// store; the candidate is ALWAYS born `class: candidate`
    /// whatever the caller asked for — admission is a
    /// separate Owner act.
    pub fn add_candidate(
        &self,
        record: &mut CharacterRecord,
        spec: CandidateSpec,
    ) -> Result<Candidate, String> {
        let source_file = spec.source_file.as_path();
        let n = record.art.candidates.len() + 1;
        let id = format!("cand-{n:04}");
        let rel = format!("art/{}/{id}-{}.png", record.id, spec.route);
        let dest = self.root.join(&rel);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("create {}: {e}", parent.display()))?;
        }
        std::fs::copy(source_file, &dest)
            .map_err(|e| format!("copy artefact {}: {e}", source_file.display()))?;
        let sha = sha256_file(&dest)?;
        let candidate = Candidate {
            id,
            file: rel,
            sha256: sha,
            route: spec.route,
            prompt: spec.prompt,
            seed: spec.seed,
            config: spec.config,
            canvas_candidate: spec.canvas.as_ref().map(|(cid, _)| cid.clone()),
            canvas_sha256: spec.canvas.as_ref().map(|(_, sha)| sha.clone()),
            class: CLASS_CANDIDATE.to_string(),
            admitted_at: None,
            created_at: now_iso(),
        };
        record.art.candidates.push(candidate.clone());
        self.save(record)?;
        // save() bumps the in-store copy's revision; keep the
        // caller's copy in step.
        record.revision = record.revision.saturating_add(1);
        Ok(candidate)
    }

    /// The Owner's admission act — the ONLY writer of the
    /// admitted class. Records its timestamp either way.
    pub fn set_admitted(
        &self,
        record: &mut CharacterRecord,
        candidate_id: &str,
        admitted: bool,
    ) -> Result<(), String> {
        let Some(candidate) = record
            .art
            .candidates
            .iter_mut()
            .find(|c| c.id == candidate_id)
        else {
            return Err(format!("no candidate {candidate_id} on {}", record.id));
        };
        if admitted {
            candidate.class = CLASS_ADMITTED.to_string();
            candidate.admitted_at = Some(now_iso());
        } else {
            candidate.class = CLASS_CANDIDATE.to_string();
            candidate.admitted_at = None;
        }
        self.save(record)?;
        record.revision = record.revision.saturating_add(1);
        Ok(())
    }

    /// Bank a fired prompt into the character's history
    /// (bounded; oldest entries fall off past the cap).
    pub fn bank_prompt(
        &self,
        record: &mut CharacterRecord,
        kind: &str,
        text: &str,
    ) -> Result<(), String> {
        record.prompt_history.push(PromptHistoryEntry {
            at: now_iso(),
            kind: kind.to_string(),
            text: text.to_string(),
        });
        if record.prompt_history.len() > PROMPT_HISTORY_MAX {
            let excess = record.prompt_history.len() - PROMPT_HISTORY_MAX;
            record.prompt_history.drain(0..excess);
        }
        self.save(record)?;
        record.revision = record.revision.saturating_add(1);
        Ok(())
    }
}

// ---------------------------------------------------------------------
// Timestamps: ISO-8601 UTC from the system clock, no deps.
// ---------------------------------------------------------------------

/// Current time as ISO-8601 UTC ("2026-11-27T18:52:03Z").
pub fn now_iso() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    iso_from_unix(secs)
}

/// Format unix seconds as ISO-8601 UTC. The civil-date
/// conversion is Howard Hinnant's days-from-civil inverse.
pub fn iso_from_unix(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let (h, m, s) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { y + 1 } else { y };
    format!("{year:04}-{month:02}-{d:02}T{h:02}:{m:02}:{s:02}Z")
}

// ---------------------------------------------------------------------
// PNG helpers + the edit-mask rasterizer.
// ---------------------------------------------------------------------

/// A decoded RGBA image (8-bit per channel).
#[derive(Debug, Clone, PartialEq)]
pub struct RgbaImage {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Row-major RGBA bytes, `width * height * 4` long.
    pub pixels: Vec<u8>,
}

/// Decode a PNG file into RGBA. Colour types beyond 8-bit
/// RGB/RGBA/grey are refused by name.
pub fn decode_png_file(path: &Path) -> Result<RgbaImage, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    decode_png_bytes(&bytes)
}

/// Decode PNG bytes into RGBA.
pub fn decode_png_bytes(bytes: &[u8]) -> Result<RgbaImage, String> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::EXPAND);
    let mut reader = decoder
        .read_info()
        .map_err(|e| format!("png decode: {e}"))?;
    let out_size = reader
        .output_buffer_size()
        .ok_or_else(|| "png: output size unknown".to_string())?;
    let mut buf = vec![0u8; out_size];
    let info = reader
        .next_frame(&mut buf)
        .map_err(|e| format!("png decode frame: {e}"))?;
    buf.truncate(info.buffer_size());
    let (width, height) = (info.width, info.height);
    if info.bit_depth != png::BitDepth::Eight {
        return Err(format!("png bit depth {:?} not supported", info.bit_depth));
    }
    let pixels = match info.color_type {
        png::ColorType::Rgba => buf,
        png::ColorType::Rgb => {
            let mut out = Vec::with_capacity((width * height * 4) as usize);
            for px in buf.chunks_exact(3) {
                out.extend_from_slice(px);
                out.push(255);
            }
            out
        }
        png::ColorType::Grayscale => {
            let mut out = Vec::with_capacity((width * height * 4) as usize);
            for g in buf {
                out.extend_from_slice(&[g, g, g, 255]);
            }
            out
        }
        png::ColorType::GrayscaleAlpha => {
            let mut out = Vec::with_capacity((width * height * 4) as usize);
            for px in buf.chunks_exact(2) {
                out.extend_from_slice(&[px[0], px[0], px[0], px[1]]);
            }
            out
        }
        other => return Err(format!("png colour type {other:?} not supported")),
    };
    Ok(RgbaImage {
        width,
        height,
        pixels,
    })
}

/// Encode an RGBA image as PNG bytes.
pub fn encode_png_rgba(image: &RgbaImage) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, image.width, image.height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder
            .write_header()
            .map_err(|e| format!("png encode: {e}"))?;
        writer
            .write_image_data(&image.pixels)
            .map_err(|e| format!("png encode data: {e}"))?;
    }
    Ok(out)
}

/// Box-downscale to fit within `max_w` x `max_h` (never
/// enlarges). Used for gallery thumbnails.
pub fn downscale(image: &RgbaImage, max_w: u32, max_h: u32) -> RgbaImage {
    if image.width <= max_w && image.height <= max_h {
        return image.clone();
    }
    let scale = (max_w as f32 / image.width as f32).min(max_h as f32 / image.height as f32);
    let out_w = ((image.width as f32 * scale) as u32).max(1);
    let out_h = ((image.height as f32 * scale) as u32).max(1);
    let mut pixels = vec![0u8; (out_w * out_h * 4) as usize];
    for oy in 0..out_h {
        let y0 = (oy * image.height / out_h) as usize;
        let y1 = (((oy + 1) * image.height / out_h) as usize).max(y0 + 1);
        for ox in 0..out_w {
            let x0 = (ox * image.width / out_w) as usize;
            let x1 = (((ox + 1) * image.width / out_w) as usize).max(x0 + 1);
            let mut acc = [0u64; 4];
            let mut n = 0u64;
            for y in y0..y1.min(image.height as usize) {
                for x in x0..x1.min(image.width as usize) {
                    let i = (y * image.width as usize + x) * 4;
                    for (c, a) in acc.iter_mut().enumerate() {
                        *a += image.pixels[i + c] as u64;
                    }
                    n += 1;
                }
            }
            let o = ((oy * out_w + ox) * 4) as usize;
            for (c, a) in acc.iter().enumerate() {
                pixels[o + c] = a.checked_div(n).unwrap_or(0) as u8;
            }
        }
    }
    RgbaImage {
        width: out_w,
        height: out_h,
        pixels,
    }
}

/// One brush stroke of an edit mask, in image pixel
/// coordinates.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Stroke {
    /// Polyline points (x, y) in image coordinates.
    pub points: Vec<(f32, f32)>,
    /// Brush radius in image pixels.
    pub radius: f32,
}

/// A paintable edit mask over a canvas of fixed size. The
/// same rasterizer serves the UI's painted strokes and the
/// headless proof's programmatic strokes; the driver's
/// `--feather` is applied by the driver, not here.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MaskCanvas {
    /// Canvas width in pixels.
    pub width: u32,
    /// Canvas height in pixels.
    pub height: u32,
    /// Painted strokes, white on black.
    pub strokes: Vec<Stroke>,
}

impl MaskCanvas {
    /// A blank mask over a `width` x `height` canvas.
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            strokes: Vec::new(),
        }
    }

    /// True when nothing is painted (an edit with an empty
    /// mask is refused upstream rather than fired blind).
    pub fn is_empty(&self) -> bool {
        self.strokes.iter().all(|s| s.points.is_empty())
    }

    /// Rasterize to a grayscale buffer (255 = take the edit,
    /// 0 = keep the canvas), matching the driver's mask
    /// convention. Strokes are stamped as filled circles
    /// along each segment; points outside the canvas clip.
    pub fn rasterize(&self) -> Vec<u8> {
        let mut buf = vec![0u8; (self.width * self.height) as usize];
        for stroke in &self.strokes {
            if stroke.points.is_empty() {
                continue;
            }
            let radius = stroke.radius.max(0.5);
            let stamp = |buf: &mut Vec<u8>, cx: f32, cy: f32| {
                let x0 = (cx - radius).floor().max(0.0) as i64;
                let x1 = (cx + radius).ceil().min(self.width as f32 - 1.0) as i64;
                let y0 = (cy - radius).floor().max(0.0) as i64;
                let y1 = (cy + radius).ceil().min(self.height as f32 - 1.0) as i64;
                for y in y0..=y1 {
                    for x in x0..=x1 {
                        let dx = x as f32 - cx;
                        let dy = y as f32 - cy;
                        if dx * dx + dy * dy <= radius * radius {
                            buf[(y as u32 * self.width + x as u32) as usize] = 255;
                        }
                    }
                }
            };
            if stroke.points.len() == 1 {
                stamp(&mut buf, stroke.points[0].0, stroke.points[0].1);
                continue;
            }
            for pair in stroke.points.windows(2) {
                let (ax, ay) = pair[0];
                let (bx, by) = pair[1];
                let dist = ((bx - ax).powi(2) + (by - ay).powi(2)).sqrt();
                let steps = (dist / (radius * 0.5)).ceil().max(1.0) as u32;
                for i in 0..=steps {
                    let t = i as f32 / steps as f32;
                    stamp(&mut buf, ax + (bx - ax) * t, ay + (by - ay) * t);
                }
            }
        }
        buf
    }

    /// Coverage: the fraction of pixels the edit would take.
    pub fn coverage(&self) -> f32 {
        let buf = self.rasterize();
        if buf.is_empty() {
            return 0.0;
        }
        let taken = buf.iter().filter(|&&v| v > 0).count();
        taken as f32 / buf.len() as f32
    }

    /// Write the rasterized mask as a grayscale PNG.
    pub fn write_png(&self, path: &Path) -> Result<(), String> {
        let buf = self.rasterize();
        let mut out = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut out, self.width, self.height);
            encoder.set_color(png::ColorType::Grayscale);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder
                .write_header()
                .map_err(|e| format!("mask png encode: {e}"))?;
            writer
                .write_image_data(&buf)
                .map_err(|e| format!("mask png encode data: {e}"))?;
        }
        std::fs::write(path, out).map_err(|e| format!("write {}: {e}", path.display()))
    }
}

// ---------------------------------------------------------------------
// Seed cast: records written on a fresh store. Prima/Lana/Sirena are
// verbatim from torogao-mobile-core::cast (the governed mobile cast);
// the MoonDeep seven are composed from MoonDeep design-party.md §1
// canon as banked in moondeep-regeneration-20261127/prompts.json.
// ---------------------------------------------------------------------

pub mod seeds {
    use super::{CharacterRecord, PromptIdentity};

    fn rec(
        id: &str,
        name: &str,
        cast: &str,
        identity: &str,
        signature: &str,
        guards: &str,
        source: &str,
    ) -> CharacterRecord {
        CharacterRecord::blank(
            id,
            name,
            cast,
            PromptIdentity {
                identity: identity.to_string(),
                signature: signature.to_string(),
                negative_guards: guards.to_string(),
                source: source.to_string(),
            },
        )
    }

    /// The seed cast for a fresh store.
    pub fn seed_records() -> Vec<CharacterRecord> {
        vec![
            rec(
                "prima",
                "Prima",
                "Torogao",
                "1girl, solo, very long wavy silver-white hair, lavender-grey hair shading, \
                 centre part, face-framing locks, deep red eyes, fair skin",
                "pink dress, frilled dress",
                "",
                "Canonical-base RUNG0-LOCK (identity lock held 2026-11-24): very long loose \
                 wavy silver-white hair with lavender-grey shadow drift, centre part; iris \
                 palette (0.656, 0.006, 0.012) — deep crimson; DressPink palette; ~7.6-head \
                 proportions. (via torogao-mobile-core::cast)",
            ),
            rec(
                "lana",
                "Lana",
                "Torogao",
                "1girl, solo, long flowing wavy red hair, amber-brown eyes, freckles, \
                 fair skin, wry smile, lean athletic body",
                "crop top, short shorts, thong above waistband, nipple piercings",
                "",
                "Identity lock D-1482 (Owner, 2026-11-13): vivid-red wavy hair, amber-brown \
                 eyes, fair skin with light freckles (hers alone), wry smile, E-cup firm/perky \
                 bust, lean athletic build; outfit as ruled. (via torogao-mobile-core::cast)",
            ),
            rec(
                "sirena",
                "Sirena",
                "Torogao",
                "1girl, solo, sleek glossy black hair, hair to mid-back, braided side hair \
                 with gold chain, round-almond plum-violet eyes, gentle confident gaze, \
                 curvaceous, wide hips, trained lower body",
                "black ribbed top, open black leather jacket, gold chain details, \
                 gold pendulum necklace, spiral nipple piercings",
                "freckles",
                "STYLE-SHEET.md v0.3 (2026-11-26) + admitted full-body keeper set v4/v7/v8/v9: \
                 mid-back black hair, plum-violet eyes, G-cup perky, wider hips and trained \
                 lower body, gold pendulum motif, spiral piercings never barbells, no freckles \
                 ever. (via torogao-mobile-core::cast)",
            ),
            rec(
                "solenne",
                "Solenne",
                "MoonDeep",
                "1girl, solo, adult woman, mid twenties, athletic fit build, toned body, \
                 soft oval face, violet eyes, glossy luminous irises, two crisp highlights \
                 in each eye, clean eyelash line, very long lavender hair, hair reaching \
                 waist, side braid over one shoulder, white veil, calm composed expression, \
                 huge breasts, largest bust of the fleet, firm breasts",
                "black and white nun outfit re-cut for a warrior monk, skin-tight white top, \
                 two wide black cloth suspender straps over the white top, wooden prayer \
                 beads, plain wood and iron cross, dark-iron ornaments, garters",
                "",
                "MoonDeep canon (MEMORY.md, refreshed 2026-11-27) + design-party.md: \
                 warrior-monk nun re-cut, NO cape, NO bra, chest bands canon (Owner ruling \
                 2026-11-27 07:26), austerity — no gems/gold/feather. Identity composed \
                 from the banked origination prompt in \
                 moondeep-regeneration-20261127/prompts.json.",
            ),
            rec(
                "umbra",
                "Umbra",
                "MoonDeep",
                "1girl, solo, adult woman, mid twenties, athletic fit build, toned body, \
                 glowing teal eyes, luminous teal irises with a faint glow, glossy luminous \
                 irises, two crisp highlights in each eye, clean eyelash line, very long \
                 purple hair, hair reaching waist, loose undone hair, a half-undone braid \
                 unraveling, bare head, no headwear, quiet sly confident expression, \
                 half-lidded eyes, huge breasts, firm breasts",
                "black cloth covering the breasts, two wide bands of black cloth, \
                 garters, micro thong, dark-iron ornaments, crosses not glowing, \
                 no earrings, no belt",
                "",
                "MoonDeep canon (Owner rulings 2026-11-27 01:17 + 01:26): teal eyes + \
                 faint glow, hair undone, no head cover, black cloth covering just the \
                 breasts, belt removed, garters + micro thong sole lower covering, \
                 dark-iron ornaments, crosses NOT glowing, no earrings. Identity \
                 composed from moondeep-regeneration-20261127/prompts.json.",
            ),
            rec(
                "wrenna",
                "Wrenna",
                "MoonDeep",
                "1girl, solo, adult woman, mid twenties, athletic soft build, round face, \
                 soft cheeks, warm brown eyes, kind heavy-lidded eyes, glossy luminous \
                 irises, two crisp highlights in each eye, clean eyelash line, chestnut \
                 brown hair in one practical braid over one shoulder, warm upbeat smile, \
                 nurturing expression, large breasts",
                "white mage, undyed linen mage garb, cream ecru linen dress, \
                 form-fitting cut, sleeves rolled up past the elbows, bare forearms, \
                 plain cord belt, soft boots, matte woven linen",
                "",
                "MoonDeep design-party.md §1.1 as amended (white mage, Owner ruling \
                 2026-11-27 05:32; upbeat nurturing register, 05:45). Identity composed \
                 from moondeep-regeneration-20261127/prompts.json.",
            ),
            rec(
                "petra",
                "Petra",
                "MoonDeep",
                "1girl, solo, adult woman, late twenties, tall athletic woman, tallest \
                 of the party, strong shoulders, sturdy build, grey eyes, glossy \
                 luminous irises, two crisp highlights in each eye, clean eyelash line, \
                 very short hair, close-cropped dark brown hair, round glasses, bright \
                 curious grin, nerdy excited energy",
                "alchemist, charcoal travel clothes under a fitted plain cream \
                 alchemist apron, belt of glass vials at her waist, matte woven fabric",
                "",
                "MoonDeep design-party.md §1.2 as amended (alchemist, Owner ruling \
                 2026-11-27 05:32; nerdy lasciviously-experimental register, 05:45). \
                 Identity composed from moondeep-regeneration-20261127/prompts.json.",
            ),
            rec(
                "sable",
                "Sable",
                "MoonDeep",
                "1girl, solo, adult woman, early twenties, slight lean athletic build, \
                 slightest build of the party, half-lidded violet-grey eyes, glossy \
                 luminous irises, two crisp highlights in each eye, clean eyelash line, \
                 long loose dark brown hair, wavy loose hair worn loose over her \
                 shoulders, boisterous confident grin",
                "black mage, tall wide-brimmed pointed black mage hat, dark plum \
                 casting robe, form-fitting dark mage garb, wooden rosary beads at \
                 her wrist, matte woven fabric",
                "",
                "MoonDeep design-party.md §1.3 as amended (black mage + canon hat, \
                 Owner rulings 2026-11-27 05:32 + §1.3; boisterous register, 05:45). \
                 Identity composed from moondeep-regeneration-20261127/prompts.json.",
            ),
            rec(
                "tamsin",
                "Tamsin",
                "MoonDeep",
                "1girl, solo, adult woman, thirty years old, mature face, athletic \
                 solid build, tall strong woman, amber-brown eyes, heavy-lidded \
                 contentment, glossy luminous irises, two crisp highlights in each \
                 eye, clean eyelash line, dark brown hair in abundant loose curls, \
                 green leaves woven into her curls, mellow half-smile, scarred knuckles",
                "druid, long-sleeved green and brown druid field tunic, covered \
                 shoulders, full sleeves, cloth and leather garb, form-fitting, \
                 living green vines worked through her clothes",
                "",
                "MoonDeep design-party.md §1.4 as amended (druid; hair = loose curls \
                 with leaves, §1.4 canon at 05a2b1c; hippie good-vibes register, \
                 05:45). Identity composed from \
                 moondeep-regeneration-20261127/prompts.json.",
            ),
            rec(
                "vesper",
                "Vesper",
                "MoonDeep",
                "1girl, solo, adult woman, mid twenties, beautiful face, athletic \
                 strong-shouldered build, most built of the party but not beefy, \
                 widest hips of the fleet, sparkling amber eyes, glossy luminous \
                 irises, two crisp highlights in each eye, clean eyelash line, dark \
                 hair dressed high in an elaborate formal updo, earnest adoring \
                 expression, cute excited smile",
                "knight, fitted matte steel knight armour, brushed steel, \
                 close-fitting mail and plate, functional form-fitting kit, deep \
                 burgundy sash worn diagonally across her torso over the armour",
                "",
                "MoonDeep design-party.md §1.5 as amended (knight, Owner ruling \
                 2026-11-27 05:32; absurdly strong + sincerely loves cute things, \
                 05:45/05:58). Identity composed from \
                 moondeep-regeneration-20261127/prompts.json.",
            ),
        ]
    }
}

/// A name-keyed view over the seed cast, for tests and the
/// proof's character pick.
pub fn seed_map() -> BTreeMap<String, CharacterRecord> {
    seeds::seed_records()
        .into_iter()
        .map(|r| (r.id.clone(), r))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prima() -> CharacterRecord {
        seed_map().get("prima").expect("prima seed").clone()
    }

    // ---- validation-half tests -------------------------------------

    #[test]
    fn workflow_matches_the_recipe_of_record() {
        let w = build_workflow(
            &prima(),
            "in a library, window light",
            42,
            &RecipeSettings::of_record(),
        )
        .expect("workflow builds");
        assert_eq!(w["1"]["inputs"]["ckpt_name"], CHECKPOINT);
        assert_eq!(w["5"]["inputs"]["steps"], STEPS);
        assert_eq!(w["5"]["inputs"]["sampler_name"], SAMPLER);
        assert_eq!(w["5"]["inputs"]["scheduler"], SCHEDULER);
        assert_eq!(w["5"]["inputs"]["seed"], 42);
        assert_eq!(w["4"]["inputs"]["width"], WIDTH);
        assert_eq!(w["4"]["inputs"]["height"], HEIGHT);
        let pos = w["2"]["inputs"]["text"].as_str().unwrap_or("");
        assert!(pos.starts_with(STYLE_PREFIX));
        assert!(pos.contains("silver-white hair"));
        assert!(pos.contains("in a library, window light"));
    }

    #[test]
    fn workflow_hash_is_deterministic_and_content_sensitive() {
        let a = build_workflow(&prima(), "rain", 7, &RecipeSettings::of_record()).unwrap();
        let b = build_workflow(&prima(), "rain", 7, &RecipeSettings::of_record()).unwrap();
        let c = build_workflow(&prima(), "rain", 8, &RecipeSettings::of_record()).unwrap();
        assert_eq!(workflow_sha256(&a), workflow_sha256(&b));
        assert_ne!(workflow_sha256(&a), workflow_sha256(&c));
        assert_eq!(workflow_sha256(&a).len(), 64);
    }

    #[test]
    fn canonical_bytes_are_key_sorted_at_every_depth() {
        let v = serde_json::json!({"b": {"d": 1, "c": 2}, "a": [3, {"z": 1, "y": 2}]});
        let bytes = canonical_workflow_bytes(&v);
        assert_eq!(
            String::from_utf8(bytes).unwrap(),
            "{\"a\":[3,{\"y\":2,\"z\":1}],\"b\":{\"c\":2,\"d\":1}}"
        );
    }

    #[test]
    fn negative_carries_character_guards() {
        let map = seed_map();
        assert!(compose_negative(map.get("sirena").unwrap()).contains("freckles"));
        assert!(!compose_negative(map.get("lana").unwrap()).contains("freckles"));
        assert!(compose_negative(&prima()).starts_with("lowres"));
    }

    #[test]
    fn store_round_trips_a_record() {
        let dir = std::env::temp_dir().join(format!("wb-test-{}", std::process::id()));
        let store = Store::open(&dir).expect("store opens");
        let mut record = store.load("prima").expect("seeded prima loads");
        assert_eq!(record.name, "Prima");
        store
            .bank_prompt(&mut record, "originate", "in a library")
            .expect("bank prompt");
        let reloaded = store.load("prima").expect("reload");
        assert_eq!(reloaded.prompt_history.len(), 1);
        assert_eq!(reloaded.prompt_history[0].text, "in a library");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn mask_stamps_a_disc_and_clips_at_bounds() {
        let mut mask = MaskCanvas::new(32, 32);
        mask.strokes.push(Stroke {
            points: vec![(16.0, 16.0)],
            radius: 4.0,
        });
        let buf = mask.rasterize();
        assert_eq!(buf[16 * 32 + 16], 255);
        assert_eq!(buf[0], 0);
        assert!(mask.coverage() > 0.0 && mask.coverage() < 0.2);
        // A stroke centred outside the canvas must clip, not panic.
        let mut edge = MaskCanvas::new(8, 8);
        edge.strokes.push(Stroke {
            points: vec![(-3.0, -3.0), (20.0, 20.0)],
            radius: 2.0,
        });
        let buf = edge.rasterize();
        assert_eq!(buf.len(), 64);
        assert!(buf.contains(&255));
    }

    #[test]
    fn iso_timestamp_shape() {
        assert_eq!(iso_from_unix(0), "1970-01-01T00:00:00Z");
        assert_eq!(iso_from_unix(1_735_689_600), "2025-01-01T00:00:00Z");
    }

    #[test]
    fn png_round_trip_and_downscale() {
        let img = RgbaImage {
            width: 4,
            height: 2,
            pixels: (0..32).collect(),
        };
        let bytes = encode_png_rgba(&img).expect("encode");
        let back = decode_png_bytes(&bytes).expect("decode");
        assert_eq!(back, img);
        let big = RgbaImage {
            width: 100,
            height: 50,
            pixels: vec![7u8; 100 * 50 * 4],
        };
        let small = downscale(&big, 40, 40);
        assert_eq!((small.width, small.height), (40, 20));
    }

    // ---- adversarial-half tests ------------------------------------

    #[test]
    fn add_candidate_always_lands_as_candidate() {
        let dir = std::env::temp_dir().join(format!("wb-test-adv-{}", std::process::id()));
        let store = Store::open(&dir).expect("store opens");
        let mut record = store.load("lana").expect("lana loads");
        let src = dir.join("fake.png");
        std::fs::write(&src, b"not a real png, bytes only").unwrap();
        let cand = store
            .add_candidate(
                &mut record,
                CandidateSpec {
                    source_file: src,
                    route: ROUTE_ORIGINATE.to_string(),
                    prompt: "prompt".to_string(),
                    seed: Some(1),
                    config: serde_json::json!({}),
                    canvas: None,
                },
            )
            .expect("candidate registers");
        assert_eq!(cand.class, CLASS_CANDIDATE);
        assert!(cand.admitted_at.is_none());
        // Admission is a separate act, and it is recorded.
        store
            .set_admitted(&mut record, &cand.id, true)
            .expect("admit");
        let reloaded = store.load("lana").expect("reload");
        let stored = reloaded.candidate(&cand.id).expect("stored candidate");
        assert_eq!(stored.class, CLASS_ADMITTED);
        assert!(stored.admitted_at.is_some());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn malformed_record_is_refused_not_overwritten() {
        let dir = std::env::temp_dir().join(format!("wb-test-bad-{}", std::process::id()));
        let store = Store::open(&dir).expect("store opens");
        let path = dir.join("characters").join("prima.json");
        std::fs::write(&path, "{ not json").unwrap();
        assert!(store.load("prima").is_err());
        // The bad file is still there, untouched.
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{ not json");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn recipe_bounds_and_prompt_cap_refuse() {
        let bad = RecipeSettings {
            steps: 500,
            cfg: 6.0,
            width: 832,
            height: 1344,
        };
        assert!(build_workflow(&prima(), "x", 1, &bad).is_err());
        let long = "a".repeat(USER_PROMPT_BYTES_MAX + 1);
        assert!(build_workflow(&prima(), &long, 1, &RecipeSettings::of_record()).is_err());
    }

    #[test]
    fn empty_mask_reports_empty() {
        let mask = MaskCanvas::new(16, 16);
        assert!(mask.is_empty());
        assert_eq!(mask.coverage(), 0.0);
    }

    #[test]
    fn seeds_cover_both_casts_and_cite_sources() {
        let map = seed_map();
        for id in [
            "prima", "lana", "sirena", "solenne", "umbra", "wrenna", "petra", "sable", "tamsin",
            "vesper",
        ] {
            let r = map.get(id).unwrap_or_else(|| panic!("seed {id} missing"));
            assert!(!r.prompt_identity.source.is_empty(), "{id} cites no source");
            assert!(
                !r.prompt_identity.identity.is_empty(),
                "{id} has no identity"
            );
            assert_eq!(r.schema, RECORD_SCHEMA);
        }
        // Freckles are Lana's lock alone (D-1482).
        assert!(map["lana"].prompt_identity.identity.contains("freckles"));
        assert!(!map["prima"].prompt_identity.identity.contains("freckles"));
        assert!(
            map["sirena"]
                .prompt_identity
                .negative_guards
                .contains("freckles")
        );
    }
}
