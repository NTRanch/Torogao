/*
    Torogao viseme tools (agent-surface handlers)

    Copyright © 2026, Nice Thoughts Ranch
    Distributed under the 2-Clause BSD License, see LICENSE file.

    viseme-check is a read-only project gate registered in
    the core namespace alongside validate. viseme-bake is
    export-class: it writes one new curve document to a
    caller-named path.
*/
module torogao.tools.visemetools;

import inochi2d.torogao.bridge : torogaoInitHeadless;
import inochi2d.core : Puppet;
import inochi2d.fmt : inLoadPuppet;
import torogao.cli.viseme;
import torogao.curves;
import torogao.tools.registry;
import std.file : exists, mkdirRecurse, readText, write;
import std.functional : toDelegate;
import std.json : JSONValue, JSONType, parseJSON;
import std.path : dirName;

private bool headlessReady;

private void ensureHeadless() {
    if (!headlessReady) {
        torogaoInitHeadless();
        headlessReady = true;
    }
}

private JSONValue schemaCheck() {
    JSONValue s;
    s["type"] = "object";
    JSONValue props;
    JSONValue project;
    project["type"] = "string";
    project["description"] = "Path to an INP/INX project file.";
    props["project"] = project;
    JSONValue contract_;
    contract_["type"] = "string";
    contract_["description"] = "Optional explicit path to a " ~
        "viseme contract (defaults to contracts/visemes.json).";
    props["contract"] = contract_;
    s["properties"] = props;
    s["required"] = [JSONValue("project")];
    s["additionalProperties"] = false;
    return s;
}

private JSONValue schemaBake() {
    JSONValue s;
    s["type"] = "object";
    JSONValue props;
    JSONValue track;
    track["type"] = "string";
    track["description"] = "Path to a viseme track JSON file " ~
        "(torogao-viseme-track/1).";
    props["track"] = track;
    JSONValue out_;
    out_["type"] = "string";
    out_["description"] = "Output path for the curve document.";
    props["out"] = out_;
    JSONValue contract_;
    contract_["type"] = "string";
    contract_["description"] = "Optional explicit path to a " ~
        "viseme contract.";
    props["contract"] = contract_;
    s["properties"] = props;
    s["required"] = [JSONValue("track"), JSONValue("out")];
    s["additionalProperties"] = false;
    return s;
}

private ToolResult toolVisemeCheck(JSONValue args) {
    if (args.type != JSONType.object)
        return ToolResult.fail("invalid_arguments",
            "arguments must be a JSON object");
    auto pv = "project" in args;
    if (pv is null || pv.type != JSONType.string || pv.str.length == 0)
        return ToolResult.fail("invalid_arguments",
            "argument 'project' (non-empty string) is required");
    string contractPath;
    if (auto cv = "contract" in args)
        if (cv.type == JSONType.string) contractPath = cv.str;
    if (!exists(pv.str))
        return ToolResult.fail("project_not_found",
            "project file not found: " ~ pv.str);
    string error;
    auto contract_ = loadVisemeContract(contractPath, error);
    if (error.length > 0)
        return ToolResult.fail("contract_unavailable", error);
    ensureHeadless();
    Puppet puppet;
    try puppet = inLoadPuppet(pv.str);
    catch (Exception e)
        return ToolResult.fail("project_unloadable", e.msg);
    auto findings = checkVisemes(puppet, contract_);
    auto covered = coveredParameters(puppet, contract_);
    JSONValue doc;
    doc["file"] = pv.str;
    doc["contract"] = contract_.setName;
    doc["covered"] = covered;
    string[] missing;
    foreach (paramName; contract_.parameters) {
        bool isCovered = false;
        foreach (c; covered) if (c == paramName) isCovered = true;
        if (!isCovered) missing ~= paramName;
    }
    doc["missing"] = missing;
    JSONValue[] list;
    foreach (f; findings) list ~= f.toJson();
    doc["result"] = findings.length > 0 ? "findings" : "clean";
    doc["findings"] = list;
    return ToolResult.ok(doc);
}

private ToolResult toolVisemeBake(JSONValue args) {
    if (args.type != JSONType.object)
        return ToolResult.fail("invalid_arguments",
            "arguments must be a JSON object");
    auto tv = "track" in args;
    auto ov = "out" in args;
    if (tv is null || tv.type != JSONType.string || tv.str.length == 0)
        return ToolResult.fail("invalid_arguments",
            "argument 'track' (non-empty string) is required");
    if (ov is null || ov.type != JSONType.string || ov.str.length == 0)
        return ToolResult.fail("invalid_arguments",
            "argument 'out' (non-empty string) is required");
    string contractPath;
    if (auto cv = "contract" in args)
        if (cv.type == JSONType.string) contractPath = cv.str;
    if (!exists(tv.str))
        return ToolResult.fail("track_not_found",
            "track file not found: " ~ tv.str);
    string error;
    auto contract_ = loadVisemeContract(contractPath, error);
    if (error.length > 0)
        return ToolResult.fail("contract_unavailable", error);
    JSONValue track;
    try track = parseJSON(readText(tv.str));
    catch (Exception e)
        return ToolResult.fail("track_unreadable",
            "track is not valid JSON: " ~ e.msg);
    CurveSet curves;
    if (!bakeVisemes(track, contract_, curves, error))
        return ToolResult.fail("bake_failed", error);
    string parent = dirName(ov.str);
    if (parent.length > 0) mkdirRecurse(parent);
    write(ov.str, curves.toJson() ~ "\n");
    JSONValue doc;
    doc["track"] = tv.str;
    doc["curves"] = ov.str;
    doc["steps"] = curves.steps();
    doc["series"] = curves.series.length;
    return ToolResult.ok(doc);
}

/// Register the viseme tools on a registry.
void registerVisemeTools(ToolRegistry reg) {
    ToolSpec checkSpec;
    checkSpec.name = "viseme-check";
    checkSpec.namespace_ = "core";
    checkSpec.title = "Check viseme coverage";
    checkSpec.description =
        "Checks a project against the viseme contract: every " ~
        "contract mouth parameter must be present (error " ~
        "TG-VISEME-PARAM-MISSING) and bound or driven " ~
        "(warning TG-VISEME-PARAM-UNBOUND).";
    checkSpec.inputSchema = schemaCheck();
    checkSpec.posture = ToolPosture.readOnly;
    reg.register(checkSpec, toDelegate(&toolVisemeCheck));

    ToolSpec bakeSpec;
    bakeSpec.name = "viseme-bake";
    bakeSpec.namespace_ = "core";
    bakeSpec.title = "Bake viseme track";
    bakeSpec.description =
        "Bakes a viseme track (timed viseme events) into a " ~
        "torogao-curves/1 document over the contract mouth " ~
        "parameters, using the contract's hold/blend rule.";
    bakeSpec.inputSchema = schemaBake();
    bakeSpec.posture = ToolPosture.exportOnly;
    reg.register(bakeSpec, toDelegate(&toolVisemeBake));
}
