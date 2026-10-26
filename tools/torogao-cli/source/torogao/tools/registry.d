/*
    Torogao agent tool registry

    Copyright © 2026, Nice Thoughts Ranch
    Distributed under the 2-Clause BSD License, see LICENSE file.

    The single tool layer for every agent-facing surface of
    Torogao. The MCP server mode, the A2A surface, and the
    editor's built-in assist all call into one ToolRegistry;
    there are no parallel implementations and no back doors.
    Later phases register their tools here under their own
    namespaces (templates, creator, tutorials, writer, scenes);
    the surfaces pick new tools up automatically because both
    tools/list and the Agent Card are generated from this
    registry.

    Mutation posture: a tool declares how it may touch the
    world. v1 registers read-only and export-only tools plus
    tools that are declared-but-disabled: discoverable in the
    registry (with the gate that keeps them off named in the
    spec), rejected with a typed error on every call. Nothing
    on any surface writes into a user's project silently.
*/
module torogao.tools.registry;

import std.json : JSONValue;
import std.algorithm : sort;
import std.exception : enforce;

/// Version of the agent tool surface (registry + MCP + A2A).
enum torogaoToolsVersion = "2.0.0";

/**
    How a tool is allowed to touch the world.

    readOnly:         reads project data, writes nothing.
    exportOnly:       writes only to a caller-named output
                      location; never writes into the project.
    declaredDisabled: registered for discovery; every call
                      fails with a typed error naming the gate.
*/
enum ToolPosture {
    readOnly,
    exportOnly,
    declaredDisabled,
}

/// Human/machine readable posture label used on the surfaces.
string postureLabel(ToolPosture posture) {
    final switch (posture) {
        case ToolPosture.readOnly:         return "read-only";
        case ToolPosture.exportOnly:       return "export";
        case ToolPosture.declaredDisabled: return "disabled";
    }
}

/// One registered tool.
struct ToolSpec {
    string name;        /// Surface name, unique across the registry.
    string namespace_;  /// Owning area: "core", later "templates", ...
    string title;       /// Short human title.
    string description; /// What the tool does, for agents and humans.
    JSONValue inputSchema; /// JSON Schema object for the arguments.
    ToolPosture posture;
    string gate;        /// declaredDisabled only: the gate keeping it off.

    /// The spec as JSON (registry dumps, cards, docs generation).
    JSONValue toJson() const {
        JSONValue j;
        j["name"] = name;
        j["namespace"] = namespace_;
        j["title"] = title;
        j["description"] = description;
        j["inputSchema"] = inputSchema;
        j["posture"] = postureLabel(posture);
        if (gate.length > 0) j["gate"] = gate;
        return j;
    }
}

/// Outcome of a tool call: a payload, or a typed failure.
struct ToolResult {
    bool isError;
    string errorCode;    /// Stable machine code when isError.
    string errorMessage; /// Human-readable detail when isError.
    JSONValue payload;   /// Structured result when !isError.

    static ToolResult ok(JSONValue payload) {
        ToolResult r;
        r.payload = payload;
        return r;
    }

    static ToolResult fail(string code, string message) {
        ToolResult r;
        r.isError = true;
        r.errorCode = code;
        r.errorMessage = message;
        return r;
    }

    /// The failure as a JSON object (surface error payloads).
    JSONValue errorJson() const {
        JSONValue j;
        JSONValue e;
        e["code"] = errorCode;
        e["message"] = errorMessage;
        j["error"] = e;
        return j;
    }
}

/// A tool implementation: JSON arguments in, a ToolResult out.
alias ToolHandler = ToolResult delegate(JSONValue arguments);

/**
    The registry. Registration order is free; every listing
    the surfaces produce is sorted by tool name so output is
    deterministic and diffable.
*/
final class ToolRegistry {
    private ToolSpec[string] specs_;
    private ToolHandler[string] handlers_;

    /// Registers a tool. Duplicate names are a programming error.
    void register(ToolSpec spec, ToolHandler handler) {
        enforce(spec.name.length > 0, "tool name must not be empty");
        enforce((spec.name in specs_) is null,
            "duplicate tool registration: " ~ spec.name);
        if (spec.posture != ToolPosture.declaredDisabled) {
            enforce(handler !is null,
                "enabled tool needs a handler: " ~ spec.name);
        }
        specs_[spec.name] = spec;
        handlers_[spec.name] = handler;
    }

    /// All specs, sorted by tool name.
    ToolSpec[] list() {
        auto names = specs_.keys;
        sort(names);
        ToolSpec[] out_;
        foreach (name; names) out_ ~= specs_[name];
        return out_;
    }

    /// Looks up one spec; null when unregistered.
    const(ToolSpec)* find(string name) const {
        return name in specs_;
    }

    /**
        Calls a tool by name. Unknown names and disabled tools
        are typed failures, never exceptions; a handler that
        throws is contained and reported as internal_error so a
        surface can never be crashed by a tool.
    */
    ToolResult call(string name, JSONValue arguments) {
        auto spec = name in specs_;
        if (spec is null)
            return ToolResult.fail("unknown_tool",
                "no tool named '" ~ name ~ "' is registered");
        if (spec.posture == ToolPosture.declaredDisabled)
            return ToolResult.fail("tool_disabled",
                "tool '" ~ name ~ "' is declared but disabled; gate: "
                ~ spec.gate);
        try {
            return handlers_[name](arguments);
        } catch (Exception ex) {
            return ToolResult.fail("internal_error", ex.msg);
        }
    }

    /// The whole registry as JSON (the dump the gates diff against).
    JSONValue dumpJson() {
        JSONValue[] tools;
        foreach (spec; list()) tools ~= spec.toJson();
        JSONValue j;
        j["version"] = torogaoToolsVersion;
        j["tools"] = tools;
        return j;
    }
}
