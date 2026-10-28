//! A loaded model: the engine puppet plus the container's
//! embedded texture payloads, with the evaluation protocol
//! the shell shares between its windowed and headless modes.
//!
//! Evaluation protocol (the same one the evaluation-parity
//! probe harness uses, so a state hash taken here is
//! comparable to a headless engine evaluation of the same
//! parameter set): fresh load → scripted clock origin (zero
//! delta) → driver reset → fixed-step drivers → assign the
//! parameter values → advance one 1/60 step → one engine
//! update. The windowed shell evaluates incrementally after
//! that first step: each edit assigns and advances one more
//! step on the same puppet, never a reload.

use torogao_engine::{Puppet, Vec2};
use torogao_format::TextureSlot;
use torogao_format::container::read_inp;
use torogao_format::payload::decode_payload;

use crate::error::{AppError, AppResult};

/// One evaluation step, in seconds (60 steps per second).
pub const STEP_SECONDS: f64 = 1.0 / 60.0;
/// Largest model file the shell will read, in bytes.
pub const MODEL_BYTES_MAX: usize = 512 * 1024 * 1024;
/// Largest parameter count a scene may carry.
pub const PARAMETERS_MAX: usize = 4096;
/// Largest node count a scene may carry.
pub const NODES_MAX: usize = 65_536;

/// A loaded, evaluated model.
pub struct Scene {
    /// The engine puppet (the only evaluator).
    pub puppet: Puppet,
    /// The embedded texture slots, in container order. A
    /// part's texture id indexes this list.
    pub textures: Vec<TextureSlot>,
    /// Where the model came from (display only).
    pub source_label: String,
    /// A process-unique generation number, assigned at load.
    /// GPU texture caches key on it: a new scene's slots must
    /// never be served the previous scene's uploads.
    pub generation: u64,
    /// The payload's `preservePixels` flag: point sampling
    /// when set, linear otherwise (the reference's rule).
    pub preserve_pixels: bool,
}

/// Source of scene generation numbers.
static NEXT_GENERATION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

impl Scene {
    /// Loads a scene from an `.inx` file path.
    pub fn load_file(path: &std::path::Path) -> AppResult<Scene> {
        let bytes = std::fs::read(path).map_err(|err| AppError::Io {
            context: "read model",
            message: err.to_string(),
        })?;
        Scene::load_bytes(&bytes, &path.display().to_string())
    }

    /// Loads a scene from container bytes. Bounds are checked
    /// before any allocation the format layer has not already
    /// bounded itself.
    pub fn load_bytes(bytes: &[u8], source_label: &str) -> AppResult<Scene> {
        if bytes.len() > MODEL_BYTES_MAX {
            return Err(AppError::Format(format!(
                "model is {} bytes; the shell limit is {MODEL_BYTES_MAX}",
                bytes.len()
            )));
        }
        let inp = read_inp(bytes).map_err(|err| AppError::Format(err.message().to_string()))?;
        let doc = decode_payload(&inp.payload)
            .map_err(|err| AppError::Format(err.message().to_string()))?;
        let puppet =
            Puppet::load(&doc, &inp).map_err(|err| AppError::Format(err.message().to_string()))?;
        if puppet.params.len() > PARAMETERS_MAX {
            return Err(AppError::Format(format!(
                "model carries {} parameters; the shell limit is {PARAMETERS_MAX}",
                puppet.params.len()
            )));
        }
        if puppet.nodes.nodes.len() > NODES_MAX {
            return Err(AppError::Format(format!(
                "model carries {} nodes; the shell limit is {NODES_MAX}",
                puppet.nodes.nodes.len()
            )));
        }
        let preserve_pixels = matches!(
            doc.meta.get("preservePixels"),
            Some(serde_json::Value::Bool(true))
        );
        Ok(Scene {
            puppet,
            textures: inp.textures.clone(),
            source_label: source_label.to_string(),
            generation: NEXT_GENERATION.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            preserve_pixels,
        })
    }

    /// The display name of the model: payload metadata when
    /// present, else the source label.
    pub fn model_name(&self) -> &str {
        if self.puppet.meta.name.is_empty() {
            &self.source_label
        } else {
            &self.puppet.meta.name
        }
    }

    /// Evaluates from a fresh-load state under the shared
    /// protocol, assigning `assignments` (parameter index,
    /// value) before the single step. This is the evaluation
    /// whose state hash the headless mode reports.
    pub fn evaluate_fresh(&mut self, assignments: &[(usize, Vec2)]) -> AppResult<()> {
        self.puppet.clock.now = self.puppet.clock.last;
        self.puppet.clock.tick();
        self.puppet.reset_drivers();
        self.puppet.set_drivers_fixed_step(true);
        for (index, value) in assignments {
            self.puppet.params[*index].value = *value;
        }
        self.step()
    }

    /// Assigns one parameter and advances the live scene by
    /// one step (the windowed edit path: the puppet keeps its
    /// driver and automation state between edits).
    pub fn set_parameter(&mut self, index: usize, value: Vec2) -> AppResult<()> {
        assert!(index < self.puppet.params.len());
        self.puppet.params[index].value = value;
        self.step()
    }

    /// Advances the clock one fixed step and runs one engine
    /// update.
    fn step(&mut self) -> AppResult<()> {
        self.advance_clock();
        self.run_update()
    }

    /// Advances the puppet clock by one fixed step without
    /// evaluating (the play path interleaves the animation
    /// player's frame render between the clock tick and the
    /// evaluation, mirroring the reference host's order:
    /// player update first, then `Puppet.update`).
    pub fn advance_clock(&mut self) {
        self.puppet.clock.now += STEP_SECONDS;
        self.puppet.clock.tick();
    }

    /// Runs one engine evaluation on the standing puppet
    /// state (the second half of a step).
    pub fn run_update(&mut self) -> AppResult<()> {
        self.puppet.update().map_err(AppError::Engine)
    }

    /// Advances the scene by one bare fixed step (clock +
    /// evaluation, no player frame): the play surface's
    /// step-with-no-playback path.
    pub fn step_once(&mut self) -> AppResult<()> {
        self.step()
    }

    /// Assigns a batch of parameter values and advances one
    /// step (the named-state application path: every recipe
    /// value lands before the single evaluation, matching
    /// the CLI named-state semantics).
    pub fn assign_and_step(&mut self, assignments: &[(usize, Vec2)]) -> AppResult<()> {
        for (index, value) in assignments {
            self.puppet.params[*index].value = *value;
        }
        self.step()
    }

    /// Resolves a parameter name to its index, refusing
    /// unknown names.
    pub fn parameter_index(&self, name: &str) -> AppResult<usize> {
        self.puppet
            .param_index(name)
            .ok_or_else(|| AppError::Param(format!("unknown parameter '{name}'")))
    }

    /// Resolves named assignments to indexed ones. A scalar
    /// assignment carries `y: None` and resolves to the
    /// parameter's default y, matching the named-state recipe
    /// semantics of the preview surface.
    pub fn resolve_assignments(&self, named: &[NamedAssignment]) -> AppResult<Vec<(usize, Vec2)>> {
        let mut resolved = Vec::with_capacity(named.len());
        for assignment in named {
            let index = self.parameter_index(&assignment.name)?;
            let param = &self.puppet.params[index];
            let y = assignment.y.unwrap_or(param.defaults.y);
            resolved.push((index, Vec2::new(assignment.x, y)));
        }
        Ok(resolved)
    }
}

/// One `--param name=value` assignment before resolution:
/// `x` always present, `y` present only when the value text
/// carried both components.
#[derive(Debug, Clone, PartialEq)]
pub struct NamedAssignment {
    /// Parameter name, exactly as authored in the model.
    pub name: String,
    /// The x component.
    pub x: f32,
    /// The y component, when given.
    pub y: Option<f32>,
}

impl NamedAssignment {
    /// Parses `name=value` where value is `x` or `x,y`. The
    /// split is at the last `=` so names may carry any other
    /// character; both components must be finite numbers.
    pub fn parse(text: &str) -> AppResult<NamedAssignment> {
        let Some((name, value)) = text.rsplit_once('=') else {
            return Err(AppError::Param(format!(
                "'{text}' is not of the form name=value"
            )));
        };
        if name.is_empty() {
            return Err(AppError::Param(format!("'{text}' has an empty name")));
        }
        let mut parts = value.split(',');
        let x = parse_component(parts.next().unwrap_or(""), text)?;
        let y = match parts.next() {
            Some(part) => Some(parse_component(part, text)?),
            None => None,
        };
        if parts.next().is_some() {
            return Err(AppError::Param(format!(
                "'{text}' carries more than two value components"
            )));
        }
        Ok(NamedAssignment {
            name: name.to_string(),
            x,
            y,
        })
    }
}

fn parse_component(text: &str, whole: &str) -> AppResult<f32> {
    let value: f32 = text
        .trim()
        .parse()
        .map_err(|_| AppError::Param(format!("'{whole}' has a non-numeric component '{text}'")))?;
    if !value.is_finite() {
        return Err(AppError::Param(format!(
            "'{whole}' has a non-finite component '{text}'"
        )));
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::NamedAssignment;

    #[test]
    fn parses_scalar_and_vec2_assignments() {
        let scalar = NamedAssignment::parse("ParamAngleX=5").expect("scalar parses");
        assert_eq!(scalar.name, "ParamAngleX");
        assert_eq!(scalar.x, 5.0);
        assert_eq!(scalar.y, None);
        let vec2 = NamedAssignment::parse("Head:: Yaw-Pitch=0.75,-0.5").expect("vec2 parses");
        assert_eq!(vec2.name, "Head:: Yaw-Pitch");
        assert_eq!(vec2.x, 0.75);
        assert_eq!(vec2.y, Some(-0.5));
    }

    #[test]
    fn refuses_malformed_assignments() {
        assert!(NamedAssignment::parse("NoEquals").is_err());
        assert!(NamedAssignment::parse("=1").is_err());
        assert!(NamedAssignment::parse("A=1,2,3").is_err());
        assert!(NamedAssignment::parse("A=NaN").is_err());
        assert!(NamedAssignment::parse("A=inf").is_err());
        assert!(NamedAssignment::parse("A=").is_err());
    }
}
