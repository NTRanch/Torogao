# Driving Torogao from an agent

Torogao's tooling is agent-drivable by design. Every agent-facing
operation lives in **one tool registry** inside `tools/torogao-cli`
(`torogao.tools.registry`); three surfaces expose it, and all three
run exactly the same code:

| Surface | Transport | Entry point |
|---|---|---|
| MCP server | JSON-RPC over stdio (NDJSON frames) | `torogao-cli mcp` |
| A2A | HTTP + JSON-RPC | `torogao-cli a2a [--bind 127.0.0.1] [--port 8377]` |
| Editor assist | The editor spawns the MCP server as a child | Tools → *Validate Project via Agent...* |

There are no back doors: anything the editor's built-in assist can
do, an external agent can do through the same tools, and later
features (templates, the character creator, tutorials, scenes)
appear on all surfaces automatically because both `tools/list`
and the Agent Card are **generated from the registry**.

## Tools (v3)

The v3 additions (P3) are the `p3` namespace: lane packs,
ratings, states, coverage, hit areas, the Creator, the writer,
and voice/SFX data.

| Tool | Posture | What it does |
|---|---|---|
| `pack-info` | read-only | Returns a lane pack in full: lane profile, rating rules + stamp contract, consent model, governed expression set, tracking sets, arc ladder, word-count bounds. |
| `rating-check` | read-only | Returns the rating stamp a project would export with; typed mismatch when the rating is one its lane forbids. |
| `list-states` | read-only | Lists a project's stored expression states, stage variants, blend shapes, hit areas, tracking claims, and rating meta. |
| `apply-state` | read-only | Runs the deterministic preview with a stored state and/or variant applied as an input layer; returns curves, changes no project. |
| `expression-check` | read-only | Coverage gate: a project's states against a pack's governed expression set (`TG-EXPR-MISSING` lists the missing names). |
| `tracking-check` | read-only | Coverage gate: a project's parameters against a governed tracking set (`TG-TRACK-MISSING` / `TG-TRACK-RANGE`). |
| `hitareas-list` | read-only | Lists a project's consumer-layer hit areas (names, shapes, part bindings). |
| `hitareas-author` | export | Writes a NEW project whose consumer layer carries the spec's hit areas; the spec is validated first and a bad spec refuses the write. |
| `create-character` | export | The Character Creator, headless: lane pack + base template + presets + a character record (consent validated against the pack's model) become a new project carrying the pack's governed states. |
| `writer-scaffold` | export | Builds a canon-grounded scene scaffold for (character × kink × stage), or a loud gap report (`TG-WRITER-GAP`) naming the missing canon. Model-free, deterministic. |
| `voice-check` | read-only | Validates a voice record: estate-synthetic provenance only, versioned anchor. |
| `sfx-list` | read-only | Lists the SFX library index (named deterministic audio recipes, attachable as cue points). |

`physics-preview` also accepts optional `state`, `variant`, and
`drivers` arguments (the ambient driver set, layered under
authored inputs — see `docs/ambient-drivers.md`).

## Format limits — a standing rule

Torogao and its native format carry **no inherited product-tier
limits**. No constant, validator bound, template default,
lane-pack rule, or export stamp may encode another product's
license-tier caps (texture size, mesh counts, parameter counts,
deformer counts, and the like). Tooling must handle textures up
to 8K (7680×4320) without rejecting or silently downscaling
them; per-platform size budgets belong only in named export
target sets, as chosen delivery settings, never as format
limits. Where an underlying library mechanism has a practical
internal limit below these targets, it is named in the phase
report as a measured upstream limit — never silently capped.

| Tool | Posture | What it does |
|---|---|---|
| `inspect` | read-only | Loads a project and returns its census: metadata, node tree, parameters, parts, texture slots. |
| `validate` | read-only | Runs the structural validation gates; returns findings with stable `TG-` codes, or `result: "clean"`. |
| `export-rig` | export | Converts a project to a lumen-rig model plus a conversion report (mapped / approximated / dropped, every entry reasoned). Writes only to the caller-named output directory; the project is never modified. |
| `project-info` | read-only | Reads only the INP container (no puppet load): size, metadata, payload sections, texture section summary. Fast; works on projects the loader would reject. |
| `apply-fixes` | **disabled** | Would rewrite a project to repair findings. Registered for discovery only — see *Mutation posture*. |
| `physics-preview` | read-only | Deterministic fixed-step physics preview: steps a project under an input track and returns the parameter curves (`torogao-curves/1`). Replays are byte-identical. |
| `list-templates` | read-only | Lists the rig templates and physics-chain presets available for instantiation. |
| `instantiate-template` | export | Builds a new INP puppet from a template plus presets at a caller-named path. Never modifies an existing project. |
| `audio-validate` | read-only | Checks a procedural-audio recipe; returns findings with stable `TG-AUDIO-` codes. |
| `audio-render` | export | Renders a recipe to a 48 kHz WAV master plus manifest (seed, engine, measured loudness/true peak, license) in a caller-named directory. |
| `audio-bake` | export | Bakes a WAV's amplitude envelope into a `torogao-curves/1` document at a caller-named path. |
| `viseme-check` | read-only | Gates a project against the viseme contract (mouth-parameter coverage). |
| `viseme-bake` | export | Bakes a viseme track into mouth-parameter curves at a caller-named path. |

Arguments are JSON objects. Every project tool takes
`{"project": "<path to an .inx/.inp file>"}`; `export-rig` also
requires `{"out": "<directory>"}`.

`torogao-cli registry-dump` prints the registry itself as JSON —
the authoritative tool list, schemas, and postures.

## MCP

Connect any MCP-capable agent host to the stdio server:

```json
{
  "mcpServers": {
    "torogao": {
      "command": "torogao-cli",
      "args": ["mcp"]
    }
  }
}
```

Protocol notes:

- Newline-delimited JSON-RPC 2.0; MCP protocol version
  `2024-11-05`; standard `initialize` / `tools/list` /
  `tools/call` / `ping` flow.
- **Frames are capped at 1 MiB.** An over-cap frame is consumed
  through its newline (the stream resyncs) and answered with a
  `frame_too_large` error. Malformed frames get a typed
  `parse_error`; nothing an agent sends can crash or hang the
  server.
- Tool descriptions, schemas, and posture annotations
  (`torogao:posture`, `torogao:namespace`, `torogao:gate` for
  disabled tools) come straight from the registry.
- Error model: client-caused problems (unknown tool, disabled
  tool, malformed arguments) are JSON-RPC errors with a stable
  machine code in `error.data.code`. Tool execution failures
  (missing project, unloadable project, failed export) are
  normal tool results with `isError: true` and the typed error
  as the text content — agents see them as tool outcomes, not
  transport failures.

## A2A

`torogao-cli a2a` serves:

- `GET /.well-known/agent-card.json` — the Agent Card. Skills
  are the registry tools (id, name, description, tags including
  posture), so the card never drifts from the MCP surface.
- `POST /` — JSON-RPC task methods `message/send`, `tasks/get`,
  `tasks/cancel`.

A work request is one data part naming the tool:

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "message/send",
  "params": {
    "message": {
      "role": "user",
      "messageId": "m1",
      "parts": [
        { "kind": "data",
          "data": { "tool": "validate",
                    "arguments": { "project": "puppet.inx" } } }
      ]
    }
  }
}
```

Tasks execute synchronously: the returned task is `completed`
with the tool's payload as its artifact, or `failed` with the
typed error as the artifact data. `tasks/get` returns a stored
task; `tasks/cancel` returns a terminal task unchanged.
Streaming and push notifications are not offered in v1 (the card
says so).

## The editor assist

Tools → **Validate Project via Agent...** runs the open project's
validation through the MCP server: the editor spawns
`torogao-cli mcp`, performs `initialize` + `tools/call validate`,
and shows the outcome in a dialog. The child is located via the
`TOROGAO_CLI` environment variable, then beside the editor
executable, then the in-tree build path, then the `PATH`. The
exchange has a 30-second deadline and never blocks the UI.

## Mutation posture

Tools on these surfaces **do not write into projects**:

- *read-only* tools write nothing anywhere;
- *export* tools write only to a caller-named output location,
  never into the project;
- tools that would modify a project are registered as
  **declared-but-disabled**: discoverable (with the gate that
  keeps them off named in the spec), rejected with a typed
  `tool_disabled` error on every call, on every surface.

v1's example is `apply-fixes`, held by the *mutation gate*: a
future phase must land a write gate (explicit operator enable,
decide-before-write) before any project-writing tool can be
enabled. A surface can never widen this posture on its own.

## Registering new tools (for contributors)

1. Implement the operation as a handler:
   `ToolResult delegate(JSONValue arguments)` returning
   `ToolResult.ok(payload)` or a typed `ToolResult.fail(code, …)`.
   Stable error codes are part of the contract.
2. Build a `ToolSpec` (name, namespace, title, description,
   JSON Schema for the arguments, posture — and a `gate` text
   when registering disabled) and `register()` it in the
   registry builder for your area.
3. Done: the MCP `tools/list`, the Agent Card skills, the
   registry dump, and every gate that diffs against them pick
   the tool up with no surface-side changes.
