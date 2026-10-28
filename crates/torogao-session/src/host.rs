//! The session host: the domain dispatcher over the
//! registry — one `Session`, its tool calls, and nothing
//! about framing.
//!
//! Framing moved to the modern MCP stack in WS2-W5 (rmcp
//! 3.8.0 behind the era guard in `mcp_guard`); capability
//! auth is a per-request framing concern there (the token
//! rides in request `_meta`), so this type no longer owns a
//! handshake or any connection state. What remains is
//! exactly Phase C's domain behavior: the nine registry
//! tools over the session state, with every authoring op
//! admitted by torogao-author's `decide()` (see `gate`).
//!
//! The host owns the `Session`. Everything the session
//! cannot answer from its own state — applying an admitted
//! op to the live scene, the evaluator's current output
//! hash — goes through the `LiveBridge` the embedding
//! process provides.

use crate::op::Op;
use crate::refusal::{R, Refusal};
use crate::registry::agent_card;
use crate::session::Session;
use serde_json::{Value, json};
use std::path::Path;

/// A point-in-time read of the live animation player, as
/// served by `play_state` and returned by `play_control`.
/// Plain data: the session crate owns no player; the
/// embedding process's play-mode host fills this in.
#[derive(Debug, Clone, Default)]
pub struct PlaySnapshot {
    /// Whether the transport is playing.
    pub playing: bool,
    /// Whether the transport is paused.
    pub paused: bool,
    /// Whether the transport is stopping (lead-out).
    pub stopping: bool,
    /// The current animation's name, when one is loaded
    /// into the player.
    pub animation: Option<String>,
    /// The playhead frame (rounded).
    pub frame: i32,
    /// The playhead floating frame.
    pub hframe: f32,
    /// Fixed steps the play host has advanced since the
    /// scene was installed (the playhead's step count).
    pub step_count: u64,
    /// Whether the current playback loops.
    pub looping: bool,
    /// Completed loops of the current playback.
    pub looped: i32,
    /// The current playback's speed multiplier.
    pub speed: f32,
    /// The last named state applied through the player
    /// surface (`play_control` `apply_state`).
    pub applied_state: Option<String>,
}

/// The embedding process's side of the host: the live scene
/// the session is a snapshot of.
pub trait LiveBridge {
    /// Applies an admitted op to the live scene (parameter
    /// write + evaluation step, visibility flag, selection).
    /// Called only between the gate's admission and the
    /// session's commit; on error nothing is logged and the
    /// session state is untouched.
    fn apply_live(&mut self, op: &Op) -> R<()>;

    /// The live evaluator's output hash for the scene as it
    /// stands now, when the embedding process evaluates.
    fn evaluated_hash(&mut self) -> Option<String>;

    /// The live player's state, when the embedding process
    /// hosts a player over a loaded scene. The default
    /// (no player) makes `play_state` refuse
    /// TG-SES-NO-HOST — the refusal survives only for the
    /// genuine no-scene-loaded case.
    fn play_state(&mut self) -> Option<PlaySnapshot> {
        None
    }

    /// Drives the live player (play / pause / stop / seek /
    /// step / apply_state). Drives evaluation only; authors
    /// nothing; never op-logged. The default refuses
    /// TG-SES-NO-HOST (no live player).
    fn play_control(&mut self, _action: &str, _args: &Value) -> R<PlaySnapshot> {
        Err(Refusal::new(
            "TG-SES-NO-HOST",
            "this host has no live player: no scene is loaded",
        ))
    }
}

/// A bridge with no live scene behind it: reads still serve,
/// live application refuses. Hosts that embed a real scene
/// provide their own bridge.
pub struct NoLiveBridge;

impl LiveBridge for NoLiveBridge {
    fn apply_live(&mut self, _op: &Op) -> R<()> {
        Err(Refusal::new(
            "TG-SES-NO-HOST",
            "this host has no live scene to apply admitted ops to",
        ))
    }

    fn evaluated_hash(&mut self) -> Option<String> {
        None
    }
}

/// The host: one session, served. Framing and auth live in
/// `mcp_guard` / `mcp_server`; this type is only the domain
/// dispatch they call into (through `dispatch`'s owner
/// thread, which is where the `&mut` lives).
pub struct Host {
    /// The session being served.
    pub session: Session,
}

impl Host {
    /// A host over `session`.
    pub fn new(session: Session) -> Host {
        Host { session }
    }

    /// Dispatches one tool call against the session.
    pub fn call_tool(&mut self, name: &str, args: &Value, bridge: &mut dyn LiveBridge) -> R<Value> {
        let s = &mut self.session;
        let st = &s.state;
        match name {
            "editor_state" => Ok(state_summary(s)),
            "scene_hierarchy" => {
                let only_parent = args
                    .get("parent")
                    .and_then(|v| v.as_u64())
                    .map(|v| v as u32);
                let mut counts: std::collections::BTreeMap<u32, usize> =
                    std::collections::BTreeMap::new();
                for n in &st.scene.nodes {
                    if let Some(p) = n.parent {
                        *counts.entry(p).or_insert(0) += 1;
                    }
                }
                let nodes: Vec<Value> = st
                    .scene
                    .nodes
                    .iter()
                    .filter(|n| only_parent.is_none_or(|p| n.parent == Some(p) || n.id == p))
                    .take(4096)
                    .map(|n| {
                        json!({
                            "id": n.id, "name": n.name, "kind": format!("{:?}", n.kind),
                            "parent": n.parent, "visible": n.visible, "zsort": n.zsort,
                            "children": counts.get(&n.id).copied().unwrap_or(0),
                        })
                    })
                    .collect();
                Ok(json!({"scene": st.scene.name, "nodes": nodes}))
            }
            "selection_context" => {
                let mut out = Vec::new();
                for id in &st.selection {
                    let node = st.node(*id).expect("selection validated");
                    let ancestors: Vec<Value> = {
                        let mut chain = Vec::new();
                        let mut cur = node.parent;
                        while let Some(pid) = cur {
                            let a = st.node(pid).expect("parents validated");
                            chain.push(
                                json!({"id": a.id, "name": a.name, "kind": format!("{:?}", a.kind)}),
                            );
                            cur = a.parent;
                        }
                        chain
                    };
                    let children: Vec<Value> = st
                        .scene
                        .nodes
                        .iter()
                        .filter(|n| n.parent == Some(*id))
                        .map(|n| json!({"id": n.id, "name": n.name, "kind": format!("{:?}", n.kind)}))
                        .collect();
                    let bound: Vec<Value> = st
                        .bindings
                        .iter()
                        .filter(|b| b.node == *id)
                        .map(|b| {
                            json!({
                                "param": b.param, "property": b.property, "kind": b.kind,
                                "value": st.params.get(&b.param).map(|p| p.value),
                            })
                        })
                        .collect();
                    let drives: Vec<Value> = {
                        let mut v = Vec::new();
                        for b in &st.bindings {
                            if bound.iter().any(|x| x["param"] == b.param) && b.node != *id {
                                v.push(
                                    json!({"param": b.param, "node": b.node, "property": b.property}),
                                );
                            }
                        }
                        v
                    };
                    out.push(json!({
                        "node": {"id": node.id, "name": node.name, "kind": format!("{:?}", node.kind),
                                 "parent": node.parent, "visible": node.visible, "zsort": node.zsort,
                                 "transform": {"tx": node.transform.tx, "ty": node.transform.ty,
                                               "rot": node.transform.rot, "sx": node.transform.sx, "sy": node.transform.sy}},
                        "ancestors": ancestors,
                        "children": children,
                        "bound_params": bound,
                        "param_also_drives": drives,
                    }));
                }
                Ok(json!({"selection": out, "state_hash": s.state_hash()}))
            }
            "param_state" => {
                if let Some(pname) = args.get("name").and_then(|v| v.as_str()) {
                    let p = st.params.get(pname).ok_or_else(|| {
                        Refusal::new("TG-AUTH-UNKNOWN-TARGET", format!("no param named {pname}"))
                    })?;
                    let bindings: Vec<Value> = st
                        .bindings
                        .iter()
                        .filter(|b| b.param == pname)
                        .map(|b| json!({"node": b.node, "property": b.property, "kind": b.kind}))
                        .collect();
                    Ok(
                        json!({"name": pname, "value": p.value, "min": p.min, "max": p.max, "bindings": bindings}),
                    )
                } else {
                    Ok(
                        json!({"params": st.params.iter().map(|(k, p)| (k.clone(), json!({"value": p.value, "min": p.min, "max": p.max}))).collect::<serde_json::Map<_, _>>()}),
                    )
                }
            }
            "play_state" => {
                let snapshot = bridge.play_state().ok_or_else(|| {
                    Refusal::new(
                        "TG-SES-NO-HOST",
                        "play state is unavailable: no scene is loaded",
                    )
                })?;
                Ok(play_json(&snapshot, bridge, s))
            }
            "play_control" => {
                let action = args.get("action").and_then(|v| v.as_str()).ok_or_else(|| {
                    Refusal::new("TG-SES-SCHEMA", "play_control requires `action`")
                })?;
                let snapshot = bridge.play_control(action, args)?;
                Ok(play_json(&snapshot, bridge, s))
            }
            "session_apply_op" => {
                let base = args
                    .get("base_hash")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        Refusal::new("TG-SES-STALE", "session_apply_op requires base_hash")
                    })?;
                if base != s.state_hash() {
                    return Err(Refusal::new(
                        "TG-SES-STALE",
                        format!(
                            "base_hash {base} != current state hash {}; re-read editor_state",
                            s.state_hash()
                        ),
                    ));
                }
                let opv = args.get("op").ok_or_else(|| {
                    Refusal::new(
                        "TG-AUTH-UNKNOWN-KIND",
                        "session_apply_op requires an `op` object",
                    )
                })?;
                let op = Op::from_json(opv)?;
                let admission = s.admit(&op)?;
                let verdict = match admission.verdict() {
                    crate::gate::GateVerdict::Apply => "apply",
                    crate::gate::GateVerdict::NoOp => "noop",
                    crate::gate::GateVerdict::SessionLocal => "session-local",
                };
                bridge.apply_live(&op)?;
                let entry = s.commit(admission)?;
                Ok(json!({
                    "seq": entry.seq, "pre": entry.pre, "post": entry.post,
                    "state_hash": entry.post, "verdict": verdict,
                }))
            }
            "session_verify" => {
                let (replayed, live, equal) = s.verify()?;
                Ok(
                    json!({"chain_intact": true, "replayed_hash": replayed, "live_hash": live, "equal": equal}),
                )
            }
            "session_compile" => {
                let out = args
                    .get("out_dir")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        Refusal::new("TG-SES-SCHEMA", "session_compile requires out_dir")
                    })?;
                let outcome = crate::session::compile_session(s, Path::new(out))?;
                Ok(json!({
                    "genesis_hash": outcome.genesis_hash,
                    "final_hash": outcome.final_hash,
                    "ops_applied": outcome.ops_applied,
                    "record": out,
                }))
            }
            other => Err(Refusal::new(
                "TG-AUTH-UNKNOWN-KIND",
                format!("tool {other:?} is not in the registry"),
            )),
        }
    }
}

/// The A2A agent card for this host (registry-generated).
pub fn agent_card_json() -> Value {
    agent_card()
}

/// The `play_state` / `play_control` wire shape: the live
/// player's snapshot plus the evaluator's current hash and
/// the session's state hash.
fn play_json(snapshot: &PlaySnapshot, bridge: &mut dyn LiveBridge, s: &Session) -> Value {
    json!({
        "playing": snapshot.playing,
        "paused": snapshot.paused,
        "stopping": snapshot.stopping,
        "current_animation": snapshot.animation,
        "frame": snapshot.frame,
        "hframe": snapshot.hframe,
        "playhead_step": snapshot.step_count,
        "step_count": snapshot.step_count,
        "looping": snapshot.looping,
        "looped": snapshot.looped,
        "speed": snapshot.speed,
        "applied_state": snapshot.applied_state,
        "eval_hash": bridge.evaluated_hash(),
        "player": "live",
        "state_hash": s.state_hash(),
    })
}

fn state_summary(s: &Session) -> Value {
    let st = &s.state;
    let mut by_kind = std::collections::BTreeMap::new();
    for n in &st.scene.nodes {
        *by_kind.entry(format!("{:?}", n.kind)).or_insert(0usize) += 1;
    }
    let sel: Vec<Value> = st
        .selection
        .iter()
        .map(|id| json!({"id": id, "name": st.node(*id).map(|n| n.name.clone()).unwrap_or_default()}))
        .collect();
    json!({
        "format": st.format,
        "project": {"name": st.project.name, "source": st.project.source},
        "scene": st.scene.name,
        "node_count": st.scene.nodes.len(),
        "nodes_by_kind": by_kind,
        "selection": sel,
        "viewport": {"cam_x": st.viewport.cam_x, "cam_y": st.viewport.cam_y, "zoom": st.viewport.zoom},
        "params": st.params.iter().map(|(k, p)| (k.clone(), json!({"value": p.value, "min": p.min, "max": p.max}))).collect::<serde_json::Map<_, _>>(),
        "genesis_hash": s.genesis_hash(),
        "state_hash": s.state_hash(),
        "op_log_len": s.log.len(),
        "op_log_head": s.log.last().map(|e| e.post.clone()),
    })
}
