//! Automations: payload-driven parameter animators (sine
//! waves and the verlet physics chain), ported from the
//! reference — including its quirks: the physics chain's
//! simulate step never applies the position itself (only
//! velocity + gravity), the damping field is carried but
//! unused, and the sine phase is a field the deserializer
//! never reads (it stays 0).

use crate::math::{Vec2, clamp_f32};
use crate::param::{MergeMode, Parameter};
use serde_json::Value;
use torogao_format::value::{jget, jnum, jstr};

/// One automation binding (parameter addressed by name).
#[derive(Debug, Clone)]
pub struct AutomationBinding {
    /// The parameter name from the payload.
    pub param_name: String,
    /// The resolved parameter index (by name, first match).
    pub param: Option<usize>,
    /// Axis index (0 = x, 1 = y).
    pub axis: i32,
    /// Output range.
    pub range: Vec2,
}

impl AutomationBinding {
    /// Decodes one binding record.
    pub fn from_payload(raw: &Value) -> AutomationBinding {
        let range = match jget(raw, "range") {
            Value::Array(items) => Vec2::new(
                items.first().and_then(|v| v.as_f64()).unwrap_or(0.0) as f32,
                items.get(1).and_then(|v| v.as_f64()).unwrap_or(0.0) as f32,
            ),
            _ => Vec2::ZERO,
        };
        AutomationBinding {
            param_name: jstr(raw, "param").to_string(),
            param: None,
            axis: jnum(raw, "axis", 0.0) as i32,
            range,
        }
    }

    /// The bound parameter's value on this binding's axis
    /// (NaN for an out-of-range axis, as in the reference).
    pub fn axis_value(&self, params: &[Parameter]) -> f32 {
        let Some(param) = self.param.and_then(|i| params.get(i)) else {
            return f32::NAN;
        };
        match self.axis {
            0 => param.value.x,
            1 => param.value.y,
            _ => f32::NAN,
        }
    }
}

/// One verlet node of the physics automation.
#[derive(Debug, Clone, Copy)]
pub struct VerletNode {
    /// Rest distance to the next node.
    pub distance: f32,
    /// Current position.
    pub position: Vec2,
    /// Previous position.
    pub old_position: Vec2,
}

/// The physics (verlet chain) automation.
#[derive(Debug, Clone)]
pub struct PhysicsAutomation {
    /// Verlet nodes.
    pub nodes: Vec<VerletNode>,
    /// Damping (carried, unused by the simulation — the
    /// reference never applies it).
    pub damping: f32,
    /// Bounciness.
    pub bounciness: f32,
    /// Gravity.
    pub gravity: f32,
}

/// The sine automation.
#[derive(Debug, Clone)]
pub struct SineAutomation {
    /// Wave speed.
    pub speed: f32,
    /// Wave phase (never deserialized; stays 0).
    pub phase: f32,
    /// Wave kind: 0 sin, 1 cos, 2 tan.
    pub sine_type: i32,
}

/// One automation.
#[derive(Debug, Clone)]
pub struct Automation {
    /// Display name.
    pub name: String,
    /// Whether the automation runs.
    pub enabled: bool,
    /// The payload `type` tag.
    pub type_id: String,
    /// Bindings.
    pub bindings: Vec<AutomationBinding>,
    /// Kind-specific state.
    pub kind: AutomationKind,
}

/// Automation kind state.
#[derive(Debug, Clone)]
pub enum AutomationKind {
    /// The verlet physics chain.
    Physics(PhysicsAutomation),
    /// The sine wave.
    Sine(SineAutomation),
}

/// The registered automation type tags in this build
/// (the reference skips unknown types at load).
pub fn is_known_automation_type(type_id: &str) -> bool {
    matches!(type_id, "physics" | "sine")
}

impl Automation {
    /// Decodes one automation record; unknown `type` values
    /// return None (the reference skips them at load).
    pub fn from_payload(raw: &Value) -> Option<Automation> {
        let type_id = jstr(raw, "type").to_string();
        if !is_known_automation_type(&type_id) {
            return None;
        }
        let bindings = match jget(raw, "bindings") {
            Value::Array(items) => items.iter().map(AutomationBinding::from_payload).collect(),
            _ => Vec::new(),
        };
        let kind = match type_id.as_str() {
            "physics" => {
                let nodes = match jget(raw, "nodes") {
                    Value::Array(items) => items
                        .iter()
                        .map(|item| VerletNode {
                            distance: jnum(item, "distance", 1.0) as f32,
                            position: vec2_of(jget(item, "position")),
                            old_position: vec2_of(jget(item, "old_position")),
                        })
                        .collect(),
                    _ => Vec::new(),
                };
                AutomationKind::Physics(PhysicsAutomation {
                    nodes,
                    damping: jnum(raw, "damping", 0.05) as f32,
                    bounciness: jnum(raw, "bounciness", 1.0) as f32,
                    gravity: jnum(raw, "gravity", 20.0) as f32,
                })
            }
            _ => AutomationKind::Sine(SineAutomation {
                speed: jnum(raw, "speed", 1.0) as f32,
                phase: 0.0,
                sine_type: jnum(raw, "sine_type", 0.0) as i32,
            }),
        };
        Some(Automation {
            name: jstr(raw, "name").to_string(),
            enabled: !matches!(jget(raw, "enabled"), Value::Bool(false)),
            type_id,
            bindings,
            kind,
        })
    }

    /// Resolves binding parameter indices by name (first
    /// match), the reference finalize.
    pub fn finalize(&mut self, params: &[Parameter]) {
        for binding in self.bindings.iter_mut() {
            binding.param = params.iter().position(|p| p.name == binding.param_name);
        }
    }

    /// Runs one update tick (the caller checks `enabled`,
    /// as the reference does twice over).
    pub fn update(&mut self, params: &mut [Parameter], current_time: f64, delta_time: f64) {
        match &mut self.kind {
            AutomationKind::Sine(sine) => {
                let speed = sine.speed as f64;
                let phase = sine.phase as f64;
                for binding in &self.bindings {
                    let raw = current_time * speed + phase;
                    let wave_unit = match sine.sine_type {
                        0 => raw.sin(),
                        1 => raw.cos(),
                        _ => raw.tan(),
                    };
                    let unit = ((wave_unit + 1.0) / 2.0) as f32;
                    let wave = binding.range.x + unit * (binding.range.y - binding.range.x);
                    if let Some(param) = binding.param.and_then(|i| params.get_mut(i)) {
                        param.push_offset_axis(binding.axis, wave, MergeMode::Passthrough, 1.0);
                    }
                }
            }
            AutomationKind::Physics(phys) => {
                if self.bindings.len() <= 1 {
                    return;
                }
                let dt = delta_time as f32;
                let gravity = phys.gravity;
                let bounciness = phys.bounciness;
                let steps = self.bindings.len().min(phys.nodes.len());
                for node in phys.nodes.iter_mut().take(steps) {
                    let tmp = node.position;
                    node.position = (node.position - node.old_position)
                        + Vec2::new(0.0, gravity) * (dt * dt) * bounciness;
                    node.old_position = tmp;
                }
                let iterations = 4 + self.bindings.len() * 2;
                for _ in 0..iterations {
                    constrain(&mut phys.nodes, &self.bindings, params);
                }
                for i in 0..steps {
                    let binding = &self.bindings[i];
                    let node = &mut phys.nodes[i];
                    node.position.x = clamp_f32(node.position.x, binding.range.x, binding.range.y);
                    let x = node.position.x;
                    if let Some(param) = binding.param.and_then(|idx| params.get_mut(idx)) {
                        param.push_offset_axis(binding.axis, x, MergeMode::Passthrough, 1.0);
                    }
                }
            }
        }
    }
}

fn constrain(nodes: &mut [VerletNode], bindings: &[AutomationBinding], params: &[Parameter]) {
    if nodes.len() < 2 {
        return;
    }
    for i in 0..nodes.len() - 1 {
        if i == 0 {
            if let Some(binding) = bindings.first() {
                let hinge = binding.axis_value(params);
                nodes[0].position = Vec2::new(hinge, 0.0);
            }
        }
        let (node1_pos, node2_pos, node2_distance) = {
            (
                nodes[i].position,
                nodes[i + 1].position,
                nodes[i + 1].distance,
            )
        };
        let diff_x = node1_pos.x - node2_pos.x;
        let diff_y = node1_pos.y - node2_pos.y;
        let dist = node1_pos.distance(node2_pos);
        let mut diff = 0.0f32;
        if dist > 0.0 {
            diff = (node2_distance - dist) / dist;
        }
        let trans = Vec2::new(diff_x, diff_y) * (0.5 * diff);
        nodes[i].position += trans;
        nodes[i + 1].position -= trans;
        let floor = nodes[i].position.y;
        nodes[i + 1].position.y = clamp_f32(nodes[i + 1].position.y, floor, f32::MAX);
    }
}

fn vec2_of(value: &Value) -> Vec2 {
    match value {
        Value::Array(items) => Vec2::new(
            items.first().and_then(|v| v.as_f64()).unwrap_or(0.0) as f32,
            items.get(1).and_then(|v| v.as_f64()).unwrap_or(0.0) as f32,
        ),
        _ => Vec2::ZERO,
    }
}

#[cfg(test)]
mod tests {
    use super::Automation;
    use serde_json::json;

    #[test]
    fn unknown_automation_type_is_skipped() {
        let raw = json!({"type": "wobble", "name": "X", "bindings": []});
        assert!(Automation::from_payload(&raw).is_none());
    }

    #[test]
    fn sine_phase_never_loads() {
        let raw =
            json!({"type": "sine", "name": "S", "speed": 2.0, "sine_type": 1, "bindings": []});
        let automation = Automation::from_payload(&raw).expect("sine");
        match automation.kind {
            super::AutomationKind::Sine(s) => {
                assert_eq!(s.speed, 2.0);
                assert_eq!(s.phase, 0.0);
                assert_eq!(s.sine_type, 1);
            }
            _ => panic!("wrong kind"),
        }
    }
}
