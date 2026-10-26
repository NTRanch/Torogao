/*
    Torogao ambient drivers — the named contract (P3 half)

    Copyright © 2026, Nice Thoughts Ranch
    Distributed under the 2-Clause BSD License, see LICENSE file.

    Breath, automatic blinking, gaze saccades, and idle
    micro-sway are a named, data-defined driver set. Each
    driver is a deterministic function of the step index,
    the step rate, and a recorded seed — no wall clock, no
    ambient randomness — so the same (project, driver set,
    seed, steps) always produces the same input values, in
    preview and in export alike (both run this code over the
    P2 solver path).

    Precedence (specified, not emergent):
        input track  >  state / variant recipe  >  ambient
        drivers  >  parameter defaults
    A driver never overrides an authored value: the preview
    merges driver output only into parameters that neither
    the track nor a state/variant recipe drives. Live
    tracking (a later phase) overrides drivers while active;
    nothing in this contract precludes that — a tracked take
    is simply another authored layer above this one.

    Driver definitions (all periods in seconds, converted to
    steps at the run's rate):
    - breath:  ParamBreath follows a 4 s sine between its
               range's 15% and 85%.
    - blink:   ParamEyeLOpen / ParamEyeROpen close to their
               range minimum for 3 steps every 4 s.
    - saccade: ParamEyeBallX / ParamEyeBallY hold a target,
               then jump to a new seeded target every
               1–3 s (seeded splitmix64 schedule); a puppet
               without eyeball parameters is simply not
               driven on this channel.
    - sway:    ParamAngleZ follows a 7 s sine at ±8% of its
               range around its default.
*/
module torogao.cli.drivers;

import inochi2d.core : Puppet;
import std.json : JSONValue, JSONType;
import std.math : sin, PI;

/// Driver set configuration (data: a JSON object with a
/// "drivers" name array and an integer "seed").
struct DriverConfig {
    bool breath = true;
    bool blink = true;
    bool saccade = true;
    bool sway = true;
    ulong seed = 1;
}

/// Parses a driver-set document; on failure error is set.
bool parseDriverConfig(JSONValue doc, out DriverConfig cfg, out string error) {
    cfg = DriverConfig.init;
    if (doc.type != JSONType.object) {
        error = "driver set must be a JSON object";
        return false;
    }
    if (auto sv = "seed" in doc) {
        if (sv.type == JSONType.integer) cfg.seed = cast(ulong) sv.integer;
        else if (sv.type == JSONType.uinteger) cfg.seed = sv.uinteger;
        else {
            error = "driver set 'seed' must be an integer";
            return false;
        }
    }
    if (auto dv = "drivers" in doc) {
        if (dv.type != JSONType.array) {
            error = "driver set 'drivers' must be an array of names";
            return false;
        }
        cfg.breath = cfg.blink = cfg.saccade = cfg.sway = false;
        foreach (v; dv.array) {
            if (v.type != JSONType.string) {
                error = "driver names must be strings";
                return false;
            }
            switch (v.str) {
                case "breath": cfg.breath = true; break;
                case "blink": cfg.blink = true; break;
                case "saccade": cfg.saccade = true; break;
                case "sway": cfg.sway = true; break;
                default:
                    error = "unknown ambient driver '" ~ v.str
                        ~ "' (known: breath, blink, saccade, sway)";
                    return false;
            }
        }
    }
    return true;
}

private ulong splitmix64(ref ulong state) {
    state += 0x9E3779B97F4A7C15UL;
    ulong z = state;
    z = (z ^ (z >> 30)) * 0xBF58476D1CE4E5B9UL;
    z = (z ^ (z >> 27)) * 0x94D049BB133111EBUL;
    return z ^ (z >> 31);
}

/**
    Computes driver input values for `steps` steps at `fps`:
    a map of parameter name -> per-step values, covering only
    parameters the puppet actually has. Parameters driven by
    physics drivers are excluded by the caller (the preview
    rejects track inputs for driven parameters); this
    function only shapes values.
*/
double[][string] driverInputs(Puppet puppet, DriverConfig cfg,
                              int steps, int fps) {
    double[][string] out_;

    double[2][string] ranges;
    double[string] defaults;
    foreach (p; puppet.parameters) {
        ranges[p.name] = [cast(double) p.min.x, cast(double) p.max.x];
        defaults[p.name] = cast(double) p.defaults.x;
    }

    double[] series(string name, double delegate(int) fn) {
        auto values = new double[](steps);
        foreach (i; 0 .. steps) values[i] = fn(i);
        return values;
    }

    if (cfg.breath && ("ParamBreath" in ranges)) {
        auto r = ranges["ParamBreath"];
        double lo = r[0] + 0.15 * (r[1] - r[0]);
        double hi = r[0] + 0.85 * (r[1] - r[0]);
        out_["ParamBreath"] = series("ParamBreath", (int i) {
            double phase = (cast(double) i / fps) / 4.0;
            double s = 0.5 + 0.5 * sin(2.0 * PI * phase);
            return lo + s * (hi - lo);
        });
    }

    if (cfg.blink) {
        int period = 4 * fps;
        int closed = 3;
        foreach (eye; ["ParamEyeLOpen", "ParamEyeROpen"]) {
            if ((eye in ranges) is null) continue;
            auto r = ranges[eye];
            double open = r[1];
            double shut = r[0];
            out_[eye] = series(eye, (int i) {
                return (i % period) < closed ? shut : open;
            });
        }
    }

    if (cfg.saccade) {
        foreach (axis; ["ParamEyeBallX", "ParamEyeBallY"]) {
            if ((axis in ranges) is null) continue;
            auto r = ranges[axis];
            ulong state = cfg.seed ^ (axis == "ParamEyeBallX"
                ? 0xA24BAED4963EE407UL : 0x9FB21C651E98DF25UL);
            double target = defaults[axis];
            int nextJump = 0;
            auto values = new double[](steps);
            foreach (i; 0 .. steps) {
                if (i >= nextJump) {
                    ulong u = splitmix64(state);
                    double frac = cast(double)(u >> 11) / cast(double)(1UL << 53);
                    target = r[0] + frac * (r[1] - r[0]);
                    ulong w = splitmix64(state);
                    int waitSeconds = 1 + cast(int)(w % 3);
                    nextJump = i + waitSeconds * fps;
                }
                values[i] = target;
            }
            out_[axis] = values;
        }
    }

    if (cfg.sway && ("ParamAngleZ" in ranges)) {
        auto r = ranges["ParamAngleZ"];
        double center = defaults["ParamAngleZ"];
        double amp = 0.08 * (r[1] - r[0]);
        out_["ParamAngleZ"] = series("ParamAngleZ", (int i) {
            double phase = (cast(double) i / fps) / 7.0;
            return center + amp * sin(2.0 * PI * phase);
        });
    }

    return out_;
}

/// The driver names active in a configuration (for reports).
string[] activeDriverNames(DriverConfig cfg) {
    string[] names;
    if (cfg.breath) names ~= "breath";
    if (cfg.blink) names ~= "blink";
    if (cfg.saccade) names ~= "saccade";
    if (cfg.sway) names ~= "sway";
    return names;
}
