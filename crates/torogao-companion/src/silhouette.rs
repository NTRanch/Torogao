//! The alpha silhouette: from a rendered frame's pixels to
//! the pointer input region — the discipline that makes
//! the companion click-through everywhere she is not.
//!
//! The region is a union of axis-aligned rectangles over a
//! coarse grid (one cell = `CELL` source pixels): a cell is
//! solid when any pixel in it clears the alpha threshold.
//! Runs of solid cells merge horizontally, then identical
//! runs merge vertically, so the rectangle count stays in
//! the low hundreds for a character-shaped silhouette —
//! small enough to hand to the compositor every time the
//! shape materially changes, coarse enough that a stray
//! translucent hair pixel does not grow her a grabbable
//! halo a metre wide (the threshold keeps only pixels a
//! viewer would call *her*).
//!
//! Pure functions over `RgbaImage` — the Wayland and winit
//! presentations share them, and the grid law is unit-gated
//! here, including the zero-alpha frame (a fully
//! transparent render yields an empty silhouette, which
//! the presentation reads as "nothing to grab", never as a
//! failure).

use torogao_app::textures::RgbaImage;

/// Alpha at or above which a pixel counts as *her*.
pub const ALPHA_THRESHOLD: u8 = 128;
/// Grid cell size in source-image pixels.
pub const CELL: u32 = 4;

/// One input-region rectangle, in pixel coordinates of the
/// space it was computed in (`x, y, width, height`).
pub type RegionRect = [i32; 4];

/// A computed silhouette: the tight bounding box of the
/// solid cells plus the merged rectangle union.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Silhouette {
    /// Bounding box `(min_x, min_y, max_x_exclusive, max_y_exclusive)`
    /// in source pixels, when anything is solid.
    pub bbox: Option<(u32, u32, u32, u32)>,
    /// The merged rectangles, in source pixels.
    pub rects: Vec<RegionRect>,
    /// How many grid cells were solid.
    pub solid_cells: usize,
}

/// Computes the silhouette of one rendered frame.
pub fn silhouette_of(image: &RgbaImage) -> Silhouette {
    let width = image.width;
    let height = image.height;
    if width == 0 || height == 0 || image.pixels.len() < (width as usize * height as usize * 4) {
        return Silhouette {
            bbox: None,
            rects: Vec::new(),
            solid_cells: 0,
        };
    }
    let cols = width.div_ceil(CELL);
    let rows = height.div_ceil(CELL);
    let mut solid = vec![false; cols as usize * rows as usize];
    let mut solid_cells = 0_usize;
    for row in 0..rows {
        for col in 0..cols {
            let x0 = col * CELL;
            let y0 = row * CELL;
            let x1 = (x0 + CELL).min(width);
            let y1 = (y0 + CELL).min(height);
            let mut cell_solid = false;
            'scan: for y in y0..y1 {
                let base = (y as usize * width as usize) * 4;
                for x in x0..x1 {
                    if image.pixels[base + x as usize * 4 + 3] >= ALPHA_THRESHOLD {
                        cell_solid = true;
                        break 'scan;
                    }
                }
            }
            if cell_solid {
                solid[row as usize * cols as usize + col as usize] = true;
                solid_cells += 1;
            }
        }
    }
    if solid_cells == 0 {
        return Silhouette {
            bbox: None,
            rects: Vec::new(),
            solid_cells: 0,
        };
    }
    // Horizontal runs per cell row, then vertical merging:
    // a run continues the active rectangle with the same
    // (x, width) from the row above, growing its height;
    // active rectangles no run continues are finished, and
    // runs nothing continues start new rectangles.
    let mut rects: Vec<RegionRect> = Vec::new();
    let mut active: Vec<RegionRect> = Vec::new();
    for row in 0..rows {
        let mut runs: Vec<(u32, u32)> = Vec::new(); // (x0, x1) cell coords
        let mut col = 0;
        while col < cols {
            if solid[row as usize * cols as usize + col as usize] {
                let start = col;
                while col < cols && solid[row as usize * cols as usize + col as usize] {
                    col += 1;
                }
                runs.push((start, col));
            } else {
                col += 1;
            }
        }
        let mut next_active: Vec<RegionRect> = Vec::new();
        let mut claimed = vec![false; active.len()];
        for (x0, x1) in &runs {
            let px = (x0 * CELL) as i32;
            let pw = ((x1 * CELL).min(width) - x0 * CELL) as i32;
            match active
                .iter()
                .position(|rect| rect[0] == px && rect[2] == pw)
            {
                Some(index) if !claimed[index] => {
                    claimed[index] = true;
                    let mut rect = active[index];
                    rect[3] += CELL as i32;
                    next_active.push(rect);
                }
                _ => next_active.push([px, (row * CELL) as i32, pw, CELL as i32]),
            }
        }
        for (index, rect) in active.iter().enumerate() {
            if !claimed[index] {
                rects.push(*rect);
            }
        }
        active = next_active;
    }
    rects.extend(active);
    // The last row's heights may overshoot the image bottom
    // by up to CELL-1 when the height is not a multiple.
    let mut min_x = u32::MAX;
    let mut min_y = u32::MAX;
    let mut max_x = 0_u32;
    let mut max_y = 0_u32;
    for rect in &mut rects {
        rect[3] = rect[3].min(height as i32 - rect[1]).max(0);
        min_x = min_x.min(rect[0] as u32);
        min_y = min_y.min(rect[1] as u32);
        max_x = max_x.max((rect[0] + rect[2]) as u32);
        max_y = max_y.max((rect[1] + rect[3]) as u32);
    }
    rects.retain(|rect| rect[2] > 0 && rect[3] > 0);
    Silhouette {
        bbox: Some((min_x, min_y, max_x, max_y)),
        rects,
        solid_cells,
    }
}

/// Scales silhouette rectangles from the render's pixel
/// space into the surface's pixel space.
pub fn scale_rects(rects: &[RegionRect], from: (u32, u32), to: (u32, u32)) -> Vec<RegionRect> {
    if from.0 == 0 || from.1 == 0 || to.0 == 0 || to.1 == 0 {
        return Vec::new();
    }
    let sx = f64::from(to.0) / f64::from(from.0);
    let sy = f64::from(to.1) / f64::from(from.1);
    rects
        .iter()
        .map(|rect| {
            let x = (f64::from(rect[0]) * sx).floor() as i32;
            let y = (f64::from(rect[1]) * sy).floor() as i32;
            let w = ((f64::from(rect[0] + rect[2]) * sx).ceil() as i32 - x).max(0);
            let h = ((f64::from(rect[1] + rect[3]) * sy).ceil() as i32 - y).max(0);
            [x, y, w, h]
        })
        .filter(|rect| rect[2] > 0 && rect[3] > 0)
        .collect()
}

/// Whether a point (in `space` surface coordinates) lands
/// on her: the fallback presentation's in-process hit test,
/// the same rect set the Wayland route hands the
/// compositor.
pub fn point_in_silhouette(
    silhouette: &Silhouette,
    silhouette_space: (u32, u32),
    surface_space: (u32, u32),
    point: (f64, f64),
) -> bool {
    let (x, y) = (point.0 as i32, point.1 as i32);
    scale_rects(&silhouette.rects, silhouette_space, surface_space)
        .iter()
        .any(|rect| x >= rect[0] && x < rect[0] + rect[2] && y >= rect[1] && y < rect[1] + rect[3])
}

/// Whether the silhouette changed materially since the
/// last applied region: the brief's recompute trigger —
/// the bounding box grew/shrank by more than 5% of its
/// area or moved by more than a cell. Identical boxes with
/// different interiors are not material at this stage (the
/// region is a grab discipline, not a pixel-perfect mask).
pub fn bbox_materially_changed(
    previous: Option<(u32, u32, u32, u32)>,
    next: Option<(u32, u32, u32, u32)>,
) -> bool {
    match (previous, next) {
        (None, None) => false,
        (None, Some(_)) | (Some(_), None) => true,
        (Some(a), Some(b)) => {
            if a == b {
                return false;
            }
            let area = |r: (u32, u32, u32, u32)| -> u64 {
                u64::from(r.2.saturating_sub(r.0)) * u64::from(r.3.saturating_sub(r.1))
            };
            let (aa, ab) = (area(a), area(b));
            let area_delta = aa.abs_diff(ab);
            let moved = a.0.abs_diff(b.0) > CELL
                || a.1.abs_diff(b.1) > CELL
                || a.2.abs_diff(b.2) > CELL
                || a.3.abs_diff(b.3) > CELL;
            moved || (aa > 0 && area_delta * 20 > aa) // > 5%
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image_with_block(width: u32, height: u32, block: (u32, u32, u32, u32)) -> RgbaImage {
        let mut pixels = vec![0_u8; (width * height * 4) as usize];
        for y in block.1..block.3 {
            for x in block.0..block.2 {
                let i = ((y * width + x) * 4) as usize;
                pixels[i + 3] = 255;
            }
        }
        RgbaImage::new(width, height, pixels)
    }

    #[test]
    fn zero_alpha_frame_has_empty_silhouette() {
        let image = RgbaImage::new(64, 64, vec![0; 64 * 64 * 4]);
        let silhouette = silhouette_of(&image);
        assert_eq!(silhouette.bbox, None);
        assert!(silhouette.rects.is_empty());
        assert_eq!(silhouette.solid_cells, 0);
    }

    #[test]
    fn a_solid_block_yields_one_rect_and_its_bbox() {
        // An 8x8 block at (8, 8): exactly 2x2 cells.
        let image = image_with_block(32, 32, (8, 8, 16, 16));
        let silhouette = silhouette_of(&image);
        assert_eq!(silhouette.bbox, Some((8, 8, 16, 16)));
        assert_eq!(silhouette.rects.len(), 1);
        assert_eq!(silhouette.rects[0], [8, 8, 8, 8]);
    }

    #[test]
    fn two_separated_blocks_yield_two_rects() {
        let mut image = image_with_block(64, 32, (4, 4, 12, 12));
        for y in 20..28 {
            for x in 40..48 {
                let i = ((y * 64 + x) * 4) as usize;
                image.pixels[i + 3] = 255;
            }
        }
        let silhouette = silhouette_of(&image);
        assert_eq!(silhouette.rects.len(), 2);
        assert_eq!(silhouette.solid_cells, 8);
    }

    #[test]
    fn translucent_pixels_below_threshold_do_not_count() {
        let mut image = RgbaImage::new(16, 16, vec![0; 16 * 16 * 4]);
        for pixel in image.pixels.chunks_exact_mut(4) {
            pixel[3] = ALPHA_THRESHOLD - 1;
        }
        assert_eq!(silhouette_of(&image).solid_cells, 0);
    }

    #[test]
    fn scaling_maps_rects_into_surface_space() {
        let rects = vec![[100, 50, 200, 100]];
        let scaled = scale_rects(&rects, (1024, 512), (512, 256));
        assert_eq!(scaled, vec![[50, 25, 100, 50]]);
    }

    #[test]
    fn material_change_detects_moves_and_ignores_identity() {
        let a = Some((10, 10, 100, 200));
        assert!(!bbox_materially_changed(a, a));
        assert!(bbox_materially_changed(a, Some((30, 10, 120, 200))));
        assert!(bbox_materially_changed(a, None));
        assert!(!bbox_materially_changed(None, None));
    }
}
