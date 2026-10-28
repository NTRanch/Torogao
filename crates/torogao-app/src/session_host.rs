//! The session host, hosted by this app: the editor-state
//! interop surface served from the shell's own live state.
//!
//! `--session-serve <model.inx> --session-dir <dir>` loads
//! the model through the same `Scene` + `ShellState` path
//! the windowed shell uses, evaluates it fresh (the Phase B
//! protocol), projects the standing state into the session's
//! genesis snapshot, and serves the registry: over stdio for
//! spawned clients, or over WebSocket on 127.0.0.1 with
//! `--session-port` (default 9770) for the app endpoint.
//!
//! The projection is a snapshot of live objects, not a
//! fixture: node ids are the puppet's payload uuids, values
//! are the puppet's standing authored values, and visibility
//! is the arena's `enabled` flag. Ops the gate admits are
//! applied back into the same scene through
//! `Scene::set_parameter` / the arena, so the served state
//! and the evaluated puppet never separate.

use std::collections::BTreeMap;
use std::path::Path;

use torogao_engine::Vec2;
use torogao_format::payload::NodeKind as PayloadNodeKind;
use torogao_session::host::{Host, LiveBridge, PlaySnapshot};
use torogao_session::op::Op;
use torogao_session::refusal::{R, Refusal};
use torogao_session::session::Session;
use torogao_session::state::{
    Binding, EditorState, Node, NodeKind, ParamState, ProjectMeta, STATE_FORMAT,
    Scene as SessionScene, Transform, Viewport,
};
use torogao_session::transport;

use crate::error::{AppError, AppResult};
use crate::player::PlayHost;
use crate::scene::Scene;
use crate::state::ShellState;
use crate::state_hash::evaluated_state_hash;

/// Projects the shell's standing state into a session
/// genesis snapshot plus the parameters' rig-declared
/// defaults (the gate mirror's input).
///
/// Vec2 parameters cannot be represented in the scalar
/// `torogao-editor-state/1` schema: each is named in
/// `uncapturable` (so compile refuses honestly and lists
/// them) and no op can address it. Scalar parameters project
/// their x component — the component the shell's scalar
/// assignment semantics address.
pub fn project_genesis(shell: &ShellState) -> AppResult<(EditorState, BTreeMap<String, f32>)> {
    let scene = shell
        .scene
        .as_ref()
        .ok_or_else(|| AppError::Format("no scene is loaded".to_string()))?;
    let puppet = &scene.puppet;
    let uuid_of = |index: usize| puppet.nodes.nodes[index].uuid;
    let mut nodes = Vec::with_capacity(puppet.nodes.nodes.len());
    for node in &puppet.nodes.nodes {
        nodes.push(Node {
            id: node.uuid,
            name: node.name.clone(),
            parent: node.parent.map(uuid_of),
            kind: match node.kind {
                PayloadNodeKind::Part | PayloadNodeKind::AnimatedPart => NodeKind::Part,
                PayloadNodeKind::SimplePhysics => NodeKind::SimplePhysics,
                _ => NodeKind::Node,
            },
            visible: node.enabled,
            zsort: node.zsort,
            transform: Transform {
                tx: node.local.translation.x,
                ty: node.local.translation.y,
                rot: node.local.rotation.z,
                sx: node.local.scale.x,
                sy: node.local.scale.y,
            },
        });
    }
    let mut params = BTreeMap::new();
    let mut defaults = BTreeMap::new();
    let mut uncapturable = Vec::new();
    for param in &puppet.params {
        if param.is_vec2 {
            uncapturable.push(format!("params/{}", param.name));
            continue;
        }
        params.insert(
            param.name.clone(),
            ParamState {
                value: param.value.x,
                min: param.min.x,
                max: param.max.x,
            },
        );
        defaults.insert(param.name.clone(), param.defaults.x);
    }
    let mut bindings = Vec::new();
    for param in &puppet.params {
        if param.is_vec2 {
            continue;
        }
        for binding in &param.bindings {
            let node_uuid = match binding.node {
                Some(index) => uuid_of(index),
                None => binding.node_ref,
            };
            bindings.push(Binding {
                param: param.name.clone(),
                node: node_uuid,
                property: binding.param_name.clone(),
                kind: if binding.is_deformation() {
                    "deformation".to_string()
                } else {
                    "value".to_string()
                },
            });
        }
    }
    let selection = match shell.selection.node_index {
        Some(index) if index < puppet.nodes.nodes.len() => vec![uuid_of(index)],
        _ => Vec::new(),
    };
    let state = EditorState {
        format: STATE_FORMAT.to_string(),
        project: ProjectMeta {
            name: scene.model_name().to_string(),
            source: scene.source_label.clone(),
        },
        scene: SessionScene {
            name: scene.model_name().to_string(),
            nodes,
        },
        selection,
        viewport: Viewport {
            cam_x: 0.0,
            cam_y: 0.0,
            zoom: 1.0,
        },
        params,
        bindings,
        uncapturable,
    };
    state
        .validate()
        .map_err(|r| AppError::Format(format!("{}: {}", r.code, r.message)))?;
    Ok((state, defaults))
}

/// The hosted scene inside a shell state, mutably;
/// TG-SES-NO-HOST when the shell holds no scene.
fn shell_scene_mut(shell: &mut ShellState) -> R<&mut Scene> {
    shell
        .scene
        .as_mut()
        .ok_or_else(|| Refusal::new("TG-SES-NO-HOST", "no scene is loaded"))
}

/// The bridge between the session host and the shell's live
/// scene: admitted ops land in the same puppet the windowed
/// shell evaluates, and the play surface drives the shell's
/// play-mode host over that same puppet.
pub struct AppBridge {
    shell: ShellState,
    player: PlayHost,
    uuid_to_index: BTreeMap<u32, usize>,
    /// The play trajectory: every play_control action this
    /// bridge has performed, in order. The save path
    /// records it so a restore can replay the play half
    /// exactly (the evaluated state is a function of the
    /// whole step history).
    play_log: Vec<crate::save::PlayOp>,
}

impl AppBridge {
    /// A bridge over a shell whose scene is installed, with
    /// a fresh play host.
    pub fn new(shell: ShellState) -> AppBridge {
        AppBridge::with_player(shell, PlayHost::new())
    }

    /// A bridge over a shell plus an existing play host
    /// (the windowed shell lends its own host so session
    /// play state and window transport state are one state).
    pub fn with_player(shell: ShellState, player: PlayHost) -> AppBridge {
        let uuid_to_index = shell
            .scene
            .as_ref()
            .map(|scene| {
                scene
                    .puppet
                    .nodes
                    .nodes
                    .iter()
                    .enumerate()
                    .map(|(index, node)| (node.uuid, index))
                    .collect()
            })
            .unwrap_or_default();
        AppBridge {
            shell,
            player,
            uuid_to_index,
            play_log: Vec::new(),
        }
    }

    /// The recorded play trajectory (see the field note).
    pub fn play_log(&self) -> &[crate::save::PlayOp] {
        &self.play_log
    }

    /// Returns the shell and the play host (the windowed
    /// shell takes its state back after serving requests).
    pub fn into_parts(self) -> (ShellState, PlayHost) {
        (self.shell, self.player)
    }

    /// The hosted scene, mutably. Call sites name
    /// `self.shell.scene` directly where the player is
    /// borrowed in the same expression (disjoint fields);
    /// this helper serves the single-borrow arms.
    fn scene_mut(&mut self) -> R<&mut Scene> {
        shell_scene_mut(&mut self.shell)
    }

    fn play_snapshot(&self) -> Option<PlaySnapshot> {
        let scene = self.shell.scene.as_ref()?;
        let state = self.player.snapshot(scene);
        Some(PlaySnapshot {
            playing: state.playing,
            paused: state.paused,
            stopping: state.stopping,
            animation: state.animation,
            frame: state.frame,
            hframe: state.hframe,
            step_count: state.step_count,
            looping: state.looping,
            looped: state.looped,
            speed: state.speed,
            applied_state: state.applied_state,
        })
    }
}

/// Maps an app error onto the session refusal vocabulary:
/// unknown names (animation, state, parameter) are the
/// author's unknown-target refusal; evaluation failures
/// are session I/O; anything else keeps its message under
/// the schema refusal.
fn refusal_of(err: AppError) -> Refusal {
    match err {
        AppError::Param(message) => Refusal::new("TG-AUTH-UNKNOWN-TARGET", message),
        AppError::Engine(message) => Refusal::new("TG-SES-IO", format!("evaluation: {message}")),
        other => Refusal::new("TG-SES-SCHEMA", other.to_string()),
    }
}

impl LiveBridge for AppBridge {
    fn apply_live(&mut self, op: &Op) -> R<()> {
        match op {
            Op::ParamSet { param, value } => {
                let scene = self.scene_mut()?;
                let index = scene
                    .parameter_index(param)
                    .map_err(|e| Refusal::new("TG-AUTH-UNKNOWN-TARGET", e.to_string()))?;
                // The op addresses the scalar (x) component;
                // the live y holds at its standing value.
                let y = scene.puppet.params[index].value.y;
                scene
                    .set_parameter(index, Vec2::new(*value, y))
                    .map_err(|e| Refusal::new("TG-SES-IO", format!("evaluation: {e}")))
            }
            Op::NodeSetVisible { node, visible } => {
                let index = self.uuid_to_index.get(node).copied().ok_or_else(|| {
                    Refusal::new("TG-AUTH-UNKNOWN-TARGET", format!("no node with id {node}"))
                })?;
                let scene = self.scene_mut()?;
                scene.puppet.nodes.nodes[index].enabled = *visible;
                // Visibility has no incremental step path;
                // re-evaluate under the fresh protocol.
                scene
                    .evaluate_fresh(&[])
                    .map_err(|e| Refusal::new("TG-SES-IO", format!("evaluation: {e}")))
            }
            Op::SelectionSet { nodes } => {
                match nodes
                    .first()
                    .and_then(|id| self.uuid_to_index.get(id).copied())
                {
                    Some(index) => self.shell.selection.select(index),
                    None => self.shell.selection.clear(),
                }
                Ok(())
            }
            Op::ViewportSet { .. } => Ok(()),
            Op::NodeSetTransform { .. } => Err(Refusal::new(
                "TG-AUTH-UNKNOWN-KIND",
                "node transforms are refused at the gate; the bridge never sees them",
            )),
        }
    }

    fn evaluated_hash(&mut self) -> Option<String> {
        let scene = self.shell.scene.as_mut()?;
        Some(format!("{:016x}", evaluated_state_hash(&mut scene.puppet)))
    }

    fn play_state(&mut self) -> Option<PlaySnapshot> {
        self.play_snapshot()
    }

    fn play_control(&mut self, action: &str, args: &serde_json::Value) -> R<PlaySnapshot> {
        if self.shell.scene.is_none() {
            return Err(Refusal::new("TG-SES-NO-HOST", "no scene is loaded"));
        }
        match action {
            "play" => {
                let animation = args.get("animation").and_then(|v| v.as_str());
                let looping = args.get("loop").and_then(|v| v.as_bool()).unwrap_or(false);
                let speed = args.get("speed").and_then(|v| v.as_f64()).map(|v| v as f32);
                let scene = self.shell.scene.as_ref().expect("scene checked");
                self.player
                    .play(scene, animation, looping, speed)
                    .map_err(refusal_of)?;
            }
            "pause" => self.player.pause(),
            "stop" => {
                let immediate = args
                    .get("immediate")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let scene = self.shell.scene.as_ref().expect("scene checked");
                self.player.stop(scene, immediate);
            }
            "seek" => {
                let frame = args.get("frame").and_then(|v| v.as_i64()).ok_or_else(|| {
                    Refusal::new("TG-SES-SCHEMA", "play_control seek requires `frame`")
                })? as i32;
                let scene = self.shell.scene.as_ref().expect("scene checked");
                self.player.seek(scene, frame).map_err(refusal_of)?;
            }
            "step" => {
                let steps = args.get("steps").and_then(|v| v.as_u64()).unwrap_or(1);
                let scene = shell_scene_mut(&mut self.shell)?;
                self.player.step(scene, steps).map_err(refusal_of)?;
            }
            "apply_state" => {
                let state = args.get("state").and_then(|v| v.as_str()).ok_or_else(|| {
                    Refusal::new("TG-SES-SCHEMA", "play_control apply_state requires `state`")
                })?;
                let scene = shell_scene_mut(&mut self.shell)?;
                self.player.apply_state(scene, state).map_err(refusal_of)?;
            }
            other => {
                return Err(Refusal::new(
                    "TG-AUTH-UNKNOWN-KIND",
                    format!("play action {other:?} is not in the registry"),
                ));
            }
        }
        // The action landed; record it in the play
        // trajectory the save path replays.
        let recorded =
            match action {
                "play" => Some(crate::save::PlayOp::Play {
                    animation: args
                        .get("animation")
                        .and_then(|v| v.as_str())
                        .map(str::to_string),
                    looping: args.get("loop").and_then(|v| v.as_bool()).unwrap_or(false),
                    speed: args.get("speed").and_then(|v| v.as_f64()).map(|v| v as f32),
                }),
                "pause" => Some(crate::save::PlayOp::Pause),
                "stop" => Some(crate::save::PlayOp::Stop {
                    immediate: args
                        .get("immediate")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false),
                }),
                "seek" => args.get("frame").and_then(|v| v.as_i64()).map(|frame| {
                    crate::save::PlayOp::Seek {
                        frame: frame as i32,
                    }
                }),
                "step" => Some(crate::save::PlayOp::Step {
                    steps: args.get("steps").and_then(|v| v.as_u64()).unwrap_or(1),
                }),
                "apply_state" => args.get("state").and_then(|v| v.as_str()).map(|state| {
                    crate::save::PlayOp::ApplyState {
                        state: state.to_string(),
                    }
                }),
                _ => None,
            };
        if let Some(op) = recorded {
            self.play_log.push(op);
        }
        self.play_snapshot()
            .ok_or_else(|| Refusal::new("TG-SES-NO-HOST", "no scene is loaded"))
    }
}

/// One request from the session owner thread to the
/// windowed shell's frame loop (windowed co-presence): the
/// owner thread blocks on the reply, the frame loop answers
/// between frames. The window side serves requests through
/// an `AppBridge` over its own shell + play host, so the
/// windowed path is the headless path, frame by frame.
pub enum BridgeRequest {
    /// An admitted op to apply to the live scene.
    Apply {
        /// The op.
        op: Op,
        /// Reply channel.
        reply: std::sync::mpsc::Sender<R<()>>,
    },
    /// The live evaluator's hash.
    EvalHash {
        /// Reply channel.
        reply: std::sync::mpsc::Sender<Option<String>>,
    },
    /// The live player's state.
    PlayState {
        /// Reply channel.
        reply: std::sync::mpsc::Sender<Option<PlaySnapshot>>,
    },
    /// A play-control drive.
    PlayControl {
        /// The action name.
        action: String,
        /// The call arguments.
        args: serde_json::Value,
        /// Reply channel.
        reply: std::sync::mpsc::Sender<R<PlaySnapshot>>,
    },
}

/// The `LiveBridge` the session owner thread holds in
/// windowed mode: every call forwards to the frame loop
/// and waits for its answer. A dropped receiver (the
/// window is closing) answers as the no-host refusal /
/// no-hash, never a hang.
pub struct ChannelBridge {
    tx: std::sync::mpsc::Sender<BridgeRequest>,
}

impl ChannelBridge {
    fn send<T>(
        &self,
        request: impl FnOnce(std::sync::mpsc::Sender<T>) -> BridgeRequest,
    ) -> Option<T> {
        let (reply_tx, reply_rx) = std::sync::mpsc::channel();
        self.tx.send(request(reply_tx)).ok()?;
        reply_rx.recv().ok()
    }
}

impl LiveBridge for ChannelBridge {
    fn apply_live(&mut self, op: &Op) -> R<()> {
        let op = op.clone();
        self.send(|reply| BridgeRequest::Apply { op, reply })
            .unwrap_or_else(|| {
                Err(Refusal::new(
                    "TG-SES-NO-HOST",
                    "the window hosting the scene has closed",
                ))
            })
    }

    fn evaluated_hash(&mut self) -> Option<String> {
        self.send(|reply| BridgeRequest::EvalHash { reply })
            .flatten()
    }

    fn play_state(&mut self) -> Option<PlaySnapshot> {
        self.send(|reply| BridgeRequest::PlayState { reply })
            .flatten()
    }

    fn play_control(&mut self, action: &str, args: &serde_json::Value) -> R<PlaySnapshot> {
        let action = action.to_string();
        let args = args.clone();
        self.send(|reply| BridgeRequest::PlayControl {
            action,
            args,
            reply,
        })
        .unwrap_or_else(|| {
            Err(Refusal::new(
                "TG-SES-NO-HOST",
                "the window hosting the scene has closed",
            ))
        })
    }
}

/// Starts the session server for the windowed shell:
/// projects the genesis from the shell's standing scene,
/// writes the capability token, and runs the WebSocket
/// transport on a background thread whose bridge forwards
/// into the frame loop. Returns the request receiver the
/// frame loop drains plus the session id. The session is
/// bound to the scene loaded at start (the headless host's
/// binding, unchanged); loading another model in the
/// window does not re-project the session.
pub fn start_window_session(
    shell: &ShellState,
    session_dir: &Path,
    port: u16,
    held_by: &str,
) -> AppResult<(std::sync::mpsc::Receiver<BridgeRequest>, String)> {
    let (genesis, defaults) = project_genesis(shell)?;
    let token = transport::make_token_file(session_dir).map_err(|e| AppError::Io {
        context: "create session dir",
        message: e.to_string(),
    })?;
    let session = Session::new(genesis, &defaults, held_by);
    let session_id = session.id.clone();
    let host = Host::new(session);
    let (tx, rx) = std::sync::mpsc::channel::<BridgeRequest>();
    let bridge: Box<dyn LiveBridge + Send> = Box::new(ChannelBridge { tx });
    std::thread::Builder::new()
        .name("torogao-window-session".to_string())
        .spawn(move || {
            let _ = transport::serve_ws(host, bridge, &token, port);
        })
        .map_err(|e| AppError::Io {
            context: "spawn session thread",
            message: e.to_string(),
        })?;
    Ok((rx, session_id))
}

/// Loads the model, installs it in a fresh shell, evaluates
/// fresh, and serves the session host until the transport
/// ends (stdio EOF) or the listener fails.
pub fn run_session_host(
    model: &Path,
    session_dir: &Path,
    port: Option<u16>,
    held_by: &str,
) -> AppResult<i32> {
    let mut scene = Scene::load_file(model)?;
    scene.evaluate_fresh(&[])?;
    let mut shell = ShellState::new();
    shell.install_scene(scene);
    let (genesis, defaults) = project_genesis(&shell)?;
    let token = transport::make_token_file(session_dir).map_err(|e| AppError::Io {
        context: "create session dir",
        message: e.to_string(),
    })?;
    let session = Session::new(genesis, &defaults, held_by);
    eprintln!(
        "torogao-app: session {} up; state {}",
        session.id,
        session.state_hash()
    );
    let host = Host::new(session);
    let bridge: Box<dyn torogao_session::host::LiveBridge + Send> = Box::new(AppBridge::new(shell));
    // Modern era (WS2-W5): both transports run the rmcp pipeline;
    // the token is the guard's per-request `_meta` check, not a
    // handshake. Clients find the session handle in the discover
    // result's `_meta`.
    match port {
        Some(port) => transport::serve_ws(host, bridge, &token, port),
        None => transport::serve_stdio(host, bridge, &token),
    }
    .map_err(|r| AppError::Format(format!("{}: {}", r.code, r.message)))?;
    Ok(0)
}
