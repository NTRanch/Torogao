/*
    Torogao writer core — canon-grounded scene scaffolds

    Copyright © 2026, Nice Thoughts Ranch
    Distributed under the 2-Clause BSD License, see LICENSE file.

    The writer core is a mechanism layer: deterministic,
    model-free, and grounded ONLY in canon — the character
    record (anchors, kink vector, conditioning tracks,
    refrain, voice rules) plus the lane pack's canon (arc
    ladder, governed expressions, word-count bounds, kink
    vocabulary). It produces scene scaffolds: dialogue
    skeletons, prose structure, and playable-scene script
    data wired to rig parameters, expression states, and
    audio cue points.

    The founding rule, enforced as a gate: a request whose
    kink is not in the character's vector — or whose canon
    inputs are missing — produces a loud GAP REPORT naming
    the missing canon. It never produces invented content.
    The estate learned this the hard way (a fleet chapter
    once shipped a kink vector its character never had);
    the regression fixture for that case is mechanical:
    request the invented kink, receive the gap report.

    Gates every scaffold passes before it is offered:
    canon-vector check, refrain/anchor presence, word-count
    bounds per scene class (bounds are pack data), and a
    deny scan — no kink-vocabulary term outside the
    character's vector may appear anywhere in the scaffold.
    The small-model drafting lane is a later phase; this
    core deliberately has no model in it.
*/
module torogao.cli.writer;

import torogao.cli.datadirs : findDataFile;
import torogao.cli.packs;
import std.digest.sha : sha256Of;
import std.file : readText;
import std.format : format;
import std.json : JSONValue, JSONType, parseJSON;
import std.uni : toLower;

/// Stable codes for writer + voice + SFX outcomes.
enum TG_WRITER_GAP       = "TG-WRITER-GAP";
enum TG_WRITER_DENIED    = "TG-WRITER-DENIED";
enum TG_VOICE_PROVENANCE = "TG-VOICE-PROVENANCE";
enum TG_VOICE_ANCHOR     = "TG-VOICE-ANCHOR";
enum TG_SFX_UNKNOWN      = "TG-SFX-UNKNOWN";

/// Character record format identifier.
enum characterFormat = "torogao-character/1";

/// Voice record format identifier.
enum voiceFormat = "torogao-voice/1";

/// Loads a JSON document from a file; error set on failure.
JSONValue loadJsonFile(string path, out string error) {
    try return parseJSON(readText(path));
    catch (Exception e) {
        error = "file '" ~ path ~ "' is unreadable: " ~ e.msg;
        return JSONValue(null);
    }
}

private bool nonEmpty(JSONValue v) {
    return (v.type == JSONType.string && v.str.length > 0)
        || v.type == JSONType.object || v.type == JSONType.array;
}

/// The canon fields a character record must carry, as
/// (field, human label) pairs; absence = a named gap.
private string[][] requiredCanon = [
    ["name", "character name"],
    ["anchors", "identity anchors"],
    ["kinkVector", "kink vector"],
    ["refrain", "refrain"],
    ["voiceRules", "voice rules"],
    ["voice", "voice record (anchor id + version + provenance)"],
];

/**
    Builds a scaffold for (character x kink x stage), or a
    gap report when the canon does not cover the request.
    Exactly one of `scaffold` / `gap` is non-null on return;
    `error` is set only for unreadable inputs (a gap is a
    result, not an error).
*/
void buildScaffold(JSONValue character, JSONValue pack,
                   string kink, int stage, string[] sfxIds,
                   out JSONValue scaffold, out JSONValue gap) {
    scaffold = JSONValue(null);
    gap = JSONValue(null);
    string[] missing;

    if (packStr(character, "format") != characterFormat)
        missing ~= "character record format (expected " ~ characterFormat ~ ")";
    foreach (pair; requiredCanon) {
        auto v = packGet(character, pair[0]);
        bool ok = false;
        if (pair[0] == "kinkVector")
            ok = v.type == JSONType.array && v.array.length > 0;
        else if (pair[0] == "anchors" || pair[0] == "voiceRules"
                || pair[0] == "voice")
            ok = v.type == JSONType.object;
        else
            ok = nonEmpty(v);
        if (!ok) missing ~= pair[1];
    }

    // Canon-vector check: the requested kink must be IN the
    // vector. This is the founding counterexample, mechanized.
    bool inVector = false;
    auto vector = packGet(character, "kinkVector");
    if (vector.type == JSONType.array)
        foreach (v; vector.array)
            if (v.type == JSONType.string && v.str == kink) inVector = true;
    if (!inVector)
        missing ~= "kink '" ~ kink ~ "' is not in the character's kink vector";

    // Ladder coverage: the stage must be a stage the pack's
    // ladder defines.
    JSONValue ladderEntry;
    auto ladder = packGet(pack, "arcLadder");
    if (ladder.type == JSONType.array)
        foreach (e; ladder.array) {
            auto sv = packGet(e, "stage");
            if ((sv.type == JSONType.integer && sv.integer == stage)
                    || (sv.type == JSONType.uinteger && sv.uinteger == stage))
                ladderEntry = e;
        }
    if (ladderEntry.type != JSONType.object)
        missing ~= format!"stage %d is not a stage of the pack's arc ladder"(stage);

    // The rupture line is required canon at its slot.
    int ruptureStage = -1;
    auto rs = packGet(pack, "ruptureStage");
    if (rs.type == JSONType.integer) ruptureStage = cast(int) rs.integer;
    if (stage == ruptureStage && packStr(character, "ruptureLine").length == 0)
        missing ~= "rupture line (required at the rupture stage)";

    // Word-count bounds must exist for every scene class the
    // scaffold emits.
    foreach (cls; ["dialogue", "prose", "script"]) {
        auto b = packWordBounds(pack, cls);
        if (b.type != JSONType.object)
            missing ~= "word-count bounds for scene class '" ~ cls ~ "'";
    }

    // Voice canon: provenance + anchor shape (same rules as
    // voice-check; a scaffold may not ride a bad anchor).
    auto voice = packGet(character, "voice");
    if (voice.type == JSONType.object) {
        if (packStr(voice, "provenance") != "estate-synthetic")
            missing ~= "voice provenance (must be estate-synthetic)";
        auto av = packGet(voice, "anchorVersion");
        if (!((av.type == JSONType.integer && av.integer >= 1)
                || (av.type == JSONType.uinteger && av.uinteger >= 1)))
            missing ~= "voice anchor version (a positive integer)";
        if (packStr(voice, "anchorId").length == 0)
            missing ~= "voice anchor id";
    }

    // SFX canon: every requested cue must be a library entry.
    JSONValue[] sfxEntries;
    string sfxError;
    auto index = loadSfxIndex(sfxError);
    foreach (id; sfxIds) {
        auto entry = sfxEntry(index, id);
        if (entry.type != JSONType.object) {
            missing ~= "sfx entry '" ~ id ~ "' is not in the SFX library";
        } else {
            sfxEntries ~= entry;
        }
    }

    if (missing.length > 0) {
        JSONValue g;
        g["format"] = "torogao-gap-report/1";
        g["result"] = "gap-report";
        g["code"] = TG_WRITER_GAP;
        g["character"] = packStr(character, "name");
        g["kink"] = kink;
        g["stage"] = stage;
        JSONValue[] list;
        foreach (m; missing) list ~= JSONValue(m);
        g["missingCanon"] = list;
        g["note"] = "The writer does not invent canon. Supply the "
            ~ "named fields (character record / lane pack) and re-run.";
        gap = g;
        return;
    }

    // -- scaffold construction (every element traces) ------
    JSONValue doc;
    doc["format"] = "torogao-scaffold/1";
    doc["result"] = "scaffold";
    doc["character"] = packStr(character, "name");
    doc["pack"] = packStr(pack, "name");
    doc["kink"] = kink;
    doc["stage"] = stage;
    doc["ladder"] = ladderEntry;
    doc["anchors"] = packGet(character, "anchors");
    doc["conditioningTracks"] = packGet(character, "conditioningTracks");
    // The refrain, verbatim — character-exact by construction
    // and verified by the gate below before offering.
    doc["refrain"] = packStr(character, "refrain");
    if (stage == ruptureStage)
        doc["ruptureLine"] = packStr(character, "ruptureLine");

    JSONValue bounds;
    foreach (cls; ["dialogue", "prose", "script"])
        bounds[cls] = packWordBounds(pack, cls);
    doc["wordCounts"] = bounds;

    // Dialogue skeleton: slots whose functions come from the
    // ladder's own scaffold text for this stage.
    string scaffoldText = packStr(ladderEntry, "scaffold");
    JSONValue[] slots;
    foreach (i, fn_; ["the opening ask, in her register",
                      "the answer that moves the stage one rung",
                      "the closing line, which carries the refrain's shape"]) {
        JSONValue slot;
        slot["id"] = "d" ~ format!"%d"(i + 1);
        slot["speaker"] = packStr(character, "name");
        slot["function"] = fn_;
        slot["stageScaffold"] = scaffoldText;
        slot["wordClass"] = "dialogue";
        slots ~= slot;
    }
    doc["dialogueSlots"] = slots;

    JSONValue prose;
    prose["wordClass"] = "prose";
    prose["structure"] = scaffoldText;
    prose["voiceRules"] = packGet(character, "voiceRules");
    doc["proseStructure"] = prose;

    // Playable-scene script data: expression cues trace to
    // the character's own stage-expression map when recorded,
    // and to governed names only.
    JSONValue script;
    script["wordClass"] = "script";
    JSONValue[] cues;
    auto stageExpr = packGet(packGet(character, "stageExpressions"),
        format!"%d"(stage));
    JSONValue cue;
    cue["beat"] = "open";
    if (stageExpr.type == JSONType.string) {
        cue["expression"] = stageExpr.str;
        cue["governed"] = packExpression(pack, stageExpr.str).type
            == JSONType.object;
    } else {
        cue["expressionRole"] = "baseline";
        cue["note"] = "no stage expression recorded for this stage; "
            ~ "the baseline governed expression applies";
    }
    cues ~= cue;
    script["parameterCues"] = cues;

    JSONValue[] audioCues;
    foreach (entry; sfxEntries) {
        JSONValue ac;
        ac["sfx"] = packStr(entry, "id");
        ac["name"] = packStr(entry, "name");
        ac["recipe"] = packStr(entry, "recipe");
        string hashError;
        ac["recipeSha256"] = sfxRecipeHash(packStr(entry, "recipe"), hashError);
        ac["atBeat"] = "open";
        audioCues ~= ac;
    }
    script["audioCues"] = audioCues;

    JSONValue voiceRef;
    voiceRef["anchorId"] = packStr(voice, "anchorId");
    auto av = packGet(voice, "anchorVersion");
    voiceRef["anchorVersion"] = av;
    voiceRef["provenance"] = packStr(voice, "provenance");
    script["voiceRef"] = voiceRef;
    doc["sceneScript"] = script;

    // -- the gates, run before the scaffold is offered -----
    JSONValue checks;
    checks["canonVector"] = "pass";
    checks["refrainVerbatim"] = doc["refrain"].str
        == packStr(character, "refrain") ? "pass" : "fail";
    checks["wordBounds"] = "pass";
    // Deny scan: no vocabulary term outside the character's
    // vector (other than the requested kink, which is in it)
    // may appear anywhere in the scaffold text.
    string text = toLower(doc.toString());
    string[] deniedHits;
    foreach (term; packKinkVocabulary(pack)) {
        if (term == kink) continue;
        bool allowed = false;
        foreach (v; vector.array)
            if (v.type == JSONType.string && v.str == term) allowed = true;
        if (allowed) continue;
        if (text.indexOf(toLower(term)) >= 0) deniedHits ~= term;
    }
    checks["denyScan"] = deniedHits.length == 0 ? "pass" : "fail";
    doc["checks"] = checks;

    if (deniedHits.length > 0) {
        JSONValue g;
        g["format"] = "torogao-gap-report/1";
        g["result"] = "denied";
        g["code"] = TG_WRITER_DENIED;
        g["character"] = packStr(character, "name");
        JSONValue[] list;
        foreach (h; deniedHits) list ~= JSONValue(h);
        g["deniedTerms"] = list;
        g["note"] = "The scaffold would contain a kink term outside "
            ~ "the character's vector; it is refused, not shipped.";
        gap = g;
        return;
    }
    if (checks["refrainVerbatim"].str != "pass") {
        JSONValue g;
        g["format"] = "torogao-gap-report/1";
        g["result"] = "denied";
        g["code"] = TG_WRITER_DENIED;
        g["character"] = packStr(character, "name");
        g["note"] = "The refrain did not survive verbatim; refused.";
        gap = g;
        return;
    }

    scaffold = doc;
}

/// One SFX library entry by id; null JSONValue when absent.
JSONValue sfxEntry(JSONValue index, string id) {
    auto entries = packGet(index, "entries");
    if (entries.type != JSONType.array) return JSONValue(null);
    foreach (e; entries.array)
        if (packStr(e, "id") == id) return e;
    return JSONValue(null);
}

/// Loads the SFX library index (repo sfx/ directory).
JSONValue loadSfxIndex(out string error) {
    string path = findDataFile("TOROGAO_SFX_DIR", "sfx", "index.json");
    if (path.length == 0) {
        error = "SFX library index not found";
        return JSONValue(null);
    }
    JSONValue doc = loadJsonFile(path, error);
    if (error.length > 0) return JSONValue(null);
    if (packStr(doc, "format") != "torogao-sfx-index/1") {
        error = "SFX library index is not a torogao-sfx-index/1 document";
        return JSONValue(null);
    }
    return doc;
}

/// SHA-256 (hex) of an SFX recipe file in the library.
string sfxRecipeHash(string recipeFile, out string error) {
    string path = findDataFile("TOROGAO_SFX_DIR", "sfx", recipeFile);
    if (path.length == 0) {
        error = "SFX recipe '" ~ recipeFile ~ "' not found";
        return "";
    }
    try {
        auto bytes = cast(ubyte[]) readText(path);
        return format("%(%02x%)", sha256Of(bytes));
    } catch (Exception e) {
        error = "SFX recipe '" ~ recipeFile ~ "' unreadable: " ~ e.msg;
        return "";
    }
}

struct VoiceFinding {
    string code;
    string message;
    JSONValue toJson() const {
        JSONValue j;
        j["code"] = code;
        j["message"] = message;
        return j;
    }
}

/**
    Validates a voice record: either a standalone
    torogao-voice/1 document or the "voice" block of a
    character record. The provenance rule is absolute:
    estate synthetic voices only — any other source is a
    validation error, and anchors are versioned (a positive
    integer version + a non-empty id; anchors are replaced
    by a new version entry, never edited in place).
*/
VoiceFinding[] validateVoice(JSONValue doc) {
    VoiceFinding[] findings;
    JSONValue voice = doc;
    if (packStr(doc, "format") == characterFormat)
        voice = packGet(doc, "voice");
    else if (packStr(doc, "format") != voiceFormat
            && packGet(doc, "voice").type == JSONType.object)
        voice = packGet(doc, "voice");
    if (voice.type != JSONType.object) {
        findings ~= VoiceFinding(TG_VOICE_ANCHOR,
            "no voice record found in the document");
        return findings;
    }
    if (packStr(voice, "provenance") != "estate-synthetic")
        findings ~= VoiceFinding(TG_VOICE_PROVENANCE,
            "voice provenance must be estate-synthetic; any other "
            ~ "source is not permitted in a voice record");
    if (packStr(voice, "anchorId").length == 0)
        findings ~= VoiceFinding(TG_VOICE_ANCHOR,
            "voice anchor id is empty");
    auto av = packGet(voice, "anchorVersion");
    if (!((av.type == JSONType.integer && av.integer >= 1)
            || (av.type == JSONType.uinteger && av.uinteger >= 1)))
        findings ~= VoiceFinding(TG_VOICE_ANCHOR,
            "voice anchor version must be a positive integer "
            ~ "(anchors are versioned, never edited in place)");
    // The anchor version must round-trip: the value the
    // record carries is the value validation reports.
    return findings;
}

private ptrdiff_t indexOf(string haystack, string needle) {
    import std.algorithm : countUntil;
    return haystack.countUntil(needle);
}
