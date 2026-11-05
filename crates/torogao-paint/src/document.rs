//! The document (SPEC SS3.1/SS3.3): a base raster, a
//! layer stack, and the op log that is their source of
//! truth.
//!
//! Invariant, maintained by construction: the layer stack
//! always equals the replay of `ops[..cursor]` over the
//! base. Mutations validate completely (bounds, existence,
//! memory-cap pre-flight) before touching anything, so a
//! refused op leaves the document bit-identical. Undo and
//! redo move the cursor and re-render by log-walking;
//! applying a new op with the cursor mid-log truncates
//! the redo tail and counts the truncated ops as
//! provenance (SS3.3).

use sha2::{Digest, Sha256};

use crate::color::{Rgba, TRANSPARENT, composite_blend, composite_over, lerp_rgba, mul255};
use crate::error::{PaintError, PaintResult};
use crate::layer::{Layer, LayerId};
use crate::ops::{LayerPropsChange, Op};
use crate::stroke::{STAMPS_PER_RUN_MAX, stamp_run_additional_bytes};
use crate::tile::TILE_PX;

/// Canvas edge ceiling in px per axis (SPEC SS3.1).
pub const CANVAS_DIM_MAX: u32 = 8192;
/// Layer count ceiling (SPEC SS3.1).
pub const LAYERS_MAX: usize = 64;
/// Op-log length ceiling: bounds replay work and the
/// on-disk log; a session past it must save and continue
/// in a fresh document rather than grow without limit.
pub const OPS_MAX: usize = 100_000;
/// Resident-memory cap in bytes (DP-4: 512 MiB).
pub const MEMORY_CAP_BYTES: u64 = 512 * 1024 * 1024;

/// Where the document's pixels start from.
#[derive(Clone)]
pub enum BaseRaster {
    /// An original drawing: no base raster; the canvas
    /// starts transparent.
    Empty,
    /// A base raster (a generated plate, an imported PNG):
    /// the log applies over it. Named by its SHA-256.
    Raster {
        /// SHA-256 of `pixels` (RGBA8, row-major).
        sha256: [u8; 32],
        /// The base pixels, `width * height * 4` bytes.
        pixels: Vec<u8>,
    },
}

impl BaseRaster {
    /// The base's hash, when it is a raster.
    pub fn sha256(&self) -> Option<[u8; 32]> {
        match self {
            BaseRaster::Empty => None,
            BaseRaster::Raster { sha256, .. } => Some(*sha256),
        }
    }

    /// Resident bytes the base occupies.
    pub fn resident_bytes(&self) -> u64 {
        match self {
            BaseRaster::Empty => 0,
            BaseRaster::Raster { pixels, .. } => pixels.len() as u64,
        }
    }

    /// One base pixel.
    pub fn pixel(&self, x: u32, y: u32, width: u32) -> Rgba {
        match self {
            BaseRaster::Empty => TRANSPARENT,
            BaseRaster::Raster { pixels, .. } => {
                let at = ((y * width + x) * 4) as usize;
                [pixels[at], pixels[at + 1], pixels[at + 2], pixels[at + 3]]
            }
        }
    }
}

/// A provenance source record (SPEC SS3.5): a raster the
/// document derives from, named by path-at-open and hash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceRef {
    /// The path the raster was read from.
    pub path: String,
    /// SHA-256 of the raster's file bytes.
    pub sha256: [u8; 32],
}

/// The paint document. See the module docs for the
/// invariant this type maintains.
pub struct Document {
    width: u32,
    height: u32,
    base: BaseRaster,
    layers: Vec<Layer>,
    next_layer_id: u32,
    ops: Vec<Op>,
    cursor: usize,
    truncated_ops: u32,
    sources: Vec<SourceRef>,
    palette_source: Option<SourceRef>,
    cap_bytes: u64,
}

impl Document {
    /// A new document over an empty (transparent) base.
    pub fn new_empty(width: u32, height: u32) -> PaintResult<Document> {
        Document::build(width, height, BaseRaster::Empty, MEMORY_CAP_BYTES)
    }

    /// A new document over an empty base with a caller-set
    /// memory cap (the policy knob tests and constrained
    /// hosts use; the default is `MEMORY_CAP_BYTES`).
    pub fn new_empty_with_cap(width: u32, height: u32, cap_bytes: u64) -> PaintResult<Document> {
        Document::build(width, height, BaseRaster::Empty, cap_bytes)
    }

    /// A new document whose base is a PNG's pixels. The
    /// PNG's file bytes are recorded as a provenance
    /// source under `source_path`.
    pub fn from_png_bytes(png_bytes: &[u8], source_path: &str) -> PaintResult<Document> {
        let (width, height, pixels) = crate::png_io::decode_png_rgba(png_bytes)?;
        let file_hash = crate::hash::sha256_bytes(png_bytes);
        let pixel_hash = crate::hash::sha256_bytes(&pixels);
        let mut doc = Document::build(
            width,
            height,
            BaseRaster::Raster {
                sha256: pixel_hash,
                pixels,
            },
            MEMORY_CAP_BYTES,
        )?;
        doc.sources.push(SourceRef {
            path: source_path.to_string(),
            sha256: file_hash,
        });
        Ok(doc)
    }

    /// Rebuild a document from its parts, replaying `ops`
    /// with full validation (the loader's constructor).
    pub fn from_parts(
        width: u32,
        height: u32,
        base: BaseRaster,
        ops: Vec<Op>,
        truncated_ops: u32,
        sources: Vec<SourceRef>,
        palette_source: Option<SourceRef>,
    ) -> PaintResult<Document> {
        let mut doc = Document::build(width, height, base, MEMORY_CAP_BYTES)?;
        doc.truncated_ops = truncated_ops;
        doc.sources = sources;
        doc.palette_source = palette_source;
        for op in ops {
            doc.apply_op(op)?;
        }
        Ok(doc)
    }

    fn build(width: u32, height: u32, base: BaseRaster, cap_bytes: u64) -> PaintResult<Document> {
        if width == 0 || width > CANVAS_DIM_MAX {
            return Err(PaintError::limit(
                "canvas width",
                u64::from(width),
                u64::from(CANVAS_DIM_MAX),
            ));
        }
        if height == 0 || height > CANVAS_DIM_MAX {
            return Err(PaintError::limit(
                "canvas height",
                u64::from(height),
                u64::from(CANVAS_DIM_MAX),
            ));
        }
        if let BaseRaster::Raster { pixels, .. } = &base {
            let expected = (width as usize) * (height as usize) * 4;
            if pixels.len() != expected {
                return Err(PaintError::format(format!(
                    "base raster has {} bytes, canvas needs {expected}",
                    pixels.len()
                )));
            }
        }
        if base.resident_bytes() > cap_bytes {
            return Err(PaintError::MemoryCap {
                cap_bytes,
                resident_bytes: 0,
                additional_bytes: base.resident_bytes(),
            });
        }
        Ok(Document {
            width,
            height,
            base,
            layers: Vec::new(),
            next_layer_id: 1,
            ops: Vec::new(),
            cursor: 0,
            truncated_ops: 0,
            sources: Vec::new(),
            palette_source: None,
            cap_bytes,
        })
    }

    /// Canvas width in px.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Canvas height in px.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// The base raster.
    pub fn base(&self) -> &BaseRaster {
        &self.base
    }

    /// The layer stack, bottom first.
    pub fn layers(&self) -> &[Layer] {
        &self.layers
    }

    /// One layer by id.
    pub fn layer(&self, id: LayerId) -> Option<&Layer> {
        self.layers.iter().find(|layer| layer.id == id)
    }

    /// The full op log (including any redo tail).
    pub fn ops(&self) -> &[Op] {
        &self.ops
    }

    /// How many ops are currently applied (the undo cursor).
    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// True when there is an applied op to undo.
    pub fn can_undo(&self) -> bool {
        self.cursor > 0
    }

    /// True when there is an undone op to redo.
    pub fn can_redo(&self) -> bool {
        self.cursor < self.ops.len()
    }

    /// Ops dropped by redo-tail truncation, cumulative
    /// (provenance, SPEC SS3.3).
    pub fn truncated_ops(&self) -> u32 {
        self.truncated_ops
    }

    /// Provenance sources recorded for this document.
    pub fn sources(&self) -> &[SourceRef] {
        &self.sources
    }

    /// Record a provenance source (deduplicated by hash).
    pub fn add_source(&mut self, source: SourceRef) {
        if !self.sources.iter().any(|held| held.sha256 == source.sha256) {
            self.sources.push(source);
        }
    }

    /// The palette record this document was painted
    /// against, if any (SPEC SS4.4).
    pub fn palette_source(&self) -> Option<&SourceRef> {
        self.palette_source.as_ref()
    }

    /// Record the palette source.
    pub fn set_palette_source(&mut self, source: Option<SourceRef>) {
        self.palette_source = source;
    }

    /// The memory cap in force for this document.
    pub fn cap_bytes(&self) -> u64 {
        self.cap_bytes
    }

    /// Resident bytes now: base raster + layer tiles. The
    /// op log lives on disk and is not counted (SS3.1);
    /// undo keeps no snapshot cache in this design, so
    /// history costs no resident bytes either.
    pub fn resident_bytes(&self) -> u64 {
        self.base.resident_bytes() + self.layers.iter().map(Layer::resident_bytes).sum::<u64>()
    }

    /// The id the next `add_layer` will issue.
    pub fn next_layer_id(&self) -> LayerId {
        LayerId(self.next_layer_id)
    }

    /// Add a layer on top of the stack (an op).
    pub fn add_layer(&mut self, name: String) -> PaintResult<LayerId> {
        let id = LayerId(self.next_layer_id);
        self.apply_op(Op::LayerAdd {
            id,
            name,
            after: None,
        })?;
        Ok(id)
    }

    /// Apply one op: validate fully, pre-flight the memory
    /// cap, truncate any redo tail, then mutate and record.
    /// A refused op leaves the document bit-identical.
    pub fn apply_op(&mut self, op: Op) -> PaintResult<()> {
        if self.ops.len() >= OPS_MAX {
            return Err(PaintError::limit(
                "op log length",
                self.ops.len() as u64 + 1,
                OPS_MAX as u64,
            ));
        }
        self.validate_op(&op)?;
        let additional = self.additional_bytes(&op)?;
        let resident = self.resident_bytes();
        if resident + additional > self.cap_bytes {
            return Err(PaintError::MemoryCap {
                cap_bytes: self.cap_bytes,
                resident_bytes: resident,
                additional_bytes: additional,
            });
        }
        self.truncate_redo_tail();
        apply_to_layers(
            &mut self.layers,
            &mut self.next_layer_id,
            &op,
            self.width,
            self.height,
        );
        self.ops.push(op);
        self.cursor += 1;
        Ok(())
    }

    /// Drop the redo tail, if any, counting the dropped
    /// ops as provenance (SS3.3). Saving does this too:
    /// a saved document never carries a redo tail.
    pub fn truncate_redo_tail(&mut self) {
        if self.cursor < self.ops.len() {
            let dropped = (self.ops.len() - self.cursor) as u32;
            self.truncated_ops = self.truncated_ops.saturating_add(dropped);
            self.ops.truncate(self.cursor);
        }
    }

    /// Move the undo cursor back one op and re-render by
    /// log-walking. False when there is nothing to undo.
    pub fn undo(&mut self) -> bool {
        if self.cursor == 0 {
            return false;
        }
        self.cursor -= 1;
        self.rebuild_layers();
        true
    }

    /// Move the undo cursor forward one op and re-render.
    /// False when there is nothing to redo.
    pub fn redo(&mut self) -> bool {
        if self.cursor >= self.ops.len() {
            return false;
        }
        self.cursor += 1;
        self.rebuild_layers();
        true
    }

    /// Re-derive the layer stack and the id counter from
    /// base + ops[..cursor] (the log-walk behind undo and
    /// redo; the module-docs invariant).
    fn rebuild_layers(&mut self) {
        let mut layers: Vec<Layer> = Vec::new();
        let mut next_id = 1u32;
        for op in &self.ops[..self.cursor] {
            apply_to_layers(&mut layers, &mut next_id, op, self.width, self.height);
        }
        self.layers = layers;
        self.next_layer_id = next_id;
    }

    /// Validate an op against the current stack. Every
    /// refusal path lives here, ahead of any mutation.
    fn validate_op(&self, op: &Op) -> PaintResult<()> {
        match op {
            Op::StampRun { layer, stamps, .. } => {
                self.require_layer(*layer)?;
                if stamps.is_empty() {
                    return Err(PaintError::invalid("stamp run carries no stamps"));
                }
                if stamps.len() > STAMPS_PER_RUN_MAX {
                    return Err(PaintError::limit(
                        "stamps per run",
                        stamps.len() as u64,
                        STAMPS_PER_RUN_MAX as u64,
                    ));
                }
                for stamp in stamps {
                    if stamp.radius_px == 0 || stamp.radius_px > crate::stroke::DIAMETER_PX_MAX / 2
                    {
                        return Err(PaintError::invalid(format!(
                            "stamp radius {} is outside 1..={}",
                            stamp.radius_px,
                            crate::stroke::DIAMETER_PX_MAX / 2
                        )));
                    }
                }
            }
            Op::FillRect { layer, rect, .. } => {
                self.require_layer(*layer)?;
                if rect.width == 0 || rect.width > CANVAS_DIM_MAX {
                    return Err(PaintError::limit(
                        "fill rect width",
                        u64::from(rect.width),
                        u64::from(CANVAS_DIM_MAX),
                    ));
                }
                if rect.height == 0 || rect.height > CANVAS_DIM_MAX {
                    return Err(PaintError::limit(
                        "fill rect height",
                        u64::from(rect.height),
                        u64::from(CANVAS_DIM_MAX),
                    ));
                }
                if intersect_rect(rect, self.width, self.height).is_none() {
                    return Err(PaintError::invalid(
                        "fill rect does not intersect the canvas",
                    ));
                }
            }
            Op::LayerAdd { id, after, .. } => {
                if self.layers.len() >= LAYERS_MAX {
                    return Err(PaintError::limit(
                        "layer count",
                        self.layers.len() as u64 + 1,
                        LAYERS_MAX as u64,
                    ));
                }
                if *id != LayerId(self.next_layer_id) {
                    return Err(PaintError::format(format!(
                        "layer add id {} is out of sequence (next is {})",
                        id.0, self.next_layer_id
                    )));
                }
                if let Some(anchor) = after {
                    self.require_layer(*anchor)?;
                }
            }
            Op::LayerRemove { id } => {
                self.require_layer(*id)?;
            }
            Op::LayerReorder { id, to_index } => {
                self.require_layer(*id)?;
                if *to_index as usize >= self.layers.len() {
                    return Err(PaintError::invalid(format!(
                        "reorder index {to_index} is outside the {}-layer stack",
                        self.layers.len()
                    )));
                }
            }
            Op::LayerProps { id, change } => {
                self.require_layer(*id)?;
                if change.is_empty() {
                    return Err(PaintError::invalid("layer props change sets nothing"));
                }
            }
        }
        Ok(())
    }

    fn require_layer(&self, id: LayerId) -> PaintResult<()> {
        if self.layer(id).is_none() {
            return Err(PaintError::LayerNotFound { id: id.0 });
        }
        Ok(())
    }

    /// Resident bytes an op would add (pre-flight). Only
    /// pixel-allocating ops add; the rest are zero or
    /// negative (removals are conservatively zero).
    fn additional_bytes(&self, op: &Op) -> PaintResult<u64> {
        match op {
            Op::StampRun { layer, stamps, .. } => {
                let target = self.layer(*layer).expect("validated above");
                Ok(stamp_run_additional_bytes(
                    target,
                    stamps,
                    self.width,
                    self.height,
                ))
            }
            Op::FillRect { layer, rect, .. } => {
                let target = self.layer(*layer).expect("validated above");
                let (x0, y0, x1, y1) =
                    intersect_rect(rect, self.width, self.height).expect("validated above");
                let mut additional = 0u64;
                let tile = TILE_PX;
                for ty in (y0 / tile)..=(y1 / tile) {
                    for tx in (x0 / tile)..=(x1 / tile) {
                        let key = (tx as i32, ty as i32);
                        if !target.has_tile(key) {
                            additional += crate::tile::TILE_BYTES as u64;
                        }
                    }
                }
                Ok(additional)
            }
            _ => Ok(0),
        }
    }

    /// The flattened composite of a canvas region (the CPU
    /// composite of SS3.2.1: base, then each visible layer
    /// bent by its blend mode at its opacity). Region must
    /// lie inside the canvas.
    pub fn composite_region(
        &self,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
    ) -> PaintResult<Vec<u8>> {
        if x + width > self.width || y + height > self.height || width == 0 || height == 0 {
            return Err(PaintError::invalid(format!(
                "composite region ({x}, {y}, {width}x{height}) is outside the {}x{} canvas",
                self.width, self.height
            )));
        }
        let mut out = vec![0u8; (width as usize) * (height as usize) * 4];
        for ly in 0..height {
            for lx in 0..width {
                let (gx, gy) = (x + lx, y + ly);
                let mut pixel = self.base.pixel(gx, gy, self.width);
                for layer in &self.layers {
                    if !layer.visible {
                        continue;
                    }
                    let source = layer.pixel(gx, gy);
                    if source[3] == 0 {
                        continue;
                    }
                    let alpha = mul255(u32::from(source[3]), u32::from(layer.opacity)) as u8;
                    if alpha == 0 {
                        continue;
                    }
                    pixel = composite_blend(
                        layer.blend,
                        [source[0], source[1], source[2], alpha],
                        pixel,
                    );
                }
                let at = ((ly * width + lx) * 4) as usize;
                out[at..at + 4].copy_from_slice(&pixel);
            }
        }
        Ok(out)
    }

    /// A region's pixels with their content hash
    /// (the `composite_hash` framing: width, height,
    /// pixels), for dirty-region tracking.
    pub fn composite_region_hashed(
        &self,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
    ) -> PaintResult<(Vec<u8>, [u8; 32])> {
        let pixels = self.composite_region(x, y, width, height)?;
        let mut hasher = Sha256::new();
        hasher.update(width.to_le_bytes());
        hasher.update(height.to_le_bytes());
        hasher.update(&pixels);
        Ok((pixels, hasher.finalize().into()))
    }

    /// The flattened composite of the whole canvas.
    pub fn composite(&self) -> Vec<u8> {
        self.composite_region(0, 0, self.width, self.height)
            .expect("the full canvas is always a valid region")
    }

    /// SHA-256 of the full composite with its dimensions:
    /// the output hash every replay gate compares.
    pub fn composite_hash(&self) -> [u8; 32] {
        let pixels = self.composite();
        let mut hasher = Sha256::new();
        hasher.update(self.width.to_le_bytes());
        hasher.update(self.height.to_le_bytes());
        hasher.update(&pixels);
        hasher.finalize().into()
    }

    /// The composite encoded as PNG bytes (export).
    pub fn export_png(&self) -> PaintResult<Vec<u8>> {
        crate::png_io::encode_png_rgba(self.width, self.height, &self.composite())
    }
}

/// Intersect a rect with the canvas; `None` when disjoint.
/// Returns (x0, y0, x1, y1) inclusive canvas coordinates.
fn intersect_rect(
    rect: &crate::ops::Rect,
    width: u32,
    height: u32,
) -> Option<(u32, u32, u32, u32)> {
    let x0 = rect.x.max(0);
    let y0 = rect.y.max(0);
    let x1 = (i64::from(rect.x) + i64::from(rect.width) - 1).min(i64::from(width) - 1);
    let y1 = (i64::from(rect.y) + i64::from(rect.height) - 1).min(i64::from(height) - 1);
    if x1 < i64::from(x0) || y1 < i64::from(y0) {
        return None;
    }
    Some((x0 as u32, y0 as u32, x1 as u32, y1 as u32))
}

/// Apply a validated op to a layer stack. Shared by the
/// live path and by replay, so the two can never drift.
fn apply_to_layers(
    layers: &mut Vec<Layer>,
    next_layer_id: &mut u32,
    op: &Op,
    width: u32,
    height: u32,
) {
    match op {
        Op::StampRun {
            layer,
            mode,
            color,
            opacity,
            flow,
            stamps,
        } => {
            let target = layers
                .iter_mut()
                .find(|held| held.id == *layer)
                .expect("validated op targets an existing layer");
            crate::stroke::apply_stamps(
                target,
                stamps,
                crate::stroke::StampStyle {
                    mode: *mode,
                    color: *color,
                    opacity: *opacity,
                    flow: *flow,
                },
                width,
                height,
            );
        }
        Op::FillRect { layer, rect, color } => {
            let target = layers
                .iter_mut()
                .find(|held| held.id == *layer)
                .expect("validated op targets an existing layer");
            apply_fill_rect(target, rect, *color, width, height);
        }
        Op::LayerAdd { id, name, after } => {
            let layer = Layer::new(*id, name.clone());
            match after {
                None => layers.push(layer),
                Some(anchor) => {
                    let at = layers
                        .iter()
                        .position(|held| held.id == *anchor)
                        .expect("validated anchor exists");
                    layers.insert(at + 1, layer);
                }
            }
            *next_layer_id = (*next_layer_id).max(id.0 + 1);
        }
        Op::LayerRemove { id } => {
            layers.retain(|held| held.id != *id);
        }
        Op::LayerReorder { id, to_index } => {
            let at = layers
                .iter()
                .position(|held| held.id == *id)
                .expect("validated layer exists");
            let layer = layers.remove(at);
            layers.insert(*to_index as usize, layer);
        }
        Op::LayerProps { id, change } => {
            let target = layers
                .iter_mut()
                .find(|held| held.id == *id)
                .expect("validated layer exists");
            apply_props(target, change);
        }
    }
}

/// Apply a `LayerProps` change to a layer.
fn apply_props(layer: &mut Layer, change: &LayerPropsChange) {
    if let Some(name) = &change.name {
        layer.name = name.clone();
    }
    if let Some(opacity) = change.opacity {
        layer.opacity = opacity;
    }
    if let Some(blend) = change.blend {
        layer.blend = blend;
    }
    if let Some(visible) = change.visible {
        layer.visible = visible;
    }
    if let Some(lock_alpha) = change.lock_alpha {
        layer.lock_alpha = lock_alpha;
    }
}

/// Fill a rectangle on a layer: source-over deposit, or
/// alpha-preserving under lock-alpha. Whole tiles fully
/// covered by an opaque fill take the fast path.
fn apply_fill_rect(
    layer: &mut Layer,
    rect: &crate::ops::Rect,
    color: Rgba,
    width: u32,
    height: u32,
) {
    let (x0, y0, x1, y1) = intersect_rect(rect, width, height).expect("validated intersection");
    let tile = TILE_PX;
    for ty in (y0 / tile)..=(y1 / tile) {
        for tx in (x0 / tile)..=(x1 / tile) {
            let (bx, by) = (tx * tile, ty * tile);
            let full_tile = x0 <= bx && y0 <= by && x1 >= bx + tile - 1 && y1 >= by + tile - 1;
            if full_tile && color[3] == 255 && !layer.lock_alpha {
                layer.tile_or_allocate((tx as i32, ty as i32)).fill(color);
                continue;
            }
            for gy in y0.max(by)..=y1.min(by + tile - 1) {
                for gx in x0.max(bx)..=x1.min(bx + tile - 1) {
                    let dest = layer.pixel(gx, gy);
                    let out = if layer.lock_alpha {
                        if dest[3] == 0 {
                            continue;
                        }
                        let mut mixed = lerp_rgba(color, dest, color[3]);
                        mixed[3] = dest[3];
                        mixed
                    } else {
                        composite_over(color, dest)
                    };
                    if out != dest {
                        layer.set_pixel(gx, gy, out);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ops::Rect;
    use crate::stroke::{Stamp, StampMode};

    fn fill_op(layer: LayerId, width: u32, height: u32, color: [u8; 4]) -> Op {
        Op::FillRect {
            layer,
            rect: Rect {
                x: 0,
                y: 0,
                width,
                height,
            },
            color,
        }
    }

    #[test]
    fn document_dimension_bounds_are_named() {
        assert!(Document::new_empty(0, 10).is_err());
        assert!(Document::new_empty(10, 0).is_err());
        assert!(Document::new_empty(8193, 10).is_err());
        assert!(Document::new_empty(8192, 8192).is_ok());
    }

    #[test]
    fn layer_count_bound_refuses_the_65th() {
        let mut doc = Document::new_empty(8, 8).expect("doc");
        for index in 0..LAYERS_MAX {
            doc.add_layer(format!("layer {index}")).expect("add");
        }
        assert!(doc.add_layer("one too many".to_string()).is_err());
    }

    #[test]
    fn adversarial_ops_against_missing_layers_refuse() {
        let mut doc = Document::new_empty(16, 16).expect("doc");
        let ghost = LayerId(99);
        let stamp = Stamp {
            x_fixed: 0,
            y_fixed: 0,
            radius_px: 2,
            alpha: 255,
        };
        for op in [
            Op::StampRun {
                layer: ghost,
                mode: StampMode::Paint,
                color: [0, 0, 0, 255],
                opacity: 255,
                flow: 255,
                stamps: vec![stamp],
            },
            fill_op(ghost, 16, 16, [0, 0, 0, 255]),
            Op::LayerRemove { id: ghost },
            Op::LayerReorder {
                id: ghost,
                to_index: 0,
            },
            Op::LayerProps {
                id: ghost,
                change: LayerPropsChange {
                    opacity: Some(3),
                    ..LayerPropsChange::default()
                },
            },
        ] {
            assert!(matches!(
                doc.apply_op(op),
                Err(PaintError::LayerNotFound { .. })
            ));
        }
        assert_eq!(doc.ops().len(), 0, "refused ops never enter the log");
    }

    #[test]
    fn undo_redo_walks_the_log_and_new_ops_truncate() {
        let mut doc = Document::new_empty(8, 8).expect("doc");
        let layer = doc.add_layer("ink".to_string()).expect("layer");
        doc.apply_op(fill_op(layer, 8, 8, [10, 0, 0, 255]))
            .expect("fill a");
        let hash_a = doc.composite_hash();
        doc.apply_op(fill_op(layer, 8, 8, [20, 0, 0, 255]))
            .expect("fill b");
        let hash_b = doc.composite_hash();
        assert_ne!(hash_a, hash_b);
        assert!(doc.undo());
        assert_eq!(doc.composite_hash(), hash_a);
        assert!(doc.redo());
        assert_eq!(doc.composite_hash(), hash_b);
        assert!(doc.undo());
        doc.apply_op(fill_op(layer, 8, 8, [30, 0, 0, 255]))
            .expect("fill c");
        assert_eq!(doc.truncated_ops(), 1);
        assert!(!doc.can_redo());
    }

    /// Gate 2 (SPEC SS3.1/SS8): 64 layers at the maximum
    /// extent; two full-canvas fills land exactly on the
    /// 512 MiB cap; the third is refused with the typed
    /// error and the document is bit-identical afterwards.
    #[test]
    fn memory_cap_holds_under_the_stress_document() {
        let mut doc = Document::new_empty(8192, 8192).expect("doc");
        let mut ids: Vec<LayerId> = Vec::new();
        for index in 0..LAYERS_MAX {
            ids.push(doc.add_layer(format!("stress {index}")).expect("add"));
        }
        doc.apply_op(fill_op(ids[0], 8192, 8192, [1, 2, 3, 255]))
            .expect("fill 1");
        assert_eq!(doc.resident_bytes(), 268_435_456);
        doc.apply_op(fill_op(ids[1], 8192, 8192, [4, 5, 6, 255]))
            .expect("fill 2");
        assert_eq!(doc.resident_bytes(), MEMORY_CAP_BYTES);
        let refused = doc.apply_op(fill_op(ids[2], 8192, 8192, [7, 8, 9, 255]));
        assert!(matches!(refused, Err(PaintError::MemoryCap { .. })));
        assert_eq!(doc.resident_bytes(), MEMORY_CAP_BYTES);
        assert!(
            !doc.layers()[2].has_tile((0, 0)),
            "refused fill left no tiles"
        );
    }

    #[test]
    fn partial_props_change_only_touches_named_fields() {
        let mut doc = Document::new_empty(8, 8).expect("doc");
        let layer = doc.add_layer("ink".to_string()).expect("layer");
        doc.apply_op(Op::LayerProps {
            id: layer,
            change: LayerPropsChange {
                visible: Some(false),
                ..LayerPropsChange::default()
            },
        })
        .expect("props");
        let held = &doc.layers()[0];
        assert!(!held.visible);
        assert_eq!(held.opacity, 255);
        assert_eq!(held.name, "ink");
    }

    #[test]
    fn adversarial_empty_ops_and_bad_params_refuse() {
        let mut doc = Document::new_empty(8, 8).expect("doc");
        let layer = doc.add_layer("ink".to_string()).expect("layer");
        let empty_props = Op::LayerProps {
            id: layer,
            change: LayerPropsChange::default(),
        };
        assert!(doc.apply_op(empty_props).is_err());
        let zero_width = Op::FillRect {
            layer,
            rect: Rect {
                x: 0,
                y: 0,
                width: 0,
                height: 4,
            },
            color: [0, 0, 0, 255],
        };
        assert!(doc.apply_op(zero_width).is_err());
        assert_eq!(doc.ops().len(), 1, "only the LayerAdd stands");
    }
}

#[cfg(test)]
mod adversarial_tests {
    use super::*;
    use crate::Rect;
    use crate::stroke::{Stamp, StampMode};

    #[test]
    fn adversarial_replay_op_after_remove_refuses() {
        let ops = vec![
            Op::LayerAdd {
                id: LayerId(1),
                name: "gone".to_string(),
                after: None,
            },
            Op::LayerRemove { id: LayerId(1) },
            Op::FillRect {
                layer: LayerId(1),
                rect: Rect {
                    x: 0,
                    y: 0,
                    width: 4,
                    height: 4,
                },
                color: [1, 2, 3, 255],
            },
        ];
        let built = Document::from_parts(8, 8, BaseRaster::Empty, ops, 0, Vec::new(), None);
        assert!(matches!(built, Err(PaintError::LayerNotFound { .. })));
    }

    #[test]
    fn adversarial_stamp_run_over_the_cap_refuses() {
        let mut doc = Document::new_empty(16, 16).expect("doc");
        let layer = doc.add_layer("ink".to_string()).expect("layer");
        let stamp = Stamp {
            x_fixed: 0,
            y_fixed: 0,
            radius_px: 1,
            alpha: 255,
        };
        let op = Op::StampRun {
            layer,
            mode: StampMode::Paint,
            color: [0, 0, 0, 255],
            opacity: 255,
            flow: 255,
            stamps: vec![stamp; crate::stroke::STAMPS_PER_RUN_MAX + 1],
        };
        assert!(doc.apply_op(op).is_err());
        assert_eq!(doc.ops().len(), 1, "only the LayerAdd stands");
    }

    #[test]
    fn adversarial_reorder_out_of_range_refuses() {
        let mut doc = Document::new_empty(8, 8).expect("doc");
        let layer = doc.add_layer("ink".to_string()).expect("layer");
        let op = Op::LayerReorder {
            id: layer,
            to_index: 99,
        };
        assert!(doc.apply_op(op).is_err());
    }
}
