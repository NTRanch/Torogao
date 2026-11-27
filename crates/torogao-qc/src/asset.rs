//! B1: the `asset` verb — a declarative project-rules engine
//! over GLB asset files (adapted 2026-11-26 from the
//! BlenderLens `check_asset` pattern, wave-2 Blender review
//! Record 1). The rules live in a JSON file the project owns;
//! the check is a pure function of (asset, rules) and runs
//! identically in a lane, in a gate script, or by hand.
//!
//! Rules file shape (every section optional; a section whose
//! inputs are absent skips its check family, as in `inspect`):
//!
//! ```json
//! {
//!   "palette": [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
//!   "palette_tolerance": 0.05,
//!   "palette_from_textures": false,
//!   "budgets": {
//!     "file_bytes_max": 1000000,
//!     "texture_max_dimension": 512,
//!     "triangles_max": 1000
//!   },
//!   "anchor": [0.0, 0.0, 0.0],
//!   "anchor_tolerance": 0.001,
//!   "size_envelope": {"min": [-1, -1, -1], "max": [1, 1, 1]}
//! }
//! ```
//!
//! No hidden defaults: a declared `palette` requires a
//! declared `palette_tolerance`, and a declared `anchor`
//! requires `anchor_tolerance` — a missing tolerance is a
//! shape error, not a silent default. Unknown keys anywhere
//! are shape errors, so a typo'd rule fails closed instead of
//! silently not applying.
//!
//! Findings carry the asset's path (as invoked) in `scene`,
//! the material/texture/node it belongs to in `element`, and
//! an RFC 6901 pointer to the rule that fired in `spec_ref`.
//! All three codes are fail-severity: a planted conformance
//! defect fails the gate (the upstream 0/1/2 verdict maps onto
//! the report's pass/fail exactly).

use crate::error::QcError;
use crate::glb::{AabbOutcome, GlbModel, vec3_of};
use crate::limits::{RULES_BYTES_MAX, TEXTURE_DOMINANT_FRACTION, TEXTURE_SAMPLE_EDGE_MAX};
use crate::read_bounded;
use crate::report::{Finding, Report, Severity, round6};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

/// The validated rules a run evaluates.
#[derive(Debug, Clone, PartialEq)]
pub struct Rules {
    palette: Option<Vec<[f64; 3]>>,
    palette_tolerance: Option<f64>,
    palette_from_textures: bool,
    file_bytes_max: Option<u64>,
    texture_max_dimension: Option<u64>,
    triangles_max: Option<u64>,
    anchor: Option<[f64; 3]>,
    anchor_tolerance: Option<f64>,
    size_envelope: Option<([f64; 3], [f64; 3])>,
}

/// Evaluates one rules file over one or more GLB assets and
/// returns the run's report.
pub fn check_assets(rules_path: &Path, asset_paths: &[PathBuf]) -> Result<Report, QcError> {
    let rules_bytes = read_bounded(rules_path, RULES_BYTES_MAX, "RULES")?;
    let rules_sha256 = format!("{:x}", Sha256::digest(&rules_bytes));
    let rules = parse_rules(&rules_bytes)?;
    let scene = rules_path
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_else(|| "asset".to_string());
    let mut report = Report::new("asset", scene);
    report.extra.push(("rules_sha256", json!(rules_sha256)));
    report
        .extra
        .push(("assets_checked", json!(asset_paths.len())));
    let mut textures_sampled = 0usize;
    let mut textures_undecodable = 0usize;
    for asset_path in asset_paths {
        let label = asset_label(asset_path);
        let model = GlbModel::load(asset_path)?;
        check_palette(
            &model,
            &rules,
            &label,
            &mut report,
            &mut textures_sampled,
            &mut textures_undecodable,
        );
        check_budgets(&model, &rules, &label, &mut report);
        check_origin(&model, &rules, &label, &mut report);
    }
    if rules.palette_from_textures {
        report
            .extra
            .push(("textures_sampled", json!(textures_sampled)));
        report
            .extra
            .push(("textures_undecodable", json!(textures_undecodable)));
    }
    report.sort_findings();
    Ok(report)
}

/// The finding's scene label for an asset: the path as invoked
/// when relative (the gate's normal form), else the file name
/// — reports never carry absolute paths.
fn asset_label(path: &Path) -> String {
    if path.is_relative() {
        return path.display().to_string();
    }
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

fn finding(
    code: &'static str,
    label: &str,
    element: Option<String>,
    detail: String,
    spec_ref: &str,
) -> Finding {
    Finding {
        code,
        severity: Severity::Fail,
        element,
        frame: None,
        detail,
        spec_ref: spec_ref.to_string(),
        data: None,
        scene_override: Some(label.to_string()),
    }
}

/// Palette conformance: every material base colour, and
/// (optionally) every dominant colour sampled from embedded
/// textures, within tolerance of a palette entry.
fn check_palette(
    model: &GlbModel,
    rules: &Rules,
    label: &str,
    report: &mut Report,
    textures_sampled: &mut usize,
    textures_undecodable: &mut usize,
) {
    let (Some(palette), Some(tolerance)) = (&rules.palette, rules.palette_tolerance) else {
        return;
    };
    for (index, material) in model.materials().iter().enumerate() {
        let colour = [
            material.base_color[0],
            material.base_color[1],
            material.base_color[2],
        ];
        let (entry, distance) = nearest(palette, &colour);
        if distance > tolerance {
            report.push(finding(
                "QC_ASSET_PALETTE",
                label,
                Some(material.name.clone().unwrap_or_else(|| format!("#{index}"))),
                format!(
                    "material base colour {} is {} from the nearest palette entry {} ({}); tolerance {}",
                    fmt_colour(&colour),
                    num(distance),
                    entry,
                    fmt_colour(&palette[entry]),
                    num(tolerance),
                ),
                "/palette",
            ));
        }
    }
    if rules.palette_from_textures {
        for index in 0..model.images().len() {
            let Some(bytes) = model.image_bytes(index) else {
                *textures_undecodable += 1;
                continue;
            };
            match dominant_colours(bytes) {
                Some(colours) => {
                    *textures_sampled += 1;
                    for colour in &colours {
                        let (entry, distance) = nearest(palette, colour);
                        if distance > tolerance {
                            report.push(finding(
                                "QC_ASSET_PALETTE",
                                label,
                                Some(format!("texture {index}")),
                                format!(
                                    "dominant texture colour {} is {} from the nearest palette entry {} ({}); tolerance {}",
                                    fmt_colour(colour),
                                    num(distance),
                                    entry,
                                    fmt_colour(&palette[entry]),
                                    num(tolerance),
                                ),
                                "/palette",
                            ));
                        }
                    }
                }
                None => *textures_undecodable += 1,
            }
        }
    }
}

/// Budgets: file size, embedded-texture max dimension, and
/// triangle count against their declared ceilings.
fn check_budgets(model: &GlbModel, rules: &Rules, label: &str, report: &mut Report) {
    if let Some(max) = rules.file_bytes_max {
        let size = model.byte_len() as u64;
        if size > max {
            report.push(finding(
                "QC_ASSET_BUDGET",
                label,
                None,
                format!("file is {size} bytes, over the file_bytes_max budget of {max}"),
                "/budgets/file_bytes_max",
            ));
        }
    }
    if let Some(max) = rules.texture_max_dimension {
        for (index, image) in model.images().iter().enumerate() {
            let dimensions = model.image_bytes(index).and_then(texture_dimensions);
            match dimensions {
                Some((width, height)) => {
                    let largest = width.max(height) as u64;
                    if largest > max {
                        report.push(finding(
                            "QC_ASSET_BUDGET",
                            label,
                            Some(format!("texture {index}")),
                            format!(
                                "texture {index} is {width}x{height}, over the texture_max_dimension budget of {max}"
                            ),
                            "/budgets/texture_max_dimension",
                        ));
                    }
                }
                None => {
                    let kind = if image.uri.is_some() {
                        "external (uri) image"
                    } else {
                        "undecodable image"
                    };
                    report.push(finding(
                        "QC_ASSET_BUDGET",
                        label,
                        Some(format!("texture {index}")),
                        format!(
                            "texture {index} is a {kind}; its dimensions cannot be measured against the texture_max_dimension budget of {max}"
                        ),
                        "/budgets/texture_max_dimension",
                    ));
                }
            }
        }
    }
    if let Some(max) = rules.triangles_max {
        let triangles = model.triangle_count();
        if triangles > max {
            report.push(finding(
                "QC_ASSET_BUDGET",
                label,
                None,
                format!(
                    "asset carries {triangles} triangles, over the triangles_max budget of {max}"
                ),
                "/budgets/triangles_max",
            ));
        }
    }
}

/// Anchor and envelope: every scene root's world translation
/// within tolerance of the declared anchor, and the asset's
/// world-space bounding box inside the declared envelope.
fn check_origin(model: &GlbModel, rules: &Rules, label: &str, report: &mut Report) {
    if let (Some(anchor), Some(tolerance)) = (rules.anchor, rules.anchor_tolerance) {
        let nodes = model.nodes();
        for root in model.scene_roots() {
            let Some(node) = nodes.get(root) else {
                continue;
            };
            let delta = [
                node.translation[0] - anchor[0],
                node.translation[1] - anchor[1],
                node.translation[2] - anchor[2],
            ];
            let distance = (delta[0] * delta[0] + delta[1] * delta[1] + delta[2] * delta[2]).sqrt();
            if distance > tolerance {
                report.push(finding(
                    "QC_ASSET_ORIGIN",
                    label,
                    Some(node.name.clone().unwrap_or_else(|| format!("#{root}"))),
                    format!(
                        "root node translation {} is {} from the declared anchor {}; tolerance {}",
                        fmt_colour(&node.translation),
                        num(distance),
                        fmt_colour(&anchor),
                        num(tolerance),
                    ),
                    "/anchor",
                ));
            }
        }
    }
    if let Some((env_min, env_max)) = rules.size_envelope {
        match model.world_aabb() {
            AabbOutcome::Measured(lo, hi) => {
                let mut violations = Vec::new();
                for axis in 0..3 {
                    if lo[axis] < env_min[axis] || hi[axis] > env_max[axis] {
                        violations.push(format!(
                            "axis {axis}: measured [{}, {}], envelope [{}, {}]",
                            num(lo[axis]),
                            num(hi[axis]),
                            num(env_min[axis]),
                            num(env_max[axis]),
                        ));
                    }
                }
                if !violations.is_empty() {
                    report.push(finding(
                        "QC_ASSET_ORIGIN",
                        label,
                        None,
                        format!(
                            "world bounding box leaves the declared size envelope: {}",
                            violations.join("; ")
                        ),
                        "/size_envelope",
                    ));
                }
            }
            AabbOutcome::TooDeep => {
                report.push(finding(
                    "QC_ASSET_ORIGIN",
                    label,
                    None,
                    format!(
                        "world bounding box could not be measured: a node chain exceeds the {}-deep bound",
                        crate::limits::NODE_DEPTH_MAX,
                    ),
                    "/size_envelope",
                ));
            }
            AabbOutcome::NoMeshes => {}
        }
    }
}

/// The dominant colours of an embedded texture: the image is
/// decoded, reduced to at most [`TEXTURE_SAMPLE_EDGE_MAX`]
/// pixels on its long edge, bucketed into a 4x4x4 RGB grid,
/// and every bucket holding at least
/// [`TEXTURE_DOMINANT_FRACTION`] of the opaque-enough pixels
/// contributes its centre colour. `None` when the bytes do
/// not decode as a supported image.
fn dominant_colours(bytes: &[u8]) -> Option<Vec<[f64; 3]>> {
    let decoded = image::load_from_memory(bytes).ok()?;
    let rgba = decoded.to_rgba8();
    let (width, height) = rgba.dimensions();
    let long_edge = width.max(height);
    let sampled = if long_edge > TEXTURE_SAMPLE_EDGE_MAX {
        let scale = TEXTURE_SAMPLE_EDGE_MAX as f64 / long_edge as f64;
        let new_width = ((width as f64 * scale).round() as u32).max(1);
        let new_height = ((height as f64 * scale).round() as u32).max(1);
        image::imageops::resize(
            &rgba,
            new_width,
            new_height,
            image::imageops::FilterType::Nearest,
        )
    } else {
        rgba
    };
    let mut bins = [0u64; 64];
    let mut total = 0u64;
    for pixel in sampled.pixels() {
        if pixel[3] < 128 {
            continue;
        }
        let index = ((pixel[0] >> 6) as usize) * 16
            + ((pixel[1] >> 6) as usize) * 4
            + (pixel[2] >> 6) as usize;
        bins[index] += 1;
        total += 1;
    }
    if total == 0 {
        return Some(Vec::new());
    }
    let mut colours = Vec::new();
    for (index, count) in bins.iter().enumerate() {
        if *count as f64 >= total as f64 * TEXTURE_DOMINANT_FRACTION {
            colours.push([
                ((index / 16) * 64 + 32) as f64 / 255.0,
                (((index / 4) % 4) * 64 + 32) as f64 / 255.0,
                ((index % 4) * 64 + 32) as f64 / 255.0,
            ]);
        }
    }
    Some(colours)
}

/// An image's pixel dimensions, read from its header.
fn texture_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    let reader = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .ok()?;
    reader.into_dimensions().ok()
}

/// Max-channel distance between two RGB colours.
fn colour_distance(a: &[f64; 3], b: &[f64; 3]) -> f64 {
    (0..3)
        .map(|axis| (a[axis] - b[axis]).abs())
        .fold(0.0, f64::max)
}

/// The palette entry nearest a colour, and its distance.
fn nearest(palette: &[[f64; 3]], colour: &[f64; 3]) -> (usize, f64) {
    let mut best = (0usize, f64::INFINITY);
    for (index, entry) in palette.iter().enumerate() {
        let distance = colour_distance(colour, entry);
        if distance < best.1 {
            best = (index, distance);
        }
    }
    best
}

/// A float for a report detail: rounded to 6 places, printed
/// without trailing noise.
fn num(value: f64) -> String {
    format!("{}", round6(value))
}

/// An RGB triple for a report detail.
fn fmt_colour(colour: &[f64; 3]) -> String {
    format!(
        "({}, {}, {})",
        num(colour[0]),
        num(colour[1]),
        num(colour[2])
    )
}

/// Parses and validates a rules document. Parse failures are
/// `QC-E-RULES-PARSE`; wrong shapes, unknown keys, undeclared
/// tolerances, and out-of-range values are `QC-E-RULES-SHAPE`.
fn parse_rules(bytes: &[u8]) -> Result<Rules, QcError> {
    let doc: Value = serde_json::from_slice(bytes).map_err(|err| {
        QcError::new(
            "QC-E-RULES-PARSE",
            format!("rules file is not valid JSON: {err}"),
        )
    })?;
    let root = doc
        .as_object()
        .ok_or_else(|| QcError::new("QC-E-RULES-SHAPE", "rules file must be a JSON object"))?;
    for key in root.keys() {
        if !matches!(
            key.as_str(),
            "palette"
                | "palette_tolerance"
                | "palette_from_textures"
                | "budgets"
                | "anchor"
                | "anchor_tolerance"
                | "size_envelope"
        ) {
            return Err(shape(format!("unknown rules key {key:?}")));
        }
    }
    let palette = match root.get("palette") {
        Some(value) => {
            let entries = value.as_array().ok_or_else(|| {
                shape("palette must be an array of [r, g, b] triples".to_string())
            })?;
            if entries.is_empty() {
                return Err(shape("palette must name at least one entry".to_string()));
            }
            let mut parsed = Vec::new();
            for entry in entries {
                let colour = vec3_of(entry).ok_or_else(|| {
                    shape("palette entries must be [r, g, b] triples".to_string())
                })?;
                if colour.iter().any(|c| !(0.0..=1.0).contains(c)) {
                    return Err(shape(
                        "palette entries must lie within 0.0..=1.0".to_string(),
                    ));
                }
                parsed.push(colour);
            }
            Some(parsed)
        }
        None => None,
    };
    let palette_from_textures = match root.get("palette_from_textures") {
        Some(value) => value
            .as_bool()
            .ok_or_else(|| shape("palette_from_textures must be a boolean".to_string()))?,
        None => false,
    };
    let palette_tolerance = match root.get("palette_tolerance") {
        Some(value) => Some(non_negative(value, "palette_tolerance")?),
        None => None,
    };
    if palette.is_some() && palette_tolerance.is_none() {
        return Err(shape(
            "palette is declared without palette_tolerance; there is no default tolerance"
                .to_string(),
        ));
    }
    if palette_from_textures && palette_tolerance.is_none() {
        return Err(shape(
            "palette_from_textures is declared without palette_tolerance".to_string(),
        ));
    }
    let mut file_bytes_max = None;
    let mut texture_max_dimension = None;
    let mut triangles_max = None;
    if let Some(budgets) = root.get("budgets") {
        let budgets = budgets
            .as_object()
            .ok_or_else(|| shape("budgets must be an object".to_string()))?;
        for (key, value) in budgets {
            let slot = match key.as_str() {
                "file_bytes_max" => &mut file_bytes_max,
                "texture_max_dimension" => &mut texture_max_dimension,
                "triangles_max" => &mut triangles_max,
                _ => return Err(shape(format!("unknown budgets key {key:?}"))),
            };
            *slot =
                Some(value.as_u64().ok_or_else(|| {
                    shape(format!("budgets.{key} must be a non-negative integer"))
                })?);
        }
    }
    let anchor = match root.get("anchor") {
        Some(value) => Some(
            vec3_of(value)
                .ok_or_else(|| shape("anchor must be an [x, y, z] triple".to_string()))?,
        ),
        None => None,
    };
    let anchor_tolerance = match root.get("anchor_tolerance") {
        Some(value) => Some(non_negative(value, "anchor_tolerance")?),
        None => None,
    };
    if anchor.is_some() && anchor_tolerance.is_none() {
        return Err(shape(
            "anchor is declared without anchor_tolerance; there is no default tolerance"
                .to_string(),
        ));
    }
    let size_envelope = match root.get("size_envelope") {
        Some(value) => {
            let envelope = value
                .as_object()
                .ok_or_else(|| shape("size_envelope must be an object".to_string()))?;
            let min = envelope.get("min").and_then(vec3_of).ok_or_else(|| {
                shape("size_envelope.min must be an [x, y, z] triple".to_string())
            })?;
            let max = envelope.get("max").and_then(vec3_of).ok_or_else(|| {
                shape("size_envelope.max must be an [x, y, z] triple".to_string())
            })?;
            if (0..3).any(|axis| min[axis] > max[axis]) {
                return Err(shape(
                    "size_envelope.min must not exceed size_envelope.max on any axis".to_string(),
                ));
            }
            Some((min, max))
        }
        None => None,
    };
    Ok(Rules {
        palette,
        palette_tolerance,
        palette_from_textures,
        file_bytes_max,
        texture_max_dimension,
        triangles_max,
        anchor,
        anchor_tolerance,
        size_envelope,
    })
}

/// A finite, non-negative number field.
fn non_negative(value: &Value, field: &str) -> Result<f64, QcError> {
    let number = value
        .as_f64()
        .filter(|n| n.is_finite() && *n >= 0.0)
        .ok_or_else(|| shape(format!("{field} must be a finite number >= 0")))?;
    Ok(number)
}

fn shape(message: String) -> QcError {
    QcError::new("QC-E-RULES-SHAPE", message)
}
