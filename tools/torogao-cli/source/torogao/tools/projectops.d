/*
    Torogao core project tools

    Copyright © 2026, Nice Thoughts Ranch
    Distributed under the 2-Clause BSD License, see LICENSE file.

    The v1 tool set: the headless project operations from the
    CLI (inspect, validate, export-rig) plus a payload-level
    project-info, exposed through the tool registry so the
    MCP server, the A2A surface, and the editor assist all run
    exactly the code the CLI runs. One declared-but-disabled
    tool (apply-fixes) demonstrates the registry's write gate:
    project-writing tools are discoverable here first and are
    only enabled by a later phase's gate, never by a surface.
*/
module torogao.tools.projectops;

import inochi2d.torogao.bridge : torogaoInitHeadless;
import inochi2d.core : Puppet;
import inochi2d.fmt : inLoadPuppet;
import inochi2d.integration : inCurrentPuppetTextureSlots;
import torogao.cli.criteria;
import torogao.cli.inpfile;
import torogao.cli.inspect_;
import torogao.cli.puppetwalk;
import torogao.cli.rigexport;
import torogao.tools.registry;
import std.functional : toDelegate;
import std.json : JSONValue, JSONType;
import std.file : exists, getSize, mkdirRecurse, read, write;
import std.path : baseName, buildPath, stripExtension;

private bool headlessReady;

private void ensureHeadless() {
    if (!headlessReady) {
        torogaoInitHeadless();
        headlessReady = true;
    }
}

private JSONValue schemaProjectOnly() {
    JSONValue s;
    s["type"] = "object";
    JSONValue props;
    JSONValue project;
    project["type"] = "string";
    project["description"] = "Path to an INP/INX project file.";
    props["project"] = project;
    s["properties"] = props;
    s["required"] = [JSONValue("project")];
    s["additionalProperties"] = false;
    return s;
}

private JSONValue schemaExport() {
    JSONValue s = schemaProjectOnly();
    JSONValue out_;
    out_["type"] = "string";
    out_["description"] = "Directory the rig model and export report are written to.";
    s["properties"]["out"] = out_;
    s["required"] = [JSONValue("project"), JSONValue("out")];
    return s;
}

/**
    Pulls the required string argument `key` out of a tool
    call. On failure returns false and sets `error`.
*/
private bool stringArg(JSONValue args, string key, out string value,
                       out ToolResult error) {
    if (args.type != JSONType.object) {
        error = ToolResult.fail("invalid_arguments",
            "arguments must be a JSON object");
        return false;
    }
    auto v = key in args;
    if (v is null || v.type != JSONType.string || v.str.length == 0) {
        error = ToolResult.fail("invalid_arguments",
            "argument '" ~ key ~ "' (non-empty string) is required");
        return false;
    }
    value = v.str;
    return true;
}

/// Shared front half of the loading tools: args -> loaded puppet + walk.
private bool loadForTool(JSONValue args, out string projectPath,
                         out Puppet puppet, out PuppetWalk walk,
                         out JSONValue payload, out size_t slotCount,
                         out ToolResult error) {
    if (!stringArg(args, "project", projectPath, error)) return false;
    if (!exists(projectPath)) {
        error = ToolResult.fail("project_not_found",
            "project file not found: " ~ projectPath);
        return false;
    }
    ensureHeadless();
    try {
        puppet = inLoadPuppet(projectPath);
    } catch (Exception ex) {
        error = ToolResult.fail("project_unloadable", ex.msg);
        return false;
    }
    walk = new PuppetWalk(puppet);
    payload = inpPayload(projectPath);
    slotCount = inCurrentPuppetTextureSlots.length;
    return true;
}

private ToolResult toolInspect(JSONValue args) {
    string projectPath; Puppet puppet; PuppetWalk walk;
    JSONValue payload; size_t slotCount; ToolResult error;
    if (!loadForTool(args, projectPath, puppet, walk, payload,
                     slotCount, error))
        return error;
    JSONValue doc;
    doc["file"] = projectPath;
    doc["census"] = census(puppet, walk);
    return ToolResult.ok(doc);
}

private ToolResult toolValidate(JSONValue args) {
    string projectPath; Puppet puppet; PuppetWalk walk;
    JSONValue payload; size_t slotCount; ToolResult error;
    if (!loadForTool(args, projectPath, puppet, walk, payload,
                     slotCount, error))
        return error;
    auto findings = validatePuppet(puppet, walk, payload, slotCount);
    JSONValue doc;
    doc["file"] = projectPath;
    doc["result"] = findings.length == 0 ? "clean" : "findings";
    JSONValue[] list;
    foreach (f; findings) list ~= f.toJson();
    doc["findings"] = list;
    return ToolResult.ok(doc);
}

private ToolResult toolExportRig(JSONValue args) {
    string projectPath; Puppet puppet; PuppetWalk walk;
    JSONValue payload; size_t slotCount; ToolResult error;
    if (!loadForTool(args, projectPath, puppet, walk, payload,
                     slotCount, error))
        return error;
    string outDir;
    if (!stringArg(args, "out", outDir, error)) return error;

    auto exporter = new RigExporter(puppet, walk, slotCount);
    ExportResult result;
    try {
        result = exporter.run();
    } catch (Exception ex) {
        return ToolResult.fail("export_failed", ex.msg);
    }
    try {
        mkdirRecurse(outDir);
        string base = projectPath.baseName.stripExtension;
        string modelPath = buildPath(outDir, base ~ ".model.json");
        string reportPath = buildPath(outDir, base ~ ".export-report.json");
        write(modelPath, result.model.toString() ~ "\n");
        JSONValue report;
        report["file"] = projectPath;
        report["model"] = modelPath;
        report["mapped"] = result.count("mapped");
        report["approximated"] = result.count("approximated");
        report["dropped"] = result.count("dropped");
        JSONValue[] list;
        foreach (e; result.entries) list ~= e.toJson();
        report["entries"] = list;
        write(reportPath, report.toString() ~ "\n");
        return ToolResult.ok(report);
    } catch (Exception ex) {
        return ToolResult.fail("io_error", ex.msg);
    }
}

/**
    Payload-level project information: reads the INP container
    only (no puppet load), so it answers fast and works even
    for projects the loader would reject.
*/
private ToolResult toolProjectInfo(JSONValue args) {
    string projectPath; ToolResult error;
    if (!stringArg(args, "project", projectPath, error)) return error;
    if (!exists(projectPath))
        return ToolResult.fail("project_not_found",
            "project file not found: " ~ projectPath);

    ubyte[] buffer;
    try {
        buffer = cast(ubyte[]) read(projectPath);
    } catch (Exception ex) {
        return ToolResult.fail("io_error", ex.msg);
    }
    JSONValue doc;
    doc["file"] = projectPath;
    doc["sizeBytes"] = buffer.length;
    enum magic = cast(ubyte[]) "TRNSRTS\0";
    if (buffer.length < 12 || buffer[0 .. 8] != magic) {
        doc["container"] = "unknown";
        doc["payload"] = false;
        return ToolResult.ok(doc);
    }
    doc["container"] = "INP";

    JSONValue payload;
    try {
        payload = inpPayload(projectPath);
    } catch (Exception ex) {
        return ToolResult.fail("project_unloadable",
            "INP payload unreadable: " ~ ex.msg);
    }
    if (payload.type == JSONType.null_) {
        doc["payload"] = false;
        return ToolResult.ok(doc);
    }
    doc["payload"] = true;
    if (auto meta = "meta" in payload) doc["meta"] = *meta;

    string[] keys;
    if (payload.type == JSONType.object)
        foreach (k, _; payload.object) keys ~= k;
    import std.algorithm : sort;
    sort(keys);
    JSONValue[] keyJson;
    foreach (k; keys) keyJson ~= JSONValue(k);
    doc["payloadKeys"] = keyJson;

    JSONValue counts;
    if (auto anims = "animations" in payload)
        if (anims.type == JSONType.array)
            counts["animations"] = anims.array.length;
    if (auto phys = "physics" in payload) {
        if (phys.type == JSONType.array)
            counts["physics"] = phys.array.length;
        else if (phys.type == JSONType.object)
            counts["physics"] = phys.object.length;
    }
    if (auto prm = "param" in payload)
        if (prm.type == JSONType.object)
            counts["parameterSections"] = prm.object.length;
    doc["counts"] = counts;

    // Texture section trailer, when present after the payload.
    import std.bitmanip : bigEndianToNative;
    ubyte[4] lenBytes = buffer[8 .. 12];
    size_t payloadEnd = 12 + bigEndianToNative!uint(lenBytes);
    JSONValue textures;
    enum texMagic = cast(ubyte[]) "TEX_SECT\0";
    if (payloadEnd + 12 <= buffer.length
            && buffer[payloadEnd .. payloadEnd + 8] == texMagic) {
        ubyte[4] countBytes = buffer[payloadEnd + 8 .. payloadEnd + 12];
        textures["present"] = true;
        textures["count"] = bigEndianToNative!uint(countBytes);
    } else {
        textures["present"] = false;
    }
    doc["textures"] = textures;
    return ToolResult.ok(doc);
}

/// Builds the registry with the v1 core tools registered.
ToolRegistry buildCoreRegistry() {
    auto reg = new ToolRegistry;

    ToolSpec inspectSpec;
    inspectSpec.name = "inspect";
    inspectSpec.namespace_ = "core";
    inspectSpec.title = "Inspect project";
    inspectSpec.description =
        "Loads an INP/INX project and returns its census: metadata, " ~
        "node tree, parameters, parts, and texture slots.";
    inspectSpec.inputSchema = schemaProjectOnly();
    inspectSpec.posture = ToolPosture.readOnly;
    reg.register(inspectSpec, toDelegate(&toolInspect));

    ToolSpec validateSpec;
    validateSpec.name = "validate";
    validateSpec.namespace_ = "core";
    validateSpec.title = "Validate project";
    validateSpec.description =
        "Runs the structural validation gates over a project and " ~
        "returns the findings (stable TG- codes); a clean project " ~
        "returns result \"clean\" with an empty findings list.";
    validateSpec.inputSchema = schemaProjectOnly();
    validateSpec.posture = ToolPosture.readOnly;
    reg.register(validateSpec, toDelegate(&toolValidate));

    ToolSpec exportSpec;
    exportSpec.name = "export-rig";
    exportSpec.namespace_ = "core";
    exportSpec.title = "Export rig model";
    exportSpec.description =
        "Converts a project to the lumen-rig model format, writing " ~
        "the model and a conversion report (mapped / approximated " ~
        "/ dropped, every entry reasoned) to the named output " ~
        "directory. Writes only to that directory; the project " ~
        "itself is never modified.";
    exportSpec.inputSchema = schemaExport();
    exportSpec.posture = ToolPosture.exportOnly;
    reg.register(exportSpec, toDelegate(&toolExportRig));

    ToolSpec infoSpec;
    infoSpec.name = "project-info";
    infoSpec.namespace_ = "core";
    infoSpec.title = "Project information";
    infoSpec.description =
        "Reads only the INP container (no puppet load): file size, " ~
        "metadata, payload sections, and texture section summary. " ~
        "Fast, and works on projects the loader would reject.";
    infoSpec.inputSchema = schemaProjectOnly();
    infoSpec.posture = ToolPosture.readOnly;
    reg.register(infoSpec, toDelegate(&toolProjectInfo));

    ToolSpec fixSpec;
    fixSpec.name = "apply-fixes";
    fixSpec.namespace_ = "core";
    fixSpec.title = "Apply validation fixes";
    fixSpec.description =
        "Would rewrite a project file to repair validation " ~
        "findings automatically. Declared for discovery only: " ~
        "project-writing tools stay disabled until a later phase " ~
        "lands the write gate.";
    fixSpec.inputSchema = schemaProjectOnly();
    fixSpec.posture = ToolPosture.declaredDisabled;
    fixSpec.gate = "mutation gate (a later phase): tools that write " ~
        "into a project require the decide()-class write gate and " ~
        "an explicit operator enable; neither exists yet";
    reg.register(fixSpec, null);

    return reg;
}
