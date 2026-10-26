/*
    Torogao physics preview — deterministic fixed-step solver

    Copyright © 2026, Nice Thoughts Ranch
    Distributed under the 2-Clause BSD License, see LICENSE file.

    Drives a loaded puppet's physics with a scripted clock at
    a fixed step rate and records every parameter's value
    after each step as a canonical curve document
    (torogao.curves). The run is a pure function of (puppet,
    input track, step rate): the library's timing function is
    replaced by a scripted clock (inSetTimingFunc), drivers
    run in the library's fixed-step mode (carried patch D15:
    whole 0.01 s physics steps, remainder carried), and no
    wall clock or unseeded randomness is consulted anywhere
    on the path. Automations, when a puppet carries them,
    read the same scripted clock.

    Step rate: 60 steps/second by default — the reference
    rate of the physics model this tool mirrors (the
    documented Calculate FPS default, and the rate shipped
    sample rigs export). The accepted set is {15, 30, 60,
    120}; anything else is rejected, because the underlying
    model's results are defined relative to its step rate.

    Input track format (torogao-input-track/1):
        {
          "format": "torogao-input-track/1",
          "fps": 60,                  // optional, default 60
          "steps": 240,
          "inputs": {
            "ParamName": [0.0, 0.1, ...],        // scalar per step
            "VecParam":  [[0.0, 0.0], [0.1, 0.0]] // vec2 per step
          }
        }
    Every input array must hold exactly `steps` entries.
    Driving an unknown parameter, or one a driver (physics)
    already drives, is an error: the track is the only
    source of input motion, by construction.
*/
module torogao.cli.physicspreview;

import inochi2d : Puppet, Parameter, deltaTime, inSetTimingFunc,
    inUpdate, lastTime;
import inochi2d.core.nodes.drivers.simplephysics : SimplePhysics;
import inochi2d.math : vec2;
import torogao.cli.drivers;
import torogao.cli.states;
import torogao.curves;
import std.json : JSONValue, JSONType;
import std.format : format;

/// Input track format identifier.
enum inputTrackFormat = "torogao-input-track/1";

/// Accepted step rates (the source model's Calculate FPS set).
enum acceptedFps = [15, 30, 60, 120];

private double simNow = 0;

private double previewClock() {
    return simNow;
}

private bool isAcceptedFps(int fps) {
    foreach (f; acceptedFps)
        if (f == fps) return true;
    return false;
}

private bool jsonInt(JSONValue v, out int value) {
    if (v.type == JSONType.integer) { value = cast(int) v.integer; return true; }
    if (v.type == JSONType.uinteger) { value = cast(int) v.uinteger; return true; }
    return false;
}

private bool jsonNum(JSONValue v, out double value) {
    if (v.type == JSONType.float_) { value = v.floating; return true; }
    if (v.type == JSONType.integer) { value = cast(double) v.integer; return true; }
    if (v.type == JSONType.uinteger) { value = cast(double) v.uinteger; return true; }
    return false;
}

/**
    Optional preview composition layers (P3): an expression
    state and/or a stage variant stored in the project's
    vendor-extension section, applied as constant input
    tracks, and the ambient driver set (Addition O), layered
    UNDER authored inputs and recipes. Precedence: input
    track > state/variant recipe > ambient drivers >
    parameter defaults. A state and a variant driving the
    same parameter is an error (two authored layers may not
    silently compose); a driver never overrides an authored
    value — it fills only parameters nothing else drives.
*/
struct PreviewOptions {
    JSONValue ext;        /// project extension doc (may be null)
    string stateName;     /// state to apply ("" = none)
    string variantId;     /// variant to apply ("" = none)
    bool hasDrivers;      /// compose the ambient driver set
    DriverConfig drivers; /// the driver set (seed is data)
}

/// The original P2 entry point: a bare track, no layers.
CurveSet runPhysicsPreview(Puppet puppet, JSONValue track,
                           int fpsOverride, string projectLabel,
                           out string error) {
    PreviewOptions none;
    return runPhysicsPreview(puppet, track, fpsOverride, projectLabel,
        none, error);
}

/**
    Runs the preview. On success `error` is empty and the
    curve set holds one series per parameter axis (axis 1
    only for vec2 parameters), `steps` values each.
*/
CurveSet runPhysicsPreview(Puppet puppet, JSONValue track,
                           int fpsOverride, string projectLabel,
                           PreviewOptions options,
                           out string error) {
    CurveSet result;
    error = null;

    if (track.type != JSONType.object) {
        error = "input track must be a JSON object";
        return result;
    }
    if (auto f = "format" in track) {
        if (f.type != JSONType.string || f.str != inputTrackFormat) {
            error = "input track format must be \"" ~ inputTrackFormat ~ "\"";
            return result;
        }
    }

    int fps = 60;
    if (auto fv = "fps" in track) {
        if (!jsonInt(*fv, fps)) {
            error = "track field 'fps' must be an integer";
            return result;
        }
    }
    if (fpsOverride > 0) fps = fpsOverride;
    if (!isAcceptedFps(fps)) {
        error = format("step rate %d is not in the accepted set {15, 30, 60, 120}", fps);
        return result;
    }

    int steps = 0;
    if (auto sv = "steps" in track) {
        if (!jsonInt(*sv, steps) || steps <= 0 || steps > 1_000_000) {
            error = "track field 'steps' must be an integer in 1..1000000";
            return result;
        }
    } else {
        error = "track field 'steps' is required";
        return result;
    }

    // Resolve the track's inputs onto puppet parameters.
    struct Input {
        Parameter param;
        vec2[] values;
    }
    Input[] inputs;
    if (auto iv = "inputs" in track) {
        if (iv.type != JSONType.object) {
            error = "track field 'inputs' must be an object";
            return result;
        }
        foreach (name, arr; iv.object) {
            Parameter param;
            foreach (p; puppet.parameters)
                if (p.name == name) { param = p; break; }
            if (param is null) {
                error = "track drives unknown parameter '" ~ name ~ "'";
                return result;
            }
            if ((param in puppet.getParameterDrivers()) !is null) {
                error = "track input '" ~ name
                    ~ "' is driven by a driver and cannot be a preview input";
                return result;
            }
            if (arr.type != JSONType.array || arr.array.length != steps) {
                error = format("track input '%s' must hold exactly %d step values",
                    name, steps);
                return result;
            }
            Input input;
            input.param = param;
            foreach (el; arr.array) {
                double x, y;
                if (jsonNum(el, x)) {
                    y = param.defaults.y;
                } else if (el.type == JSONType.array && el.array.length == 2
                        && jsonNum(el.array[0], x) && jsonNum(el.array[1], y)) {
                    // vec2 step value
                } else {
                    error = "track input '" ~ name
                        ~ "' entries must be numbers or [x, y] pairs";
                    return result;
                }
                input.values ~= vec2(cast(float) x, cast(float) y);
            }
            inputs ~= input;
        }
    }

    // -- P3 composition layers --------------------------------
    // Authored parameters so far (the track). Recipes and
    // drivers compose under the documented precedence.
    bool[string] authored;
    foreach (input; inputs) authored[input.param.name] = true;

    Parameter findParam(string name) {
        foreach (p; puppet.parameters)
            if (p.name == name) return p;
        return null;
    }

    bool appendConstant(string name, double value, string layer) {
        auto param = findParam(name);
        if (param is null) {
            error = layer ~ " drives unknown parameter '" ~ name ~ "'";
            return false;
        }
        if ((param in puppet.getParameterDrivers()) !is null) {
            error = layer ~ " input '" ~ name
                ~ "' is driven by a driver and cannot be a preview input";
            return false;
        }
        Input input;
        input.param = param;
        foreach (i; 0 .. steps)
            input.values ~= vec2(cast(float) value,
                cast(float) param.defaults.y);
        inputs ~= input;
        authored[name] = true;
        return true;
    }

    if (options.stateName.length > 0 || options.variantId.length > 0) {
        if (options.ext.type != JSONType.object) {
            error = "the project carries no torogao extension data; "
                ~ "it has no states or variants to apply";
            return result;
        }
        auto ranges = paramRanges(puppet);
        double[string] recipeValues;
        string[string] recipeLayer;
        void mergeRecipe(JSONValue recipe, string layer) {
            if (error !is null) return;
            string[] problems;
            auto resolved = resolveRecipe(options.ext, recipe, ranges,
                problems);
            if (problems.length > 0) {
                error = layer ~ " recipe is invalid: " ~ problems[0];
                return;
            }
            foreach (name, value; resolved) {
                if ((name in recipeValues) !is null) {
                    error = "state and variant both drive parameter '"
                        ~ name ~ "'; two authored layers may not "
                        ~ "silently compose";
                    return;
                }
                recipeValues[name] = value;
                recipeLayer[name] = layer;
            }
        }
        if (options.stateName.length > 0) {
            auto state = findState(options.ext, options.stateName);
            if (state.type != JSONType.object) {
                error = "state '" ~ options.stateName
                    ~ "' is not stored in the project";
                return result;
            }
            mergeRecipe(state["recipe"], "state '" ~ options.stateName ~ "'");
            if (error !is null) return result;
        }
        if (options.variantId.length > 0) {
            auto variant = findVariant(options.ext, options.variantId);
            if (variant.type != JSONType.object) {
                error = "variant '" ~ options.variantId
                    ~ "' is not stored in the project";
                return result;
            }
            mergeRecipe(variant["recipe"],
                "variant '" ~ options.variantId ~ "'");
            if (error !is null) return result;
        }
        foreach (name, value; recipeValues) {
            if ((name in authored) !is null) {
                error = "the input track and " ~ recipeLayer[name]
                    ~ " both drive parameter '" ~ name ~ "'";
                return result;
            }
            if (!appendConstant(name, value, recipeLayer[name]))
                return result;
        }
    }

    string[] driverNamesUsed;
    if (options.hasDrivers) {
        auto driven = driverInputs(puppet, options.drivers, steps, fps);
        foreach (name, values; driven) {
            if ((name in authored) !is null) continue; // authored wins
            auto param = findParam(name);
            if (param is null) continue;
            if ((param in puppet.getParameterDrivers()) !is null) {
                error = "ambient driver input '" ~ name
                    ~ "' is driven by a physics driver and cannot be "
                    ~ "a preview input";
                return result;
            }
            Input input;
            input.param = param;
            foreach (v; values)
                input.values ~= vec2(cast(float) v,
                    cast(float) param.defaults.y);
            inputs ~= input;
            authored[name] = true;
            driverNamesUsed ~= name;
        }
    }

    // Series layout: every parameter, in puppet order.
    CurveSeries[] series;
    foreach (p; puppet.parameters) {
        CurveSeries sx;
        sx.parameter = p.name;
        sx.axis = 0;
        series ~= sx;
        if (p.isVec2) {
            CurveSeries sy;
            sy.parameter = p.name;
            sy.axis = 1;
            series ~= sy;
        }
    }

    // Scripted clock + fresh driver state. The clock starts
    // at the library's current time origin so the first
    // inUpdate computes a zero delta even in a long-lived
    // host process (the MCP server).
    simNow = lastTime();
    inSetTimingFunc(&previewClock);
    inUpdate();
    puppet.resetDrivers();
    foreach (driver; puppet.getDrivers()) {
        if (auto sp = cast(SimplePhysics) driver)
            sp.fixedStep = true;
    }

    const double dt = 1.0 / fps;
    foreach (i; 0 .. steps) {
        foreach (input; inputs)
            input.param.value = input.values[i];
        simNow += dt;
        inUpdate();
        puppet.update();

        size_t s = 0;
        foreach (p; puppet.parameters) {
            series[s].values ~= p.value.x;
            s++;
            if (p.isVec2) {
                series[s].values ~= p.value.y;
                s++;
            }
        }
    }

    result.source = "physics-preview";
    result.dt = dt;
    result.series = series;
    result.stepCount = steps;
    string[2] kvProject = ["project", quote(projectLabel)];
    string[2] kvFps = ["fps", format("%d", fps)];
    result.extra ~= kvProject;
    result.extra ~= kvFps;
    if (options.stateName.length > 0) {
        string[2] kv = ["state", quote(options.stateName)];
        result.extra ~= kv;
    }
    if (options.variantId.length > 0) {
        string[2] kv = ["variant", quote(options.variantId)];
        result.extra ~= kv;
    }
    if (options.hasDrivers) {
        import std.algorithm : sort;
        import std.array : join;
        sort(driverNamesUsed);
        string[2] kvDrivers = ["drivers",
            quote(activeDriverNames(options.drivers).join(","))];
        string[2] kvSeed = ["driver-seed",
            format("%d", options.drivers.seed)];
        result.extra ~= kvDrivers;
        result.extra ~= kvSeed;
    }
    return result;
}
