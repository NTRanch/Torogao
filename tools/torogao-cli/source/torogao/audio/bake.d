/*
    Torogao audio: envelope baking to parameter curves

    Copyright © 2026, Nice Thoughts Ranch
    Distributed under the 2-Clause BSD License, see LICENSE file.

    Bakes a WAV master or stem into a torogao-curves/1
    document holding the signal's amplitude envelope, so
    audio drives parameters (mouth, breath, impact accents)
    in the deterministic preview and in exported rigs.

    The follower is the house-standard one-pole envelope:
    alpha(dt) = 1 - exp(-dt / tau), with tau = 10 ms while
    the signal rises (attack) and tau = 80 ms while it falls
    (release) — the same constants the rig runtime's live
    follower uses, so a baked curve and a live-followed
    performance agree. Channels are averaged to mono before
    following. Steps land on exact sample boundaries: every
    accepted rate (15/30/60/120 fps) divides 48 kHz evenly.
*/
module torogao.audio.bake;

import std.json : JSONType, JSONValue;
import std.math : exp, fabs;

import torogao.audio.wav : WavData;
import torogao.curves : CurveSeries, CurveSet;

/// House envelope constants (seconds).
enum envelopeAttackTau = 0.010;
enum envelopeReleaseTau = 0.080;

/// Bake a decoded WAV into an envelope curve set.
/// Returns false and sets error on a bad bake request.
bool bakeEnvelope(in WavData wav, JSONValue recipe, string sourceLabel,
                  out CurveSet result, out string error) {
    int fps = 60;
    string target = "AudioLevel";
    if (recipe.type == JSONType.object) {
        if (auto f = "fps" in recipe) {
            if (f.type == JSONType.integer) fps = cast(int) f.integer;
            else if (f.type == JSONType.float_) fps = cast(int) f.floating;
        }
        if (auto t = "target" in recipe)
            if (t.type == JSONType.string) target = t.str;
        if (recipe.getStr("format") != "" &&
            recipe.getStr("format") != "torogao-audio-bake/1") {
            error = "bake recipe format must be " ~
                "\"torogao-audio-bake/1\"";
            return false;
        }
    }
    if (fps != 15 && fps != 30 && fps != 60 && fps != 120) {
        error = "bake fps must be one of 15, 30, 60, 120";
        return false;
    }
    if (wav.sampleRate == 0 || wav.frames == 0) {
        error = "audio is empty";
        return false;
    }
    size_t samplesPerStep = wav.sampleRate / fps;
    if (samplesPerStep * fps != wav.sampleRate) {
        error = "sample rate does not divide evenly at this fps";
        return false;
    }
    double dtSample = 1.0 / wav.sampleRate;
    double alphaAttack = 1.0 - exp(-dtSample / envelopeAttackTau);
    double alphaRelease = 1.0 - exp(-dtSample / envelopeReleaseTau);
    size_t steps = wav.frames / samplesPerStep;
    double[] values;
    values.length = steps;
    double env = 0;
    size_t stepIdx = 0;
    foreach (n; 0 .. wav.frames) {
        double mono = 0;
        foreach (ch; wav.channels) mono += ch[n];
        mono /= wav.channels.length;
        double a = fabs(mono);
        double alpha = (a > env) ? alphaAttack : alphaRelease;
        env += alpha * (a - env);
        if ((n + 1) % samplesPerStep == 0 && stepIdx < steps)
            values[stepIdx++] = env;
    }
    result.source = "audio-bake";
    result.dt = 1.0 / fps;
    result.series = [CurveSeries(target, 0, values)];
    result.stepCount = cast(long) steps;
    result.extra = [["file", JSONValue(sourceLabel).toString()],
                    ["sampleRate", JSONValue(wav.sampleRate).toString()],
                    ["attackTau", JSONValue(envelopeAttackTau).toString()],
                    ["releaseTau", JSONValue(envelopeReleaseTau).toString()]];
    return true;
}

private string getStr(JSONValue v, string key) {
    if (v.type != JSONType.object) return "";
    auto p = key in v;
    if (p is null || p.type != JSONType.string) return "";
    return p.str;
}
