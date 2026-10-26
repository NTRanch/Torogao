/*
    Torogao P3 tools (agent-surface handlers)

    Copyright © 2026, Nice Thoughts Ranch
    Distributed under the 2-Clause BSD License, see LICENSE file.

    The P3 tool family: lane packs, rating checks, expression
    states + variants, expression/tracking coverage, hit
    areas, the Character Creator, the writer core, and the
    voice/SFX data checks. Every handler runs exactly the
    code the CLI subcommands run. Read/validate tools write
    nothing; apply-state is a PREVIEW input (read-class — it
    returns a curve document and changes no project);
    hitareas-author and create-character write NEW files to
    caller-named paths only (export-class).
*/
module torogao.tools.p3tools;

import inochi2d.torogao.bridge : torogaoInitHeadless;
import inochi2d.core : Puppet;
import inochi2d.fmt : inLoadPuppet;
import torogao.cli.creator;
import torogao.cli.criteria : jget;
import torogao.cli.extdata : torogaoExtFormat, writeProjectWithExt;
import torogao.cli.packs;
import torogao.cli.physicspreview;
import torogao.cli.puppetwalk;
import torogao.cli.states;
import torogao.cli.writer;
import torogao.tools.registry;
import std.file : exists, readText, write;
import std.functional : toDelegate;
import std.json : JSONValue, JSONType, parseJSON;
import std.path : dirName;
import std.file : mkdirRecurse;

private bool headlessReady;

private void ensureHeadless() {
    if (!headlessReady) {
        torogaoInitHeadless();
        headlessReady = true;
    }
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

private bool optString(JSONValue args, string key, out string value) {
    auto v = key in args;
    if (v is null || v.type != JSONType.string) return false;
    value = v.str;
    return true;
}

private JSONValue schemaProps(JSONValue[string] props, string[] required) {
    JSONValue s;
    s["type"] = "object";
    JSONValue p;
    foreach (k, v; props) p[k] = v;
    s["properties"] = p;
    JSONValue[] req;
    foreach (r; required) req ~= JSONValue(r);
    s["required"] = req;
    s["additionalProperties"] = false;
    return s;
}

private JSONValue strProp(string description) {
    JSONValue v;
    v["type"] = "string";
    v["description"] = description;
    return v;
}

private bool loadProjectArg(JSONValue args, out string projectPath,
                            out Puppet puppet, out PuppetWalk walk,
                            out ToolResult error) {
    if (!reqString(args, "project", projectPath, error)) return false;
    if (!exists(projectPath)) {
        error = ToolResult.fail("project_not_found",
            "project file not found: " ~ projectPath);
        return false;
    }
    ensureHeadless();
    try puppet = inLoadPuppet(projectPath);
    catch (Exception e) {
        error = ToolResult.fail("project_unloadable", e.msg);
        return false;
    }
    walk = new PuppetWalk(puppet);
    return true;
}

private bool loadPackArg(JSONValue args, out JSONValue pack,
                         out ToolResult error) {
    string name;
    if (!reqString(args, "pack", name, error)) return false;
    string packError;
    pack = loadPack(name, packError);
    if (packError.length > 0) {
        error = ToolResult.fail("pack_not_found", packError);
        return false;
    }
    return true;
}

private bool loadJsonArg(JSONValue args, string key, out JSONValue doc,
                         out ToolResult error) {
    string path;
    if (!reqString(args, key, path, error)) return false;
    string loadError;
    doc = loadJsonFile(path, loadError);
    if (loadError.length > 0) {
        error = ToolResult.fail("file_unreadable", loadError);
        return false;
    }
    return true;
}

// ------------------------------------------------------------ tools

private ToolResult toolPackInfo(JSONValue args) {
    JSONValue pack; ToolResult error;
    if (!loadPackArg(args, pack, error)) return error;
    JSONValue doc;
    doc["pack"] = pack;
    doc["packs"] = packNames();
    return ToolResult.ok(doc);
}

private ToolResult toolRatingCheck(JSONValue args) {
    string path; Puppet puppet; PuppetWalk walk; ToolResult error;
    if (!loadProjectArg(args, path, puppet, walk, error)) return error;
    auto view = extOf(puppet);
    JSONValue doc;
    doc["file"] = path;
    doc["stamp"] = ratingStamp(view.doc);
    string reason;
    bool blocked = view.present && ratingBlocksExport(view.doc, reason);
    doc["result"] = blocked ? "mismatch" : "ok";
    if (blocked) {
        doc["code"] = "TG-RATING-LANE-MISMATCH";
        doc["error"] = reason;
    }
    return ToolResult.ok(doc);
}

private ToolResult toolListStates(JSONValue args) {
    string path; Puppet puppet; PuppetWalk walk; ToolResult error;
    if (!loadProjectArg(args, path, puppet, walk, error)) return error;
    JSONValue doc;
    doc["file"] = path;
    doc["extension"] = extSummary(extOf(puppet).doc);
    return ToolResult.ok(doc);
}

private ToolResult toolApplyState(JSONValue args) {
    string path; Puppet puppet; PuppetWalk walk; ToolResult error;
    if (!loadProjectArg(args, path, puppet, walk, error)) return error;
    string trackPath;
    if (!reqString(args, "track", trackPath, error)) return error;
    string stateName, variantId;
    optString(args, "state", stateName);
    optString(args, "variant", variantId);
    if (stateName.length == 0 && variantId.length == 0)
        return ToolResult.fail("invalid_arguments",
            "one of 'state' or 'variant' is required");
    if (!exists(trackPath))
        return ToolResult.fail("track_not_found",
            "input track file not found: " ~ trackPath);
    JSONValue track;
    try track = parseJSON(readText(trackPath));
    catch (Exception e)
        return ToolResult.fail("track_unreadable",
            "input track is not valid JSON: " ~ e.msg);
    PreviewOptions options;
    options.ext = extOf(puppet).doc;
    options.stateName = stateName;
    options.variantId = variantId;
    string previewError;
    auto curves = runPhysicsPreview(puppet, track, 0, path, options,
        previewError);
    if (previewError !is null)
        return ToolResult.fail("preview_failed", previewError);
    return ToolResult.ok(curves.toJsonValue());
}

private ToolResult toolExpressionCheck(JSONValue args) {
    string path; Puppet puppet; PuppetWalk walk; ToolResult error;
    if (!loadProjectArg(args, path, puppet, walk, error)) return error;
    JSONValue pack;
    if (!loadPackArg(args, pack, error)) return error;
    auto missing = expressionMissing(extOf(puppet).doc, pack);
    JSONValue doc;
    doc["file"] = path;
    doc["pack"] = packStr(pack, "name");
    doc["missing"] = missing;
    doc["result"] = missing.length == 0 ? "covered" : "findings";
    if (missing.length > 0) doc["code"] = TG_EXPR_MISSING;
    return ToolResult.ok(doc);
}

private ToolResult toolTrackingCheck(JSONValue args) {
    string path; Puppet puppet; PuppetWalk walk; ToolResult error;
    if (!loadProjectArg(args, path, puppet, walk, error)) return error;
    JSONValue pack;
    if (!loadPackArg(args, pack, error)) return error;
    string setId;
    if (!reqString(args, "set", setId, error)) return error;
    auto set = packTrackingSet(pack, setId);
    if (set.type != JSONType.object)
        return ToolResult.fail("set_not_found",
            "pack '" ~ packStr(pack, "name") ~ "' has no tracking set '"
            ~ setId ~ "'");
    string[] missing, rangeBad;
    trackingCoverage(puppet, set, missing, rangeBad);
    JSONValue doc;
    doc["file"] = path;
    doc["pack"] = packStr(pack, "name");
    doc["set"] = setId;
    doc["completeness"] = packStr(set, "completeness");
    doc["missing"] = missing;
    doc["rangeViolations"] = rangeBad;
    bool ok = missing.length == 0 && rangeBad.length == 0;
    doc["result"] = ok ? "covered" : "findings";
    if (missing.length > 0) doc["code"] = TG_TRACK_MISSING;
    else if (rangeBad.length > 0) doc["code"] = TG_TRACK_RANGE;
    return ToolResult.ok(doc);
}

private ToolResult toolHitareasList(JSONValue args) {
    string path; Puppet puppet; PuppetWalk walk; ToolResult error;
    if (!loadProjectArg(args, path, puppet, walk, error)) return error;
    auto view = extOf(puppet);
    JSONValue doc;
    doc["file"] = path;
    doc["hitAreas"] = view.present
        ? jget(jget(view.doc, "consumer"), "hitAreas") : JSONValue(null);
    return ToolResult.ok(doc);
}

private ToolResult toolHitareasAuthor(JSONValue args) {
    string path; Puppet puppet; PuppetWalk walk; ToolResult error;
    if (!loadProjectArg(args, path, puppet, walk, error)) return error;
    JSONValue spec;
    if (!loadJsonArg(args, "spec", spec, error)) return error;
    string outPath;
    if (!reqString(args, "out", outPath, error)) return error;
    auto areas = jget(spec, "hitAreas");
    if (areas.type != JSONType.array) areas = spec;
    if (areas.type != JSONType.array)
        return ToolResult.fail("invalid_arguments",
            "spec must be an array or an object with a hitAreas array");
    auto view = extOf(puppet);
    JSONValue ext = view.present ? view.doc : JSONValue.init;
    if (ext.type != JSONType.object) ext["format"] = torogaoExtFormat;
    JSONValue consumer = jget(ext, "consumer");
    if (consumer.type != JSONType.object) consumer = JSONValue.init;
    consumer["hitAreas"] = areas;
    ext["consumer"] = consumer;
    auto problems = validateHitAreasOnly(ext, puppet, walk);
    if (problems.length > 0) {
        JSONValue doc;
        doc["result"] = "refused";
        JSONValue[] list;
        foreach (f; problems) list ~= f.toJson();
        doc["findings"] = list;
        return ToolResult.ok(doc);
    }
    string parent = dirName(outPath);
    if (parent.length > 0) mkdirRecurse(parent);
    string writeError;
    if (!writeProjectWithExt(path, outPath, ext, writeError))
        return ToolResult.fail("io_error", writeError);
    JSONValue doc;
    doc["result"] = "authored";
    doc["out"] = outPath;
    doc["hitAreas"] = areas.array.length;
    return ToolResult.ok(doc);
}

private ToolResult toolCreateCharacter(JSONValue args) {
    JSONValue identity; ToolResult error;
    if (!loadJsonArg(args, "identity", identity, error)) return error;
    JSONValue pack;
    if (!loadPackArg(args, pack, error)) return error;
    string outPath;
    if (!reqString(args, "out", outPath, error)) return error;
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
    CreatorProblem[] problems;
    string createError;
    auto summary = createCharacter(identity, pack, presets, outPath,
        problems, createError);
    if (problems.length > 0) {
        JSONValue doc;
        doc["result"] = "refused";
        JSONValue[] list;
        foreach (p; problems) list ~= p.toJson();
        doc["problems"] = list;
        return ToolResult.ok(doc);
    }
    if (createError.length > 0)
        return ToolResult.fail("create_failed", createError);
    return ToolResult.ok(summary);
}

private ToolResult toolWriterScaffold(JSONValue args) {
    JSONValue character; ToolResult error;
    if (!loadJsonArg(args, "character", character, error)) return error;
    JSONValue pack;
    if (!loadPackArg(args, pack, error)) return error;
    string kink;
    if (!reqString(args, "kink", kink, error)) return error;
    int stage = -1;
    if (auto sv = "stage" in args) {
        if (sv.type == JSONType.integer) stage = cast(int) sv.integer;
        else if (sv.type == JSONType.uinteger)
            stage = cast(int) sv.uinteger;
    }
    if (stage < 0)
        return ToolResult.fail("invalid_arguments",
            "argument 'stage' (non-negative integer) is required");
    string[] sfxIds;
    if (auto xv = "sfx" in args) {
        if (xv.type != JSONType.array)
            return ToolResult.fail("invalid_arguments",
                "argument 'sfx' must be an array of SFX entry ids");
        foreach (x; xv.array) {
            if (x.type != JSONType.string)
                return ToolResult.fail("invalid_arguments",
                    "argument 'sfx' must be an array of SFX entry ids");
            sfxIds ~= x.str;
        }
    }
    JSONValue scaffold, gap;
    buildScaffold(character, pack, kink, stage, sfxIds, scaffold, gap);
    JSONValue doc = gap.type == JSONType.object ? gap : scaffold;
    string outPath;
    if (optString(args, "out", outPath)) {
        string parent = dirName(outPath);
        if (parent.length > 0) mkdirRecurse(parent);
        write(outPath, doc.toString() ~ "\n");
        JSONValue wrap;
        wrap["out"] = outPath;
        wrap["document"] = doc;
        return ToolResult.ok(wrap);
    }
    return ToolResult.ok(doc);
}

private ToolResult toolVoiceCheck(JSONValue args) {
    JSONValue record; ToolResult error;
    if (!loadJsonArg(args, "record", record, error)) return error;
    auto findings = validateVoice(record);
    JSONValue doc;
    doc["result"] = findings.length == 0 ? "clean" : "findings";
    JSONValue[] list;
    foreach (f; findings) list ~= f.toJson();
    doc["findings"] = list;
    return ToolResult.ok(doc);
}

private ToolResult toolSfxList(JSONValue args) {
    string sfxError;
    auto index = loadSfxIndex(sfxError);
    if (sfxError.length > 0)
        return ToolResult.fail("sfx_unavailable", sfxError);
    return ToolResult.ok(index);
}

/// Registers the P3 tool family on a registry.
void registerP3Tools(ToolRegistry reg) {
    void add(string name, string title, string desc,
             JSONValue[string] props, string[] required,
             ToolPosture posture, ToolHandler handler) {
        ToolSpec spec;
        spec.name = name;
        spec.namespace_ = "p3";
        spec.title = title;
        spec.description = desc;
        spec.inputSchema = schemaProps(props, required);
        spec.posture = posture;
        reg.register(spec, handler);
    }

    JSONValue[string] packOnly;
    packOnly["pack"] = strProp("Lane pack name (adult, all-ages).");
    JSONValue[string] projectOnly;
    projectOnly["project"] = strProp("Path to an INP/INX project file.");
    JSONValue[string] projectPack;
    projectPack["project"] = strProp("Path to an INP/INX project file.");
    projectPack["pack"] = strProp("Lane pack name (adult, all-ages).");

    add("pack-info", "Lane pack information",
        "Returns a lane pack in full: lane profile, rating rules "
        ~ "and stamp contract, consent model, governed expression "
        ~ "set, tracking sets, arc ladder, and word-count bounds.",
        packOnly, ["pack"], ToolPosture.readOnly,
        toDelegate(&toolPackInfo));

    add("rating-check", "Check project rating stamp",
        "Returns the rating stamp a project would export with, "
        ~ "and reports a typed mismatch when the rating is one "
        ~ "its lane forbids.",
        projectOnly, ["project"], ToolPosture.readOnly,
        toDelegate(&toolRatingCheck));

    add("list-states", "List states and variants",
        "Lists a project's stored expression states, stage "
        ~ "variants, blend shapes, hit areas, tracking claims, "
        ~ "and rating meta from its vendor-extension section.",
        projectOnly, ["project"], ToolPosture.readOnly,
        toDelegate(&toolListStates));

    JSONValue[string] applyProps;
    applyProps["project"] = strProp("Path to an INP/INX project file.");
    applyProps["track"] = strProp("Path to an input track JSON file.");
    applyProps["state"] = strProp("Stored state name to apply.");
    applyProps["variant"] = strProp("Stored variant id to apply.");
    add("apply-state", "Apply state/variant in preview",
        "Runs the deterministic physics preview with a stored "
        ~ "expression state and/or stage variant applied as an "
        ~ "input layer. Read-class: it returns curves and "
        ~ "changes no project.",
        applyProps, ["project", "track"], ToolPosture.readOnly,
        toDelegate(&toolApplyState));

    add("expression-check", "Check expression coverage",
        "Checks a project's states against a lane pack's "
        ~ "governed expression set; missing names are listed "
        ~ "(TG-EXPR-MISSING).",
        projectPack, ["project", "pack"], ToolPosture.readOnly,
        toDelegate(&toolExpressionCheck));

    JSONValue[string] trackProps;
    trackProps["project"] = strProp("Path to an INP/INX project file.");
    trackProps["pack"] = strProp("Lane pack name (adult, all-ages).");
    trackProps["set"] = strProp("Tracking set id in the pack.");
    add("tracking-check", "Check tracking set coverage",
        "Checks a project's parameters against a governed "
        ~ "tracking set: every ID present (TG-TRACK-MISSING) "
        ~ "and in range (TG-TRACK-RANGE).",
        trackProps, ["project", "pack", "set"], ToolPosture.readOnly,
        toDelegate(&toolTrackingCheck));

    add("hitareas-list", "List hit areas",
        "Lists a project's consumer-layer hit areas (names, "
        ~ "shapes, part bindings).",
        projectOnly, ["project"], ToolPosture.readOnly,
        toDelegate(&toolHitareasList));

    JSONValue[string] authorProps;
    authorProps["project"] = strProp("Path to an INP/INX project file.");
    authorProps["spec"] = strProp("Path to a hit-area spec JSON file.");
    authorProps["out"] = strProp("Output path for the NEW project file.");
    add("hitareas-author", "Author hit areas (new file)",
        "Writes a NEW project file whose consumer layer carries "
        ~ "the spec's hit areas; the spec is validated first "
        ~ "(unknown part or bad shape refuses the write). The "
        ~ "source project is never modified.",
        authorProps, ["project", "spec", "out"],
        ToolPosture.exportOnly, toDelegate(&toolHitareasAuthor));

    JSONValue[string] createProps;
    createProps["identity"] = strProp("Path to a character record "
        ~ "(torogao-character/1).");
    createProps["pack"] = strProp("Lane pack name (adult, all-ages).");
    createProps["out"] = strProp("Output path for the NEW project file.");
    JSONValue presetsProp;
    presetsProp["type"] = "array";
    JSONValue presetItems;
    presetItems["type"] = "string";
    presetsProp["items"] = presetItems;
    presetsProp["description"] = "Physics-chain preset names.";
    createProps["presets"] = presetsProp;
    add("create-character", "Create character project",
        "The Character Creator, headless: lane pack + base "
        ~ "template + presets + a character record (consent "
        ~ "validated against the pack's model) become a NEW "
        ~ "project carrying the pack's governed states.",
        createProps, ["identity", "pack", "out"],
        ToolPosture.exportOnly, toDelegate(&toolCreateCharacter));

    JSONValue[string] writerProps;
    writerProps["character"] = strProp("Path to a character record "
        ~ "(torogao-character/1).");
    writerProps["pack"] = strProp("Lane pack name (adult, all-ages).");
    writerProps["kink"] = strProp("The requested kink (must be in "
        ~ "the character's vector).");
    JSONValue stageProp;
    stageProp["type"] = "integer";
    stageProp["description"] = "Arc ladder stage (0-based).";
    writerProps["stage"] = stageProp;
    JSONValue sfxProp;
    sfxProp["type"] = "array";
    JSONValue sfxItems;
    sfxItems["type"] = "string";
    sfxProp["items"] = sfxItems;
    sfxProp["description"] = "SFX library entry ids to attach.";
    writerProps["sfx"] = sfxProp;
    writerProps["out"] = strProp("Optional output path for the "
        ~ "scaffold/gap document (export-class).");
    add("writer-scaffold", "Build a writer scaffold",
        "Builds a canon-grounded scene scaffold for "
        ~ "(character x kink x stage), or a loud gap report "
        ~ "naming the missing canon when the canon does not "
        ~ "cover the request. Model-free and deterministic.",
        writerProps, ["character", "pack", "kink", "stage"],
        ToolPosture.exportOnly, toDelegate(&toolWriterScaffold));

    JSONValue[string] voiceProps;
    voiceProps["record"] = strProp("Path to a voice record or a "
        ~ "character record carrying one.");
    add("voice-check", "Validate a voice record",
        "Validates a voice record: estate-synthetic provenance "
        ~ "only (TG-VOICE-PROVENANCE) and a versioned anchor "
        ~ "(TG-VOICE-ANCHOR).",
        voiceProps, ["record"], ToolPosture.readOnly,
        toDelegate(&toolVoiceCheck));

    JSONValue[string] noProps;
    add("sfx-list", "List the SFX library",
        "Lists the SFX library index: named entries authored "
        ~ "as deterministic audio recipes, attachable as cue "
        ~ "points in scene data.",
        noProps, [], ToolPosture.readOnly,
        toDelegate(&toolSfxList));
}
