//! The companion core: the session host's live state —
//! one `ShellState` plus one `PlayHost` over Prima's scene
//! — with no presentation attached. Both window surfaces
//! (the Wayland layer-shell and the winit fallback) drive
//! exactly this object, and the session server the brief
//! requires is the windowed shell's own arrangement
//! (`torogao_app::session_host::start_window_session`): a
//! WebSocket transport thread whose bridge requests the
//! frame loop drains between frames, each answered through
//! an `AppBridge` built over this same shell + play host.
//!
//! There is no second player and no companion-private
//! scene: local pointer actions enter through
//! `AppBridge::play_control` — the very entry point the
//! session host dispatches the registry's `play_control`
//! tool to after `decide()` admits a call — and remote
//! calls land in the same place through the bridge queue.
//! That is the "no side-channel state" property
//! (PROGRAM.md §3.4) stated as code structure.

use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;

use torogao_app::player::PlayHost;
use torogao_app::scene::Scene;
use torogao_app::session_host::{AppBridge, BridgeRequest};
use torogao_app::state::ShellState;
use torogao_app::state_hash::evaluated_state_hash;
use torogao_session::host::LiveBridge;

use crate::cast::CastMember;
use crate::error::{CompanionError, CompanionResult};
use crate::gaze::{GazeChannels, GazeDriver};

/// The ambient performance (Daydream on loop) and the
/// click performance (Greeting, once through).
pub const AMBIENT_ANIMATION: &str = "Daydream";
/// The click reaction performance.
pub const CLICK_ANIMATION: &str = "Greeting";
/// Her face's anchor in surface fractions (x, y): the
/// cast rigs share one portrait framing — full body,
/// head in the top band, horizontally centred — so the
/// pointer's direction is measured from this point.
/// Declared here because the gaze gate's scripted paths
/// are written against the same anchor.
pub const FACE_ANCHOR: (f64, f64) = (0.5, 0.14);
/// The governed viewing distance for the pointer
/// conversion, in surface heights: the cursor sits this
/// far from her face plane, so a half-surface horizontal
/// offset reads as atan(0.5 × aspect / 1.35).
pub const VIEW_DISTANCE_HEIGHTS: f64 = 1.35;

/// The companion's live host state.
pub struct CompanionCore {
    shell: ShellState,
    play: PlayHost,
    session_rx: Option<Receiver<BridgeRequest>>,
    /// The hosted session's id, when serving.
    pub session_id: Option<String>,
    /// The model path the scene was loaded from (the
    /// right-click menu's "open Torogao" reopens it).
    pub model_path: PathBuf,
    /// The directory the cast's repertoire fixtures live
    /// in (the loaded model's parent): character switches
    /// resolve inside it.
    cast_dir: PathBuf,
    /// The cast member on stage, when the loaded model is
    /// one of the governed roster.
    cast_member: Option<&'static CastMember>,
    /// The Addition G gaze driver (gaze-follow, idle
    /// saccades, blink discipline, quiet salience).
    gaze: GazeDriver,
    /// The driver's parameter channels in the current
    /// scene, resolved at every scene install.
    gaze_channels: GazeChannels,
    /// Channel names the current rig does not carry (the
    /// load-time gap report; empty for the governed cast).
    gaze_gaps: Vec<&'static str>,
}

impl CompanionCore {
    /// Loads the model, evaluates it fresh under the shared
    /// protocol, installs it in a fresh shell, and — when a
    /// session directory is given — starts the session
    /// server (WebSocket transport on `session_port`)
    /// exactly as the windowed shell does in co-presence
    /// mode. The ambient performance is started before
    /// returning, so the first presented frame is already
    /// her at rest in Daydream.
    pub fn load(
        model_path: &Path,
        session_dir: Option<&Path>,
        session_port: Option<u16>,
        held_by: &str,
    ) -> CompanionResult<CompanionCore> {
        let mut scene =
            Scene::load_file(model_path).map_err(|err| CompanionError::model(err.to_string()))?;
        scene
            .evaluate_fresh(&[])
            .map_err(|err| CompanionError::model(err.to_string()))?;
        let mut shell = ShellState::new();
        shell.install_scene(scene);
        let mut core = CompanionCore {
            shell,
            play: PlayHost::new(),
            session_rx: None,
            session_id: None,
            model_path: model_path.to_path_buf(),
            cast_dir: model_path
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_default(),
            cast_member: crate::cast::member_for_path(model_path),
            gaze: GazeDriver::new(),
            gaze_channels: GazeChannels::default(),
            gaze_gaps: Vec::new(),
        };
        core.refresh_gaze_channels()?;
        if let Some(dir) = session_dir {
            let port = session_port.unwrap_or(torogao_session::transport::DEFAULT_WS_PORT);
            let (rx, session_id) =
                torogao_app::session_host::start_window_session(&core.shell, dir, port, held_by)
                    .map_err(|err| CompanionError::new("TG-COMP-SESSION", err.to_string()))?;
            core.session_rx = Some(rx);
            core.session_id = Some(session_id);
        }
        core.ensure_ambient()?;
        Ok(core)
    }

    /// The hosted scene, when one is installed (startup
    /// guarantees one; the `Option` is the shell's shape).
    pub fn scene_mut(&mut self) -> CompanionResult<&mut Scene> {
        self.shell
            .scene
            .as_mut()
            .ok_or_else(|| CompanionError::model("no scene is installed"))
    }

    /// The hosted scene, read-only.
    pub fn scene(&self) -> CompanionResult<&Scene> {
        self.shell
            .scene
            .as_ref()
            .ok_or_else(|| CompanionError::model("no scene is installed"))
    }

    /// Runs `f` against an `AppBridge` over this core's own
    /// shell + play host, then takes the state back — the
    /// windowed shell's drain, kept in one place so every
    /// admitted op and every play action in the companion
    /// goes through the one bridge type.
    fn with_bridge<T>(&mut self, f: impl FnOnce(&mut AppBridge) -> T) -> T {
        let shell = std::mem::take(&mut self.shell);
        let play = std::mem::take(&mut self.play);
        let mut bridge = AppBridge::with_player(shell, play);
        let out = f(&mut bridge);
        let (shell, play) = bridge.into_parts();
        self.shell = shell;
        self.play = play;
        out
    }

    /// Answers every queued session bridge request (called
    /// between frames, the windowed shell's discipline).
    /// Returns how many were answered.
    pub fn drain_session_requests(&mut self) -> usize {
        let Some(rx) = self.session_rx.take() else {
            return 0;
        };
        let mut pending: Vec<BridgeRequest> = Vec::new();
        while let Ok(request) = rx.try_recv() {
            pending.push(request);
        }
        self.session_rx = Some(rx);
        let count = pending.len();
        if count == 0 {
            return 0;
        }
        self.with_bridge(|bridge| {
            for request in pending {
                match request {
                    BridgeRequest::Apply { op, reply } => {
                        let _ = reply.send(bridge.apply_live(&op));
                    }
                    BridgeRequest::EvalHash { reply } => {
                        let _ = reply.send(bridge.evaluated_hash());
                    }
                    BridgeRequest::PlayState { reply } => {
                        let _ = reply.send(bridge.play_state());
                    }
                    BridgeRequest::PlayControl {
                        action,
                        args,
                        reply,
                    } => {
                        let _ = reply.send(bridge.play_control(&action, &args));
                    }
                }
            }
        });
        count
    }

    /// Drives the player through the bridge (the
    /// `play_control` tool's `play` action): local pointer
    /// actions and remote registry calls share this entry.
    pub fn play(&mut self, animation: &str, looping: bool) -> CompanionResult<()> {
        let args = serde_json::json!({
            "animation": animation,
            "loop": looping,
        });
        self.with_bridge(|bridge| bridge.play_control("play", &args))
            .map_err(|refusal| {
                CompanionError::new(
                    "TG-COMP-SESSION",
                    format!("{} {}", refusal.code, refusal.message),
                )
            })?;
        Ok(())
    }

    /// Starts the ambient performance when nothing else owns
    /// the player: Daydream loops as her at-rest state. A
    /// finished Greeting yields back to it; a paused or
    /// stopping playback is left alone.
    pub fn ensure_ambient(&mut self) -> CompanionResult<()> {
        let scene = self.scene()?;
        let snapshot = self.play.snapshot(scene);
        if snapshot.paused || snapshot.stopping {
            return Ok(());
        }
        if snapshot.animation.as_deref() == Some(AMBIENT_ANIMATION) && snapshot.playing {
            return Ok(());
        }
        if snapshot.playing {
            // Something else (a Greeting, a remote call) is
            // mid-performance; it yields when it finishes.
            return Ok(());
        }
        self.play(AMBIENT_ANIMATION, true)
    }

    /// One presentation tick: drain the session queue,
    /// restore the ambient performance when the player is
    /// free, advance one fixed step while a performance is
    /// advancing, and return the evaluated state hash of
    /// the resulting frame.
    pub fn tick(&mut self) -> CompanionResult<u64> {
        self.drain_session_requests();
        self.ensure_ambient()?;
        if self.play.is_advancing() {
            // The gaze driver advances with the scene (a
            // paused puppet's gaze is frozen with her) and
            // writes through the step hook, composing with
            // the performance (see gaze.rs).
            self.gaze.step();
            // Field-level borrows: the player, the shell,
            // and the driver are disjoint fields, so the
            // step needs no take/replace dance.
            let gaze = &self.gaze;
            let channels = &self.gaze_channels;
            let scene = self
                .shell
                .scene
                .as_mut()
                .ok_or_else(|| CompanionError::model("no scene is installed"))?;
            self.play
                .step_hooked(scene, 1, &mut |scene: &mut Scene| {
                    crate::gaze::apply(gaze, channels, scene);
                })
                .map_err(|err| CompanionError::model(err.to_string()))?;
        }
        let scene = self.scene_mut()?;
        Ok(evaluated_state_hash(&mut scene.puppet))
    }

    /// The play host's current snapshot, for reports.
    pub fn play_snapshot(&self) -> CompanionResult<torogao_app::player::PlayState> {
        let scene = self.scene()?;
        Ok(self.play.snapshot(scene))
    }

    /// The repertoire model this companion presents.
    pub fn model_path(&self) -> &Path {
        &self.model_path
    }

    /// The cast member on stage, when the loaded model is
    /// one of the governed roster.
    pub fn cast_member(&self) -> Option<&'static CastMember> {
        self.cast_member
    }

    /// Puts another cast member on stage: loads her
    /// repertoire fixture from the cast directory,
    /// installs it in the same shell, and restarts the
    /// ambient performance — a scene install, exactly the
    /// shared shell operation the panel performs on load.
    /// The hosted session stays bound to its genesis
    /// projection (the session host's documented binding,
    /// unchanged by who is on stage). The gaze driver
    /// carries over: attention is the companion's, not the
    /// performer's.
    pub fn switch_character(&mut self, member: &'static CastMember) -> CompanionResult<()> {
        if self.cast_member == Some(member) {
            return Ok(());
        }
        let path = crate::cast::model_path(&self.cast_dir, member)?;
        let mut scene =
            Scene::load_file(&path).map_err(|err| CompanionError::model(err.to_string()))?;
        scene
            .evaluate_fresh(&[])
            .map_err(|err| CompanionError::model(err.to_string()))?;
        self.shell.install_scene(scene);
        self.play = PlayHost::new();
        self.model_path = path;
        self.cast_member = Some(member);
        self.refresh_gaze_channels()?;
        self.ensure_ambient()
    }

    /// Feeds the pointer to the gaze driver: `pointer` is
    /// the cursor's position in surface fractions (x right,
    /// y down, origin top-left), `surface_aspect` the
    /// surface's width/height. The conversion to a gaze
    /// direction measures from `FACE_ANCHOR` at
    /// `VIEW_DISTANCE_HEIGHTS`. `None` (the pointer left
    /// her surface) returns her to the idle schedule.
    pub fn set_pointer(&mut self, pointer: Option<(f64, f64)>, surface_aspect: f64) {
        let target = pointer.map(|(fx, fy)| {
            let dx = (fx - FACE_ANCHOR.0) * surface_aspect;
            let dy = fy - FACE_ANCHOR.1;
            (
                dx.atan2(VIEW_DISTANCE_HEIGHTS).to_degrees(),
                dy.atan2(VIEW_DISTANCE_HEIGHTS).to_degrees(),
            )
        });
        self.gaze.set_pointer_target(target);
    }

    /// The pointer-click reaction: Greeting, once through,
    /// through the shared play entry. In quiet mode the
    /// reaction is suppressed (§3.3: quiet hours suppress
    /// reactions) and this returns `false`; the ambient
    /// performance is never interrupted either way beyond
    /// the reaction itself.
    pub fn click_reaction(&mut self) -> CompanionResult<bool> {
        if self.gaze.quiet() {
            return Ok(false);
        }
        self.play(CLICK_ANIMATION, false)?;
        Ok(true)
    }

    /// Sets quiet mode (focus discipline, §3.3). The
    /// placement state persists the choice; the core only
    /// carries it live.
    pub fn set_quiet(&mut self, quiet: bool) {
        self.gaze.set_quiet(quiet);
    }

    /// Whether quiet mode is on.
    pub fn quiet(&self) -> bool {
        self.gaze.quiet()
    }

    /// The gaze driver's current evaluated direction
    /// (yaw, pitch) in degrees — the gate instrument's
    /// read of where she is looking.
    pub fn gaze_degrees(&self) -> (f64, f64) {
        self.gaze.gaze_degrees()
    }

    /// The gaze channels the current rig does not carry
    /// (empty for the governed cast).
    pub fn gaze_gaps(&self) -> &[&'static str] {
        &self.gaze_gaps
    }

    /// Re-resolves the gaze channels against the installed
    /// scene and records any gap.
    fn refresh_gaze_channels(&mut self) -> CompanionResult<()> {
        let scene = self.scene()?;
        self.gaze_channels = GazeChannels::resolve(scene);
        self.gaze_gaps = self.gaze_channels.missing();
        let eye_channels =
            self.gaze_channels.gaze_x.is_some() && self.gaze_channels.gaze_y.is_some();
        self.gaze.set_eye_channels(eye_channels);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ambient_and_click_names_are_the_landed_repertoire() {
        // The slice presents the Phase F repertoire only;
        // renaming here without renaming there would make
        // every play call an unknown-animation refusal.
        assert_eq!(AMBIENT_ANIMATION, "Daydream");
        assert_eq!(CLICK_ANIMATION, "Greeting");
    }
}
