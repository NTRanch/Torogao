/*
    Torogao template tools (agent-surface handlers)

    Copyright © 2026, Nice Thoughts Ranch
    Distributed under the 2-Clause BSD License, see LICENSE file.

    list-templates is read-only. instantiate-template writes
    a NEW puppet file to a caller-named path — the same
    export-class posture as export-rig; it never modifies
    an existing project.
*/
module torogao.tools.templatools;

import torogao.cli.templates;
import torogao.tools.registry;
import std.functional : toDelegate;
import std.json : JSONValue, JSONType;

private JSONValue schemaEmpty() {
    JSONValue s;
    s["type"] = "object";
    JSONValue props;
    s["properties"] = props;
    s["additionalProperties"] = false;
    return s;
}

private JSONValue schemaInstantiate() {
    JSONValue s;
    s["type"] = "object";
    JSONValue props;
    JSONValue template_;
    template_["type"] = "string";
    template_["description"] = "Name of a puppet template " ~
        "(e.g. base-humanoid).";
    props["template"] = template_;
    JSONValue presets;
    presets["type"] = "array";
    JSONValue items;
    items["type"] = "string";
    presets["items"] = items;
    presets["description"] = "Names of physics-chain presets " ~
        "to apply, in order.";
    props["presets"] = presets;
    JSONValue out_;
    out_["type"] = "string";
    out_["description"] = "Output path for the new INP file.";
    props["out"] = out_;
    s["properties"] = props;
    s["required"] = [JSONValue("template"), JSONValue("out")];
    s["additionalProperties"] = false;
    return s;
}

private ToolResult toolListTemplates(JSONValue args) {
    JSONValue[] list;
    foreach (t; listTemplates()) {
        JSONValue e;
        e["name"] = t.name;
        e["kind"] = t.kind;
        e["title"] = t.title;
        e["description"] = t.description;
        list ~= e;
    }
    JSONValue doc;
    doc["templates"] = list;
    return ToolResult.ok(doc);
}

private ToolResult toolInstantiate(JSONValue args) {
    if (args.type != JSONType.object)
        return ToolResult.fail("invalid_arguments",
            "arguments must be a JSON object");
    auto tv = "template" in args;
    auto ov = "out" in args;
    if (tv is null || tv.type != JSONType.string || tv.str.length == 0)
        return ToolResult.fail("invalid_arguments",
            "argument 'template' (non-empty string) is required");
    if (ov is null || ov.type != JSONType.string || ov.str.length == 0)
        return ToolResult.fail("invalid_arguments",
            "argument 'out' (non-empty string) is required");
    string[] presets;
    if (auto pv = "presets" in args) {
        if (pv.type != JSONType.array)
            return ToolResult.fail("invalid_arguments",
                "argument 'presets' must be an array of names");
        foreach (p; pv.array) {
            if (p.type != JSONType.string)
                return ToolResult.fail("invalid_arguments",
                    "argument 'presets' must be an array of names");
            presets ~= p.str;
        }
    }
    string error;
    JSONValue summary;
    try summary = instantiateTemplate(tv.str, presets, ov.str, error);
    catch (Exception e)
        return ToolResult.fail("instantiate_failed", e.msg);
    if (error.length > 0)
        return ToolResult.fail("instantiate_failed", error);
    return ToolResult.ok(summary);
}

/// Register the template tools on a registry.
void registerTemplateTools(ToolRegistry reg) {
    ToolSpec listSpec;
    listSpec.name = "list-templates";
    listSpec.namespace_ = "templates";
    listSpec.title = "List rig templates";
    listSpec.description =
        "Lists the rig templates and physics-chain presets " ~
        "available for instantiation (base puppets, the six " ~
        "physics presets, and any lane-pack templates).";
    listSpec.inputSchema = schemaEmpty();
    listSpec.posture = ToolPosture.readOnly;
    reg.register(listSpec, toDelegate(&toolListTemplates));

    ToolSpec instSpec;
    instSpec.name = "instantiate-template";
    instSpec.namespace_ = "templates";
    instSpec.title = "Instantiate rig template";
    instSpec.description =
        "Instantiates a puppet template plus zero or more " ~
        "physics-chain presets into a NEW INP puppet file at " ~
        "the named output path. Writes only that file; no " ~
        "existing project is modified.";
    instSpec.inputSchema = schemaInstantiate();
    instSpec.posture = ToolPosture.exportOnly;
    reg.register(instSpec, toDelegate(&toolInstantiate));
}
