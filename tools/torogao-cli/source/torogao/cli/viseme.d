/*
    Torogao viseme contract: coverage check and curve baker

    Copyright © 2026, Nice Thoughts Ranch
    Distributed under the 2-Clause BSD License, see LICENSE file.

    The contract (contracts/visemes.json, format
    torogao-visemes/1) names the standard 15-viseme set and
    maps every viseme to target values over three mouth
    parameters (ParamMouthOpenY, ParamMouthForm, ParamMouthX)
    that every puppet in the lane implements.

    viseme-check is a project gate: a puppet missing a
    contract mouth parameter cannot answer voice at all
    (TG-VISEME-PARAM-MISSING, error); one that has the
    parameter but leaves it unbound and undriven cannot move
    it (TG-VISEME-PARAM-UNBOUND, warning).

    viseme-bake turns a viseme track (a timed event list,
    e.g. from a voice-analysis pass) into a torogao-curves/1
    document over the three mouth parameters. The
    interpolation rule is fully specified in docs/visemes.md
    — hold semantics with a fixed linear blend window after
    each event start — so a baked track is byte-stable and
    means the same thing in preview and in export.
*/
module torogao.cli.viseme;

import std.file : readText;
import std.json : JSONType, JSONValue, parseJSON;

import inochi2d.core : Puppet;
import torogao.cli.criteria : Finding;
import torogao.cli.datadirs : findDataFile;
import torogao.curves : CurveSeries, CurveSet;

enum TG_VISEME_PARAM_MISSING = "TG-VISEME-PARAM-MISSING";
enum TG_VISEME_PARAM_UNBOUND = "TG-VISEME-PARAM-UNBOUND";

/// A loaded viseme contract.
struct VisemeContract {
    string setName;
    string[] parameters;             /// ordered contract parameters
    double[string][string] targets;  /// targets[viseme][parameter]
    string path;

    bool hasViseme(string name) const {
        return (name in targets) !is null;
    }
}

/// Load the contract. The path may be given explicitly;
/// otherwise the contracts search directories are used.
VisemeContract loadVisemeContract(string explicitPath, out string error) {
    string path = explicitPath;
    if (path.length == 0)
        path = findDataFile("TOROGAO_CONTRACTS_DIR", "contracts",
            "visemes.json");
    if (path.length == 0) {
        error = "viseme contract not found (contracts/visemes.json)";
        return VisemeContract.init;
    }
    JSONValue doc;
    try doc = parseJSON(readText(path));
    catch (Exception e) {
        error = "viseme contract unreadable: " ~ e.msg;
        return VisemeContract.init;
    }
    if (doc.getStr("format") != "torogao-visemes/1") {
        error = "viseme contract format must be \"torogao-visemes/1\"";
        return VisemeContract.init;
    }
    VisemeContract c;
    c.path = path;
    c.setName = doc.getStr("set");
    foreach (p; doc["parameters"].array)
        c.parameters ~= p.str;
    foreach (name, viseme; doc["visemes"].object) {
        double[string] t;
        foreach (paramName, value; viseme.object) {
            if (value.type == JSONType.integer)
                t[paramName] = cast(double) value.integer;
            else if (value.type == JSONType.uinteger)
                t[paramName] = cast(double) value.uinteger;
            else
                t[paramName] = value.floating;
        }
        c.targets[name] = t;
    }
    if (c.parameters.length == 0 || c.targets.length == 0) {
        error = "viseme contract is empty";
        return VisemeContract.init;
    }
    return c;
}

/// Run the coverage check over a loaded puppet.
Finding[] checkVisemes(Puppet puppet, in VisemeContract contract_) {
    Finding[] findings;
    auto driven = puppet.getParameterDrivers();
    foreach (paramName; contract_.parameters) {
        bool found = false, bound = false, isDriven = false;
        foreach (p; puppet.parameters) {
            if (p.name == paramName) {
                found = true;
                bound = p.bindings.length > 0;
                isDriven = (p in driven) !is null;
            }
        }
        if (!found) {
            findings ~= Finding(TG_VISEME_PARAM_MISSING, "error",
                paramName,
                "contract mouth parameter '" ~ paramName ~
                "' is absent; the puppet cannot answer the " ~
                "viseme contract");
        } else if (!bound && !isDriven) {
            findings ~= Finding(TG_VISEME_PARAM_UNBOUND, "warning",
                paramName,
                "contract mouth parameter '" ~ paramName ~
                "' has no bindings and no driver; viseme " ~
                "curves would move nothing");
        }
    }
    return findings;
}

/// Names of the contract parameters the puppet implements.
string[] coveredParameters(Puppet puppet, in VisemeContract contract_) {
    string[] covered;
    foreach (paramName; contract_.parameters)
        foreach (p; puppet.parameters)
            if (p.name == paramName) covered ~= paramName;
    return covered;
}

/// Bake a viseme track to mouth-parameter curves.
/// Track format torogao-viseme-track/1:
///   {format, fps (default 60), durationSeconds (required),
///    blendSeconds (default 0.06),
///    events: [{viseme, start, end}]}
/// Times are seconds. See docs/visemes.md for the exact
/// hold/blend rule. Returns false with error on bad input.
bool bakeVisemes(JSONValue track, in VisemeContract contract_,
                 out CurveSet result, out string error) {
    if (track.type != JSONType.object ||
        track.getStr("format") != "torogao-viseme-track/1") {
        error = "track format must be \"torogao-viseme-track/1\"";
        return false;
    }
    int fps = 60;
    if (auto f = "fps" in track) {
        if (f.type == JSONType.integer) fps = cast(int) f.integer;
        else if (f.type == JSONType.float_) fps = cast(int) f.floating;
    }
    if (fps != 15 && fps != 30 && fps != 60 && fps != 120) {
        error = "track fps must be one of 15, 30, 60, 120";
        return false;
    }
    double duration = track.getD("durationSeconds", 0);
    if (!(duration > 0)) {
        error = "track durationSeconds (positive) is required";
        return false;
    }
    double blend = track.getD("blendSeconds", 0.06);
    if (blend < 0) {
        error = "blendSeconds must be >= 0";
        return false;
    }
    struct Event { string viseme; double start; }
    Event[] events;
    if (auto ev = "events" in track) {
        if (ev.type != JSONType.array) {
            error = "track events must be an array";
            return false;
        }
        foreach (e; ev.array) {
            string name = e.getStr("viseme");
            if (!contract_.hasViseme(name)) {
                error = "unknown viseme '" ~ name ~
                    "' (not in the contract set)";
                return false;
            }
            events ~= Event(name, e.getD("start", 0));
        }
    }
    // Events apply in start order; later starts win ties by
    // track order (stable sort on start).
    import std.algorithm : sort;
    sort!((a, b) => a.start < b.start)(events);

    size_t steps = cast(size_t)(duration * fps + 0.5);
    double dt = 1.0 / fps;
    auto silTargets = contract_.targets["sil"];
    // The held targets (what the curve is heading to / sitting
    // at), the snapshot a blend started from, and the blend
    // window. A blend snapshots the EVALUATED value at the
    // event's start, so chained events and interrupted blends
    // stay continuous.
    double[string] held, blendFrom;
    foreach (paramName; contract_.parameters) {
        held[paramName] = silTargets[paramName];
        blendFrom[paramName] = silTargets[paramName];
    }
    double blendStart = 0;
    bool blending = false;

    double valueAt(string paramName, double t) {
        double target = held[paramName];
        if (!blending || blend == 0) return target;
        double k = (t - blendStart) / blend;
        if (k >= 1.0) return target;
        if (k <= 0.0) return blendFrom[paramName];
        return blendFrom[paramName] +
            (target - blendFrom[paramName]) * k;
    }

    auto seriesValues = new double[][contract_.parameters.length];
    foreach (i; 0 .. contract_.parameters.length) {
        seriesValues[i] = new double[](steps);
        seriesValues[i][] = 0.0;
    }

    size_t nextEvent = 0;
    foreach (step; 0 .. steps) {
        double t = step * dt;
        while (nextEvent < events.length &&
               events[nextEvent].start <= t) {
            auto ev = events[nextEvent];
            foreach (paramName; contract_.parameters)
                blendFrom[paramName] = valueAt(paramName, ev.start);
            foreach (paramName; contract_.parameters)
                held[paramName] = contract_.targets[ev.viseme][paramName];
            blendStart = ev.start;
            blending = blend > 0;
            nextEvent++;
        }
        foreach (i, paramName; contract_.parameters)
            seriesValues[i][step] = valueAt(paramName, t);
    }

    CurveSeries[] series;
    foreach (i, paramName; contract_.parameters)
        series ~= CurveSeries(paramName, 0, seriesValues[i]);
    result.source = "viseme-bake";
    result.dt = dt;
    result.series = series;
    result.stepCount = cast(long) steps;
    result.extra = [["contract", JSONValue(contract_.setName).toString()]];
    return true;
}

private string getStr(JSONValue v, string key) {
    if (v.type != JSONType.object) return "";
    auto p = key in v;
    if (p is null || p.type != JSONType.string) return "";
    return p.str;
}

private double getD(JSONValue v, string key, double fallback) {
    if (v.type != JSONType.object) return fallback;
    auto p = key in v;
    if (p is null) return fallback;
    if (p.type == JSONType.integer) return cast(double) p.integer;
    if (p.type == JSONType.uinteger) return cast(double) p.uinteger;
    if (p.type == JSONType.float_) return p.floating;
    return fallback;
}
