//! The round brush (SPEC SS4.1) and eraser (SS4.2).
//!
//! The determinism split of SS3.2.4 lives here: a live
//! stroke is a list of input samples; `build_stamp_run`
//! turns samples into stamp placements through the
//! stabilizer and spacing interpolation, and the placements
//! are what the op log records. Replay re-executes
//! placements through `apply_stamps` and never re-runs the
//! stabilizer against live timing. All pixel change is
//! integer arithmetic from the quantized placements.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::color::{Rgba, composite_over, erase_strength, lerp_rgba, mul255};
use crate::error::{PaintError, PaintResult};
use crate::layer::Layer;
use crate::tile::{TILE_BYTES, TILE_PX};

/// Brush diameter ceiling (SPEC SS4.1: size 1-1024 px).
pub const DIAMETER_PX_MAX: u32 = 1024;
/// Stamps in one run: a bounded-work ceiling. A run past
/// this is refused whole, never truncated mid-stroke.
pub const STAMPS_PER_RUN_MAX: usize = 1_000_000;
/// Resident bytes of one stroke-buffer tile (alpha only).
pub const STROKE_TILE_BYTES: usize = 65_536;
/// Default spacing fraction (SPEC SS4.1).
pub const SPACING_FRACTION_DEFAULT: f64 = 0.25;
/// Default stabilizer coefficient (SPEC SS4.1).
pub const STABILIZER_ALPHA_DEFAULT: f64 = 0.35;

/// The stamp falloff table: part of the format spec
/// (SPEC SS4.1). Index `q = (dist^2 * 255) / radius^2`
/// clamped to 0..=255; value is the stamp's alpha
/// multiplier at that fractional distance, the smoothstep
/// profile `1 - (3f^2 - 2f^3)` at `f = sqrt(q / 255)`,
/// quantized round-half-up. A unit test re-derives every
/// entry from the formula; the table, not the formula, is
/// what replay executes.
pub const FALLOFF_LUT: [u8; 256] = [
    255, 252, 249, 247, 244, 241, 239, 236, 234, 231, 229, 227, 224, 222, 220, 217, 215, 213, 211,
    208, 206, 204, 202, 200, 198, 196, 194, 192, 190, 188, 186, 184, 182, 180, 178, 176, 174, 172,
    170, 169, 167, 165, 163, 161, 160, 158, 156, 154, 153, 151, 149, 148, 146, 144, 143, 141, 139,
    138, 136, 135, 133, 132, 130, 129, 127, 126, 124, 123, 121, 120, 118, 117, 116, 114, 113, 111,
    110, 109, 107, 106, 105, 103, 102, 101, 99, 98, 97, 96, 94, 93, 92, 91, 90, 88, 87, 86, 85, 84,
    83, 81, 80, 79, 78, 77, 76, 75, 74, 73, 72, 71, 69, 68, 67, 66, 65, 64, 63, 63, 62, 61, 60, 59,
    58, 57, 56, 55, 54, 53, 52, 52, 51, 50, 49, 48, 47, 46, 46, 45, 44, 43, 42, 42, 41, 40, 39, 39,
    38, 37, 37, 36, 35, 34, 34, 33, 32, 32, 31, 30, 30, 29, 28, 28, 27, 27, 26, 25, 25, 24, 24, 23,
    23, 22, 22, 21, 20, 20, 19, 19, 18, 18, 17, 17, 17, 16, 16, 15, 15, 14, 14, 13, 13, 13, 12, 12,
    11, 11, 11, 10, 10, 10, 9, 9, 9, 8, 8, 8, 7, 7, 7, 6, 6, 6, 6, 5, 5, 5, 5, 4, 4, 4, 4, 3, 3, 3,
    3, 3, 3, 2, 2, 2, 2, 2, 2, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
];

/// One input sample: a pointer position and pressure at a
/// moment in a stroke. Samples are data crossing into the
/// core (SS3.1); the core never reads a device.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StrokeSample {
    /// Canvas-space x, in pixels (fractional).
    pub x_px: f64,
    /// Canvas-space y, in pixels (fractional).
    pub y_px: f64,
    /// Pressure in [0, 1]; mouse input reports 1.0.
    pub pressure: f64,
}

/// Which stroke parameters pressure drives (SPEC SS4.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PressureMap {
    /// Pressure scales the stamp radius.
    pub size: bool,
    /// Pressure scales the stamp alpha.
    pub alpha: bool,
}

impl Default for PressureMap {
    fn default() -> PressureMap {
        PressureMap {
            size: true,
            alpha: false,
        }
    }
}

/// The brush parameters of one stroke. Parameters shape
/// placement *building*; the built run also records the
/// deposit parameters (colour, opacity, flow) it needs at
/// replay, so a run is self-contained.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StrokeParams {
    /// Deposit colour; its alpha multiplies stamp alpha.
    pub color: Rgba,
    /// Brush diameter, 1..=1024 px.
    pub diameter_px: u32,
    /// Stroke opacity ceiling (total deposit cap).
    pub opacity: u8,
    /// Per-stamp deposit cap (build-up rate).
    pub flow: u8,
    /// Spacing as a fraction of radius, in (0, 1].
    pub spacing_fraction: f64,
    /// One-pole stabilizer coefficient, in (0, 1].
    pub stabilizer_alpha: f64,
    /// Pressure mapping toggles.
    pub pressure_map: PressureMap,
}

impl Default for StrokeParams {
    fn default() -> StrokeParams {
        StrokeParams {
            color: [0, 0, 0, 255],
            diameter_px: 24,
            opacity: 255,
            flow: 255,
            spacing_fraction: SPACING_FRACTION_DEFAULT,
            stabilizer_alpha: STABILIZER_ALPHA_DEFAULT,
            pressure_map: PressureMap::default(),
        }
    }
}

/// What a stamp run does to the layer (SPEC SS4.2: the
/// eraser is a mode of the brush, not a tool).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StampMode {
    /// Deposit colour (source-over; alpha-preserving
    /// under lock-alpha).
    Paint,
    /// Write alpha toward zero (destination-out).
    Erase,
}

/// One stamp placement: the recorded outcome of stroke
/// building (SPEC SS3.2.4). Position is 1/64 px fixed
/// point; radius and alpha are integers. Replay executes
/// exactly these numbers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stamp {
    /// Centre x, 1/64 px fixed point.
    pub x_fixed: i64,
    /// Centre y, 1/64 px fixed point.
    pub y_fixed: i64,
    /// Stamp radius in px, 1..=512.
    pub radius_px: u32,
    /// Stamp alpha ceiling before falloff, 0..=255.
    pub alpha: u8,
}

/// Fixed-point units per pixel in stamp positions.
pub const FIXED_PER_PX: i64 = 64;

/// Grain cell-size ceiling in px (the document
/// validates).
pub const GRAIN_CELL_PX_MAX: u32 = 256;
/// Grain octave ceiling (the document validates).
pub const GRAIN_OCTAVES_MAX: u8 = 4;

/// A deterministic grain field modulating stamp deposit
/// (the textured-brush term of SS4.3): value noise in
/// canvas space, so every stamp and run sharing a spec
/// deposits through the same paper. The field is a pure
/// integer function of (seed, position) — no float, no
/// clock, no platform variance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GrainSpec {
    /// Field seed; different seeds are different papers.
    pub seed: u64,
    /// Base cell size in px, 1..=256. Octaves halve it.
    pub cell_px: u32,
    /// Modulation depth, 0..=255: the deposit factor runs
    /// from `255 - strength` (field value 0) to 255
    /// (field value 255). Strength 0 is a plain stamp run.
    pub strength: u8,
    /// Noise octaves, 1..=4; octave 0 carries the most
    /// weight, each later octave half the cell size and
    /// half the weight.
    pub octaves: u8,
    /// Stepped sampling: each cell reads its own hash
    /// value flat, with no bilinear interpolation, so
    /// cell boundaries are step edges — the tooth of a
    /// real brush/paper grain, where the smooth field
    /// reads as mottling. Default false (the smooth
    /// field); old documents carry no field and parse
    /// as smooth.
    #[serde(default)]
    pub stepped: bool,
}

/// One cell corner's hash: a splitmix-style finaliser
/// over the seed and the cell coordinates. Pure integer
/// arithmetic; identical on every machine.
fn grain_hash(seed: u64, octave: u8, cx: i64, cy: i64) -> u32 {
    let mut h = seed ^ u64::from(octave).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    h ^= (cx as u64).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    h = (h ^ (h >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    h ^= (cy as u64).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    h = (h ^ (h >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    h ^= h >> 31;
    (h >> 56) as u32
}

/// One octave of the grain field at a canvas pixel:
/// bilinear interpolation of the four surrounding cell
/// hashes, in exact integer arithmetic.
fn grain_octave(seed: u64, octave: u8, cell: u64, x: i64, y: i64, stepped: bool) -> u32 {
    let c = cell as i64;
    let cx = x.div_euclid(c);
    let cy = y.div_euclid(c);
    if stepped {
        return grain_hash(seed, octave, cx, cy);
    }
    let fx = x.rem_euclid(c) as u64;
    let fy = y.rem_euclid(c) as u64;
    let h00 = u64::from(grain_hash(seed, octave, cx, cy));
    let h10 = u64::from(grain_hash(seed, octave, cx + 1, cy));
    let h01 = u64::from(grain_hash(seed, octave, cx, cy + 1));
    let h11 = u64::from(grain_hash(seed, octave, cx + 1, cy + 1));
    let numerator = h00 * (cell - fx) * (cell - fy)
        + h10 * fx * (cell - fy)
        + h01 * (cell - fx) * fy
        + h11 * fx * fy;
    let denominator = cell * cell;
    ((numerator + denominator / 2) / denominator) as u32
}

/// The grain field's value at a canvas pixel, 0..=255:
/// the weighted mean of its octaves (octave 0 weighs
/// most). An octave count of 0 — refused at the document
/// — reads as full value here, i.e. no modulation.
pub fn grain_value(grain: &GrainSpec, x: i64, y: i64) -> u32 {
    if grain.octaves == 0 {
        return 255;
    }
    let mut weighted = 0u64;
    let mut weights = 0u64;
    for octave in 0..grain.octaves {
        let cell = (u64::from(grain.cell_px) >> octave).max(1);
        let weight = 1u64 << (grain.octaves - 1 - octave);
        weighted += weight * u64::from(grain_octave(grain.seed, octave, cell, x, y, grain.stepped));
        weights += weight;
    }
    ((weighted + weights / 2) / weights) as u32
}

/// Quantize a pixel-space coordinate to stamp fixed point,
/// clamped so hostile sample values cannot overflow the
/// placement arithmetic downstream.
fn quantize_fixed(value_px: f64) -> i64 {
    const CLAMP_PX: f64 = 1_000_000.0;
    let clamped = value_px.clamp(-CLAMP_PX, CLAMP_PX);
    (clamped * FIXED_PER_PX as f64).round() as i64
}

/// The pressure-mapped radius for a sample.
fn radius_for(params: &StrokeParams, pressure: f64) -> u32 {
    let base = f64::from(params.diameter_px) / 2.0;
    let scaled = if params.pressure_map.size {
        base * pressure
    } else {
        base
    };
    (scaled.round() as u32).clamp(1, DIAMETER_PX_MAX / 2)
}

/// The pressure-mapped alpha ceiling for a sample.
fn alpha_for(params: &StrokeParams, pressure: f64) -> u8 {
    if params.pressure_map.alpha {
        (255.0 * pressure).round() as u8
    } else {
        255
    }
}

/// Validate stroke parameters; refuses before any work.
fn validate_params(params: &StrokeParams) -> PaintResult<()> {
    if params.diameter_px == 0 || params.diameter_px > DIAMETER_PX_MAX {
        return Err(PaintError::limit(
            "brush diameter",
            u64::from(params.diameter_px),
            u64::from(DIAMETER_PX_MAX),
        ));
    }
    if !params.spacing_fraction.is_finite()
        || params.spacing_fraction <= 0.0
        || params.spacing_fraction > 1.0
    {
        return Err(PaintError::invalid(format!(
            "spacing fraction {} is outside (0, 1]",
            params.spacing_fraction
        )));
    }
    if !params.stabilizer_alpha.is_finite()
        || params.stabilizer_alpha <= 0.0
        || params.stabilizer_alpha > 1.0
    {
        return Err(PaintError::invalid(format!(
            "stabilizer alpha {} is outside (0, 1]",
            params.stabilizer_alpha
        )));
    }
    Ok(())
}

/// Build a stamp run from input samples (SPEC SS4.1):
/// stabilize positions with the one-pole filter, walk each
/// segment placing stamps every `max(1, radius * spacing)`
/// px of arc length, linearly interpolating position and
/// pressure. Pure: the same samples and parameters always
/// build the same run.
pub fn build_stamp_run(samples: &[StrokeSample], params: &StrokeParams) -> PaintResult<Vec<Stamp>> {
    validate_params(params)?;
    for sample in samples {
        if !sample.x_px.is_finite() || !sample.y_px.is_finite() || !sample.pressure.is_finite() {
            return Err(PaintError::invalid("stroke sample is not finite"));
        }
    }
    if samples.is_empty() {
        return Ok(Vec::new());
    }
    // Stabilize positions (pressure is interpolated raw).
    let alpha = params.stabilizer_alpha;
    let mut smoothed: Vec<(f64, f64)> = Vec::with_capacity(samples.len());
    let (mut sx, mut sy) = (samples[0].x_px, samples[0].y_px);
    smoothed.push((sx, sy));
    for sample in &samples[1..] {
        sx += alpha * (sample.x_px - sx);
        sy += alpha * (sample.y_px - sy);
        smoothed.push((sx, sy));
    }
    let mut stamps: Vec<Stamp> = Vec::new();
    for i in 0..samples.len().saturating_sub(1) {
        let (ax, ay) = smoothed[i];
        let (bx, by) = smoothed[i + 1];
        let pa = samples[i].pressure;
        let pb = samples[i + 1].pressure;
        let dx = bx - ax;
        let dy = by - ay;
        let dist = (dx * dx + dy * dy).sqrt();
        let spacing =
            (f64::from(radius_for(params, pa.clamp(0.0, 1.0))) * params.spacing_fraction).max(1.0);
        let steps = (dist / spacing).ceil() as u64;
        for k in 0..steps {
            let f = k as f64 / steps as f64;
            push_stamp(
                &mut stamps,
                ax + dx * f,
                ay + dy * f,
                pa + (pb - pa) * f,
                params,
            )?;
        }
    }
    if let (Some(last), Some((lx, ly))) = (samples.last(), smoothed.last()) {
        push_stamp(&mut stamps, *lx, *ly, last.pressure, params)?;
    }
    Ok(stamps)
}

/// Push one placement onto a run under construction,
/// enforcing the per-run stamp ceiling.
fn push_stamp(
    stamps: &mut Vec<Stamp>,
    x: f64,
    y: f64,
    pressure: f64,
    params: &StrokeParams,
) -> PaintResult<()> {
    if stamps.len() >= STAMPS_PER_RUN_MAX {
        return Err(PaintError::limit(
            "stamps per run",
            stamps.len() as u64 + 1,
            STAMPS_PER_RUN_MAX as u64,
        ));
    }
    let p = pressure.clamp(0.0, 1.0);
    stamps.push(Stamp {
        x_fixed: quantize_fixed(x),
        y_fixed: quantize_fixed(y),
        radius_px: radius_for(params, p),
        alpha: alpha_for(params, p),
    });
    Ok(())
}

/// The tile keys a stamp run would touch on a canvas of
/// `width` x `height`, sorted. Pre-flight accounting and
/// the editor's dirty-region tracking share this.
pub fn touched_tile_keys(stamps: &[Stamp], width: u32, height: u32) -> Vec<(i32, i32)> {
    let mut keys: Vec<(i32, i32)> = Vec::new();
    for stamp in stamps {
        let cx = stamp.x_fixed.div_euclid(FIXED_PER_PX);
        let cy = stamp.y_fixed.div_euclid(FIXED_PER_PX);
        let r = i64::from(stamp.radius_px);
        let x0 = (cx - r).max(0);
        let y0 = (cy - r).max(0);
        let x1 = (cx + r).min(i64::from(width) - 1);
        let y1 = (cy + r).min(i64::from(height) - 1);
        if x1 < x0 || y1 < y0 {
            continue;
        }
        let tile = i64::from(TILE_PX);
        for ty in (y0 / tile)..=(y1 / tile) {
            for tx in (x0 / tile)..=(x1 / tile) {
                keys.push((tx as i32, ty as i32));
            }
        }
    }
    keys.sort_unstable();
    keys.dedup();
    keys
}

/// Resident bytes a stamp run would add to `layer`:
/// not-yet-allocated layer tiles at the full tile size,
/// plus one stroke-buffer tile per touched tile.
pub fn stamp_run_additional_bytes(layer: &Layer, stamps: &[Stamp], width: u32, height: u32) -> u64 {
    let keys = touched_tile_keys(stamps, width, height);
    let mut additional = (keys.len() as u64) * (STROKE_TILE_BYTES as u64);
    for key in keys {
        if !layer.has_tile(key) {
            additional += TILE_BYTES as u64;
        }
    }
    additional
}

/// One stamp's alpha at pixel offset (`dx`, `dy`) from its
/// centre, through the falloff table; 0 outside the disc.
fn stamp_alpha_at(stamp: &Stamp, dx: i64, dy: i64) -> u8 {
    let r = i64::from(stamp.radius_px);
    let dist2 = dx * dx + dy * dy;
    if dist2 > r * r {
        return 0;
    }
    let q = (dist2 * 255 / (r * r)) as usize;
    mul255(u32::from(stamp.alpha), u32::from(FALLOFF_LUT[q])) as u8
}

/// The deposit side of a stamp run: everything the
/// deposit functions need besides the target layer, the
/// stamps, and the canvas size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StampStyle {
    /// Paint or Erase.
    pub mode: StampMode,
    /// Deposit colour; its alpha multiplies stamp alpha.
    pub color: Rgba,
    /// Stroke opacity ceiling.
    pub opacity: u8,
    /// Per-stamp deposit cap.
    pub flow: u8,
}

/// Apply a stamp run to a layer (SPEC SS4.1/SS4.2).
///
/// Stamps first accumulate into a sparse stroke buffer at
/// `flow` per stamp (source-over on alpha, so overlapping
/// stamps build up only under flow); the buffer then
/// deposits at `opacity` total — the Krita opacity/flow
/// distinction of SS4.1. Paint deposits source-over, or
/// alpha-preserving under lock-alpha; Erase writes alpha
/// toward zero. The caller owns cap pre-flight
/// (`stamp_run_additional_bytes`); this function allocates
/// exactly the tiles that computation names.
pub fn apply_stamps(
    layer: &mut Layer,
    stamps: &[Stamp],
    style: StampStyle,
    width: u32,
    height: u32,
) {
    apply_stamps_grained(layer, stamps, style, None, width, height);
}

/// Apply a stamp run with an optional grain field
/// (SS4.3): identical to `apply_stamps` except each
/// stamp's per-pixel contribution is first scaled by the
/// grain factor at that pixel, `(255 - strength) +
/// strength * field / 255`. With `grain: None` — or a
/// grain of strength 0 — the arithmetic is exactly
/// `apply_stamps`' own, so plain runs never change.
pub fn apply_stamps_grained(
    layer: &mut Layer,
    stamps: &[Stamp],
    style: StampStyle,
    grain: Option<GrainSpec>,
    width: u32,
    height: u32,
) {
    let (opacity, flow) = (style.opacity, style.flow);
    if stamps.is_empty() || opacity == 0 || flow == 0 {
        return;
    }
    // Pass 1: accumulate the stroke buffer.
    let mut buffer: BTreeMap<(i32, i32), Box<[u8; STROKE_TILE_BYTES]>> = BTreeMap::new();
    for stamp in stamps {
        accumulate_stamp(&mut buffer, stamp, flow, grain.as_ref(), width, height);
    }
    // Pass 2: deposit the buffer onto the layer.
    for (key, tile_buf) in &buffer {
        deposit_tile(layer, *key, tile_buf, style, width, height);
    }
}

/// Accumulate one stamp into the stroke buffer.
fn accumulate_stamp(
    buffer: &mut BTreeMap<(i32, i32), Box<[u8; STROKE_TILE_BYTES]>>,
    stamp: &Stamp,
    flow: u8,
    grain: Option<&GrainSpec>,
    width: u32,
    height: u32,
) {
    let cx = stamp.x_fixed.div_euclid(FIXED_PER_PX);
    let cy = stamp.y_fixed.div_euclid(FIXED_PER_PX);
    let r = i64::from(stamp.radius_px);
    let x0 = (cx - r).max(0);
    let y0 = (cy - r).max(0);
    let x1 = (cx + r).min(i64::from(width) - 1);
    let y1 = (cy + r).min(i64::from(height) - 1);
    if x1 < x0 || y1 < y0 {
        return;
    }
    let tile = i64::from(TILE_PX);
    for ty in (y0 / tile)..=(y1 / tile) {
        for tx in (x0 / tile)..=(x1 / tile) {
            let entry = buffer
                .entry((tx as i32, ty as i32))
                .or_insert_with(|| Box::new([0u8; STROKE_TILE_BYTES]));
            let lx0 = (x0 - tx * tile).max(0);
            let ly0 = (y0 - ty * tile).max(0);
            let lx1 = (x1 - tx * tile).min(tile - 1);
            let ly1 = (y1 - ty * tile).min(tile - 1);
            for ly in ly0..=ly1 {
                for lx in lx0..=lx1 {
                    let gx = tx * tile + lx;
                    let gy = ty * tile + ly;
                    let a = stamp_alpha_at(stamp, gx - cx, gy - cy);
                    if a == 0 {
                        continue;
                    }
                    let contribution = mul255(u32::from(a), u32::from(flow)) as u8;
                    let contribution = match grain {
                        Some(spec) => {
                            let factor = 255 - u32::from(spec.strength)
                                + mul255(u32::from(spec.strength), grain_value(spec, gx, gy));
                            mul255(u32::from(contribution), factor) as u8
                        }
                        None => contribution,
                    };
                    let at = (ly * tile + lx) as usize;
                    let current = u32::from(entry[at]);
                    entry[at] = (u32::from(contribution)
                        + mul255(current, 255 - u32::from(contribution)))
                        as u8;
                }
            }
        }
    }
}

/// Deposit one stroke-buffer tile onto the layer.
fn deposit_tile(
    layer: &mut Layer,
    key: (i32, i32),
    tile_buf: &[u8; STROKE_TILE_BYTES],
    style: StampStyle,
    width: u32,
    height: u32,
) {
    let (mode, color, opacity) = (style.mode, style.color, style.opacity);
    let tile = i64::from(TILE_PX);
    let base_x = i64::from(key.0) * tile;
    let base_y = i64::from(key.1) * tile;
    for ly in 0..tile {
        for lx in 0..tile {
            let strength_raw = tile_buf[(ly * tile + lx) as usize];
            if strength_raw == 0 {
                continue;
            }
            let gx = base_x + lx;
            let gy = base_y + ly;
            if gx < 0 || gy < 0 || gx >= i64::from(width) || gy >= i64::from(height) {
                continue;
            }
            let (x, y) = (gx as u32, gy as u32);
            let strength = mul255(u32::from(strength_raw), u32::from(opacity)) as u8;
            let dest = layer.pixel(x, y);
            let out = match mode {
                StampMode::Paint => {
                    if layer.lock_alpha {
                        if dest[3] == 0 {
                            continue;
                        }
                        let mut mixed = lerp_rgba(color, dest, strength);
                        mixed[3] = dest[3];
                        mixed
                    } else {
                        let s = mul255(u32::from(strength), u32::from(color[3])) as u8;
                        composite_over([color[0], color[1], color[2], s], dest)
                    }
                }
                StampMode::Erase => erase_strength(dest, strength),
            };
            if out != dest {
                layer.set_pixel(x, y, out);
            }
        }
    }
}

impl serde::Serialize for StampMode {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(match self {
            StampMode::Paint => "Paint",
            StampMode::Erase => "Erase",
        })
    }
}

impl<'de> serde::Deserialize<'de> for StampMode {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<StampMode, D::Error> {
        struct StampModeVisitor;
        impl serde::de::Visitor<'_> for StampModeVisitor {
            type Value = StampMode;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("a stamp mode string")
            }

            fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<StampMode, E> {
                match value {
                    "Paint" => Ok(StampMode::Paint),
                    "Erase" => Ok(StampMode::Erase),
                    other => Err(E::custom(format!("unknown stamp mode {other:?}"))),
                }
            }
        }
        deserializer.deserialize_str(StampModeVisitor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layer::LayerId;

    #[test]
    fn falloff_lut_matches_its_formula() {
        for (q, entry) in FALLOFF_LUT.iter().enumerate() {
            let f = (q as f64 / 255.0).sqrt();
            let smooth = 1.0 - f * f * (3.0 - 2.0 * f);
            let expected = (255.0 * smooth).round() as u8;
            assert_eq!(*entry, expected, "LUT[{q}]");
        }
        assert_eq!(FALLOFF_LUT[0], 255);
        assert_eq!(FALLOFF_LUT[255], 0);
    }

    fn two_sample_run() -> (Vec<StrokeSample>, StrokeParams) {
        let samples = vec![
            StrokeSample {
                x_px: 10.0,
                y_px: 10.0,
                pressure: 1.0,
            },
            StrokeSample {
                x_px: 30.0,
                y_px: 10.0,
                pressure: 1.0,
            },
        ];
        let params = StrokeParams {
            diameter_px: 20,
            stabilizer_alpha: 1.0,
            ..StrokeParams::default()
        };
        (samples, params)
    }

    #[test]
    fn build_is_pure_and_spaced() {
        let (samples, params) = two_sample_run();
        let a = build_stamp_run(&samples, &params).expect("build");
        let b = build_stamp_run(&samples, &params).expect("build");
        assert_eq!(a, b);
        // Segment length 20, radius 10, spacing 2.5 -> 8
        // interpolated stamps + the final sample's stamp.
        assert_eq!(a.len(), 9);
        assert_eq!(a[0].x_fixed, 10 * 64);
        assert_eq!(a[8].x_fixed, 30 * 64);
        assert!(a.iter().all(|s| s.radius_px == 10));
    }

    #[test]
    fn single_sample_is_one_stamp_and_empty_is_empty() {
        let params = StrokeParams::default();
        let one = build_stamp_run(
            &[StrokeSample {
                x_px: 5.0,
                y_px: 6.0,
                pressure: 0.5,
            }],
            &params,
        )
        .expect("build");
        assert_eq!(one.len(), 1);
        assert_eq!(one[0].x_fixed, 5 * 64);
        assert!(build_stamp_run(&[], &params).expect("build").is_empty());
    }

    #[test]
    fn pressure_mapping_drives_size_and_alpha() {
        let params = StrokeParams {
            diameter_px: 40,
            stabilizer_alpha: 1.0,
            pressure_map: PressureMap {
                size: true,
                alpha: true,
            },
            ..StrokeParams::default()
        };
        let run = build_stamp_run(
            &[StrokeSample {
                x_px: 0.0,
                y_px: 0.0,
                pressure: 0.5,
            }],
            &params,
        )
        .expect("build");
        assert_eq!(run[0].radius_px, 10);
        assert_eq!(run[0].alpha, 128);
    }

    #[test]
    fn stabilizer_pulls_toward_the_first_sample() {
        let params = StrokeParams {
            stabilizer_alpha: 0.5,
            ..StrokeParams::default()
        };
        let samples = vec![
            StrokeSample {
                x_px: 0.0,
                y_px: 0.0,
                pressure: 1.0,
            },
            StrokeSample {
                x_px: 100.0,
                y_px: 0.0,
                pressure: 1.0,
            },
        ];
        let run = build_stamp_run(&samples, &params).expect("build");
        // Smoothed end is x=50, not the raw 100.
        assert_eq!(run.last().expect("stamp").x_fixed, 50 * 64);
    }

    #[test]
    fn paint_then_erase_round_trip_on_a_layer() {
        let mut layer = Layer::new(LayerId(1), "ink".to_string());
        let stamps = vec![Stamp {
            x_fixed: 100 * 64,
            y_fixed: 100 * 64,
            radius_px: 10,
            alpha: 255,
        }];
        apply_stamps(
            &mut layer,
            &stamps,
            StampStyle {
                mode: StampMode::Paint,
                color: [200, 30, 30, 255],
                opacity: 255,
                flow: 255,
            },
            400,
            400,
        );
        let centre = layer.pixel(100, 100);
        assert_eq!(centre, [200, 30, 30, 255]);
        apply_stamps(
            &mut layer,
            &stamps,
            StampStyle {
                mode: StampMode::Erase,
                color: [0, 0, 0, 255],
                opacity: 255,
                flow: 255,
            },
            400,
            400,
        );
        assert_eq!(layer.pixel(100, 100), crate::color::TRANSPARENT);
    }

    #[test]
    fn lock_alpha_paints_colour_but_never_alpha() {
        let mut layer = Layer::new(LayerId(1), "ink".to_string());
        layer.lock_alpha = true;
        let stamps = vec![Stamp {
            x_fixed: 50 * 64,
            y_fixed: 50 * 64,
            radius_px: 8,
            alpha: 255,
        }];
        // Nothing to lock onto yet: deposit is a no-op.
        apply_stamps(
            &mut layer,
            &stamps,
            StampStyle {
                mode: StampMode::Paint,
                color: [10, 200, 10, 255],
                opacity: 255,
                flow: 255,
            },
            200,
            200,
        );
        assert_eq!(layer.tile_count(), 0);
        layer.lock_alpha = false;
        apply_stamps(
            &mut layer,
            &stamps,
            StampStyle {
                mode: StampMode::Paint,
                color: [10, 200, 10, 255],
                opacity: 255,
                flow: 255,
            },
            200,
            200,
        );
        assert_eq!(layer.pixel(50, 50), [10, 200, 10, 255]);
        layer.lock_alpha = true;
        apply_stamps(
            &mut layer,
            &stamps,
            StampStyle {
                mode: StampMode::Paint,
                color: [250, 0, 0, 255],
                opacity: 255,
                flow: 255,
            },
            200,
            200,
        );
        assert_eq!(layer.pixel(50, 50), [250, 0, 0, 255]);
    }

    #[test]
    fn adversarial_inputs_refuse_cleanly() {
        let params = StrokeParams::default();
        let nan = [StrokeSample {
            x_px: f64::NAN,
            y_px: 0.0,
            pressure: 1.0,
        }];
        assert!(build_stamp_run(&nan, &params).is_err());
        let bad_spacing = StrokeParams {
            spacing_fraction: 0.0,
            ..params
        };
        assert!(build_stamp_run(&[], &bad_spacing).is_err());
        let bad_diameter = StrokeParams {
            diameter_px: 0,
            ..params
        };
        assert!(build_stamp_run(&[], &bad_diameter).is_err());
        let huge = StrokeParams {
            diameter_px: DIAMETER_PX_MAX + 1,
            ..params
        };
        assert!(build_stamp_run(&[], &huge).is_err());
    }

    #[test]
    fn grain_field_is_deterministic_bounded_and_varies() {
        let spec = GrainSpec {
            seed: 20261127,
            cell_px: 7,
            strength: 200,
            octaves: 3,
            stepped: false,
        };
        let mut min = 255u32;
        let mut max = 0u32;
        for y in 0..140 {
            for x in 0..140 {
                let v = grain_value(&spec, x, y);
                assert!(v <= 255);
                assert_eq!(v, grain_value(&spec, x, y), "field must be a pure function");
                min = min.min(v);
                max = max.max(v);
            }
        }
        // A field that never varies is not a texture.
        assert!(max - min > 100, "field range {min}..={max} is too flat");
        // A different seed is a different paper.
        let other = GrainSpec { seed: 42, ..spec };
        assert!((0..140).any(|x| grain_value(&spec, x, 70) != grain_value(&other, x, 70)));
    }

    #[test]
    fn stepped_grain_is_flat_within_a_cell_and_steps_at_boundaries() {
        let spec = GrainSpec {
            seed: 20261129,
            cell_px: 6,
            strength: 220,
            octaves: 1,
            stepped: true,
        };
        // Flat within the cell (single octave: the value is
        // the cell's hash, wherever the pixel sits in it).
        let v = grain_value(&spec, 0, 0);
        for x in 0..6 {
            for y in 0..6 {
                assert_eq!(
                    grain_value(&spec, x, y),
                    v,
                    "stepped value varies inside a cell"
                );
            }
        }
        // And it steps somewhere along the next cells —
        // the field is not one flat value.
        assert!((0..60).any(|x| grain_value(&spec, x, 0) != v));
        // The smooth field at the same parameters is NOT
        // flat within the cell (the modes genuinely differ).
        let smooth = GrainSpec {
            stepped: false,
            ..spec
        };
        assert!((1..6).any(|x| grain_value(&smooth, x, 0) != grain_value(&smooth, 0, 0)));
    }

    #[test]
    fn grain_at_strength_zero_is_a_plain_run() {
        let stamps = vec![
            Stamp {
                x_fixed: 60 * 64,
                y_fixed: 60 * 64,
                radius_px: 24,
                alpha: 200,
            },
            Stamp {
                x_fixed: 84 * 64,
                y_fixed: 60 * 64,
                radius_px: 24,
                alpha: 200,
            },
        ];
        let style = StampStyle {
            mode: StampMode::Paint,
            color: [180, 90, 40, 255],
            opacity: 230,
            flow: 190,
        };
        let mut plain = Layer::new(LayerId(1), "ink".to_string());
        apply_stamps(&mut plain, &stamps, style, 160, 160);
        let mut grained = Layer::new(LayerId(1), "ink".to_string());
        apply_stamps_grained(
            &mut grained,
            &stamps,
            style,
            Some(GrainSpec {
                seed: 7,
                cell_px: 5,
                strength: 0,
                octaves: 3,
                stepped: false,
            }),
            160,
            160,
        );
        for y in 0..160 {
            for x in 0..160 {
                assert_eq!(plain.pixel(x, y), grained.pixel(x, y), "pixel ({x}, {y})");
            }
        }
    }

    #[test]
    fn grain_only_ever_thins_a_deposit() {
        let stamps = vec![Stamp {
            x_fixed: 60 * 64,
            y_fixed: 60 * 64,
            radius_px: 30,
            alpha: 255,
        }];
        let style = StampStyle {
            mode: StampMode::Paint,
            color: [200, 200, 200, 255],
            opacity: 255,
            flow: 255,
        };
        let mut plain = Layer::new(LayerId(1), "ink".to_string());
        apply_stamps(&mut plain, &stamps, style, 140, 140);
        let mut grained = Layer::new(LayerId(1), "ink".to_string());
        apply_stamps_grained(
            &mut grained,
            &stamps,
            style,
            Some(GrainSpec {
                seed: 99,
                cell_px: 6,
                strength: 220,
                stepped: false,
                octaves: 2,
            }),
            140,
            140,
        );
        let mut thinned = false;
        for y in 0..140 {
            for x in 0..140 {
                let p = plain.pixel(x, y);
                let g = grained.pixel(x, y);
                assert!(g[3] <= p[3], "grain raised alpha at ({x}, {y})");
                if g != p {
                    thinned = true;
                }
            }
        }
        assert!(thinned, "strength-220 grain changed nothing");
    }

    #[test]
    fn adversarial_off_canvas_stamps_allocate_nothing() {
        let mut layer = Layer::new(LayerId(1), "ink".to_string());
        let stamps = vec![Stamp {
            x_fixed: -500 * 64,
            y_fixed: -500 * 64,
            radius_px: 10,
            alpha: 255,
        }];
        assert_eq!(stamp_run_additional_bytes(&layer, &stamps, 100, 100), 0);
        apply_stamps(
            &mut layer,
            &stamps,
            StampStyle {
                mode: StampMode::Paint,
                color: [1, 2, 3, 255],
                opacity: 255,
                flow: 255,
            },
            100,
            100,
        );
        assert_eq!(layer.tile_count(), 0);
    }
}

#[cfg(test)]
mod adversarial_tests {
    use super::*;
    use crate::layer::{Layer, LayerId};

    #[test]
    fn adversarial_stamp_coordinates_at_extremes_allocate_nothing() {
        let mut layer = Layer::new(LayerId(1), "ink".to_string());
        let stamps = vec![Stamp {
            x_fixed: i64::MAX,
            y_fixed: i64::MIN,
            radius_px: 10,
            alpha: 255,
        }];
        apply_stamps(
            &mut layer,
            &stamps,
            StampStyle {
                mode: StampMode::Paint,
                color: [1, 1, 1, 255],
                opacity: 255,
                flow: 255,
            },
            100,
            100,
        );
        assert_eq!(layer.tile_count(), 0);
    }
}
