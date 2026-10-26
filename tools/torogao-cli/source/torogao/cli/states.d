/*
    Torogao extension data — states, variants, ratings, consumer layer

    Copyright © 2026, Nice Thoughts Ranch
    Distributed under the 2-Clause BSD License, see LICENSE file.

    Everything Torogao stores in a project's EXT vendor
    section (see extdata.d) is validated and applied here:

    - meta: the rating stamp source + character record. A
      project carrying Torogao data must carry a complete
      stamp (pack, lane, rating, basis); the rating must be
      one the lane permits under its pack's rules.
    - states: named expression states — a state is a recipe
      (parameter -> value) applied as preview input, never as
      stored puppet mutation.
    - variants: stage variant sets — a recipe plus named
      part visibility/appearance data, switchable in preview
      (recipe half) and carried for scene/export consumers
      (visibility/appearance half).
    - blendshapes: additive keyform deltas as a parameter
      kind, held as extension data (the boundary is stated in
      docs/expressions.md): each names the parameter it folds
      into, and states may target a blend shape by name.
    - consumer: the general consumer layer — named record
      collections that are NOT part of the deterministic
      simulation. v1 ships hitAreas; later collections (e.g.
      motion event tracks) join the same layer by adding a
      key, not a new pocket.

    Glue: the INP1 model has no glue construct (the core
    library contains none), so any glue-class data a foreign
    project carries — a "glue" key anywhere in its payload —
    is counted and reported here, never silently dropped.
*/
module torogao.cli.states;

import inochi2d.core : Puppet;
import torogao.cli.criteria : Finding, jget;
import torogao.cli.extdata : torogaoExtOf;
import torogao.cli.packs;
import torogao.cli.puppetwalk : PuppetWalk;
import std.format : format;
import std.json : JSONValue, JSONType;

/// Stable finding codes for the extension gates.
enum TG_RATING_STAMP_MISSING = "TG-RATING-STAMP-MISSING";
enum TG_RATING_LANE_MISMATCH = "TG-RATING-LANE-MISMATCH";
enum TG_RATING_UNKNOWN       = "TG-RATING-UNKNOWN";
enum TG_STATE_UNKNOWN_PARAM  = "TG-STATE-UNKNOWN-PARAM";
enum TG_VARIANT_UNKNOWN_PARAM = "TG-VARIANT-UNKNOWN-PARAM";
enum TG_VARIANT_UNKNOWN_PART = "TG-VARIANT-UNKNOWN-PART";
enum TG_BLEND_UNKNOWN_PARAM  = "TG-BLEND-UNKNOWN-PARAM";
enum TG_BLEND_UNKNOWN_PART   = "TG-BLEND-UNKNOWN-PART";
enum TG_HITAREA_UNKNOWN_PART = "TG-HITAREA-UNKNOWN-PART";
enum TG_HITAREA_SHAPE        = "TG-HITAREA-SHAPE";
enum TG_EXPR_MISSING         = "TG-EXPR-MISSING";
enum TG_TRACK_MISSING        = "TG-TRACK-MISSING";
enum TG_TRACK_RANGE          = "TG-TRACK-RANGE";
enum TG_GLUE_UNMAPPED        = "TG-GLUE-UNMAPPED";
enum TG_EXT_MALFORMED        = "TG-EXT-MALFORMED";

/// The parsed extension document plus its provenance state.
struct ExtView {
    JSONValue doc;       /// null JSONValue when absent
    bool present;
    bool malformed;
}

/// Reads the extension document off a loaded puppet.
ExtView extOf(Puppet puppet) {
    ExtView v;
    bool malformed;
    v.doc = torogaoExtOf(puppet.extData, malformed);
    v.malformed = malformed;
    v.present = v.doc.type == JSONType.object;
    return v;
}

private bool isNum(JSONValue v) {
    return v.type == JSONType.float_ || v.type == JSONType.integer
        || v.type == JSONType.uinteger;
}

private double numOf(JSONValue v) {
    if (v.type == JSONType.float_) return v.floating;
    if (v.type == JSONType.integer) return cast(double) v.integer;
    return cast(double) v.uinteger;
}

/// Parameter name -> [min, max] (x axis) for a puppet.
double[2][string] paramRanges(Puppet puppet) {
    double[2][string] out_;
    foreach (p; puppet.parameters)
        out_[p.name] = [cast(double) p.min.x, cast(double) p.max.x];
    return out_;
}

/// All node names in a puppet (parts included).
bool[string] nodeNames(PuppetWalk walk) {
    bool[string] out_;
    foreach (w; walk.order) out_[w.node.name] = true;
    return out_;
}

/// Counts "glue" keys anywhere in a JSON document.
size_t countGlueKeys(JSONValue v) {
    size_t n = 0;
    if (v.type == JSONType.object) {
        foreach (k, sub; v.object) {
            if (k == "glue") n++;
            n += countGlueKeys(sub);
        }
    } else if (v.type == JSONType.array) {
        foreach (sub; v.array) n += countGlueKeys(sub);
    }
    return n;
}

/// Blend-shape name -> the parameter it folds into.
string[string] blendParamByName(JSONValue ext) {
    string[string] out_;
    auto arr = jget(ext, "blendshapes");
    if (arr.type != JSONType.array) return out_;
    foreach (b; arr.array) {
        auto name = jget(b, "name");
        auto param = jget(b, "parameter");
        if (name.type == JSONType.string && param.type == JSONType.string)
            out_[name.str] = param.str;
    }
    return out_;
}

/**
    Resolves a state/variant recipe into parameter -> value:
    a key is either a parameter name or a blend-shape name
    (which folds into its parameter). Unknown keys and
    non-numeric values collect into `problems`.
*/
double[string] resolveRecipe(JSONValue ext, JSONValue recipe,
                             double[2][string] ranges,
                             out string[] problems) {
    double[string] out_;
    if (recipe.type != JSONType.object) {
        problems ~= "recipe is not an object";
        return out_;
    }
    auto blends = blendParamByName(ext);
    foreach (k, v; recipe.object) {
        string param = k;
        if (auto bp = k in blends) param = *bp;
        if ((param in ranges) is null) {
            problems ~= "recipe key '" ~ k ~ "' is neither a parameter nor a blend shape";
            continue;
        }
        if (!isNum(v)) {
            problems ~= "recipe value for '" ~ k ~ "' is not a number";
            continue;
        }
        out_[param] = numOf(v);
    }
    return out_;
}

/// The rating stamp an export carries for this extension:
/// the meta fields verbatim, or the explicit unrated stamp
/// for projects without Torogao metadata.
JSONValue ratingStamp(JSONValue ext) {
    JSONValue stamp;
    auto meta = jget(ext, "meta");
    if (meta.type != JSONType.object) {
        stamp["rating"] = "unrated";
        stamp["lane"] = "none";
        stamp["pack"] = "";
        stamp["basis"] = "no torogao rating metadata in the project";
        stamp["stamped"] = true;
        return stamp;
    }
    stamp["rating"] = jget(meta, "rating");
    stamp["lane"] = jget(meta, "lane");
    stamp["pack"] = jget(meta, "pack");
    stamp["basis"] = jget(meta, "basis");
    stamp["stamped"] = true;
    return stamp;
}

/// Is this extension's rating internally inconsistent in a
/// way that must refuse an export? (Rating the lane forbids,
/// per the meta pack's rules; unknown pack/rating counts.)
bool ratingBlocksExport(JSONValue ext, out string reason) {
    auto meta = jget(ext, "meta");
    if (meta.type != JSONType.object) return false;
    string packName = packStr(meta, "pack");
    string lane = packStr(meta, "lane");
    string rating = packStr(meta, "rating");
    if (packName.length == 0 || lane.length == 0 || rating.length == 0)
        return false; // incomplete stamps are validate's finding
    string err;
    auto pack = loadPack(packName, err);
    if (err.length > 0) {
        reason = "rating meta names pack '" ~ packName
            ~ "', which is not a shipped lane pack";
        return true;
    }
    if (!packRatingKnown(pack, rating)) {
        reason = "rating '" ~ rating ~ "' is not a rating value of pack '"
            ~ packName ~ "'";
        return true;
    }
    if (!packRatingAllowed(pack, lane, rating)) {
        reason = "rating '" ~ rating ~ "' is not permitted in lane '"
            ~ lane ~ "' by pack '" ~ packName ~ "'";
        return true;
    }
    return false;
}

/**
    The full extension validation: rating stamp, states,
    variants, blend shapes, hit areas, glue visibility.
    Runs against the loaded puppet; appends to the caller's
    findings (the CLI's validate gate folds these in).
*/
Finding[] validateExt(Puppet puppet, PuppetWalk walk, JSONValue payload) {
    Finding[] findings;
    auto view = extOf(puppet);

    // Glue visibility applies to every project, with or
    // without Torogao data: never a silent drop.
    size_t glue = countGlueKeys(payload);
    if (view.present) glue += countGlueKeys(jget(view.doc, "foreign"));
    if (glue > 0) {
        findings ~= Finding(TG_GLUE_UNMAPPED, "warning", "payload",
            format!"project carries %d glue record(s); the INP1 model has no glue construct, so the coupling is reported here and not consumed — it is never silently dropped"
                (glue));
    }

    if (!view.present) {
        if (view.malformed) {
            findings ~= Finding(TG_EXT_MALFORMED, "error", "ext",
                "the torogao EXT section is not parseable JSON");
        }
        return findings;
    }
    auto ext = view.doc;
    auto ranges = paramRanges(puppet);
    auto nodes = nodeNames(walk);

    // -- rating stamp ----------------------------------------
    auto meta = jget(ext, "meta");
    if (meta.type != JSONType.object) {
        findings ~= Finding(TG_RATING_STAMP_MISSING, "error", "meta",
            "project carries torogao data but no rating meta; a stamped project must carry pack, lane, rating, and basis");
    } else {
        foreach (field; ["pack", "lane", "rating", "basis"]) {
            auto fv = jget(meta, field);
            if (fv.type != JSONType.string || fv.str.length == 0) {
                findings ~= Finding(TG_RATING_STAMP_MISSING, "error",
                    "meta." ~ field,
                    "rating stamp field '" ~ field ~ "' is missing or empty");
            }
        }
        string reason;
        if (ratingBlocksExport(ext, reason)) {
            // Lane mismatch and unknown values share one
            // refusal path; the code names the class.
            string code = TG_RATING_UNKNOWN;
            if (reason.indexOf("not permitted in lane") >= 0)
                code = TG_RATING_LANE_MISMATCH;
            findings ~= Finding(code, "error", "meta", reason);
        }
    }

    // -- states ----------------------------------------------
    auto states = jget(ext, "states");
    if (states.type == JSONType.array) {
        foreach (s; states.array) {
            string sname = packStr(s, "name");
            string[] problems;
            resolveRecipe(ext, jget(s, "recipe"), ranges, problems);
            foreach (p; problems)
                findings ~= Finding(TG_STATE_UNKNOWN_PARAM, "error",
                    sname.length ? sname : "<unnamed state>", p);
        }
    }

    // -- variants ----------------------------------------------
    auto variants = jget(ext, "variants");
    if (variants.type == JSONType.array) {
        foreach (v; variants.array) {
            string vid = packStr(v, "id");
            string subject = vid.length ? vid : "<unnamed variant>";
            string[] problems;
            resolveRecipe(ext, jget(v, "recipe"), ranges, problems);
            foreach (p; problems)
                findings ~= Finding(TG_VARIANT_UNKNOWN_PARAM, "error",
                    subject, p);
            auto vis = jget(v, "visibility");
            if (vis.type == JSONType.object)
                foreach (part, _; vis.object)
                    if ((part in nodes) is null)
                        findings ~= Finding(TG_VARIANT_UNKNOWN_PART,
                            "error", subject,
                            "variant visibility names part '" ~ part
                            ~ "', which is not a node in the project");
            auto app = jget(v, "appearance");
            if (app.type == JSONType.object)
                foreach (part, _; app.object)
                    if ((part in nodes) is null)
                        findings ~= Finding(TG_VARIANT_UNKNOWN_PART,
                            "error", subject,
                            "variant appearance names part '" ~ part
                            ~ "', which is not a node in the project");
        }
    }

    // -- blend shapes ------------------------------------------
    auto blends = jget(ext, "blendshapes");
    if (blends.type == JSONType.array) {
        foreach (b; blends.array) {
            string bname = packStr(b, "name");
            string subject = bname.length ? bname : "<unnamed blend shape>";
            string param = packStr(b, "parameter");
            if ((param in ranges) is null)
                findings ~= Finding(TG_BLEND_UNKNOWN_PARAM, "error",
                    subject, "blend shape folds into parameter '" ~ param
                    ~ "', which the project does not have");
            auto deltas = jget(b, "deltas");
            if (deltas.type == JSONType.array)
                foreach (d; deltas.array) {
                    string part = packStr(d, "part");
                    if (part.length > 0 && (part in nodes) is null)
                        findings ~= Finding(TG_BLEND_UNKNOWN_PART,
                            "error", subject,
                            "blend shape delta names part '" ~ part
                            ~ "', which is not a node in the project");
                }
        }
    }

    // -- consumer layer: hit areas ------------------------------
    findings ~= validateHitAreasOnly(ext, puppet, walk);

    return findings;
}

/**
    Validates only the consumer layer's hit areas of an
    extension document against a puppet — the authoring
    path runs this BEFORE writing, so a bad spec never
    reaches a file.
*/
Finding[] validateHitAreasOnly(JSONValue ext, Puppet puppet,
                               PuppetWalk walk) {
    Finding[] findings;
    auto nodes = nodeNames(walk);
    auto hitAreas = jget(jget(ext, "consumer"), "hitAreas");
    if (hitAreas.type != JSONType.array) return findings;
    foreach (h; hitAreas.array) {
        string hname = packStr(h, "name");
        string subject = hname.length ? hname : "<unnamed hit area>";
        string part = packStr(jget(h, "binding"), "part");
        if (part.length == 0 || (part in nodes) is null)
            findings ~= Finding(TG_HITAREA_UNKNOWN_PART, "error",
                subject, "hit area binds part '" ~ part
                ~ "', which is not a node in the project");
        auto shape = jget(h, "shape");
        string kind = packStr(shape, "kind");
        bool shapeOk = false;
        if (kind == "rect")
            shapeOk = isNum(jget(shape, "x")) && isNum(jget(shape, "y"))
                && isNum(jget(shape, "w")) && isNum(jget(shape, "h"));
        else if (kind == "circle")
            shapeOk = isNum(jget(shape, "x")) && isNum(jget(shape, "y"))
                && isNum(jget(shape, "r"));
        if (!shapeOk)
            findings ~= Finding(TG_HITAREA_SHAPE, "error", subject,
                "hit area shape must be a rect {x,y,w,h} or a "
                ~ "circle {x,y,r} with numeric fields");
    }
    return findings;
}

/// A compact summary of the extension for inspect/list tools.
JSONValue extSummary(JSONValue ext) {
    JSONValue doc;
    if (ext.type != JSONType.object) {
        doc["present"] = false;
        return doc;
    }
    doc["present"] = true;
    doc["format"] = jget(ext, "format");
    doc["meta"] = jget(ext, "meta");
    string[] stateNames;
    auto states = jget(ext, "states");
    if (states.type == JSONType.array)
        foreach (s; states.array) stateNames ~= packStr(s, "name");
    doc["states"] = stateNames;
    string[] variantIds;
    auto variants = jget(ext, "variants");
    if (variants.type == JSONType.array)
        foreach (v; variants.array) variantIds ~= packStr(v, "id");
    doc["variants"] = variantIds;
    string[] blendNames;
    auto blends = jget(ext, "blendshapes");
    if (blends.type == JSONType.array)
        foreach (b; blends.array) blendNames ~= packStr(b, "name");
    doc["blendshapes"] = blendNames;
    string[] hitNames;
    auto hitAreas = jget(jget(ext, "consumer"), "hitAreas");
    if (hitAreas.type == JSONType.array)
        foreach (h; hitAreas.array) hitNames ~= packStr(h, "name");
    doc["hitAreas"] = hitNames;
    doc["trackingClaims"] = jget(ext, "trackingClaims");
    auto character = jget(ext, "character");
    if (character.type == JSONType.object)
        doc["character"] = packStr(character, "name");
    return doc;
}

/// Finds one state by name; null JSONValue when absent.
JSONValue findState(JSONValue ext, string name) {
    auto states = jget(ext, "states");
    if (states.type != JSONType.array) return JSONValue(null);
    foreach (s; states.array)
        if (packStr(s, "name") == name) return s;
    return JSONValue(null);
}

/// Finds one variant by id; null JSONValue when absent.
JSONValue findVariant(JSONValue ext, string id) {
    auto variants = jget(ext, "variants");
    if (variants.type != JSONType.array) return JSONValue(null);
    foreach (v; variants.array)
        if (packStr(v, "id") == id) return v;
    return JSONValue(null);
}

/**
    Expression coverage: the governed names of `pack` that
    the project's states do NOT implement.
*/
string[] expressionMissing(JSONValue ext, JSONValue pack) {
    bool[string] have;
    auto states = jget(ext, "states");
    if (states.type == JSONType.array)
        foreach (s; states.array) have[packStr(s, "name")] = true;
    string[] missing;
    foreach (e; packExpressions(pack)) {
        string name = packStr(e, "name");
        if ((name in have) is null) missing ~= name;
    }
    return missing;
}

/**
    Tracking coverage for one set of a pack against a
    puppet: returns (missing ids, range violations).
*/
void trackingCoverage(Puppet puppet, JSONValue set,
                      out string[] missing, out string[] rangeBad) {
    double[2][string] ranges = paramRanges(puppet);
    auto ids = jget(set, "ids");
    if (ids.type != JSONType.array) return;
    foreach (entry; ids.array) {
        string id = packStr(entry, "id");
        auto r = id in ranges;
        if (r is null) {
            missing ~= id;
            continue;
        }
        double setMin = numOf(jget(entry, "min"));
        double setMax = numOf(jget(entry, "max"));
        if ((*r)[0] > setMin + 1e-9 || (*r)[1] < setMax - 1e-9)
            rangeBad ~= format!"%s (rig %g..%g, set %g..%g)"
                (id, (*r)[0], (*r)[1], setMin, setMax);
    }
}

private ptrdiff_t indexOf(string haystack, string needle) {
    import std.algorithm : countUntil;
    return haystack.countUntil(needle);
}
