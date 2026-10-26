//! The physics preview (physicspreview.d port): layered
//! parameter composition over the deterministic engine
//! update, producing a torogao-curves/1 document.

use crate::curves::CurveSet;
use serde_json::Value;
use torogao_engine::Puppet;

/// The layered inputs of one preview run.
#[derive(Default)]
pub struct PreviewOptions {
    /// The project's extension document (null when none).
    pub ext: Value,
    /// State name to apply, when any.
    pub state_name: String,
    /// Variant id to apply, when any.
    pub variant_id: String,
    /// Whether a baked viseme layer is present.
    pub has_viseme: bool,
    /// The baked viseme curves.
    pub viseme_curves: CurveSet,
    /// Whether an ambient driver set is present.
    pub has_drivers: bool,
    /// The parsed ambient driver configuration.
    pub drivers: Value,
    /// Whether a controller layer is present.
    pub has_controllers: bool,
    /// Resolved controller values (parameter -> value).
    pub controller_values: Vec<(String, f64)>,
    /// Whether an override take is present.
    pub has_override: bool,
    /// The override take document.
    pub override_curves: Value,
}

/// Runs the preview; on failure returns the reference's
/// error string.
pub fn run_physics_preview(
    _puppet: &mut Puppet,
    _track: &Value,
    _fps_override: i32,
    _project_label: &str,
    _options: &PreviewOptions,
) -> Result<CurveSet, String> {
    Err("physics preview is not ported yet".to_string())
}
