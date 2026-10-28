//! The play-mode host: the engine's animation player plus
//! the transport state the shell and the session surface
//! share, driven on the scene's fixed-step clock.
//!
//! One play step is, in order: advance the puppet clock by
//! `STEP_SECONDS`, let the player advance by the same delta
//! and render its frame into the puppet's parameters (the
//! push path), then run one engine evaluation. That is the
//! reference host's frame order (animation player update,
//! then `Puppet.update`) pinned to the fixed step the rest
//! of the shell already uses, so a played puppet's state
//! hash is a function of its step count alone — the
//! property the Phase D exit gate compares against a
//! headless engine evaluation at equal step count.
//!
//! The host counts every step it advances (transport steps
//! and manual steps alike). `apply_state` applies one of
//! the model's EXT named-state recipes — the fixture rigs'
//! behaviour set — as a batch assignment plus one step.

use torogao_engine::AnimationPlayer;

use crate::error::{AppError, AppResult};
use crate::scene::{STEP_SECONDS, Scene};

/// Largest step count one `step` call may advance (the
/// session schema carries the same bound).
pub const STEP_CALL_MAX: u64 = 100_000;

/// A point-in-time read of the play host, shaped for the
/// session surface and the windowed transport row alike.
#[derive(Debug, Clone, Default)]
pub struct PlayState {
    /// Whether the transport is playing.
    pub playing: bool,
    /// Whether the transport is paused.
    pub paused: bool,
    /// Whether the transport is stopping (lead-out).
    pub stopping: bool,
    /// The current animation's name.
    pub animation: Option<String>,
    /// The playhead frame (rounded).
    pub frame: i32,
    /// The playhead floating frame.
    pub hframe: f32,
    /// Fixed steps advanced since the scene was installed.
    pub step_count: u64,
    /// Whether the current playback loops.
    pub looping: bool,
    /// Completed loops of the current playback.
    pub looped: i32,
    /// The current playback's speed multiplier.
    pub speed: f32,
    /// The last EXT named state applied through the host.
    pub applied_state: Option<String>,
}

/// The play-mode host state for one installed scene.
#[derive(Default)]
pub struct PlayHost {
    player: AnimationPlayer,
    current: Option<String>,
    step_count: u64,
    applied_state: Option<String>,
}

impl PlayHost {
    /// A fresh host (no scene stepped yet).
    pub fn new() -> PlayHost {
        PlayHost::default()
    }

    /// The engine player (transport widgets read it).
    pub fn player(&self) -> &AnimationPlayer {
        &self.player
    }

    /// Steps advanced since install.
    pub fn step_count(&self) -> u64 {
        self.step_count
    }

    /// The current animation's name, when one is loaded.
    pub fn current(&self) -> Option<&str> {
        self.current.as_deref()
    }

    /// Whether any playback is running (playing and not
    /// paused): the windowed loop steps the scene while
    /// this holds.
    pub fn is_advancing(&self) -> bool {
        let Some(name) = &self.current else {
            return false;
        };
        let Some(playback) = self.player.get(name) else {
            return false;
        };
        playback.playing() && !playback.paused()
    }

    /// A snapshot of the transport against `scene`.
    pub fn snapshot(&self, scene: &Scene) -> PlayState {
        let mut state = PlayState {
            step_count: self.step_count,
            speed: 1.0,
            applied_state: self.applied_state.clone(),
            ..Default::default()
        };
        if let Some(name) = &self.current {
            state.animation = Some(name.clone());
            if let Some(playback) = self.player.get(name) {
                state.playing = playback.playing();
                state.paused = playback.paused();
                state.stopping = playback.stopping();
                state.looping = playback.looping();
                state.looped = playback.looped();
                state.speed = playback.speed();
                if let Some((_, anim)) = scene
                    .puppet
                    .animations
                    .iter()
                    .find(|(anim_name, _)| anim_name == name)
                {
                    state.frame = playback.frame(anim);
                    state.hframe = playback.hframe(anim);
                }
            }
        }
        state
    }

    /// Plays `name` (or the current / first animation when
    /// `name` is None), with the reference play semantics:
    /// resuming a paused playback only unpauses; any other
    /// play restarts from frame 0.
    pub fn play(
        &mut self,
        scene: &Scene,
        name: Option<&str>,
        looping: bool,
        speed: Option<f32>,
    ) -> AppResult<()> {
        let name = match name {
            Some(name) => name.to_string(),
            None => match &self.current {
                Some(current) => current.clone(),
                None => scene
                    .puppet
                    .animations
                    .first()
                    .map(|(name, _)| name.clone())
                    .ok_or_else(|| {
                        AppError::Param("the model carries no animations".to_string())
                    })?,
            },
        };
        self.player
            .create_or_get(&scene.puppet, &name)
            .ok_or_else(|| AppError::Param(format!("unknown animation '{name}'")))?;
        let playback = self
            .player
            .get_mut(&name)
            .expect("playback exists after create_or_get");
        // The reference `play(loop, playLeadOut)`: a paused
        // playback only unpauses (its loop flag stands); any
        // other play restarts with the given loop flag.
        playback.play(looping, true);
        if let Some(speed) = speed {
            playback.set_speed(speed);
        }
        self.current = Some(name);
        Ok(())
    }

    /// Pauses the current playback (its frame keeps
    /// rendering on subsequent steps, as in the reference).
    pub fn pause(&mut self) {
        if let Some(name) = self.current.clone() {
            if let Some(playback) = self.player.get_mut(&name) {
                playback.pause();
            }
        }
    }

    /// Stops the current playback (lead-out rules apply
    /// unless `immediate`).
    pub fn stop(&mut self, scene: &Scene, immediate: bool) {
        if let Some(name) = self.current.clone() {
            if let Some((_, anim)) = scene
                .puppet
                .animations
                .iter()
                .find(|(anim_name, _)| anim_name == &name)
            {
                let anim = anim.clone();
                if let Some(playback) = self.player.get_mut(&name) {
                    playback.stop(&anim, immediate);
                }
            }
        }
    }

    /// Seeks the current playback to `frame`.
    pub fn seek(&mut self, scene: &Scene, frame: i32) -> AppResult<()> {
        let name = self
            .current
            .clone()
            .ok_or_else(|| AppError::Param("no animation is loaded in the player".to_string()))?;
        let anim = scene
            .puppet
            .animations
            .iter()
            .find(|(anim_name, _)| anim_name == &name)
            .map(|(_, anim)| anim.clone())
            .ok_or_else(|| AppError::Param(format!("unknown animation '{name}'")))?;
        let playback = self
            .player
            .get_mut(&name)
            .ok_or_else(|| AppError::Param(format!("unknown animation '{name}'")))?;
        playback.seek(&anim, frame);
        Ok(())
    }

    /// Sets the current playback's speed multiplier.
    pub fn set_speed(&mut self, speed: f32) {
        if let Some(name) = self.current.clone() {
            if let Some(playback) = self.player.get_mut(&name) {
                playback.set_speed(speed);
            }
        }
    }

    /// Sets the current playback's loop flag.
    pub fn set_looping(&mut self, looping: bool) {
        if let Some(name) = self.current.clone() {
            if let Some(playback) = self.player.get_mut(&name) {
                playback.set_looping(looping);
            }
        }
    }

    /// Advances the scene by `steps` fixed steps through
    /// the player (see the module note for the per-step
    /// order). With no playback loaded this is a bare
    /// evaluation step sequence (physics and automations
    /// still run).
    pub fn step(&mut self, scene: &mut Scene, steps: u64) -> AppResult<()> {
        if steps > STEP_CALL_MAX {
            return Err(AppError::Param(format!(
                "step count {steps} exceeds the per-call bound {STEP_CALL_MAX}"
            )));
        }
        for _ in 0..steps {
            scene.advance_clock();
            self.player.update(STEP_SECONDS as f32, &mut scene.puppet);
            scene.run_update()?;
            self.step_count += 1;
        }
        Ok(())
    }

    /// Applies one EXT named-state recipe (`state`) to the
    /// scene: every recipe value is assigned (scalar
    /// semantics: the value addresses x; y takes the
    /// parameter's default, as in the CLI named-state path)
    /// and the scene advances one step.
    pub fn apply_state(&mut self, scene: &mut Scene, state: &str) -> AppResult<()> {
        let recipe = named_state_recipe(scene, state)?;
        let mut assignments = Vec::with_capacity(recipe.len());
        for (param_name, value) in &recipe {
            let index = scene.parameter_index(param_name)?;
            let defaults_y = scene.puppet.params[index].defaults.y;
            assignments.push((index, torogao_engine::Vec2::new(*value, defaults_y)));
        }
        scene.assign_and_step(&assignments)?;
        self.step_count += 1;
        self.applied_state = Some(state.to_string());
        Ok(())
    }
}

/// Reads one named-state recipe from the puppet's
/// `io.ntranch.torogao` EXT section (`states`: a list of
/// `{name, recipe: {param: value}}`). Names every failure:
/// no EXT section, no states list, unknown state, and a
/// recipe value that is not a number all refuse typed.
pub fn named_state_recipe(scene: &Scene, state: &str) -> AppResult<Vec<(String, f32)>> {
    let bytes = scene
        .puppet
        .ext_section("io.ntranch.torogao")
        .ok_or_else(|| AppError::Param("the model carries no torogao EXT section".to_string()))?;
    let doc: serde_json::Value = serde_json::from_slice(bytes)
        .map_err(|err| AppError::Format(format!("torogao EXT section is not valid JSON: {err}")))?;
    let states = doc
        .get("states")
        .and_then(|value| value.as_array())
        .ok_or_else(|| AppError::Param("the EXT section carries no states list".to_string()))?;
    for entry in states {
        if entry.get("name").and_then(|value| value.as_str()) != Some(state) {
            continue;
        }
        let recipe = entry
            .get("recipe")
            .and_then(|value| value.as_object())
            .ok_or_else(|| AppError::Param(format!("state '{state}' carries no recipe object")))?;
        let mut out = Vec::with_capacity(recipe.len());
        for (param, value) in recipe {
            let value = value.as_f64().ok_or_else(|| {
                AppError::Param(format!(
                    "state '{state}' recipe value for '{param}' is not a number"
                ))
            })? as f32;
            if !value.is_finite() {
                return Err(AppError::Param(format!(
                    "state '{state}' recipe value for '{param}' is not finite"
                )));
            }
            out.push((param.clone(), value));
        }
        // Recipe application order is observable only
        // through the final assignment set (all values land
        // before the one evaluation), but keep the order
        // deterministic across runs anyway.
        out.sort_by(|a, b| a.0.cmp(&b.0));
        return Ok(out);
    }
    Err(AppError::Param(format!(
        "the model carries no state named '{state}'"
    )))
}
