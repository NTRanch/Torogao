/*
    Torogao validation gates v1 — criteria engine

    Copyright © 2026, Nice Thoughts Ranch
    Distributed under the 2-Clause BSD License, see LICENSE file.

    One mechanism, many callers: this module is the single home of
    the P1 structural criteria. The headless CLI calls it today;
    the editor's export gate and the tutorial checker call the same
    functions later. Nothing in here knows about the CLI.
*/
module torogao.cli.criteria;

import inochi2d.core;
import torogao.cli.puppetwalk;
import std.json : JSONValue, JSONType;
import std.format : format;

/// Stable finding codes (TG- prefix; never renumbered).
enum TG_MISSING_TEXTURE   = "TG-MISSING-TEXTURE";
enum TG_PARAM_NO_BINDINGS = "TG-PARAM-NO-BINDINGS";
enum TG_BINDING_NODE      = "TG-BINDING-MISSING-NODE";
enum TG_BINDING_PARAM     = "TG-BINDING-MISSING-PARAM";
enum TG_EMPTY_PART        = "TG-EMPTY-PART";
enum TG_DUPLICATE_NAME    = "TG-DUPLICATE-NAME";

struct Finding {
    string code;
    string severity; /// "error" or "warning"
    string subject;  /// node path or parameter name
    string message;

    JSONValue toJson() const {
        JSONValue j;
        j["code"] = code;
        j["severity"] = severity;
        j["subject"] = subject;
        j["message"] = message;
        return j;
    }
}

/**
    Runs the v1 criteria over a loaded puppet.

    `rawPayload` is the puppet's INP JSON payload when available
    (may be a null JSONValue). It is required for one check: the
    loader silently drops bindings whose target node does not
    exist (Parameter.finalize), so a dangling binding is only
    observable in the payload. All other checks run on the object
    model alone.
*/
Finding[] validatePuppet(Puppet puppet, PuppetWalk walk,
                         JSONValue rawPayload, size_t textureSlotCount) {
    Finding[] findings;

    // -- TG-EMPTY-PART + TG-MISSING-TEXTURE (per drawable) --------
    foreach(w; walk.order) {
        auto drawable = cast(Drawable)w.node;
        if (drawable is null) continue;
        auto mesh = drawable.getMesh();
        if (mesh.vertices.length == 0 || mesh.indices.length == 0) {
            findings ~= Finding(TG_EMPTY_PART, "error",
                PuppetWalk.pathOf(w),
                format!"%s has no geometry (%d vertices, %d indices); it renders nothing"
                    (w.typeId, mesh.vertices.length, mesh.indices.length));
        }
        if (auto part = cast(Part)w.node) {
            if (part.textureIds.length == 0) {
                findings ~= Finding(TG_MISSING_TEXTURE, "warning",
                    PuppetWalk.pathOf(w),
                    "part references no texture slot");
            }
            foreach(slot; part.textureIds) {
                if (slot >= textureSlotCount) {
                    findings ~= Finding(TG_MISSING_TEXTURE, "error",
                        PuppetWalk.pathOf(w),
                        format!"part references texture slot %d but the project embeds only %d slot(s)"
                            (slot, textureSlotCount));
                }
            }
        }
    }

    // -- Parameters: TG-PARAM-NO-BINDINGS, TG-DUPLICATE-NAME,
    //    TG-BINDING-MISSING-PARAM (object model) ------------------
    auto driven = puppet.getParameterDrivers();
    bool[string] seenParamNames;
    foreach(param; puppet.parameters) {
        if (param.name in seenParamNames) {
            findings ~= Finding(TG_DUPLICATE_NAME, "error", param.name,
                "duplicate parameter name in the parameter list");
        }
        seenParamNames[param.name] = true;

        bool isDriven = (param in driven) !is null;
        if (param.bindings.length == 0 && !isDriven) {
            findings ~= Finding(TG_PARAM_NO_BINDINGS, "warning", param.name,
                "parameter has no bindings and no driver; it cannot affect the puppet");
        }
        foreach(binding; param.bindings) {
            Node target = binding.getNode();
            if (target is null) {
                findings ~= Finding(TG_BINDING_NODE, "error", param.name,
                    format!"binding to node uuid %d did not resolve to a node"
                        (binding.getNodeUUID()));
                continue;
            }
            if (auto vb = cast(ValueParameterBinding)binding) {
                if (!target.hasParam(vb.getName())) {
                    findings ~= Finding(TG_BINDING_PARAM, "error", param.name,
                        format!"binding targets property '%s', which node '%s' does not provide"
                            (vb.getName(), target.name));
                }
            } else if (cast(DeformationParameterBinding)binding) {
                if (cast(Drawable)target is null) {
                    findings ~= Finding(TG_BINDING_PARAM, "error", param.name,
                        format!"deformation binding targets '%s', which is not a drawable"
                            (target.name));
                }
            }
        }
    }

    // -- TG-DUPLICATE-NAME among siblings --------------------------
    foreach(w; walk.order) {
        bool[string] seen;
        foreach(child; w.children) {
            if (child.name in seen) {
                findings ~= Finding(TG_DUPLICATE_NAME, "error",
                    PuppetWalk.pathOf(child),
                    format!"duplicate node name '%s' under '%s'"
                        (child.name, PuppetWalk.pathOf(w)));
            }
            seen[child.name] = true;
        }
    }

    // -- TG-BINDING-MISSING-NODE in the raw payload ----------------
    // The loader drops these silently; the payload is the only
    // place they remain visible.
    if (rawPayload.type == JSONType.object) {
        bool[uint] nodeUuids;
        collectNodeUuids(jget(rawPayload, "nodes"), nodeUuids);
        auto params = jget(rawPayload, "param");
        if (params.type == JSONType.array) {
            foreach(pv; params.array) {
                string pname = "<unnamed parameter>";
                if (jget(pv, "name").type == JSONType.string)
                    pname = jget(pv, "name").str;
                auto bindings = jget(pv, "bindings");
                if (bindings.type != JSONType.array) continue;
                foreach(bv; bindings.array) {
                    auto nodeRef = jget(bv, "node");
                    if (nodeRef.type != JSONType.integer &&
                        nodeRef.type != JSONType.uinteger) continue;
                    uint uuid = cast(uint)nodeRef.integer;
                    if (uuid !in nodeUuids) {
                        findings ~= Finding(TG_BINDING_NODE, "error", pname,
                            format!"payload binding references node uuid %d, which is not in the node tree (the loader drops it silently)"
                                (uuid));
                    }
                }
            }
        }
    }

    return findings;
}

/// Object member access that yields a null JSONValue for a
/// missing key instead of throwing.
JSONValue jget(JSONValue obj, string key) {
    if (obj.type != JSONType.object) return JSONValue(null);
    auto p = key in obj.object;
    return p is null ? JSONValue(null) : *p;
}

private void collectNodeUuids(JSONValue node, ref bool[uint] acc) {
    if (node.type != JSONType.object) return;
    auto uuid = jget(node, "uuid");
    if (uuid.type == JSONType.integer || uuid.type == JSONType.uinteger)
        acc[cast(uint)uuid.integer] = true;
    auto children = jget(node, "children");
    if (children.type == JSONType.array)
        foreach(child; children.array)
            collectNodeUuids(child, acc);
}
