//! The physics preview (physicspreview.d port): layered
//! parameter composition over the deterministic engine
//! update, producing a torogao-curves/1 document.
//!
//! Layer precedence, highest first, enforced by refusal:
//! override take > input track > viseme > controllers >
//! state/variant recipes > ambient drivers > defaults.

use crate::curves::{quote, CurveSeries, CurveSet};
use crate::drivers::{active_driver_names, driver_inputs, parse_driver_config};
use crate::extdoc::{
    count_states_named, count_variants_named, find_state, find_variant, param_ranges,
    resolve_recipe,
};
use serde_json::Value;
use std::collections::HashSet;
use torogao_engine::{Puppet, Vec2};

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

/// One resolved input: a parameter index and its per-step
/// values.
struct Input {
    /// Parameter index in the puppet.
    param: usize,
    /// Per-step values.
    values: Vec<Vec2>,
}

/// Whether a JSON value is an integer (signed or unsigned
/// storage), returning it as i32 like the reference's
/// cast.
fn json_int(value: &Value) -> Option<i32> {
    if let Some(v) = value.as_i64() {
        return Some(v as i32);
    }
    value.as_u64().map(|v| v as i32)
}

/// Whether a JSON value is numeric, returning it as f64.
fn json_num(value: &Value) -> Option<f64> {
    value.as_f64()
}

/// Appends one constant input layer entry.
#[allow(clippy::too_many_arguments)]
fn append_constant(
    puppet: &Puppet,
    inputs: &mut Vec<Input>,
    authored: &mut HashSet<String>,
    name: &str,
    value: f64,
    layer: &str,
    steps: usize,
) -> Result<(), String> {
    let Some(param) = puppet.param_index(name) else {
        return Err(format!("{layer} drives unknown parameter '{name}'"));
    };
    if puppet.is_driven(param) {
        return Err(format!(
            "{layer} input '{name}' is driven by a driver and cannot be a preview input"
        ));
    }
    let default_y = puppet.params[param].defaults.y;
    inputs.push(Input {
        param,
        values: (0..steps)
            .map(|_| Vec2::new(value as f32, default_y))
            .collect(),
    });
    authored.insert(name.to_string());
    Ok(())
}

/// Runs the preview; on failure returns the reference's
/// error string.
pub fn run_physics_preview(
    puppet: &mut Puppet,
    track: &Value,
    fps_override: i32,
    project_label: &str,
    options: &PreviewOptions,
) -> Result<CurveSet, String> {
    if !track.is_object() {
        return Err("input track must be a JSON object".to_string());
    }
    if let Some(format) = track.get("format") {
        if format.as_str() != Some("torogao-input-track/1") {
            return Err("input track format must be \"torogao-input-track/1\"".to_string());
        }
    }

    let mut fps: i32 = 60;
    if let Some(field) = track.get("fps") {
        match json_int(field) {
            Some(value) => fps = value,
            None => return Err("track field 'fps' must be an integer".to_string()),
        }
    }
    if fps_override > 0 {
        fps = fps_override;
    }
    if ![15, 30, 60, 120].contains(&fps) {
        return Err(format!(
            "step rate {fps} is not in the accepted set {{15, 30, 60, 120}}"
        ));
    }

    let steps: usize = match track.get("steps") {
        Some(field) => match json_int(field) {
            Some(value) if value > 0 && value <= 1_000_000 => value as usize,
            _ => {
                return Err(
                    "track field 'steps' must be an integer in 1..1000000".to_string(),
                )
            }
        },
        None => return Err("track field 'steps' is required".to_string()),
    };

    // -- the input track ----------------------------------
    let mut inputs: Vec<Input> = Vec::new();
    let mut authored: HashSet<String> = HashSet::new();
    if let Some(raw_inputs) = track.get("inputs") {
        if !raw_inputs.is_object() {
            return Err("track field 'inputs' must be an object".to_string());
        }
        for (name, arr) in raw_inputs.as_object().unwrap() {
            let Some(param) = puppet.param_index(name) else {
                return Err(format!("track drives unknown parameter '{name}'"));
            };
            if puppet.is_driven(param) {
                return Err(format!(
                    "track input '{name}' is driven by a driver and cannot be a preview input"
                ));
            }
            let Some(entries) = arr.as_array() else {
                return Err(format!(
                    "track input '{name}' must hold exactly {steps} step values"
                ));
            };
            if entries.len() != steps {
                return Err(format!(
                    "track input '{name}' must hold exactly {steps} step values"
                ));
            }
            let default_y = puppet.params[param].defaults.y;
            let mut values = Vec::with_capacity(steps);
            for entry in entries {
                if let Some(x) = json_num(entry) {
                    values.push(Vec2::new(x as f32, default_y));
                } else if let Some(pair) = entry.as_array().filter(|p| p.len() == 2) {
                    match (json_num(&pair[0]), json_num(&pair[1])) {
                        (Some(x), Some(y)) => values.push(Vec2::new(x as f32, y as f32)),
                        _ => {
                            return Err(format!(
                                "track input '{name}' entries must be numbers or [x, y] pairs"
                            ))
                        }
                    }
                } else {
                    return Err(format!(
                        "track input '{name}' entries must be numbers or [x, y] pairs"
                    ));
                }
            }
            authored.insert(name.clone());
            inputs.push(Input { param, values });
        }
    }

    // -- the override take --------------------------------
    if options.has_override {
        let take = &options.override_curves;
        if !take.is_object()
            || take.get("format").and_then(Value::as_str) != Some("torogao-curves/1")
        {
            return Err("override take is not a torogao-curves/1 document".to_string());
        }
        let Some(series_list) = take.get("series").and_then(Value::as_array) else {
            return Err("override take carries no series array".to_string());
        };
        let mut axis_x: Vec<(String, Vec<f64>)> = Vec::new();
        let mut axis_y: Vec<(String, Vec<f64>)> = Vec::new();
        for series in series_list {
            let name = series
                .get("parameter")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let Some(values) = series.get("values").and_then(Value::as_array) else {
                continue;
            };
            if name.is_empty() {
                continue;
            }
            let axis: i64 = series
                .get("axis")
                .and_then(json_int)
                .map(i64::from)
                .unwrap_or(0);
            let mut vals = Vec::with_capacity(values.len());
            for entry in values {
                match json_num(entry) {
                    Some(d) => vals.push(d),
                    None => {
                        return Err(format!(
                            "override take series for '{name}' holds a non-numeric value"
                        ))
                    }
                }
            }
            if vals.len() != steps {
                return Err(format!(
                    "override take series for '{}' holds {} values but the preview runs {} steps",
                    name,
                    vals.len(),
                    steps
                ));
            }
            if axis == 1 {
                axis_y.push((name, vals));
            } else {
                axis_x.push((name, vals));
            }
        }
        let mut take_params: Vec<String> = Vec::new();
        for (name, _) in &axis_x {
            if !take_params.contains(name) {
                take_params.push(name.clone());
            }
        }
        for (name, _) in &axis_y {
            if !take_params.contains(name) {
                take_params.push(name.clone());
            }
        }
        for name in &take_params {
            if authored.contains(name) {
                return Err(format!(
                    "the input track and the override take both drive parameter '{name}'; two authored layers may not silently compose"
                ));
            }
            let Some(param) = puppet.param_index(name) else {
                return Err(format!("override take drives unknown parameter '{name}'"));
            };
            if puppet.is_driven(param) {
                return Err(format!(
                    "override take input '{name}' is driven by a driver and cannot be a preview input"
                ));
            }
            let xs = axis_x.iter().find(|(n, _)| n == name).map(|(_, v)| v);
            let ys = axis_y.iter().find(|(n, _)| n == name).map(|(_, v)| v);
            let defaults = puppet.params[param].defaults;
            let mut values = Vec::with_capacity(steps);
            for i in 0..steps {
                let x = xs.map(|v| v[i]).unwrap_or(f64::from(defaults.x));
                let y = ys.map(|v| v[i]).unwrap_or(f64::from(defaults.y));
                values.push(Vec2::new(x as f32, y as f32));
            }
            authored.insert(name.clone());
            inputs.push(Input { param, values });
        }
    }

    // -- state / variant recipes ---------------------------
    if !options.state_name.is_empty() || !options.variant_id.is_empty() {
        if !options.ext.is_object() {
            return Err(
                "the project carries no torogao extension data; it has no states or variants to apply"
                    .to_string(),
            );
        }
        let ranges = param_ranges(puppet);
        let mut recipe_values: Vec<(String, f64, String)> = Vec::new();
        if !options.state_name.is_empty() {
            let name = &options.state_name;
            let dup_count = count_states_named(&options.ext, name);
            if dup_count > 1 {
                return Err(format!(
                    "state '{name}' is defined {dup_count} times in the project (TG-STATE-DUPLICATE); refusing the ambiguity"
                ));
            }
            let state = find_state(&options.ext, name);
            if !state.is_object() {
                return Err(format!("state '{name}' is not stored in the project"));
            }
            let recipe = state.get("recipe").cloned().unwrap_or(Value::Null);
            let (resolved, problems) =
                resolve_recipe(&options.ext, &recipe, &ranges);
            if let Some(first) = problems.first() {
                return Err(format!("state '{name}' recipe is invalid: {first}"));
            }
            for (pname, value) in resolved {
                if recipe_values.iter().any(|(n, _, _)| *n == pname) {
                    return Err(format!(
                        "state and variant both drive parameter '{pname}'; two authored layers may not silently compose"
                    ));
                }
                recipe_values.push((pname, value, format!("state '{name}'")));
            }
        }
        if !options.variant_id.is_empty() {
            let id = &options.variant_id;
            let dup_count = count_variants_named(&options.ext, id);
            if dup_count > 1 {
                return Err(format!(
                    "variant '{id}' is defined {dup_count} times in the project (TG-STATE-DUPLICATE); refusing the ambiguity"
                ));
            }
            let variant = find_variant(&options.ext, id);
            if !variant.is_object() {
                return Err(format!("variant '{id}' is not stored in the project"));
            }
            let recipe = variant.get("recipe").cloned().unwrap_or(Value::Null);
            let (resolved, problems) =
                resolve_recipe(&options.ext, &recipe, &ranges);
            if let Some(first) = problems.first() {
                return Err(format!("variant '{id}' recipe is invalid: {first}"));
            }
            for (pname, value) in resolved {
                if recipe_values.iter().any(|(n, _, _)| *n == pname) {
                    return Err(format!(
                        "state and variant both drive parameter '{pname}'; two authored layers may not silently compose"
                    ));
                }
                recipe_values.push((pname, value, format!("variant '{id}'")));
            }
        }
        for (name, value, layer) in &recipe_values {
            if authored.contains(name) {
                return Err(format!(
                    "the input track and {layer} both drive parameter '{name}'"
                ));
            }
            append_constant(puppet, &mut inputs, &mut authored, name, *value, layer, steps)?;
        }
    }

    // -- the viseme layer ----------------------------------
    if options.has_viseme {
        if options.viseme_curves.steps() != steps {
            return Err(format!(
                "viseme curves hold {} steps but the preview runs {}; bake the viseme track at the preview's step count",
                options.viseme_curves.steps(),
                steps
            ));
        }
        for series in &options.viseme_curves.series {
            let name = &series.parameter;
            if authored.contains(name) {
                return Err(format!(
                    "the input track or a recipe and the viseme layer both drive parameter '{name}'"
                ));
            }
            let Some(param) = puppet.param_index(name) else {
                return Err(format!("viseme layer drives unknown parameter '{name}'"));
            };
            if puppet.is_driven(param) {
                return Err(format!(
                    "viseme layer input '{name}' is driven by a driver and cannot be a preview input"
                ));
            }
            let default_y = puppet.params[param].defaults.y;
            inputs.push(Input {
                param,
                values: series
                    .values
                    .iter()
                    .map(|v| Vec2::new(*v as f32, default_y))
                    .collect(),
            });
            authored.insert(name.clone());
        }
    }

    // -- the controllers layer -----------------------------
    if options.has_controllers {
        for (name, value) in &options.controller_values {
            if authored.contains(name) {
                return Err(format!(
                    "a higher layer and the controllers both drive parameter '{name}'; two layers may not silently compose"
                ));
            }
            append_constant(
                puppet,
                &mut inputs,
                &mut authored,
                name,
                *value,
                "controllers",
                steps,
            )?;
        }
    }

    // -- the ambient drivers -------------------------------
    let driver_cfg = if options.has_drivers {
        Some(parse_driver_config(&options.drivers).unwrap_or_default())
    } else {
        None
    };
    if let Some(cfg) = &driver_cfg {
        for (name, values) in driver_inputs(puppet, cfg, steps, fps) {
            if authored.contains(&name) {
                continue;
            }
            let Some(param) = puppet.param_index(&name) else {
                continue;
            };
            if puppet.is_driven(param) {
                return Err(format!(
                    "ambient driver input '{name}' is driven by a physics driver and cannot be a preview input"
                ));
            }
            let default_y = puppet.params[param].defaults.y;
            inputs.push(Input {
                param,
                values: values
                    .iter()
                    .map(|v| Vec2::new(*v as f32, default_y))
                    .collect(),
            });
            authored.insert(name);
        }
    }

    // -- series layout: every parameter, in puppet order ---
    let mut series: Vec<CurveSeries> = Vec::new();
    for param in &puppet.params {
        series.push(CurveSeries {
            parameter: param.name.clone(),
            axis: 0,
            values: Vec::with_capacity(steps),
        });
        if param.is_vec2 {
            series.push(CurveSeries {
                parameter: param.name.clone(),
                axis: 1,
                values: Vec::with_capacity(steps),
            });
        }
    }

    // Scripted clock + fresh driver state.
    puppet.clock.now = puppet.clock.last;
    puppet.clock.tick();
    puppet.reset_drivers();
    puppet.set_drivers_fixed_step(true);

    let dt = 1.0 / f64::from(fps);
    for i in 0..steps {
        for input in &inputs {
            puppet.params[input.param].value = input.values[i];
        }
        puppet.clock.now += dt;
        puppet.clock.tick();
        puppet.update()?;
        let mut s = 0;
        for param in &puppet.params {
            let effective = if param.active {
                param.last_internal
            } else {
                param.value
            };
            series[s].values.push(f64::from(effective.x));
            s += 1;
            if param.is_vec2 {
                series[s].values.push(f64::from(effective.y));
                s += 1;
            }
        }
    }

    let mut result = CurveSet::new("physics-preview", dt);
    result.series = series;
    result.step_count = steps as i64;
    result
        .extra
        .push(("project".to_string(), quote(project_label)));
    result.extra.push(("fps".to_string(), format!("{fps}")));
    if !options.state_name.is_empty() {
        result
            .extra
            .push(("state".to_string(), quote(&options.state_name)));
    }
    if !options.variant_id.is_empty() {
        result
            .extra
            .push(("variant".to_string(), quote(&options.variant_id)));
    }
    if let Some(cfg) = &driver_cfg {
        result.extra.push((
            "drivers".to_string(),
            quote(&active_driver_names(cfg).join(",")),
        ));
        result
            .extra
            .push(("driver-seed".to_string(), format!("{}", cfg.seed)));
    }
    Ok(result)
}
