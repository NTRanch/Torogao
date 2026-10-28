//! ONE registry. The MCP `tools/list`, the A2A Agent Card,
//! and the CLI `registry` dump are all generated from this
//! table — never hand-duplicated.

use serde_json::{Value, json};

/// One tool's registry entry.
pub struct ToolDef {
    /// Wire name (`tools/call` addresses tools by it).
    pub name: &'static str,
    /// Human title.
    pub title: &'static str,
    /// Agent-card namespace.
    pub namespace: &'static str,
    /// Read/write posture.
    pub posture: &'static str,
    /// The gate text a client reads before calling.
    pub gate: &'static str,
    /// What the tool does.
    pub description: &'static str,
    /// JSON Schema for the arguments object.
    pub schema: Value,
}

/// The full registry, in surface order.
pub fn registry() -> Vec<ToolDef> {
    vec![
        ToolDef {
            name: "editor_state",
            title: "Editor state snapshot",
            namespace: "editor.state",
            posture: "read-only",
            gate: "session auth (capability token)",
            description: "Full session snapshot: project, scene, selection, viewport, all parameters with values and declared ranges, state hash, genesis hash, op-log head.",
            schema: json!({"type":"object","additionalProperties":false,"properties":{}}),
        },
        ToolDef {
            name: "scene_hierarchy",
            title: "Scene hierarchy",
            namespace: "editor.scene",
            posture: "read-only",
            gate: "session auth (capability token)",
            description: "Node tree: id, name, kind, parent, visible, zsort, child counts. Count-capped at 4096 nodes (named cap); pass `parent` to page one subtree.",
            schema: json!({"type":"object","additionalProperties":false,"properties":{"parent":{"type":"integer","minimum":0}}}),
        },
        ToolDef {
            name: "selection_context",
            title: "Selection context",
            namespace: "editor.scene",
            posture: "read-only",
            gate: "session auth (capability token)",
            description: "What the model is looking at: per selected node, its record, ancestors, children, bound parameters with current values, and zsort neighbours.",
            schema: json!({"type":"object","additionalProperties":false,"properties":{}}),
        },
        ToolDef {
            name: "param_state",
            title: "Parameter state",
            namespace: "editor.state",
            posture: "read-only",
            gate: "session auth (capability token)",
            description: "One parameter (by `name`) or all: value, min, max, and binding summary.",
            schema: json!({"type":"object","additionalProperties":false,"properties":{"name":{"type":"string"}}}),
        },
        ToolDef {
            name: "play_state",
            title: "Play state inspection",
            namespace: "editor.play",
            posture: "read-only",
            gate: "session auth; served from the app host's live animation player: transport state, current animation, playhead frame, step count, and the evaluated-state hash of the playing puppet. Refuses TG-SES-NO-HOST only when no scene is loaded.",
            description: "Playing/paused/stopping, current animation, playhead frame and step, loop state, speed, applied expression/state, live evaluation output hash.",
            schema: json!({"type":"object","additionalProperties":false,"properties":{}}),
        },
        ToolDef {
            name: "play_control",
            title: "Play control",
            namespace: "editor.play",
            posture: "session",
            gate: "session auth; drives the live animation player on the app's fixed-step clock. Drives evaluation only; authors nothing; never op-logged. Refuses TG-SES-NO-HOST only when no scene is loaded.",
            description: "play | pause | stop | seek | step | apply_state against the live player. `play` takes `animation` (default: current or first), `loop`, `speed`; `seek` takes `frame`; `step` takes `steps` (default 1); `stop` takes `immediate`; `apply_state` takes `state` (a named state of the model's EXT recipes).",
            schema: json!({"type":"object","additionalProperties":false,"required":["action"],"properties":{"action":{"type":"string","enum":["play","pause","stop","seek","step","apply_state"]},"animation":{"type":"string"},"frame":{"type":"integer"},"steps":{"type":"integer","minimum":1,"maximum":100000},"speed":{"type":"number"},"loop":{"type":"boolean"},"immediate":{"type":"boolean"},"state":{"type":"string"}}}),
        },
        ToolDef {
            name: "session_apply_op",
            title: "Apply one typed op",
            namespace: "editor.op",
            posture: "write",
            gate: "session auth, then base_hash freshness (TG-SES-STALE), then torogao-author's AUTH-D2 decide() — deny is final, no override exists. Applied ops append to the AUTH-D1 JSONL op log with pre/post hashes.",
            description: "One typed mutation op: param_set | node_set_visible | node_set_transform | selection_set | viewport_set. The only mutation path; there is no freehand state write.",
            schema: json!({"type":"object","additionalProperties":false,"required":["base_hash","op"],"properties":{"base_hash":{"type":"string"},"op":{"type":"object"}}}),
        },
        ToolDef {
            name: "session_verify",
            title: "Verify session replay",
            namespace: "editor.session",
            posture: "read-only",
            gate: "session auth (capability token)",
            description: "Replays the session op log against genesis in-process; reports chain status, replayed hash, live hash, equality.",
            schema: json!({"type":"object","additionalProperties":false,"properties":{}}),
        },
        ToolDef {
            name: "session_compile",
            title: "Compile session to canonical record",
            namespace: "editor.session",
            posture: "record",
            gate: "session auth; refuses TG-SES-UNCAPTURABLE / TG-SES-LOG-CHAIN and writes nothing on refusal.",
            description: "Emits base.ron (genesis) + ops.jsonl (replayable sequence) + project.ron (final state) + session.json into `out_dir`. A fresh replay of the record must reproduce the live state hash exactly.",
            schema: json!({"type":"object","additionalProperties":false,"required":["out_dir"],"properties":{"out_dir":{"type":"string"}}}),
        },
    ]
}

/// One registry entry as its MCP `tools/list` JSON.
pub fn tool_json(t: &ToolDef) -> Value {
    json!({
        "name": t.name,
        "title": t.title,
        "namespace": t.namespace,
        "posture": t.posture,
        "gate": t.gate,
        "description": t.description,
        "inputSchema": t.schema,
    })
}

/// The A2A Agent Card, generated from the registry: skills
/// are grouped from registry namespaces — never hand-written.
pub fn agent_card() -> Value {
    let mut by_ns: std::collections::BTreeMap<&str, Vec<&str>> = std::collections::BTreeMap::new();
    for t in registry() {
        by_ns.entry(t.namespace).or_default().push(t.name);
    }
    let skills: Vec<Value> = by_ns
        .iter()
        .map(|(ns, tools)| {
            json!({
                "id": format!("{}.skill", ns),
                "name": ns,
                "description": format!("Generated from the editor-state interop registry namespace {ns}."),
                "tools": tools,
            })
        })
        .collect();
    json!({
        "name": "torogao-editor-interop",
        "description": "Live editor-state interop for Torogao: session-authenticated MCP/A2A surface over one registry. Mutations are typed ops through torogao-author's decide(); sessions compile to canonical deterministic records.",
        "version": "0.1.0",
        "protocol": "torogao-editor-interop/1",
        "skills": skills,
    })
}
