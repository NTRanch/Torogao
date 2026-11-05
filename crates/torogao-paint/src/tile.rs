//! One canvas tile: a 256x256 RGBA8 block (SPEC SS3.1).
//! Tiles are the unit of sparsity, of dirty-region upload
//! in the editor, and of memory accounting: one tile is
//! always exactly `TILE_BYTES` resident bytes.

use crate::color::Rgba;

/// Tile edge length in pixels (DP-4).
pub const TILE_PX: u32 = 256;

/// Resident bytes of one tile: 256 * 256 * 4.
pub const TILE_BYTES: usize = 262_144;

/// A single tile's pixels, row-major RGBA8.
pub struct Tile {
    pixels: Box<[u8; TILE_BYTES]>,
}

impl Tile {
    /// A fully transparent tile.
    pub fn new() -> Tile {
        Tile {
            pixels: Box::new([0; TILE_BYTES]),
        }
    }

    /// The raw pixel buffer.
    pub fn pixels(&self) -> &[u8; TILE_BYTES] {
        &self.pixels
    }

    /// The raw pixel buffer, mutable.
    pub fn pixels_mut(&mut self) -> &mut [u8; TILE_BYTES] {
        &mut self.pixels
    }

    /// Byte index of local pixel (`lx`, `ly`) inside the tile.
    /// Callers guarantee both are `< TILE_PX` (asserted).
    pub fn index(lx: u32, ly: u32) -> usize {
        debug_assert!(lx < TILE_PX && ly < TILE_PX);
        ((ly * TILE_PX + lx) * 4) as usize
    }

    /// Read one pixel by local coordinates.
    pub fn pixel(&self, lx: u32, ly: u32) -> Rgba {
        let at = Tile::index(lx, ly);
        [
            self.pixels[at],
            self.pixels[at + 1],
            self.pixels[at + 2],
            self.pixels[at + 3],
        ]
    }

    /// Write one pixel by local coordinates.
    pub fn set_pixel(&mut self, lx: u32, ly: u32, pixel: Rgba) {
        let at = Tile::index(lx, ly);
        self.pixels[at..at + 4].copy_from_slice(&pixel);
    }

    /// Fill the whole tile with one pixel value.
    pub fn fill(&mut self, pixel: Rgba) {
        for chunk in self.pixels.chunks_exact_mut(4) {
            chunk.copy_from_slice(&pixel);
        }
    }

    /// True when every pixel is transparent black. A scan,
    /// used by tests and by save-time sparsity checks; not
    /// on any hot path.
    pub fn is_transparent(&self) -> bool {
        self.pixels.iter().all(|byte| *byte == 0)
    }
}

impl Default for Tile {
    fn default() -> Tile {
        Tile::new()
    }
}

impl Clone for Tile {
    fn clone(&self) -> Tile {
        Tile {
            pixels: self.pixels.clone(),
        }
    }
}

/// The tile-grid key for a canvas pixel coordinate.
pub fn tile_key(x: u32, y: u32) -> (i32, i32) {
    (
        i32::try_from(x / TILE_PX).unwrap_or(i32::MAX),
        i32::try_from(y / TILE_PX).unwrap_or(i32::MAX),
    )
}

/// Split a canvas coordinate into (tile key, local x, local y).
pub fn tile_local(x: u32, y: u32) -> ((i32, i32), u32, u32) {
    (tile_key(x, y), x % TILE_PX, y % TILE_PX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tile_round_trip_and_fill() {
        let mut tile = Tile::new();
        assert!(tile.is_transparent());
        assert_eq!(tile.pixel(3, 4), crate::color::TRANSPARENT);
        tile.set_pixel(3, 4, [1, 2, 3, 4]);
        assert_eq!(tile.pixel(3, 4), [1, 2, 3, 4]);
        assert!(!tile.is_transparent());
        tile.fill([9, 9, 9, 255]);
        assert_eq!(tile.pixel(0, 0), [9, 9, 9, 255]);
        assert_eq!(tile.pixel(255, 255), [9, 9, 9, 255]);
    }

    #[test]
    fn tile_addressing_corners() {
        assert_eq!(Tile::index(0, 0), 0);
        assert_eq!(Tile::index(255, 255), TILE_BYTES - 4);
        assert_eq!(tile_local(0, 0), ((0, 0), 0, 0));
        assert_eq!(tile_local(255, 255), ((0, 0), 255, 255));
        assert_eq!(tile_local(256, 0), ((1, 0), 0, 0));
        assert_eq!(tile_local(8191, 8191), ((31, 31), 255, 255));
    }
}
