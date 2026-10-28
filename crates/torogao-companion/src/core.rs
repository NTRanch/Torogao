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

use crate::error::{CompanionError, CompanionResult};

/// The ambient performance (Daydream on loop) and the
/// click performance (Greeting, once through).
pub const AMBIENT_ANIMATION: &str = "Daydream";
/// The click reaction performance.
pub const CLICK_ANIMATION: &str = "Greeting";

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
        };
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
            // Field-level borrows: the player and the shell
            // are disjoint fields, so the step needs no
            // take/replace dance.
            let scene = self
                .shell
                .scene
                .as_mut()
                .ok_or_else(|| CompanionError::model("no scene is installed"))?;
            self.play
                .step(scene, 1)
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
