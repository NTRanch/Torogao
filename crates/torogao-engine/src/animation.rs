//! Animation data and playback: the payload records, the
//! lane sampler, and the player — a port of the reference
//! `Animation` / `AnimationLane` / `AnimationPlayer` /
//! `AnimationPlayback` surface.
//!
//! Playback semantics are the reference's: a playback owns
//! a clock (`time`, in seconds), transport flags
//! (playing / paused / looping / stopping, lead-in and
//! lead-out points), a strength multiplier, and a speed
//! multiplier. Each update advances the clock and *renders*
//! the current frame: every lane samples its keyframes at
//! the floating frame (`time / timestep`) and pushes the
//! sampled value into its parameter through the parameter
//! push path (`push_offset_axis` with the lane's merge
//! mode) — the puppet's next evaluation consumes the
//! pushes exactly as it consumes automation pushes. A
//! `Forced` lane therefore writes the parameter value
//! outright (absolute application); `Additive` /
//! `Multiplicative` / `Weighted` lanes compose on top of
//! the standing value for one evaluation (additive
//! application). The animation-level `additive` flag and
//! `animation_weight` are payload metadata the reference
//! engine itself never consumes during playback (the
//! creator's editor reads them); they are carried here
//! verbatim, and the merge mode is the application
//! semantics, as it is in the reference.
//!
//! Two documented deviations, both safety-side:
//! - Lane keyframes are stable-sorted by frame at decode
//!   (the reference sorts on edit via `updateFrames` and
//!   its sampler assumes ascending order; sorting at the
//!   load boundary makes that assumption true for every
//!   payload instead of only edited ones).
//! - A lane whose parameter uuid does not resolve is
//!   skipped at render time. The reference resolves lanes
//!   at finalize and dereferences the result unchecked;
//!   skipping is the only behaviour available for a lane
//!   that can never apply, and it changes nothing for any
//!   lane that resolves.
//! - `speed` is applied to the update delta. The reference
//!   stores the field (clamped 1..=10) but its `update`
//!   never multiplies by it — and its property setter is
//!   typed `bool`, so the field was never exercisable as
//!   shipped. The port applies it, because a play surface
//!   whose speed control is inert would be a hidden
//!   behaviour of the opposite kind.

use crate::math::clamp_f32;
use crate::param::{InterpolateMode, MergeMode};
use crate::puppet::Puppet;
use serde_json::Value;
use torogao_format::value::{jget, jnum, jstr};

/// One keyframe of an animation lane.
#[derive(Debug, Clone, Copy)]
pub struct Keyframe {
    /// The frame (step index) at which this key occurs.
    pub frame: i32,
    /// The keyed value.
    pub value: f32,
    /// Interpolation tension (Bezier lanes).
    pub tension: f32,
}

/// One animation lane (one parameter axis over time).
#[derive(Debug, Clone)]
pub struct AnimationLane {
    /// The animated parameter's uuid.
    pub param_uuid: u32,
    /// The animated axis.
    pub axis: i32,
    /// Interpolation mode between keyframes.
    pub interpolation: InterpolateMode,
    /// How sampled values merge into the parameter.
    pub merge_mode: MergeMode,
    /// Keyframes, ascending by frame.
    pub frames: Vec<Keyframe>,
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a * (1.0 - t) + b * t
}

/// The reference cubic (inmath's coefficient form — the
/// same polynomial the binding interpolator uses).
fn cubic(p0: f32, p1: f32, p2: f32, p3: f32, t: f32) -> f32 {
    let a = -0.5 * p0 + 1.5 * p1 - 1.5 * p2 + 0.5 * p3;
    let b = p0 - 2.5 * p1 + 2.0 * p2 - 0.5 * p3;
    let c = -0.5 * p0 + 0.5 * p2;
    let d = p1;
    a * (t * t * t) + b * (t * t) + c * t + d
}

/// The reference hermite (inmath): endpoints `x`, `y` with
/// tangents `tx`, `ty`.
fn hermite(x: f32, tx: f32, y: f32, ty: f32, t: f32) -> f32 {
    let h1 = 2.0 * t * t * t - 3.0 * t * t + 1.0;
    let h2 = -2.0 * t * t * t + 3.0 * t * t;
    let h3 = t * t * t - 2.0 * t * t + t;
    let h4 = t * t * t - t * t;
    h1 * x + h3 * tx + h2 * y + h4 * ty
}

impl AnimationLane {
    /// Samples the lane at floating frame `frame` (the
    /// reference `AnimationLane.get`): the first keyframe
    /// at or after `frame` closes the segment, and the
    /// segment's interpolation mode shapes the value.
    /// `snap` floors the frame first (the player's
    /// snap-to-framerate option).
    pub fn sample(&self, frame: f32, snap: bool) -> f32 {
        if self.frames.is_empty() {
            return 0.0;
        }
        let frame = if snap { frame.floor() } else { frame };
        if self.frames.len() == 1 {
            return self.frames[0].value;
        }
        for i in 0..self.frames.len() {
            if (self.frames[i].frame as f32) < frame {
                continue;
            }
            if i == 0 {
                return self.frames[0].value;
            }
            let tonext = self.frames[i].frame as f32 - frame;
            let ilen = (self.frames[i].frame - self.frames[i - 1].frame) as f32;
            let t = 1.0 - tonext / ilen;
            let tension = self.frames[i].tension;
            return match self.interpolation {
                InterpolateMode::Nearest => {
                    if t > 0.5 {
                        self.frames[i].value
                    } else {
                        self.frames[i - 1].value
                    }
                }
                InterpolateMode::Stepped => self.frames[i - 1].value,
                InterpolateMode::Linear => lerp(self.frames[i - 1].value, self.frames[i].value, t),
                InterpolateMode::Cubic => {
                    let len = self.frames.len();
                    let prev = self.frames[i.saturating_sub(2)].value;
                    let curr = self.frames[i - 1].value;
                    let next1 = self.frames[i.min(len - 1)].value;
                    let next2 = self.frames[(i + 1).min(len - 1)].value;
                    cubic(prev, curr, next1, next2, t)
                }
                InterpolateMode::Bezier => lerp(
                    self.frames[i - 1].value,
                    self.frames[i].value,
                    clamp_f32(hermite(0.0, 2.0 * tension, 1.0, 2.0 * tension, t), 0.0, 1.0),
                ),
            };
        }
        self.frames[self.frames.len() - 1].value
    }
}

/// One named animation.
#[derive(Debug, Clone)]
pub struct Animation {
    /// Whether the animation is additive (payload metadata;
    /// see the module note — application semantics live in
    /// the lanes' merge modes).
    pub additive: bool,
    /// The animation weight (payload metadata, carried
    /// verbatim; the reference player does not consume it).
    pub animation_weight: f32,
    /// Lanes.
    pub lanes: Vec<AnimationLane>,
    /// Length in steps.
    pub length: i32,
    /// Seconds per step.
    pub timestep: f32,
    /// Frame where the lead-in ends (-1 when absent).
    pub lead_in: i32,
    /// Frame where the lead-out starts (-1 when absent).
    pub lead_out: i32,
}

impl Animation {
    /// Duration in seconds: length × timestep (the motion
    /// event validator's formula).
    pub fn duration(&self) -> f64 {
        self.length as f64 * self.timestep as f64
    }

    /// Total keyframes across all lanes.
    pub fn keyframe_count(&self) -> usize {
        self.lanes.iter().map(|lane| lane.frames.len()).sum()
    }

    /// Decodes an animation record.
    pub fn from_payload(raw: &Value) -> Animation {
        let lanes = match jget(raw, "lanes") {
            Value::Array(items) => items
                .iter()
                .map(|lane| {
                    let mut frames: Vec<Keyframe> = match jget(lane, "keyframes") {
                        Value::Array(frames) => frames
                            .iter()
                            .map(|frame| Keyframe {
                                frame: jnum(frame, "frame", 0.0) as i32,
                                value: jnum(frame, "value", 0.0) as f32,
                                tension: jnum(frame, "tension", 0.5) as f32,
                            })
                            .collect(),
                        _ => Vec::new(),
                    };
                    frames.sort_by_key(|frame| frame.frame);
                    AnimationLane {
                        param_uuid: jnum(lane, "uuid", 0.0) as u32,
                        axis: jnum(lane, "target", 0.0) as i32,
                        interpolation: InterpolateMode::parse(jstr(lane, "interpolation"))
                            .unwrap_or(InterpolateMode::Linear),
                        merge_mode: MergeMode::parse(jstr(lane, "merge_mode"))
                            .unwrap_or(MergeMode::Forced),
                        frames,
                    }
                })
                .collect(),
            _ => Vec::new(),
        };
        Animation {
            additive: matches!(jget(raw, "additive"), Value::Bool(true)),
            animation_weight: jnum(raw, "animationWeight", 1.0) as f32,
            lanes,
            length: jnum(raw, "length", 0.0) as i32,
            timestep: jnum(raw, "timestep", 0.0166) as f32,
            lead_in: jnum(raw, "leadIn", -1.0) as i32,
            lead_out: jnum(raw, "leadOut", -1.0) as i32,
        }
    }
}

/// One running (or stopped) playback of one animation —
/// the reference `AnimationPlayback` state machine. The
/// playback names its animation by the puppet's animation
/// index, so it carries no reference into the puppet.
#[derive(Debug, Clone)]
pub struct AnimationPlayback {
    name: String,
    anim_index: usize,
    valid: bool,
    play_lead_out: bool,
    paused: bool,
    playing: bool,
    looping: bool,
    stopping: bool,
    time: f32,
    strength: f32,
    speed: f32,
    looped: i32,
}

impl AnimationPlayback {
    /// The animation's name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The puppet animation index this playback runs.
    pub fn anim_index(&self) -> usize {
        self.anim_index
    }

    /// Whether the playback is still valid (not destroyed).
    pub fn is_valid(&self) -> bool {
        self.valid
    }

    /// Whether the transport is playing.
    pub fn playing(&self) -> bool {
        self.playing
    }

    /// Whether the transport is paused.
    pub fn paused(&self) -> bool {
        self.paused
    }

    /// Whether the transport is stopping (playing its
    /// lead-out after a stop request).
    pub fn stopping(&self) -> bool {
        self.stopping
    }

    /// Whether playback loops.
    pub fn looping(&self) -> bool {
        self.looping
    }

    /// Sets whether playback loops.
    pub fn set_looping(&mut self, value: bool) {
        self.looping = value;
    }

    /// How many times the animation has looped.
    pub fn looped(&self) -> i32 {
        self.looped
    }

    /// The speed multiplier.
    pub fn speed(&self) -> f32 {
        self.speed
    }

    /// Sets the speed multiplier, clamped to 1..=10 (the
    /// reference setter's clamp).
    pub fn set_speed(&mut self, value: f32) {
        self.speed = clamp_f32(value, 1.0, 10.0);
    }

    /// The strength multiplier.
    pub fn strength(&self) -> f32 {
        self.strength
    }

    /// Sets the strength multiplier, clamped to 0..=1.
    pub fn set_strength(&mut self, value: f32) {
        self.strength = clamp_f32(value, 0.0, 1.0);
    }

    /// Playback time, in seconds.
    pub fn time(&self) -> f32 {
        self.time
    }

    /// The current frame (rounded), against `anim`.
    pub fn frame(&self, anim: &Animation) -> i32 {
        (self.time / anim.timestep).round() as i32
    }

    /// The current floating frame, against `anim`.
    pub fn hframe(&self, anim: &Animation) -> f32 {
        self.time / anim.timestep
    }

    /// Whether the animation has a lead-in.
    pub fn has_lead_in(&self, anim: &Animation) -> bool {
        anim.lead_in > 0 && anim.lead_in + 1 < anim.length
    }

    /// Whether the animation has a lead-out.
    pub fn has_lead_out(&self, anim: &Animation) -> bool {
        anim.lead_out > 0 && anim.lead_out + 1 < anim.length
    }

    /// The frame looping begins at.
    pub fn loop_point_begin(&self, anim: &Animation) -> i32 {
        if self.has_lead_in(anim) {
            anim.lead_in
        } else {
            0
        }
    }

    /// The frame looping ends at.
    pub fn loop_point_end(&self, anim: &Animation) -> i32 {
        if self.has_lead_out(anim) {
            anim.lead_out
        } else {
            anim.length
        }
    }

    /// Whether the playback is in its lead-out.
    pub fn is_playing_lead_out(&self, anim: &Animation) -> bool {
        ((self.playing && !self.looping) || self.stopping)
            && self.play_lead_out
            && self.frame(anim) < anim.length
    }

    /// Whether the playback is running (main part or
    /// lead-out).
    pub fn is_running(&self, anim: &Animation) -> bool {
        self.playing || self.is_playing_lead_out(anim)
    }

    /// Whether the playhead has reached the animation end.
    pub fn eof(&self, anim: &Animation) -> bool {
        self.frame(anim) >= anim.length
    }

    /// Plays (or resumes) the animation. Resuming a paused
    /// playback only unpauses; any other play restarts from
    /// frame 0 (the reference `play`).
    pub fn play(&mut self, looping: bool, play_lead_out: bool) {
        if self.paused {
            self.paused = false;
        } else {
            self.looped = 0;
            self.time = 0.0;
            self.stopping = false;
            self.playing = true;
            self.looping = looping;
            self.play_lead_out = play_lead_out;
        }
    }

    /// Pauses the animation (its frame keeps rendering on
    /// update, as in the reference).
    pub fn pause(&mut self) {
        self.paused = true;
    }

    /// Stops the animation: immediately when asked, at
    /// frame 0, while paused, or when the animation has no
    /// lead-out; otherwise the lead-out plays and the stop
    /// completes when it ends (the reference `stop`).
    pub fn stop(&mut self, anim: &Animation, immediate: bool) {
        if self.stopping {
            return;
        }
        let should_stop_immediate =
            immediate || self.frame(anim) == 0 || self.paused || !self.has_lead_out(anim);
        self.stopping = !should_stop_immediate;
        self.looping = false;
        self.paused = false;
        self.playing = false;
        self.play_lead_out = !should_stop_immediate;
        if should_stop_immediate {
            self.time = 0.0;
            self.looped = 0;
        }
    }

    /// Seeks to `frame` (clamped to the animation's frame
    /// range); the loop count resets.
    pub fn seek(&mut self, anim: &Animation, frame: i32) {
        let clamped = clamp_f32(frame as f32, 0.0, anim.length as f32);
        self.time = clamped * anim.timestep;
        self.looped = 0;
    }

    /// Advances the playback by `delta` seconds and renders
    /// its frame into the puppet (the reference `update`:
    /// loop wrap, end clamp, lead-out completion, then
    /// render; a paused playback renders without advancing).
    pub fn update(&mut self, delta: f32, puppet: &mut Puppet, snap_to_framerate: bool) {
        let Some(anim) = puppet
            .animations
            .get(self.anim_index)
            .map(|(_, anim)| anim.clone())
        else {
            return;
        };
        if !self.valid || !self.is_running(&anim) {
            return;
        }
        if self.paused {
            self.render(puppet, snap_to_framerate);
            return;
        }
        self.time += delta * self.speed;
        if !self.is_playing_lead_out(&anim)
            && self.looping
            && self.frame(&anim) >= self.loop_point_end(&anim)
        {
            self.time = self.loop_point_begin(&anim) as f32 * anim.timestep;
            self.looped += 1;
        }
        if self.frame(&anim) + 1 >= anim.length {
            self.time = (anim.length - 1) as f32 * anim.timestep;
        }
        self.render(puppet, snap_to_framerate);
        if !self.looping && self.is_playing_lead_out(&anim) && self.frame(&anim) + 1 >= anim.length
        {
            self.playing = false;
            self.play_lead_out = false;
            self.stopping = false;
            self.time = 0.0;
            self.looped = 0;
        }
    }

    /// Renders the current frame: every lane samples at the
    /// floating frame and pushes into its parameter through
    /// the push path, scaled by strength.
    pub fn render(&self, puppet: &mut Puppet, snap_to_framerate: bool) {
        let Some(anim) = puppet
            .animations
            .get(self.anim_index)
            .map(|(_, anim)| anim.clone())
        else {
            return;
        };
        let strength = clamp_f32(self.strength, 0.0, 1.0);
        let hframe = self.hframe(&anim);
        for lane in &anim.lanes {
            let Some(index) = puppet
                .params
                .iter()
                .position(|param| param.uuid == lane.param_uuid)
            else {
                continue;
            };
            let value = lane.sample(hframe, snap_to_framerate) * strength;
            puppet.params[index].push_offset_axis(lane.axis, value, lane.merge_mode, 1.0);
        }
    }
}

/// The player: owns the playbacks of one puppet (the
/// reference `AnimationPlayer`). The puppet is passed per
/// call, as the reference's player holds it by reference.
#[derive(Debug, Clone, Default)]
pub struct AnimationPlayer {
    playbacks: Vec<AnimationPlayback>,
    /// Whether lane sampling snaps to whole frames.
    pub snap_to_framerate: bool,
}

impl AnimationPlayer {
    /// A player with no playbacks.
    pub fn new() -> AnimationPlayer {
        AnimationPlayer::default()
    }

    /// The live playbacks, in creation order.
    pub fn playbacks(&self) -> &[AnimationPlayback] {
        &self.playbacks
    }

    /// Finds a playback by animation name.
    pub fn get(&self, name: &str) -> Option<&AnimationPlayback> {
        self.playbacks.iter().find(|p| p.name == name && p.valid)
    }

    /// Finds a playback by animation name, mutably.
    pub fn get_mut(&mut self, name: &str) -> Option<&mut AnimationPlayback> {
        self.playbacks
            .iter_mut()
            .find(|p| p.name == name && p.valid)
    }

    /// Returns the playback for `name`, creating it when
    /// the puppet carries the animation; None when it does
    /// not (the reference `createOrGet`).
    pub fn create_or_get(&mut self, puppet: &Puppet, name: &str) -> Option<usize> {
        if let Some(index) = self
            .playbacks
            .iter()
            .position(|p| p.name == name && p.valid)
        {
            return Some(index);
        }
        let anim_index = puppet
            .animations
            .iter()
            .position(|(anim_name, _)| anim_name == name)?;
        self.playbacks.push(AnimationPlayback {
            name: name.to_string(),
            anim_index,
            valid: true,
            play_lead_out: false,
            paused: false,
            playing: false,
            looping: false,
            stopping: false,
            time: 0.0,
            strength: 1.0,
            speed: 1.0,
            looped: 0,
        });
        Some(self.playbacks.len() - 1)
    }

    /// Convenience: create (if needed) and play `name`
    /// (the reference `play`: no loop, lead-out played).
    pub fn play(&mut self, puppet: &Puppet, name: &str) -> Option<usize> {
        let index = self.create_or_get(puppet, name)?;
        self.playbacks[index].play(false, true);
        Some(index)
    }

    /// Advances every valid playback by `delta` seconds.
    pub fn update(&mut self, delta: f32, puppet: &mut Puppet) {
        let snap = self.snap_to_framerate;
        for index in 0..self.playbacks.len() {
            if self.playbacks[index].valid {
                self.playbacks[index].update(delta, puppet, snap);
            }
        }
    }

    /// Renders every playback's current frame without
    /// advancing any clock (the reference `prerenderAll`).
    pub fn prerender_all(&self, puppet: &mut Puppet) {
        for playback in &self.playbacks {
            if playback.valid {
                playback.render(puppet, self.snap_to_framerate);
            }
        }
    }

    /// Stops every playback (the reference `stopAll`).
    pub fn stop_all(&mut self, puppet: &Puppet, immediate: bool) {
        for index in 0..self.playbacks.len() {
            if let Some((_, anim)) = puppet.animations.get(self.playbacks[index].anim_index) {
                let anim = anim.clone();
                self.playbacks[index].stop(&anim, immediate);
            }
        }
    }

    /// Destroys every playback (the reference
    /// `destroyAll`).
    pub fn destroy_all(&mut self) {
        for playback in &mut self.playbacks {
            playback.valid = false;
        }
        self.playbacks.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::{Animation, AnimationLane, AnimationPlayer};
    use crate::param::{InterpolateMode, MergeMode};
    use crate::puppet::Puppet;
    use serde_json::json;
    use torogao_format::{decode_payload, read_inp};

    fn rich_puppet() -> Puppet {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tests/fixtures/p2/fixtures/rich.inx"
        );
        let bytes = std::fs::read(path).expect("rich fixture");
        let inp = read_inp(&bytes).expect("container");
        let doc = decode_payload(&inp.payload).expect("payload");
        Puppet::load(&doc, &inp).expect("puppet loads")
    }

    #[test]
    fn playback_drives_the_blink_lane_through_the_push_path() {
        let mut puppet = rich_puppet();
        let angle = puppet.param_index("Angle").expect("Angle param");
        let mut player = AnimationPlayer::new();
        player.play(&puppet, "Blink").expect("Blink exists");
        // Half-way to the frame-30 key (0.5): the Forced lane
        // writes the parameter value at render.
        let timestep = puppet.animations[0].1.timestep;
        player.update(timestep * 15.0, &mut puppet);
        assert!((puppet.params[angle].value.x - 0.25).abs() < 1e-4);
        // The puppet's own evaluation consumes the value.
        puppet.clock.now += 1.0 / 60.0;
        puppet.clock.tick();
        puppet.update().expect("update");
        assert!((puppet.params[angle].last_internal.x - 0.25).abs() < 1e-4);
    }

    #[test]
    fn playback_loops_and_counts() {
        let mut puppet = rich_puppet();
        let mut player = AnimationPlayer::new();
        let index = player.create_or_get(&puppet, "Blink").expect("Blink");
        player.playbacks[index].play(true, true);
        let timestep = puppet.animations[0].1.timestep;
        // One full length plus a step: the playhead wraps to
        // the loop begin and the loop counter moves.
        for _ in 0..61 {
            player.update(timestep, &mut puppet);
        }
        let playback = &player.playbacks[index];
        assert_eq!(playback.looped(), 1);
        assert!(playback.playing());
    }

    #[test]
    fn pause_holds_the_frame_and_stop_resets() {
        let mut puppet = rich_puppet();
        let angle = puppet.param_index("Angle").expect("Angle param");
        let mut player = AnimationPlayer::new();
        let index = player.play(&puppet, "Blink").expect("Blink");
        let timestep = puppet.animations[0].1.timestep;
        player.update(timestep * 15.0, &mut puppet);
        player.playbacks[index].pause();
        player.update(timestep * 15.0, &mut puppet);
        // Paused: time held, frame re-rendered at the same
        // sampled value.
        assert!((player.playbacks[index].time() - timestep * 15.0).abs() < 1e-6);
        assert!((puppet.params[angle].value.x - 0.25).abs() < 1e-4);
        // No lead-out on Blink: stop is immediate.
        let anim = puppet.animations[0].1.clone();
        player.playbacks[index].stop(&anim, false);
        assert_eq!(player.playbacks[index].time(), 0.0);
        assert!(!player.playbacks[index].playing());
    }

    #[test]
    fn seek_lands_on_the_sampled_value() {
        let mut puppet = rich_puppet();
        let angle = puppet.param_index("Angle").expect("Angle param");
        let mut player = AnimationPlayer::new();
        let index = player.play(&puppet, "Blink").expect("Blink");
        let anim = puppet.animations[0].1.clone();
        player.playbacks[index].seek(&anim, 30);
        player.playbacks[index].render(&mut puppet, false);
        assert!((puppet.params[angle].value.x - 0.5).abs() < 1e-6);
    }

    fn lane(interpolation: InterpolateMode, frames: &[(i32, f32)]) -> AnimationLane {
        AnimationLane {
            param_uuid: 1,
            axis: 0,
            interpolation,
            merge_mode: MergeMode::Forced,
            frames: frames
                .iter()
                .map(|(frame, value)| super::Keyframe {
                    frame: *frame,
                    value: *value,
                    tension: 0.5,
                })
                .collect(),
        }
    }

    #[test]
    fn decodes_the_corpus_blink_shape() {
        // The corpus's one real animation record (rich.inx):
        // keyframes carry `frame`, interpolation is a name,
        // merge_mode is absent (the lane default, Forced).
        let raw = json!({
            "timestep": 0.0166, "additive": false, "animationWeight": 1.0,
            "length": 60, "leadIn": -1, "leadOut": -1,
            "lanes": [{"interpolation": "Linear", "uuid": 2001, "target": 0,
                       "keyframes": [{"frame": 0, "value": 0.0},
                                     {"frame": 30, "value": 0.5}]}]
        });
        let anim = Animation::from_payload(&raw);
        assert_eq!(anim.length, 60);
        assert_eq!(anim.lead_in, -1);
        assert_eq!(anim.lanes.len(), 1);
        assert_eq!(anim.lanes[0].param_uuid, 2001);
        assert_eq!(anim.lanes[0].merge_mode, MergeMode::Forced);
        assert_eq!(anim.lanes[0].frames[1].frame, 30);
        assert_eq!(anim.lanes[0].frames[1].value, 0.5);
    }

    #[test]
    fn linear_lane_interpolates_and_clamps() {
        let lane = lane(InterpolateMode::Linear, &[(0, 0.0), (30, 0.5)]);
        assert_eq!(lane.sample(0.0, false), 0.0);
        assert!((lane.sample(15.0, false) - 0.25).abs() < 1e-6);
        assert_eq!(lane.sample(30.0, false), 0.5);
        assert_eq!(lane.sample(45.0, false), 0.5);
        assert_eq!(lane.sample(-3.0, false), 0.0);
    }

    #[test]
    fn nearest_and_stepped_lanes_switch_as_the_reference() {
        let frames = &[(0, 1.0), (10, 3.0)];
        let nearest = lane(InterpolateMode::Nearest, frames);
        assert_eq!(nearest.sample(4.0, false), 1.0);
        assert_eq!(nearest.sample(6.0, false), 3.0);
        let stepped = lane(InterpolateMode::Stepped, frames);
        assert_eq!(stepped.sample(9.9, false), 1.0);
        // At the exact key frame the segment that closes is
        // still the previous one (the sampler takes the
        // first key >= the frame and steps return its
        // predecessor) — the reference's behaviour, pinned.
        assert_eq!(stepped.sample(10.0, false), 1.0);
        assert_eq!(stepped.sample(10.5, false), 3.0);
    }

    #[test]
    fn cubic_lane_passes_through_its_keys() {
        let cubic_lane = lane(
            InterpolateMode::Cubic,
            &[(0, 0.0), (10, 1.0), (20, 0.0), (30, 1.0)],
        );
        assert!((cubic_lane.sample(10.0, false) - 1.0).abs() < 1e-6);
        assert!((cubic_lane.sample(20.0, false) - 0.0).abs() < 1e-6);
        // A constant lane stays constant under cubic.
        let flat = lane(InterpolateMode::Cubic, &[(0, 2.0), (10, 2.0)]);
        assert!((flat.sample(5.0, false) - 2.0).abs() < 1e-6);
    }

    #[test]
    fn bezier_lane_with_half_tension_is_linear() {
        // hermite(0, 1, 1, 1, t) == t, so tension 0.5
        // reproduces the linear ramp.
        let lane = lane(InterpolateMode::Bezier, &[(0, 0.0), (10, 2.0)]);
        assert!((lane.sample(5.0, false) - 1.0).abs() < 1e-5);
    }

    #[test]
    fn snap_floors_the_frame() {
        let lane = lane(InterpolateMode::Linear, &[(0, 0.0), (30, 0.5)]);
        assert_eq!(lane.sample(15.9, true), lane.sample(15.0, false));
    }
}
