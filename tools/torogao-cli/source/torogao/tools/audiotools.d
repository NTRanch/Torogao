/*
    Torogao audio tools (agent-surface handlers)

    Copyright © 2026, Nice Thoughts Ranch
    Distributed under the 2-Clause BSD License, see LICENSE file.

    audio-validate is read-only. audio-render and
    audio-bake write NEW files (a master + manifest, a
    curve document) to caller-named paths — export-class,
    like export-rig; they never modify an input.
*/
module torogao.tools.audiotools;

import torogao.audio.bake;
import torogao.audio.proc;
import torogao.audio.wav;
import torogao.curves;
import torogao.tools.registry;
import std.file : exists, mkdirRecurse, readText, write;
import std.functional : toDelegate;
import std.json : JSONValue, JSONType, parseJSON;
import std.path : dirName;

private JSONValue schemaRecipe() {
    JSONValue s;
    s["type"] = "object";
    JSONValue props;
    JSONValue recipe;
    recipe["type"] = "string";
    recipe["description"] = "Path to a recipe JSON file " ~
        "(torogao-audio-recipe/1).";
    props["recipe"] = recipe;
    s["properties"] = props;
    s["required"] = [JSONValue("recipe")];
    s["additionalProperties"] = false;
    return s;
}

private JSONValue schemaRender() {
    JSONValue s = schemaRecipe();
    JSONValue out_;
    out_["type"] = "string";
    out_["description"] = "Output directory for the WAV " ~
        "master and its manifest.";
    s["properties"]["out"] = out_;
    s["required"] = [JSONValue("recipe"), JSONValue("out")];
    return s;
}

private JSONValue schemaBake() {
    JSONValue s;
    s["type"] = "object";
    JSONValue props;
    JSONValue wav;
    wav["type"] = "string";
    wav["description"] = "Path to a WAV master or stem.";
    props["wav"] = wav;
    JSONValue recipe;
    recipe["type"] = "string";
    recipe["description"] = "Path to a bake recipe JSON file " ~
        "(torogao-audio-bake/1).";
    props["recipe"] = recipe;
    JSONValue out_;
    out_["type"] = "string";
    out_["description"] = "Output path for the curve document.";
    props["out"] = out_;
    s["properties"] = props;
    s["required"] = [JSONValue("wav"), JSONValue("recipe"),
                     JSONValue("out")];
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

private bool loadRecipe(string path, out JSONValue recipe,
                        out string recipeText, out ToolResult error) {
    if (!exists(path)) {
        error = ToolResult.fail("recipe_not_found",
            "recipe file not found: " ~ path);
        return false;
    }
    try {
        recipeText = readText(path);
        recipe = parseJSON(recipeText);
    } catch (Exception e) {
        error = ToolResult.fail("recipe_unreadable",
            "recipe is not valid JSON: " ~ e.msg);
        return false;
    }
    return true;
}

private ToolResult toolAudioValidate(JSONValue args) {
    if (args.type != JSONType.object)
        return ToolResult.fail("invalid_arguments",
            "arguments must be a JSON object");
    string path;
    ToolResult error;
    if (!reqString(args, "recipe", path, error)) return error;
    JSONValue recipe;
    string recipeText;
    if (!loadRecipe(path, recipe, recipeText, error)) return error;
    auto findings = validateRecipe(recipe);
    JSONValue doc;
    doc["file"] = path;
    bool hasError = false;
    JSONValue[] list;
    foreach (f; findings) {
        list ~= f.toJson();
        if (f.severity == "error") hasError = true;
    }
    doc["result"] = hasError ? "findings" : "clean";
    doc["findings"] = list;
    return ToolResult.ok(doc);
}

private ToolResult toolAudioRender(JSONValue args) {
    if (args.type != JSONType.object)
        return ToolResult.fail("invalid_arguments",
            "arguments must be a JSON object");
    string path, outDir;
    ToolResult error;
    if (!reqString(args, "recipe", path, error)) return error;
    if (!reqString(args, "out", outDir, error)) return error;
    JSONValue recipe;
    string recipeText;
    if (!loadRecipe(path, recipe, recipeText, error)) return error;
    mkdirRecurse(outDir);
    auto result = renderRecipe(recipe, recipeText, outDir);
    if (result.error.length > 0)
        return ToolResult.fail("render_failed", result.error);
    return ToolResult.ok(result.manifest);
}

private ToolResult toolAudioBake(JSONValue args) {
    if (args.type != JSONType.object)
        return ToolResult.fail("invalid_arguments",
            "arguments must be a JSON object");
    string wavPath, recipePath, outPath;
    ToolResult error;
    if (!reqString(args, "wav", wavPath, error)) return error;
    if (!reqString(args, "recipe", recipePath, error)) return error;
    if (!reqString(args, "out", outPath, error)) return error;
    if (!exists(wavPath))
        return ToolResult.fail("wav_not_found",
            "WAV file not found: " ~ wavPath);
    JSONValue recipe;
    string recipeText;
    if (!loadRecipe(recipePath, recipe, recipeText, error)) return error;
    WavData wav;
    try wav = readWavFile(wavPath);
    catch (Exception e)
        return ToolResult.fail("wav_unreadable", e.msg);
    CurveSet curves;
    string bakeError;
    if (!bakeEnvelope(wav, recipe, wavPath, curves, bakeError))
        return ToolResult.fail("bake_failed", bakeError);
    string parent = dirName(outPath);
    if (parent.length > 0) mkdirRecurse(parent);
    write(outPath, curves.toJson() ~ "\n");
    JSONValue doc;
    doc["file"] = wavPath;
    doc["curves"] = outPath;
    doc["steps"] = curves.steps();
    doc["series"] = curves.series.length;
    return ToolResult.ok(doc);
}

/// Register the audio tools on a registry.
void registerAudioTools(ToolRegistry reg) {
    ToolSpec validateSpec;
    validateSpec.name = "audio-validate";
    validateSpec.namespace_ = "audio";
    validateSpec.title = "Validate audio recipe";
    validateSpec.description =
        "Checks a torogao-audio-recipe/1 document against " ~
        "the recipe schema and returns findings with stable " ~
        "TG-AUDIO- codes.";
    validateSpec.inputSchema = schemaRecipe();
    validateSpec.posture = ToolPosture.readOnly;
    reg.register(validateSpec, toDelegate(&toolAudioValidate));

    ToolSpec renderSpec;
    renderSpec.name = "audio-render";
    renderSpec.namespace_ = "audio";
    renderSpec.title = "Render procedural audio";
    renderSpec.description =
        "Renders a recipe deterministically to a 48 kHz " ~
        "24-bit WAV master normalized to the loudness " ~
        "standard, plus a manifest (recipe hash, seed, " ~
        "engine, measured loudness and true peak, license). " ~
        "Writes only into the named output directory.";
    renderSpec.inputSchema = schemaRender();
    renderSpec.posture = ToolPosture.exportOnly;
    reg.register(renderSpec, toDelegate(&toolAudioRender));

    ToolSpec bakeSpec;
    bakeSpec.name = "audio-bake";
    bakeSpec.namespace_ = "audio";
    bakeSpec.title = "Bake audio envelope curve";
    bakeSpec.description =
        "Bakes a WAV file's amplitude envelope (house " ~
        "attack/release follower constants) into a " ~
        "torogao-curves/1 document that drives parameters.";
    bakeSpec.inputSchema = schemaBake();
    bakeSpec.posture = ToolPosture.exportOnly;
    reg.register(bakeSpec, toDelegate(&toolAudioBake));
}
