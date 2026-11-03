//! The cue→morph mapping table: `torogao-viseme-map/1`.
//!
//! The table is data, not code: `data/viseme-map-1.json`,
//! embedded in the crate and parsed at load. It assigns
//! every Rhubarb shape a weight for every governed Fac_
//! mouth channel — all nine shapes, all thirteen channels,
//! every cell explicit. A row or cell missing from the
//! data is a load-time failure (`TV-MAP-INVALID`), never a
//! silent zero: the zeros in the shipped table are
//! authored zeros (see the crate README for the per-shape
//! rationale), and the bake law reads absence from a
//! shape's row within a frame as rest only because the
//! table itself defines the full row.
//!
//! The channel set is the Fac_ mouth vocabulary of
//! `mmd-profile/1` (the estate's governed face channels):
//! the five vowel morphs plus the per-side lip-corner
//! controls. Amending a weight amends the data file and
//! bumps the map id; code never changes for a retune.

use crate::cue::Shape;
use crate::error::{VisemeError, VisemeResult};
use serde::Deserialize;
use std::collections::BTreeMap;

/// The governed Fac_ mouth channels, alphabetical — the
/// order the baked track emits and the order each mapping
/// row's weights align to.
pub const MOUTH_CHANNELS: [&str; 13] = [
    "Fac_Mth_Aa",
    "Fac_Mth_Ee",
    "Fac_Mth_Ii",
    "Fac_Mth_L_Down",
    "Fac_Mth_L_In",
    "Fac_Mth_L_Out",
    "Fac_Mth_L_Up",
    "Fac_Mth_Oo",
    "Fac_Mth_R_Down",
    "Fac_Mth_R_In",
    "Fac_Mth_R_Out",
    "Fac_Mth_R_Up",
    "Fac_Mth_Uu",
];

/// The mapping table's id, carried into every baked
/// track's provenance by the converter version record.
pub const MAP_ID: &str = "torogao-viseme-map/1";

/// The shipped mapping data, embedded verbatim.
pub const MAP_JSON: &str = include_str!("../data/viseme-map-1.json");

#[derive(Deserialize)]
struct MapFile {
    channels: Vec<String>,
    id: String,
    shapes: BTreeMap<String, Vec<f64>>,
}

/// A validated mapping table: one weight row per shape,
/// each row aligned to `MOUTH_CHANNELS`.
#[derive(Debug, Clone, PartialEq)]
pub struct VisemeMap {
    rows: BTreeMap<Shape, Vec<f64>>,
}

impl VisemeMap {
    /// Loads and validates the shipped table
    /// (`data/viseme-map-1.json`).
    pub fn load() -> VisemeResult<VisemeMap> {
        VisemeMap::from_json(MAP_JSON)
    }

    /// Parses and validates a mapping table document.
    /// Validation is complete: the id must be
    /// `torogao-viseme-map/1`, the channel list must equal
    /// `MOUTH_CHANNELS` exactly (names and order), every
    /// shape A–H and X must carry a row of one finite
    /// weight in `[0, 1]` per channel, and no unknown
    /// shape token may appear.
    pub fn from_json(text: &str) -> VisemeResult<VisemeMap> {
        let file: MapFile = serde_json::from_str(text).map_err(|err| {
            VisemeError::new(
                "TV-MAP-INVALID",
                format!("mapping table does not parse: {err}"),
            )
        })?;
        if file.id != MAP_ID {
            return Err(VisemeError::new(
                "TV-MAP-INVALID",
                format!("mapping table id is {:?}; expected {MAP_ID:?}", file.id),
            ));
        }
        let expected: Vec<String> = MOUTH_CHANNELS.iter().map(ToString::to_string).collect();
        if file.channels != expected {
            return Err(VisemeError::new(
                "TV-MAP-INVALID",
                "mapping table channels do not equal the governed Fac_ mouth set",
            ));
        }
        let mut rows: BTreeMap<Shape, Vec<f64>> = BTreeMap::new();
        for (token, weights) in &file.shapes {
            let shape = Shape::from_token(token).map_err(|_| {
                VisemeError::new(
                    "TV-MAP-INVALID",
                    format!("mapping table carries unknown shape {token:?}"),
                )
            })?;
            if weights.len() != MOUTH_CHANNELS.len() {
                return Err(VisemeError::new(
                    "TV-MAP-INVALID",
                    format!(
                        "shape {token} row has {} weights for {} channels",
                        weights.len(),
                        MOUTH_CHANNELS.len()
                    ),
                ));
            }
            for weight in weights {
                if !weight.is_finite() || !(0.0..=1.0).contains(weight) {
                    return Err(VisemeError::new(
                        "TV-MAP-INVALID",
                        format!("shape {token} weight {weight} is not a finite value in [0, 1]"),
                    ));
                }
            }
            rows.insert(shape, weights.clone());
        }
        for shape in Shape::ALL {
            if !rows.contains_key(&shape) {
                return Err(VisemeError::new(
                    "TV-MAP-INVALID",
                    format!("mapping table has no row for shape {}", shape.token()),
                ));
            }
        }
        Ok(VisemeMap { rows })
    }

    /// The weight row for a shape, aligned to
    /// `MOUTH_CHANNELS`. Every shape has a row — the
    /// table's completeness is a load-time invariant.
    pub fn weights(&self, shape: Shape) -> &[f64] {
        self.rows
            .get(&shape)
            .expect("validated map carries every shape")
    }
}

#[cfg(test)]
mod tests {
    use super::{MOUTH_CHANNELS, VisemeMap};
    use crate::cue::Shape;

    #[test]
    fn shipped_map_is_complete_and_in_range() {
        let map = VisemeMap::load().expect("shipped map validates");
        for shape in Shape::ALL {
            assert_eq!(map.weights(shape).len(), MOUTH_CHANNELS.len());
        }
        // Spot anchors of the authored table: D is the wide
        // open shape (Fac_Mth_Aa at full), X is rest.
        assert_eq!(map.weights(Shape::D)[0], 1.0);
        assert!(map.weights(Shape::X).iter().all(|w| *w == 0.0));
    }

    #[test]
    fn map_missing_a_shape_row_is_invalid() {
        let broken = super::MAP_JSON.replace("\"X\":", "\"Z\":");
        let err = VisemeMap::from_json(&broken).expect_err("missing X row must fail");
        assert_eq!(err.code(), "TV-MAP-INVALID");
    }

    #[test]
    fn map_with_out_of_range_weight_is_invalid() {
        let broken = super::MAP_JSON.replace("1.0]", "1.5]");
        let err = VisemeMap::from_json(&broken).expect_err("weight 1.5 must fail");
        assert_eq!(err.code(), "TV-MAP-INVALID");
    }
}
