//! Scene documents and scene validation.
//!
//! Sequencing note: this module's real implementation lands
//! with the P5 family port (scenes/fluids/effects). Until
//! then `load_scene_doc` reports the scene as unavailable, so
//! tutorial scene checks fail honestly rather than passing.

use serde_json::Value;

use torogao_engine::Puppet;

use crate::criteria::Finding;
use crate::walk::Walk;

pub fn load_scene_doc(_name: &str) -> Result<Value, String> {
    Err("scene support arrives with the P5 scenes module".to_string())
}

pub fn validate_scene(_scene: &Value) -> Vec<Finding> {
    Vec::new()
}

pub fn validate_scene_grounded(
    _scene: &Value,
    _puppet: &Puppet,
    _walk: &Walk,
    _pack: &Value,
) -> Vec<Finding> {
    Vec::new()
}
