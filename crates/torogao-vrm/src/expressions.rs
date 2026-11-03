//! VRM expressions from the model's morph targets.
//!
//! Two sources feed the `VRMC_vrm` expressions block:
//! - the character's governed expression map
//!   (`torogao-expression-map-3d/1`): authored expressions whose
//!   names match a VRM preset land in the preset block, the rest
//!   in the custom block, binds carried verbatim;
//! - a fixed synthesis table binding the viseme and blink presets
//!   to their single canonical `Fac_` channels, applied only when
//!   the governed map does not already define that preset.
//!
//! Every bind must resolve against the model's declared morph
//! targets. An unresolvable channel is a named error, never a
//! dropped bind. Zero-weight binds are dropped at emission: they
//! carry no shape, and the governed maps use them only to state
//! a rest-state vector explicitly.

use std::collections::BTreeMap;

use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::error::{VrmError, VrmResult};
use crate::glb::mesh_target_names;

/// The governed expression map's format token.
pub const EXPRESSION_MAP_FORMAT: &str = "torogao-expression-map-3d/1";

/// Largest expression-map document this crate will parse, in bytes.
pub const EXPRESSION_MAP_BYTES_MAX: usize = 1_048_576;

/// The VRM 1.0 preset expression names, alphabetical.
pub const VRM_PRESET_NAMES: &[&str] = &[
    "aa",
    "blink",
    "blinkLeft",
    "blinkRight",
    "ee",
    "angry",
    "happy",
    "ih",
    "lookDown",
    "lookLeft",
    "lookRight",
    "lookUp",
    "neutral",
    "oh",
    "ou",
    "relaxed",
    "sad",
    "surprised",
];

/// Preset → canonical Fac_ channel for the synthesized presets
/// (visemes and the blink family). Each is a single-channel
/// expression at weight 1.0 by definition of the channel.
pub const PRESET_SYNTHESIS: &[(&str, &str)] = &[
    ("aa", "Fac_Mth_Aa"),
    ("blink", "Fac_Eye_Close"),
    ("blinkLeft", "Fac_Eye_L_Wink"),
    ("blinkRight", "Fac_Eye_R_Wink"),
    ("ee", "Fac_Mth_Ee"),
    ("ih", "Fac_Mth_Ii"),
    ("oh", "Fac_Mth_Oo"),
    ("ou", "Fac_Mth_Uu"),
];

/// One governed expression: channel → weight.
#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct GovernedExpression {
    /// The morph weights (`Fac_` channel → 0.0..=1.0).
    pub morphs: BTreeMap<String, f64>,
}

/// A parsed `torogao-expression-map-3d/1` document.
#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct ExpressionMap {
    /// The character the map belongs to.
    pub character: String,
    /// Governed expressions by name.
    pub expressions: BTreeMap<String, GovernedExpression>,
    /// The format token (checked against
    /// [`EXPRESSION_MAP_FORMAT`]).
    pub format: String,
}

impl ExpressionMap {
    /// Parse and validate an expression-map document (JSON).
    /// Weights must be finite and inside `0.0..=1.0`.
    pub fn parse(text: &str) -> VrmResult<ExpressionMap> {
        if text.len() > EXPRESSION_MAP_BYTES_MAX {
            return Err(VrmError::GlbJsonParse {
                detail: format!(
                    "expression map is {} bytes, over the {EXPRESSION_MAP_BYTES_MAX}-byte bound",
                    text.len()
                ),
            });
        }
        let map: ExpressionMap =
            serde_json::from_str(text).map_err(|err| VrmError::GlbJsonParse {
                detail: format!("expression map: {err}"),
            })?;
        if map.format != EXPRESSION_MAP_FORMAT {
            return Err(VrmError::ExpressionMapWrongFormat {
                found: map.format.clone(),
            });
        }
        for (expression, governed) in &map.expressions {
            for (channel, weight) in &governed.morphs {
                if !weight.is_finite() || !(0.0..=1.0).contains(weight) {
                    return Err(VrmError::ExpressionWeightOutOfRange {
                        channel: channel.clone(),
                        expression: expression.clone(),
                        weight: *weight,
                    });
                }
            }
        }
        Ok(map)
    }
}

/// A morph target's address in the model: the node carrying the
/// mesh, and the target's index within that mesh.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MorphAddress {
    /// Index into the target list of the mesh at `node`.
    pub index: usize,
    /// glTF node index carrying the mesh.
    pub node: usize,
}

/// Build the channel → address table from a glTF JSON document:
/// every mesh's declared target names, addressed through the
/// node that carries the mesh. A channel carried by two meshes
/// is ambiguous and refused by name.
pub fn morph_addresses(json: &Value) -> VrmResult<BTreeMap<String, MorphAddress>> {
    let mut out: BTreeMap<String, MorphAddress> = BTreeMap::new();
    let nodes = json
        .get("nodes")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let meshes = json
        .get("meshes")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    for (node_index, node) in nodes.iter().enumerate() {
        let Some(mesh_index) = node.get("mesh").and_then(Value::as_u64) else {
            continue;
        };
        let Some(mesh) = meshes.get(mesh_index as usize) else {
            continue;
        };
        for (target_index, name) in mesh_target_names(mesh).iter().enumerate() {
            let address = MorphAddress {
                index: target_index,
                node: node_index,
            };
            if out.insert(name.clone(), address).is_some() {
                return Err(VrmError::MorphTargetAmbiguous {
                    channel: name.clone(),
                });
            }
        }
    }
    Ok(out)
}

/// The built expressions block: preset and custom expression
/// JSON objects, ready to embed in `VRMC_vrm`.
#[derive(Clone, Debug, PartialEq)]
pub struct BuiltExpressions {
    /// Custom expressions (governed names with no preset slot).
    pub custom: Map<String, Value>,
    /// Preset expressions (governed matches plus synthesis).
    pub preset: Map<String, Value>,
}

/// Build the expressions block. `map` may be `None`, in which
/// case only the synthesized presets are emitted. Every channel
/// in play must resolve in `addresses`.
pub fn build_expressions(
    map: Option<&ExpressionMap>,
    addresses: &BTreeMap<String, MorphAddress>,
) -> VrmResult<BuiltExpressions> {
    let mut preset: Map<String, Value> = Map::new();
    let mut custom: Map<String, Value> = Map::new();
    if let Some(map) = map {
        for (name, governed) in &map.expressions {
            let expression = expression_json(&governed.morphs, addresses)?;
            if VRM_PRESET_NAMES.contains(&name.as_str()) {
                preset.insert(name.clone(), expression);
            } else {
                custom.insert(name.clone(), expression);
            }
        }
    }
    for (preset_name, channel) in PRESET_SYNTHESIS {
        if preset.contains_key(*preset_name) {
            continue;
        }
        // A model without the channel simply has no such preset;
        // absence of a synthesized preset is not an error. (A
        // governed expression naming an absent channel IS an
        // error — that path goes through expression_json.)
        if let Some(address) = addresses.get(*channel) {
            preset.insert(
                (*preset_name).to_string(),
                json!({
                    "morphTargetBinds": [
                        {"index": address.index, "node": address.node, "weight": 1.0}
                    ]
                }),
            );
        }
    }
    Ok(BuiltExpressions { custom, preset })
}

/// One expression's JSON: binds sorted by (node, index),
/// zero-weight binds dropped.
fn expression_json(
    morphs: &BTreeMap<String, f64>,
    addresses: &BTreeMap<String, MorphAddress>,
) -> VrmResult<Value> {
    let mut binds: Vec<(usize, usize, f64)> = Vec::new();
    for (channel, weight) in morphs {
        if *weight == 0.0 {
            continue;
        }
        let address = addresses
            .get(channel)
            .ok_or_else(|| VrmError::MorphTargetAbsent {
                channel: channel.clone(),
            })?;
        binds.push((address.node, address.index, *weight));
    }
    binds.sort_by_key(|bind| (bind.0, bind.1));
    let bind_values: Vec<Value> = binds
        .iter()
        .map(|(node, index, weight)| json!({"index": index, "node": node, "weight": weight}))
        .collect();
    if bind_values.is_empty() {
        return Ok(json!({}));
    }
    Ok(json!({ "morphTargetBinds": bind_values }))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use serde_json::json;

    use super::{ExpressionMap, MorphAddress, build_expressions, morph_addresses};
    use crate::error::VrmError;

    fn addresses() -> BTreeMap<String, MorphAddress> {
        let json = json!({
            "nodes": [{"mesh": 0, "name": "Body"}],
            "meshes": [{"extras": {"targetNames": ["Fac_Eye_Close", "Fac_Mth_Aa"]}}],
        });
        morph_addresses(&json).expect("addresses")
    }

    /// Governed names split into preset and custom; synthesis
    /// fills the viseme the map did not define.
    #[test]
    fn governed_and_synthesized_presets_compose() {
        let map = ExpressionMap::parse(
            r#"{"character":"T","format":"torogao-expression-map-3d/1",
                "expressions":{
                  "joy":{"morphs":{"Fac_Mth_Aa":0.5}},
                  "neutral":{"morphs":{"Fac_Eye_Close":0.0}}
                }}"#,
        )
        .expect("map parses");
        let built = build_expressions(Some(&map), &addresses()).expect("builds");
        assert!(built.custom.contains_key("joy"));
        assert_eq!(built.preset["neutral"], json!({}));
        // blink synthesized from Fac_Eye_Close; aa NOT synthesized
        // over the governed joy usage — aa stays absent (the map
        // defined no aa preset) but the channel exists, so aa IS
        // synthesized. Assert the documented rule: synthesis fills
        // every preset whose channel the model carries.
        assert!(built.preset.contains_key("blink"));
        assert!(built.preset.contains_key("aa"));
    }

    /// A governed channel the model lacks is a named error.
    #[test]
    fn absent_channel_is_refused_by_name() {
        let map = ExpressionMap::parse(
            r#"{"character":"T","format":"torogao-expression-map-3d/1",
                "expressions":{"joy":{"morphs":{"Fac_Mth_Oo":0.5}}}}"#,
        )
        .expect("map parses");
        let err = build_expressions(Some(&map), &addresses()).expect_err("must refuse");
        assert!(matches!(err, VrmError::MorphTargetAbsent { .. }));
    }

    /// Out-of-range weights fail at parse.
    #[test]
    fn out_of_range_weight_is_refused() {
        let err = ExpressionMap::parse(
            r#"{"character":"T","format":"torogao-expression-map-3d/1",
                "expressions":{"joy":{"morphs":{"Fac_Mth_Aa":1.5}}}}"#,
        )
        .expect_err("must refuse");
        assert!(matches!(err, VrmError::ExpressionWeightOutOfRange { .. }));
    }
}
