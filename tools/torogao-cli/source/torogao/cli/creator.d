/*
    Torogao Character Creator — headless creation flow

    Copyright © 2026, Nice Thoughts Ranch
    Distributed under the 2-Clause BSD License, see LICENSE file.

    The Creator flow, headless-first: start from a lane pack
    and the base template, apply zero or more physics-chain
    presets, take anchors and identity fields (including the
    consent-model schema) from a character record, install
    the pack's governed expression set as the character's own
    states, claim the standard face tracking set, and save
    the result as a NEW project file (an export-class write;
    no existing project is ever modified).

    The character record is the identity input and is stored
    verbatim in the project's vendor-extension section, so
    every later surface — writer, validators, export stamps —
    reads the same canon the Creator was given. Consent is
    validated against the pack's own consent model before
    anything is written: the model's required fields must all
    be present and true in the record.
*/
module torogao.cli.creator;

import torogao.cli.extdata : torogaoExtFormat, writeProjectWithExt;
import torogao.cli.packs;
import torogao.cli.templates : instantiateTemplate;
import torogao.cli.writer : characterFormat;
import std.file : exists, remove;
import std.json : JSONValue, JSONType;
import std.path : dirName;
import std.file : mkdirRecurse;

/// Creation failure codes surfaced to CLI + registry callers.
enum TG_CREATOR_CONSENT = "TG-CREATOR-CONSENT";
enum TG_CREATOR_IDENTITY = "TG-CREATOR-IDENTITY";

/// One creation problem: a stable code plus a named field.
struct CreatorProblem {
    string code;
    string field;
    string message;

    JSONValue toJson() const {
        JSONValue j;
        j["code"] = code;
        j["field"] = field;
        j["message"] = message;
        return j;
    }
}

/**
    Validates the identity record against the pack: format,
    name, anchors, and the pack's consent model. Returns the
    problem list (empty = the identity may be created).
*/
CreatorProblem[] checkIdentity(JSONValue identity, JSONValue pack) {
    CreatorProblem[] problems;
    if (packStr(identity, "format") != characterFormat) {
        problems ~= CreatorProblem(TG_CREATOR_IDENTITY, "format",
            "identity record is not a " ~ characterFormat ~ " document");
        return problems;
    }
    if (packStr(identity, "name").length == 0)
        problems ~= CreatorProblem(TG_CREATOR_IDENTITY, "name",
            "identity record carries no character name");
    auto anchors = packGet(identity, "anchors");
    if (anchors.type != JSONType.object)
        problems ~= CreatorProblem(TG_CREATOR_IDENTITY, "anchors",
            "identity record carries no anchors object");

    auto consent = packGet(identity, "consent");
    auto model = packGet(pack, "consentModel");
    auto required = packGet(model, "requiredFields");
    if (consent.type != JSONType.object) {
        problems ~= CreatorProblem(TG_CREATOR_CONSENT, "consent",
            "identity record carries no consent object; the pack's "
            ~ "consent model requires one");
        return problems;
    }
    if (packStr(consent, "schema") != packStr(model, "schema"))
        problems ~= CreatorProblem(TG_CREATOR_CONSENT, "consent.schema",
            "consent schema does not match the pack's consent model");
    if (required.type == JSONType.object) {
        foreach (field, want; required.object) {
            auto got = packGet(consent, field);
            bool ok = false;
            if (want.type == JSONType.true_ && got.type == JSONType.true_)
                ok = true;
            if (!ok)
                problems ~= CreatorProblem(TG_CREATOR_CONSENT,
                    "consent." ~ field,
                    "consent field '" ~ field ~ "' must be true under "
                    ~ "the pack's consent model");
        }
    }
    return problems;
}

/**
    Creates a character project. On success `summary` holds
    the creation record and error is empty. All validation
    happens before any file is written; the only write is
    the new project at outPath (plus a temporary sibling
    during assembly, removed before return).
*/
JSONValue createCharacter(JSONValue identity, JSONValue pack,
                          string[] presets, string outPath,
                          out CreatorProblem[] problems,
                          out string error) {
    problems = checkIdentity(identity, pack);
    if (problems.length > 0) return JSONValue(null);

    string tmpPath = outPath ~ ".creator-tmp.inx";
    string instError;
    JSONValue inst;
    try inst = instantiateTemplate("base-humanoid", presets, tmpPath,
        instError);
    catch (Exception e) instError = e.msg;
    if (instError.length > 0) {
        error = "template instantiation failed: " ~ instError;
        return JSONValue(null);
    }

    // The extension document: pack-governed states (the
    // coverage obligation, satisfied from the pack's own
    // default recipes), the creation stamp, the identity
    // record verbatim, and empty consumer/blendshape/variant
    // pockets the authoring surfaces fill later.
    JSONValue ext;
    ext["format"] = torogaoExtFormat;
    JSONValue meta;
    string lane = packStr(pack, "lane");
    meta["pack"] = packStr(pack, "name");
    meta["lane"] = lane;
    meta["rating"] = lane == "adult" ? "adult" : "all-ages";
    meta["basis"] = "lane pack default at creation (create-character)";
    ext["meta"] = meta;
    ext["character"] = identity;

    JSONValue[] stateList;
    foreach (e; packExpressions(pack)) {
        JSONValue s;
        s["name"] = packStr(e, "name");
        s["recipe"] = packGet(e, "defaultRecipe");
        stateList ~= s;
    }
    ext["states"] = stateList;
    JSONValue[] noArr;
    ext["variants"] = noArr;
    ext["blendshapes"] = noArr;
    ext["trackingClaims"] = [JSONValue("face-standard")];
    JSONValue consumer;
    consumer["hitAreas"] = noArr;
    ext["consumer"] = consumer;

    string parent = dirName(outPath);
    if (parent.length > 0) mkdirRecurse(parent);
    string writeError;
    bool ok = writeProjectWithExt(tmpPath, outPath, ext, writeError);
    if (exists(tmpPath)) remove(tmpPath);
    if (!ok) {
        error = writeError;
        return JSONValue(null);
    }

    JSONValue summary;
    summary["result"] = "created";
    summary["character"] = packStr(identity, "name");
    summary["pack"] = packStr(pack, "name");
    summary["lane"] = lane;
    summary["rating"] = meta["rating"].str;
    summary["presets"] = presets;
    summary["states"] = stateList.length;
    summary["trackingClaims"] = ext["trackingClaims"];
    summary["out"] = outPath;
    return summary;
}
