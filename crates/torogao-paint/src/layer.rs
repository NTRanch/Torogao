//! The layer stack's unit (SPEC SS3.1): a sparse tile grid
//! plus the presentation properties the op log records.
//! Layers are pure data; all painting semantics live in
//! `stroke` and `document`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::color::{Rgba, TRANSPARENT};
use crate::tile::{TILE_BYTES, Tile, tile_local};

/// A layer's identity. Ids are issued by the document from
/// a monotone counter and never reused within a document,
/// so an op log entry names one layer forever.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct LayerId(pub u32);

/// How a layer bends against the composite below it
/// (SPEC SS3.1). The arithmetic is `color::blend_channel`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlendMode {
    /// Source-over, unbended.
    Normal,
    /// Per-channel product.
    Multiply,
    /// Per-channel overlay (multiply below mid, screen above).
    Overlay,
}

/// One raster layer.
#[derive(Clone)]
pub struct Layer {
    /// Identity, unique within the document.
    pub id: LayerId,
    /// Display name (provenance-readable, not semantic).
    pub name: String,
    /// Layer opacity, 0-255, applied at composite time.
    pub opacity: u8,
    /// Blend mode applied at composite time.
    pub blend: BlendMode,
    /// Invisible layers composite as nothing.
    pub visible: bool,
    /// Lock-alpha: painting may change colour but never
    /// alpha, and only where alpha already exists.
    pub lock_alpha: bool,
    tiles: BTreeMap<(i32, i32), Tile>,
}

impl Layer {
    /// A new empty layer with default properties.
    pub fn new(id: LayerId, name: String) -> Layer {
        Layer {
            id,
            name,
            opacity: 255,
            blend: BlendMode::Normal,
            visible: true,
            lock_alpha: false,
            tiles: BTreeMap::new(),
        }
    }

    /// Resident bytes: allocated tiles at the fixed tile size.
    pub fn resident_bytes(&self) -> u64 {
        (self.tiles.len() as u64) * (TILE_BYTES as u64)
    }

    /// Number of allocated tiles.
    pub fn tile_count(&self) -> usize {
        self.tiles.len()
    }

    /// The allocated tile keys, in sorted order.
    pub fn tile_keys(&self) -> impl Iterator<Item = (i32, i32)> + '_ {
        self.tiles.keys().copied()
    }

    /// Read a canvas-space pixel; absent tiles read as
    /// transparent.
    pub fn pixel(&self, x: u32, y: u32) -> Rgba {
        let (key, lx, ly) = tile_local(x, y);
        match self.tiles.get(&key) {
            Some(tile) => tile.pixel(lx, ly),
            None => TRANSPARENT,
        }
    }

    /// Write a canvas-space pixel, allocating its tile.
    pub fn set_pixel(&mut self, x: u32, y: u32, pixel: Rgba) {
        let (key, lx, ly) = tile_local(x, y);
        self.tiles.entry(key).or_default().set_pixel(lx, ly, pixel);
    }

    /// The tile at `key`, if allocated.
    pub fn tile(&self, key: (i32, i32)) -> Option<&Tile> {
        self.tiles.get(&key)
    }

    /// The tile at `key`, allocating it transparent if absent.
    pub fn tile_or_allocate(&mut self, key: (i32, i32)) -> &mut Tile {
        self.tiles.entry(key).or_default()
    }

    /// True when `key` is already allocated (pre-flight
    /// accounting reads this; it never allocates).
    pub fn has_tile(&self, key: (i32, i32)) -> bool {
        self.tiles.contains_key(&key)
    }
}

impl serde::Serialize for BlendMode {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(match self {
            BlendMode::Normal => "Normal",
            BlendMode::Multiply => "Multiply",
            BlendMode::Overlay => "Overlay",
        })
    }
}

impl<'de> serde::Deserialize<'de> for BlendMode {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<BlendMode, D::Error> {
        struct BlendModeVisitor;
        impl serde::de::Visitor<'_> for BlendModeVisitor {
            type Value = BlendMode;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("a blend mode string")
            }

            fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<BlendMode, E> {
                match value {
                    "Normal" => Ok(BlendMode::Normal),
                    "Multiply" => Ok(BlendMode::Multiply),
                    "Overlay" => Ok(BlendMode::Overlay),
                    other => Err(E::custom(format!("unknown blend mode {other:?}"))),
                }
            }
        }
        deserializer.deserialize_str(BlendModeVisitor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sparse_layer_accounting() {
        let mut layer = Layer::new(LayerId(1), "ink".to_string());
        assert_eq!(layer.resident_bytes(), 0);
        assert_eq!(layer.pixel(500, 500), TRANSPARENT);
        layer.set_pixel(500, 500, [1, 2, 3, 255]);
        assert_eq!(layer.tile_count(), 1);
        assert_eq!(layer.resident_bytes(), TILE_BYTES as u64);
        assert_eq!(layer.pixel(500, 500), [1, 2, 3, 255]);
        // Same tile, no new allocation.
        layer.set_pixel(501, 500, [4, 5, 6, 255]);
        assert_eq!(layer.tile_count(), 1);
        // Neighbouring tile allocates.
        layer.set_pixel(756, 500, [7, 8, 9, 255]);
        assert_eq!(layer.tile_count(), 2);
    }

    #[test]
    fn layer_defaults_are_the_spec_defaults() {
        let layer = Layer::new(LayerId(7), "plate".to_string());
        assert_eq!(layer.opacity, 255);
        assert_eq!(layer.blend, BlendMode::Normal);
        assert!(layer.visible);
        assert!(!layer.lock_alpha);
    }
}
