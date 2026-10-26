/*
    Torogao physics tools

    Copyright © 2026, Nice Thoughts Ranch
    Distributed under the 2-Clause BSD License, see LICENSE file.

    The deterministic physics preview as a registry tool:
    the same runPhysicsPreview the CLI subcommand drives,
    exposed to the MCP and A2A surfaces. Read-only — the
    preview never writes to the project or anywhere else;
    it returns the curve document as its payload.
*/
module torogao.tools.physicstools;

import inochi2d.torogao.bridge : torogaoInitHeadless;
import inochi2d.core : Puppet;
import inochi2d.fmt : inLoadPuppet;
import torogao.cli.drivers;
import torogao.cli.physicspreview;
import torogao.cli.states;
import torogao.tools.registry;
import std.functional : toDelegate;
import std.json : JSONValue, JSONType, parseJSON;
import std.file : exists, readText;

private bool headlessReady;

private void ensureHeadless() {
    if (!headlessReady) {
        torogaoInitHeadless();
        headlessReady = true;
    }
}

private JSONValue schemaPreview() {
    JSONValue s;
    s["type"] = "object";
    JSONValue props;
    JSONValue project;
    project["type"] = "string";
    project["description"] = "Path to an INP/INX project file.";
    props["project"] = project;
    JSONValue track;
    track["type"] = "string";
    track["description"] =
        "Path to an input track JSON file (torogao-input-track/1).";
    props["track"] = track;
    JSONValue fps;
    fps["type"] = "integer";
    fps["description"] =
        "Optional step-rate override; one of 15, 30, 60, 120.";
    props["fps"] = fps;
    JSONValue state;
    state["type"] = "string";
    state["description"] =
        "Optional stored expression state to apply as an input layer.";
    props["state"] = state;
    JSONValue variant;
    variant["type"] = "string";
    variant["description"] =
        "Optional stored stage variant id to apply as an input layer.";
    props["variant"] = variant;
    JSONValue drivers;
    drivers["type"] = "object";
    drivers["description"] =
        "Optional ambient driver set {drivers: [names], seed: n}, " ~
        "layered under authored inputs.";
    props["drivers"] = drivers;
    s["properties"] = props;
    s["required"] = [JSONValue("project"), JSONValue("track")];
    s["additionalProperties"] = false;
    return s;
}

private bool reqString(JSONValue args, string key, out string value,
                       out ToolResult error) {
    auto v = key in args;
    if (v is null || v.type != JSONType.string || v.str.length == 0) {
        error = ToolResult.fail("invalid_arguments",
            "argument '" ~ key ~ "' (non-empty string) is required");
        return false;
    }
    value = v.str;
    return true;
}

private ToolResult toolPhysicsPreview(JSONValue args) {
    if (args.type != JSONType.object)
        return ToolResult.fail("invalid_arguments",
            "arguments must be a JSON object");
    string projectPath, trackPath;
    ToolResult error;
    if (!reqString(args, "project", projectPath, error)) return error;
    if (!reqString(args, "track", trackPath, error)) return error;
    int fpsOverride = 0;
    if (auto fv = "fps" in args) {
        if (fv.type == JSONType.integer)
            fpsOverride = cast(int) fv.integer;
        else if (fv.type == JSONType.uinteger)
            fpsOverride = cast(int) fv.uinteger;
        else
            return ToolResult.fail("invalid_arguments",
                "argument 'fps' must be an integer");
    }
    if (!exists(projectPath))
        return ToolResult.fail("project_not_found",
            "project file not found: " ~ projectPath);
    if (!exists(trackPath))
        return ToolResult.fail("track_not_found",
            "input track file not found: " ~ trackPath);

    JSONValue track;
    try {
        track = parseJSON(readText(trackPath));
    } catch (Exception ex) {
        return ToolResult.fail("track_unreadable",
            "input track is not valid JSON: " ~ ex.msg);
    }

    ensureHeadless();
    Puppet puppet;
    try {
        puppet = inLoadPuppet(projectPath);
    } catch (Exception ex) {
        return ToolResult.fail("project_unloadable", ex.msg);
    }

    PreviewOptions options;
    options.ext = extOf(puppet).doc;
    if (auto sv = "state" in args)
        if (sv.type == JSONType.string) options.stateName = sv.str;
    if (auto vv = "variant" in args)
        if (vv.type == JSONType.string) options.variantId = vv.str;
    if (auto dv = "drivers" in args) {
        string driverError;
        if (!parseDriverConfig(*dv, options.drivers, driverError))
            return ToolResult.fail("invalid_arguments", driverError);
        options.hasDrivers = true;
    }

    string previewError;
    auto curves = runPhysicsPreview(puppet, track, fpsOverride,
        projectPath, options, previewError);
    if (previewError !is null)
        return ToolResult.fail("preview_failed", previewError);
    return ToolResult.ok(curves.toJsonValue());
}

/// Registers the physics tools on a registry.
void registerPhysicsTools(ToolRegistry reg) {
    ToolSpec spec;
    spec.name = "physics-preview";
    spec.namespace_ = "physics";
    spec.title = "Deterministic physics preview";
    spec.description =
        "Steps a project's physics at a fixed rate (default 60/s) " ~
        "under a scripted input track and returns every parameter's " ~
        "curve as a canonical torogao-curves/1 document. The run is " ~
        "a pure function of the project, the track, and the rate: " ~
        "identical inputs produce byte-identical curve files.";
    spec.inputSchema = schemaPreview();
    spec.posture = ToolPosture.readOnly;
    reg.register(spec, toDelegate(&toolPhysicsPreview));
}
