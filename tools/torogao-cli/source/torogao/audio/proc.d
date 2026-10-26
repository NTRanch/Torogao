/*
    Torogao audio: procedural synthesis core

    Copyright © 2026, Nice Thoughts Ranch
    Distributed under the 2-Clause BSD License, see LICENSE file.

    Deterministic procedural audio from recipes
    (torogao-audio-recipe/1): sine tones, binaural pairs
    (two sine stems at an exact frequency offset, one per
    ear), filtered noise beds, and tone cues, mixed and
    normalized to the house master standard — 48 kHz
    WAV, integrated loudness target (default -16 LUFS),
    true peak at or under the ceiling (default -1 dBTP).
    When both cannot hold, the true-peak ceiling wins and
    the manifest reports the measured loudness actually
    achieved; loudness is never faked by peak normalization.

    Randomness, where a recipe uses it (noise), comes from
    xoshiro256** seeded via splitmix64 from the recipe's
    seed — integer math only, so noise is identical on
    every platform. Oscillators and filters run in double
    precision; rendering the same recipe twice on one
    machine yields a byte-identical master.

    Loudness is ITU-R BS.1770-4: K-weighting (the published
    48 kHz coefficients), 400 ms blocks at 75% overlap,
    absolute gate -70 LUFS, relative gate -10 LU. True peak
    is measured at 4x oversampling with a windowed-sinc
    (Blackman, 16 taps per phase) interpolator.

    House envelope constants for audio-driven curves live
    in torogao.audio.bake (attack 10 ms, release 80 ms).
*/
module torogao.audio.proc;

import std.digest.sha : sha256Of;
import std.format : format;
import std.json : JSONType, JSONValue;
import std.math : cos, exp, log10, sin, PI, isFinite;

import torogao.audio.wav : WavData, writeWavFile;
import torogao.cli.criteria : Finding;

enum audioSampleRate = 48000;
enum audioEngineId = "torogao-audio/1";
enum audioLicense = "Original work of the studio; procedural " ~
    "synthesis from the recipe, no third-party audio.";

// ------------------------------------------------------------ PRNG

/// splitmix64: expands the recipe seed into generator state.
struct SplitMix64 {
    ulong state;
    ulong next() {
        state += 0x9E3779B97F4A7C15UL;
        ulong z = state;
        z = (z ^ (z >> 30)) * 0xBF58476D1CE4E5B9UL;
        z = (z ^ (z >> 27)) * 0x94D049BB133111EBUL;
        return z ^ (z >> 31);
    }
}

/// xoshiro256** — the named generator for all recipe noise.
struct Xoshiro256 {
    ulong[4] s;
    static Xoshiro256 fromSeed(ulong seed) {
        auto sm = SplitMix64(seed);
        Xoshiro256 g;
        foreach (i; 0 .. 4) g.s[i] = sm.next();
        return g;
    }
    private static ulong rotl(ulong x, int k) {
        return (x << k) | (x >> (64 - k));
    }
    ulong next() {
        ulong result = rotl(s[1] * 5, 7) * 9;
        ulong t = s[1] << 17;
        s[2] ^= s[0];
        s[3] ^= s[1];
        s[1] ^= s[2];
        s[0] ^= s[3];
        s[2] ^= t;
        s[3] = rotl(s[3], 45);
        return result;
    }
    /// Uniform double in [-1, 1) from the top 53 bits.
    double uniform() {
        return (cast(double)(next() >> 11) / 9007199254740992.0) * 2.0 - 1.0;
    }
}

// ------------------------------------------------------------ recipe

private double getD(JSONValue v, string key, double fallback) {
    if (v.type != JSONType.object) return fallback;
    auto p = key in v;
    if (p is null) return fallback;
    if (p.type == JSONType.integer) return cast(double) p.integer;
    if (p.type == JSONType.uinteger) return cast(double) p.uinteger;
    if (p.type == JSONType.float_) return p.floating;
    return fallback;
}

private long getI(JSONValue v, string key, long fallback) {
    if (v.type != JSONType.object) return fallback;
    auto p = key in v;
    if (p is null) return fallback;
    if (p.type == JSONType.integer) return p.integer;
    if (p.type == JSONType.uinteger) return cast(long) p.uinteger;
    if (p.type == JSONType.float_) return cast(long) p.floating;
    return fallback;
}

private Finding err(string code, string msg) {
    return Finding(code, "error", "", msg);
}

/// Validate a recipe document; returns findings (empty =
/// renderable). Shared by audio-validate and audio-render.
Finding[] validateRecipe(JSONValue recipe) {
    Finding[] findings;
    if (recipe.type != JSONType.object) {
        findings ~= err("TG-AUDIO-FORMAT",
            "recipe must be a JSON object");
        return findings;
    }
    if (recipe.getStr("format") != "torogao-audio-recipe/1")
        findings ~= err("TG-AUDIO-FORMAT",
            "format must be \"torogao-audio-recipe/1\"");
    if (("seed" in recipe) is null ||
        (recipe["seed"].type != JSONType.integer &&
         recipe["seed"].type != JSONType.uinteger))
        findings ~= err("TG-AUDIO-SEED",
            "seed (integer) is required; rendering must be seeded");
    double duration = getD(recipe, "durationSeconds", 0);
    if (!(duration > 0) || duration > 600)
        findings ~= err("TG-AUDIO-DURATION",
            "durationSeconds must be in (0, 600]");
    double target = getD(recipe, "targetLufs", -16.0);
    if (target < -60 || target > -6)
        findings ~= err("TG-AUDIO-TARGET",
            "targetLufs must be in [-60, -6]");
    double ceiling = getD(recipe, "truePeakDb", -1.0);
    if (ceiling > 0 || ceiling < -20)
        findings ~= err("TG-AUDIO-TARGET",
            "truePeakDb must be in [-20, 0]");
    auto stems = "stems" in recipe;
    if (stems is null || stems.type != JSONType.array ||
        stems.array.length == 0) {
        findings ~= err("TG-AUDIO-STEM-TYPE",
            "stems must be a non-empty array");
        return findings;
    }
    foreach (i, stem; stems.array) {
        string where = "stem " ~ format("%d", i) ~ ": ";
        string type = stem.getStr("type");
        if (type != "sine" && type != "binaural" &&
            type != "noise" && type != "tone")
            findings ~= err("TG-AUDIO-STEM-TYPE",
                where ~ "unknown stem type \"" ~ type ~ "\"");
        double gain = getD(stem, "gain", -1);
        if (gain < 0 || gain > 1)
            findings ~= err("TG-AUDIO-GAIN",
                where ~ "gain must be in [0, 1]");
        if (type == "sine" || type == "tone") {
            double f = getD(stem, "freqHz", 0);
            if (!(f > 0) || f > 20000)
                findings ~= err("TG-AUDIO-FREQ",
                    where ~ "freqHz must be in (0, 20000]");
        }
        if (type == "binaural") {
            double base = getD(stem, "baseHz", 0);
            double beat = getD(stem, "beatHz", -1);
            if (!(base > 0) || base > 20000)
                findings ~= err("TG-AUDIO-FREQ",
                    where ~ "baseHz must be in (0, 20000]");
            if (beat < 0 || beat > 100)
                findings ~= err("TG-AUDIO-FREQ",
                    where ~ "beatHz must be in [0, 100]");
        }
        if (type == "noise") {
            double fc = getD(stem, "cutoffHz", 0);
            if (!(fc > 0) || fc > 20000)
                findings ~= err("TG-AUDIO-FREQ",
                    where ~ "cutoffHz must be in (0, 20000]");
        }
        if (auto env = "envelope" in stem) {
            if (getD(*env, "attackSeconds", 0) < 0 ||
                getD(*env, "releaseSeconds", 0) < 0)
                findings ~= err("TG-AUDIO-ENVELOPE",
                    where ~ "envelope times must be >= 0");
        }
        double start = getD(stem, "startSeconds", 0);
        if (start < 0 || start >= duration)
            findings ~= err("TG-AUDIO-DURATION",
                where ~ "startSeconds must be in [0, duration)");
    }
    return findings;
}

private string getStr(JSONValue v, string key) {
    if (v.type != JSONType.object) return "";
    auto p = key in v;
    if (p is null || p.type != JSONType.string) return "";
    return p.str;
}

// ------------------------------------------------------------ loudness

/// K-weight one channel (two cascaded biquads, direct form I).
private double[] kWeight(in double[] x) {
    // BS.1770-4 K-weighting coefficients at 48 kHz.
    enum double[3] b1 = [1.53512485958697, -2.69169618940638,
                         1.19839281085285];
    enum double[3] a1 = [1.0, -1.69065929318241, 0.73248077421585];
    enum double[3] b2 = [1.0, -2.0, 1.0];
    enum double[3] a2 = [1.0, -1.99004745483398, 0.99007225036621];
    auto y = new double[](x.length);
    double x1 = 0, x2 = 0, y1 = 0, y2 = 0;
    double u1 = 0, u2 = 0, v1 = 0, v2 = 0;
    foreach (n, xn; x) {
        double s = b1[0] * xn + b1[1] * x1 + b1[2] * x2
                 - a1[1] * y1 - a1[2] * y2;
        x2 = x1; x1 = xn; y2 = y1; y1 = s;
        double t = b2[0] * s + b2[1] * u1 + b2[2] * u2
                 - a2[1] * v1 - a2[2] * v2;
        u2 = u1; u1 = s; v2 = v1; v1 = t;
        y[n] = t;
    }
    return y;
}

/// Integrated loudness (LUFS) of a signal, BS.1770-4 gated.
double measureLufs(in double[][] channels, uint sampleRate) {
    size_t blockLen = cast(size_t)(0.400 * sampleRate);
    size_t hop = cast(size_t)(0.100 * sampleRate);
    size_t frames = channels.length ? channels[0].length : 0;
    if (frames < blockLen) return double.nan;
    auto weighted = new double[][](channels.length);
    foreach (c, ch; channels) weighted[c] = kWeight(ch);
    // Per-block per-channel mean squares.
    double[][] blockMs;
    for (size_t start = 0; start + blockLen <= frames; start += hop) {
        auto ms = new double[](channels.length);
        foreach (c; 0 .. channels.length) {
            double sum = 0;
            foreach (n; start .. start + blockLen)
                sum += weighted[c][n] * weighted[c][n];
            ms[c] = sum / blockLen;
        }
        blockMs ~= ms;
    }
    double blockLoudness(double[] ms) {
        double z = 0;
        foreach (m; ms) z += m;   // stereo/mono channel weights are 1.0
        return -0.691 + 10.0 * log10(z);
    }
    // Absolute gate.
    double[][] kept;
    foreach (ms; blockMs)
        if (blockLoudness(ms) > -70.0) kept ~= ms;
    if (kept.length == 0) return double.nan;
    // Relative gate at (ungated mean loudness - 10 LU).
    double[] meanMs = new double[](channels.length);
    meanMs[] = 0.0;
    foreach (ms; kept)
        foreach (c; 0 .. channels.length) meanMs[c] += ms[c];
    foreach (c; 0 .. channels.length) meanMs[c] /= kept.length;
    double relThreshold = blockLoudness(meanMs) - 10.0;
    double[][] kept2;
    foreach (ms; kept)
        if (blockLoudness(ms) > relThreshold) kept2 ~= ms;
    if (kept2.length == 0) kept2 = kept;
    double[] finalMs = new double[](channels.length);
    finalMs[] = 0.0;
    foreach (ms; kept2)
        foreach (c; 0 .. channels.length) finalMs[c] += ms[c];
    foreach (c; 0 .. channels.length) finalMs[c] /= kept2.length;
    return blockLoudness(finalMs);
}

/// True peak (dBTP) at 4x oversampling, windowed-sinc.
/// The interpolation weights depend only on the quarter
/// phase and the tap offset, so they are computed once.
double measureTruePeakDb(in double[][] channels) {
    static double[3][32] weights;
    static bool weightsReady = false;
    if (!weightsReady) {
        foreach (q; 1 .. 4) {
            for (int k = -15; k <= 16; k++) {
                double u = q * 0.25 - k;
                double sinc = (u == 0) ? 1.0 : sin(PI * u) / (PI * u);
                weights[k + 15][q - 1] = sinc * blackman32(u);
            }
        }
        weightsReady = true;
    }
    double peak = 0;
    foreach (ch; channels) {
        foreach (v; ch)
            if (abs(v) > peak) peak = abs(v);
        foreach (n; 0 .. ch.length) {
            foreach (q; 1 .. 4) {
                double acc = 0;
                for (int k = -15; k <= 16; k++) {
                    long idx = cast(long) n + k;
                    double xv = (idx >= 0 && idx < cast(long) ch.length)
                        ? ch[cast(size_t) idx] : 0.0;
                    acc += xv * weights[k + 15][q - 1];
                }
                if (abs(acc) > peak) peak = abs(acc);
            }
        }
    }
    if (peak <= 0) return -120.0;
    return 20.0 * log10(peak);
}

/// Blackman window over u in [-16, 16] (zero outside).
private double blackman32(double u) {
    if (u < -16.0 || u > 16.0) return 0.0;
    double p = (u + 16.0) / 32.0;
    return 0.42 - 0.5 * cos(2.0 * PI * p) + 0.08 * cos(4.0 * PI * p);
}

private double abs(double v) { return v < 0 ? -v : v; }

// ------------------------------------------------------------ synthesis

private double envelopeAt(JSONValue stem, double t, double stemDur) {
    double start = getD(stem, "startSeconds", 0);
    double local = t - start;
    if (local < 0 || local >= stemDur) return 0;
    double attack = 0, release = 0;
    if (auto env = "envelope" in stem) {
        attack = getD(*env, "attackSeconds", 0);
        release = getD(*env, "releaseSeconds", 0);
    }
    double g = 1.0;
    if (attack > 0 && local < attack) g = local / attack;
    if (release > 0 && local > stemDur - release)
        g = (stemDur - local) / release;
    return g;
}

/// Render a validated recipe to channels (not yet normalized).
private double[][] synthesize(JSONValue recipe, size_t frames) {
    double duration = getD(recipe, "durationSeconds", 0);
    ulong seed = cast(ulong) getI(recipe, "seed", 0);
    auto channels = new double[][](2, frames);
    foreach (ch; channels) ch[] = 0.0;  // double.init is NaN
    foreach (stem; recipe["stems"].array) {
        string type = stem.getStr("type");
        double gain = getD(stem, "gain", 0);
        double start = getD(stem, "startSeconds", 0);
        double stemDur = getD(stem, "durationSeconds", duration - start);
        size_t n0 = cast(size_t)(start * audioSampleRate);
        size_t n1 = cast(size_t)((start + stemDur) * audioSampleRate);
        if (n1 > frames) n1 = frames;
        if (type == "sine" || type == "tone") {
            double f = getD(stem, "freqHz", 0);
            foreach (n; n0 .. n1) {
                double t = cast(double) n / audioSampleRate;
                double v = gain * sin(2.0 * PI * f * t)
                    * envelopeAt(stem, t, stemDur);
                channels[0][n] += v;
                channels[1][n] += v;
            }
        } else if (type == "binaural") {
            double base = getD(stem, "baseHz", 0);
            double beat = getD(stem, "beatHz", 0);
            double fl = base - beat / 2, fr = base + beat / 2;
            foreach (n; n0 .. n1) {
                double t = cast(double) n / audioSampleRate;
                double e = envelopeAt(stem, t, stemDur);
                channels[0][n] += gain * sin(2.0 * PI * fl * t) * e;
                channels[1][n] += gain * sin(2.0 * PI * fr * t) * e;
            }
        } else if (type == "noise") {
            double fc = getD(stem, "cutoffHz", 1000);
            double a = 1.0 - exp(-2.0 * PI * fc / audioSampleRate);
            foreach (chIdx; 0 .. 2) {
                auto gen = Xoshiro256.fromSeed(
                    seed + 7919 * (cast(ulong) chIdx + 1));
                double lp = 0;
                foreach (n; 0 .. n1) {
                    double w = gen.uniform();
                    lp += a * (w - lp);
                    if (n >= n0) {
                        double t = cast(double) n / audioSampleRate;
                        channels[chIdx][n] += gain * lp
                            * envelopeAt(stem, t, stemDur);
                    }
                }
            }
        }
    }
    return channels;
}

/// Render result: the manifest document.
struct RenderResult {
    JSONValue manifest;
    string error;
}

/// Render a recipe to a WAV master + manifest in outDir.
/// File names derive from the recipe's name field.
RenderResult renderRecipe(JSONValue recipe, string recipeText,
                          string outDir) {
    RenderResult result;
    auto findings = validateRecipe(recipe);
    foreach (f; findings)
        if (f.severity == "error") {
            result.error = f.code ~ ": " ~ f.message;
            return result;
        }
    double duration = getD(recipe, "durationSeconds", 0);
    size_t frames = cast(size_t)(duration * audioSampleRate + 0.5);
    auto channels = synthesize(recipe, frames);
    double target = getD(recipe, "targetLufs", -16.0);
    double ceiling = getD(recipe, "truePeakDb", -1.0);

    double lufs = measureLufs(channels, audioSampleRate);
    if (lufs != lufs || !isFinite(lufs)) {
        result.error = "recipe renders silence (no measurable loudness)";
        return result;
    }
    double gainDb = target - lufs;
    double g = pow10(gainDb / 20.0);
    foreach (ch; channels)
        foreach (ref v; ch) v *= g;
    double tp = measureTruePeakDb(channels);
    if (tp > ceiling) {
        double g2 = pow10((ceiling - tp) / 20.0);
        foreach (ch; channels)
            foreach (ref v; ch) v *= g2;
        gainDb += ceiling - tp;
        tp = measureTruePeakDb(channels);
    }
    // The manifest always reports the final signal's measured
    // loudness, never the pre-normalization value.
    lufs = measureLufs(channels, audioSampleRate);
    string name = recipe.getStr("name");
    if (name.length == 0) name = "untitled";
    string wavName = name ~ ".wav";
    import std.path : buildPath;
    writeWavFile(buildPath(outDir, wavName), channels, audioSampleRate);

    JSONValue manifest;
    manifest["file"] = wavName;
    manifest["recipe"] = name;
    manifest["recipeSha256"] = format("%(%02x%)",
        sha256Of(cast(ubyte[]) recipeText));
    manifest["seed"] = getI(recipe, "seed", 0);
    manifest["engine"] = audioEngineId;
    manifest["prng"] = "xoshiro256** seeded via splitmix64";
    manifest["sampleRate"] = audioSampleRate;
    manifest["channels"] = 2;
    manifest["bitsPerSample"] = 24;
    manifest["durationSeconds"] = cast(double) frames / audioSampleRate;
    manifest["measuredLufs"] = round3(lufs);
    manifest["measuredTruePeakDb"] = round3(tp);
    manifest["targetLufs"] = target;
    manifest["truePeakCeilingDb"] = ceiling;
    manifest["gainAppliedDb"] = round3(gainDb);
    manifest["license"] = audioLicense;
    writeManifest(buildPath(outDir, name ~ ".manifest.json"), manifest);
    result.manifest = manifest;
    return result;
}

private double pow10(double e) {
    import std.math : pow;
    return pow(10.0, e);
}

private double round3(double v) {
    import std.math : round;
    return round(v * 1000.0) / 1000.0;
}

private void writeManifest(string path, JSONValue manifest) {
    import std.file : write;
    write(path, manifest.toString() ~ "\n");
}
