/*
    Torogao lane packs — loading and governed vocabularies

    Copyright © 2026, Nice Thoughts Ranch
    Distributed under the 2-Clause BSD License, see LICENSE file.

    A lane pack is versioned data (format torogao-pack/1) in
    the repository's packs/ directory: the lane profile
    (adult / all-ages), the rating rules and stamp contract,
    the consent-model schema, ONE governed expression set,
    the governed tracking parameter sets, the writer's arc
    ladder and word-count bounds, and the pack's kink
    vocabulary (the writer's deny mechanism works against it).

    The pack format hardcodes neither lane: both shipped packs
    are the same shape with different data, and a third lane
    would be a third file, not a code change.
*/
module torogao.cli.packs;

import torogao.cli.datadirs : findDataFile;
import std.file : readText;
import std.json : JSONValue, JSONType, parseJSON;

/// Pack document format identifier.
enum packFormat = "torogao-pack/1";

/// Loads and format-checks a pack by name ("adult", "all-ages").
JSONValue loadPack(string name, out string error) {
    string path = findDataFile("TOROGAO_PACKS_DIR", "packs", name ~ ".json");
    if (path.length == 0) {
        error = "lane pack '" ~ name ~ "' not found";
        return JSONValue(null);
    }
    JSONValue doc;
    try doc = parseJSON(readText(path));
    catch (Exception e) {
        error = "lane pack '" ~ name ~ "' is unreadable: " ~ e.msg;
        return JSONValue(null);
    }
    if (doc.type != JSONType.object || packStr(doc, "format") != packFormat) {
        error = "lane pack '" ~ name ~ "' is not a " ~ packFormat ~ " document";
        return JSONValue(null);
    }
    return doc;
}

/// Names of all shipped packs (the v1 pair; the directory is
/// the registry, so a new pack file needs no code change to
/// be loadable — listing is by the known v1 names plus any
/// file the search directories carry is out of scope here).
string[] packNames() {
    string[] names;
    foreach (candidate; ["adult", "all-ages"]) {
        if (findDataFile("TOROGAO_PACKS_DIR", "packs",
                candidate ~ ".json").length > 0)
            names ~= candidate;
    }
    return names;
}

/// Object member access yielding a null JSONValue when absent.
JSONValue packGet(JSONValue obj, string key) {
    if (obj.type != JSONType.object) return JSONValue(null);
    auto p = key in obj.object;
    return p is null ? JSONValue(null) : *p;
}

/// String member access yielding "" when absent or mistyped.
string packStr(JSONValue obj, string key) {
    auto v = packGet(obj, key);
    return v.type == JSONType.string ? v.str : "";
}

/// The pack's governed expression set (array of
/// {name, semantics, defaultRecipe}).
JSONValue[] packExpressions(JSONValue pack) {
    auto arr = packGet(pack, "expressions");
    if (arr.type != JSONType.array) return [];
    return arr.array.dup;
}

/// One governed expression by name; null JSONValue when absent.
JSONValue packExpression(JSONValue pack, string name) {
    foreach (e; packExpressions(pack))
        if (packStr(e, "name") == name) return e;
    return JSONValue(null);
}

/// One tracking set by id from the pack; null when absent.
JSONValue packTrackingSet(JSONValue pack, string setId) {
    auto sets = packGet(pack, "trackingSets");
    if (sets.type != JSONType.array) return JSONValue(null);
    foreach (s; sets.array)
        if (packStr(s, "id") == setId) return s;
    return JSONValue(null);
}

/// The pack's rating rule object.
JSONValue packRatingRules(JSONValue pack) {
    return packGet(pack, "ratingRules");
}

/// Is `rating` permitted for a project in lane `lane`,
/// per the pack's own lane-rating table?
bool packRatingAllowed(JSONValue pack, string lane, string rating) {
    auto table = packGet(packRatingRules(pack), "laneRatings");
    auto allowed = packGet(table, lane);
    if (allowed.type != JSONType.array) return false;
    foreach (v; allowed.array)
        if (v.type == JSONType.string && v.str == rating) return true;
    return false;
}

/// Is `rating` a value the pack knows at all?
bool packRatingKnown(JSONValue pack, string rating) {
    auto values = packGet(packRatingRules(pack), "values");
    if (values.type != JSONType.array) return false;
    foreach (v; values.array)
        if (v.type == JSONType.string && v.str == rating) return true;
    return false;
}

/// The pack's word-count bounds for a scene class
/// ({minWords, maxWords}); null JSONValue when the class is
/// not a class the pack defines.
JSONValue packWordBounds(JSONValue pack, string sceneClass) {
    return packGet(packGet(pack, "wordCountBounds"), sceneClass);
}

/// The pack's kink vocabulary (the writer's deny reference).
string[] packKinkVocabulary(JSONValue pack) {
    auto arr = packGet(pack, "kinkVocabulary");
    string[] out_;
    if (arr.type == JSONType.array)
        foreach (v; arr.array)
            if (v.type == JSONType.string) out_ ~= v.str;
    return out_;
}
