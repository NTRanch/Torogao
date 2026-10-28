//! The session owner thread: the one place the `Host` (and
//! with it the `Session`, the gate mirror, and the live
//! bridge) is owned and mutated.
//!
//! Why a thread: the modern protocol era is stateless — no
//! connection owns the session, and rmcp handlers are shared
//! (`Send + Sync`) and concurrent. Phase C's design already
//! kept all domain state in one `Host` behind a single
//! serving loop; this module preserves that confinement
//! exactly, behind the explicit-handle rule: clients learn
//! the session's handle (id + genesis/state hashes) from the
//! `server/discover` result's `_meta`, authenticate every
//! request with the capability token in `_meta`, and this
//! thread serializes their calls against the one session.
//! Nothing here leans on protocol-session affinity: any
//! connection, in any order, reaches the same session
//! through the handle.
//!
//! The info snapshot is updated by the owner thread after
//! every call, before the reply is sent — so a `discover`
//! issued after a completed op reads the post-op state hash,
//! exactly as Phase C's handshake read the then-current hash.

use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};

use serde_json::Value;
use tokio::sync::oneshot;

use crate::host::{Host, LiveBridge};
use crate::refusal::{R, Refusal};
use crate::registry::registry;

/// A point-in-time read of the session's public identity:
/// what the discover handshake hands to a new client.
#[derive(Debug, Clone)]
pub struct SessionInfo {
    /// Session id (`ses-<genesis prefix>`).
    pub session_id: String,
    /// Hash of the genesis (starting) state.
    pub genesis_hash: String,
    /// Hash of the current state; moves with every applied op.
    pub state_hash: String,
    /// Number of tools in the registry.
    pub tool_count: usize,
}

/// One request to the owner thread.
enum DispatchRequest {
    CallTool {
        name: String,
        arguments: Value,
        reply: oneshot::Sender<R<Value>>,
    },
}

/// The handle every framing side shares: a request channel
/// to the owner thread plus the info snapshot it maintains.
#[derive(Clone)]
pub struct DispatchHandle {
    tx: Sender<DispatchRequest>,
    info: Arc<Mutex<SessionInfo>>,
}

impl DispatchHandle {
    /// The current session identity snapshot.
    pub fn info(&self) -> SessionInfo {
        self.info
            .lock()
            .expect("info lock is never held across a panic")
            .clone()
    }

    /// Dispatches one tool call on the owner thread and waits
    /// for its outcome. Domain refusals travel back as
    /// `Refusal`; a dead owner thread is a `TG-SES-IO` refusal
    /// (the host process is ending; there is no session left
    /// to answer with).
    pub async fn call_tool(&self, name: &str, arguments: Value) -> R<Value> {
        let (reply, wait) = oneshot::channel();
        let request = DispatchRequest::CallTool {
            name: name.to_string(),
            arguments,
            reply,
        };
        if self.tx.send(request).is_err() {
            return Err(Refusal::new(
                "TG-SES-IO",
                "the session owner thread has ended",
            ));
        }
        wait.await.unwrap_or_else(|_| {
            Err(Refusal::new(
                "TG-SES-IO",
                "the session owner thread ended mid-call",
            ))
        })
    }
}

/// Spawns the owner thread over `host` and `bridge` and
/// returns the shared handle. The thread runs until every
/// `DispatchHandle` clone is dropped (all framing sides
/// down), then ends with the host uncommitted beyond what
/// its calls already committed — the same lifecycle Phase
/// C's serving loop had.
pub fn spawn(host: Host, mut bridge: Box<dyn LiveBridge + Send>) -> DispatchHandle {
    let info = Arc::new(Mutex::new(SessionInfo {
        session_id: host.session.id.clone(),
        genesis_hash: host.session.genesis_hash(),
        state_hash: host.session.state_hash(),
        tool_count: registry().len(),
    }));
    let (tx, rx) = channel::<DispatchRequest>();
    let thread_info = Arc::clone(&info);
    let mut host = host;
    std::thread::Builder::new()
        .name("torogao-session-owner".to_string())
        .spawn(move || run(&mut host, &mut *bridge, &rx, &thread_info))
        .expect("session owner thread spawns");
    DispatchHandle { tx, info }
}

/// The owner loop: serialize calls against the host, keep
/// the info snapshot current, answer. Refusals and payloads
/// are Phase C's `Host::call_tool` outcomes verbatim.
fn run(
    host: &mut Host,
    bridge: &mut dyn LiveBridge,
    rx: &Receiver<DispatchRequest>,
    info: &Arc<Mutex<SessionInfo>>,
) {
    while let Ok(request) = rx.recv() {
        match request {
            DispatchRequest::CallTool {
                name,
                arguments,
                reply,
            } => {
                let outcome = host.call_tool(&name, &arguments, bridge);
                {
                    let mut snapshot = info.lock().expect("info lock is never held across a panic");
                    snapshot.state_hash = host.session.state_hash();
                }
                let _ = reply.send(outcome);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::NoLiveBridge;
    use crate::session::Session;
    use crate::state::{EditorState, ProjectMeta, STATE_FORMAT, Scene, Viewport};
    use std::collections::BTreeMap;

    fn genesis() -> EditorState {
        EditorState {
            format: STATE_FORMAT.to_string(),
            project: ProjectMeta {
                name: "dispatch-test".to_string(),
                source: "test".to_string(),
            },
            scene: Scene {
                name: "dispatch-test".to_string(),
                nodes: Vec::new(),
            },
            selection: Vec::new(),
            viewport: Viewport {
                cam_x: 0.0,
                cam_y: 0.0,
                zoom: 1.0,
            },
            params: BTreeMap::new(),
            bindings: Vec::new(),
            uncapturable: Vec::new(),
        }
    }

    /// A bridge with a live player behind it: the minimal
    /// transport state machine the dispatch tests drive.
    struct FakePlayBridge {
        playing: bool,
        steps: u64,
    }

    impl crate::host::LiveBridge for FakePlayBridge {
        fn apply_live(&mut self, _op: &crate::op::Op) -> R<()> {
            Ok(())
        }

        fn evaluated_hash(&mut self) -> Option<String> {
            Some(format!("{:016x}", 0xaa00 + self.steps))
        }

        fn play_state(&mut self) -> Option<crate::host::PlaySnapshot> {
            Some(crate::host::PlaySnapshot {
                playing: self.playing,
                animation: Some("Blink".to_string()),
                step_count: self.steps,
                speed: 1.0,
                ..Default::default()
            })
        }

        fn play_control(&mut self, action: &str, args: &Value) -> R<crate::host::PlaySnapshot> {
            match action {
                "play" => self.playing = true,
                "pause" => self.playing = false,
                "step" => {
                    self.steps += args.get("steps").and_then(|v| v.as_u64()).unwrap_or(1);
                }
                other => {
                    return Err(Refusal::new(
                        "TG-AUTH-UNKNOWN-KIND",
                        format!("unknown play action {other:?}"),
                    ));
                }
            }
            Ok(self.play_state().expect("fake bridge has a player"))
        }
    }

    #[tokio::test]
    async fn dispatch_round_trip_and_info_snapshot() {
        let session = Session::new(genesis(), &BTreeMap::new(), "dispatch test");
        let handle = spawn(Host::new(session), Box::new(NoLiveBridge));
        let info = handle.info();
        assert!(info.session_id.starts_with("ses-"));
        assert_eq!(info.tool_count, 9);
        assert_eq!(info.state_hash, info.genesis_hash);
        let state = handle
            .call_tool("editor_state", serde_json::json!({}))
            .await
            .expect("editor_state answers");
        assert_eq!(state["project"]["name"], "dispatch-test");
        // TG-SES-NO-HOST survives for the genuine
        // no-scene-loaded case: a bridge with no player
        // refuses both play tools.
        let refusal = handle
            .call_tool("play_control", serde_json::json!({"action": "play"}))
            .await
            .expect_err("play_control refuses with no live player");
        assert_eq!(refusal.code, "TG-SES-NO-HOST");
        let refusal = handle
            .call_tool("play_state", serde_json::json!({}))
            .await
            .expect_err("play_state refuses with no live player");
        assert_eq!(refusal.code, "TG-SES-NO-HOST");
    }

    #[tokio::test]
    async fn dispatch_drives_the_live_player() {
        let session = Session::new(genesis(), &BTreeMap::new(), "dispatch test");
        let bridge = Box::new(FakePlayBridge {
            playing: false,
            steps: 0,
        });
        let handle = spawn(Host::new(session), bridge);
        let state = handle
            .call_tool("play_state", serde_json::json!({}))
            .await
            .expect("play_state answers from the live player");
        assert_eq!(state["playing"], false);
        assert_eq!(state["current_animation"], "Blink");
        assert_eq!(state["player"], "live");
        let played = handle
            .call_tool("play_control", serde_json::json!({"action": "play"}))
            .await
            .expect("play_control play answers");
        assert_eq!(played["playing"], true);
        let stepped = handle
            .call_tool(
                "play_control",
                serde_json::json!({"action": "step", "steps": 30}),
            )
            .await
            .expect("play_control step answers");
        assert_eq!(stepped["step_count"], 30);
        assert_eq!(stepped["playhead_step"], 30);
        // Play control never op-logs: the session state hash
        // is still the genesis hash.
        assert_eq!(handle.info().state_hash, handle.info().genesis_hash);
    }
}
