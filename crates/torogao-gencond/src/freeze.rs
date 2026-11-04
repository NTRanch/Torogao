//! The freeze operations (D-838, the LIFT layer, Stage 1):
//! the verbs by which a neural proposal becomes deterministic
//! data an author can keep.
//!
//! `freeze_track` takes a dense parameter track — samples a
//! proposer emitted — and reduces it to keyforms with fitted
//! tangents under a stated deviation contract (the epsilon).
//! The epsilon is never an invented quality threshold (the
//! Stage-1 lesson of the conditioning seam: no numeric band
//! without evidence). It is either the caller's contract
//! value, or [`track_noise_epsilon`] — a value derived from
//! the track's own samples. The frozen curve is a piecewise
//! cubic Hermite interpolation through the keyforms; deviation
//! is measured at every source sample, and a track that cannot
//! meet its epsilon within the keyform bound refuses whole
//! (`TG-GENCOND-FREEZE-DEVIATION`) instead of silently
//! shipping a curve outside its contract.
//!
//! `freeze_palette` snaps a colour set onto a locked palette
//! under a caller-stated tolerance (Chebyshev distance in
//! sRGB, per-channel maximum). A colour whose nearest palette
//! entry lies beyond the tolerance refuses the whole snap
//! (`TG-GENCOND-PALETTE-UNMATCHED`), naming every offending
//! colour and its nearest distance — nothing snaps partially,
//! the same refuse/report discipline as
//! `normalize_reference_embed` (DESIGN §2.2).
//!
//! Both operations are pure functions: same inputs,
//! byte-identical outputs, no clock, no environment. Their
//! results hash canonically, and the scene record carries the
//! (source hash -> frozen hash) provenance of every freeze
//! applied to it.

use crate::compile::sha256_hex;
use crate::error::GencondResult;
use crate::error::{
    CODE_CONFLICT, CODE_FREEZE_DEVIATION, CODE_MALFORMED, CODE_PALETTE_UNMATCHED, CODE_RANGE,
    refuse,
};
use crate::limits::{KEYFORMS_MAX, PALETTE_COLORS_MAX, TRACK_SAMPLES_MAX};
use serde::{Deserialize, Serialize};

/// One dense sample of a parameter track.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrackSample {
    /// Sample time (seconds; the track's own clock).
    pub time: f64,
    /// Parameter value at `time`.
    pub value: f64,
}

/// A dense parameter track: a proposer's sampled output for
/// one named parameter, times strictly increasing.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParameterTrack {
    /// The parameter this track drives (a source-id-shaped
    /// name).
    pub parameter: String,
    /// The samples, at least two, times strictly increasing.
    pub samples: Vec<TrackSample>,
}

/// One frozen keyform: a key time, its value, and the fitted
/// Hermite tangents on both sides. This freeze fits a single
/// tangent per key, so `tangent_in == tangent_out` by
/// construction; the pair is stored because the record speaks
/// the rig vocabulary, where a later stage may break a
/// tangent deliberately.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Keyform {
    /// Key time (a source sample's time).
    pub time: f64,
    /// Parameter value at the key.
    pub value: f64,
    /// Fitted tangent entering the key (value per second).
    pub tangent_in: f64,
    /// Fitted tangent leaving the key (value per second).
    pub tangent_out: f64,
}

/// A track after freezing: the keyforms plus the measured
/// worst deviation of the fitted curve from the source
/// samples (at sample times; between samples the Hermite
/// curve is the definition, there is no other ground truth).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FrozenTrack {
    /// The parameter this track drives.
    pub parameter: String,
    /// The keyforms, times strictly increasing.
    pub keyforms: Vec<Keyform>,
    /// Worst absolute deviation from the source samples,
    /// measured at every sample time after fitting.
    pub max_deviation: f64,
}

impl ParameterTrack {
    /// Validates the track: id syntax, bounds, finite values,
    /// strictly increasing times.
    pub fn validate(&self) -> GencondResult<()> {
        crate::schema::check_source_id(&self.parameter)?;
        if self.samples.len() < 2 {
            return refuse(
                CODE_MALFORMED,
                "a parameter track needs at least two samples",
            );
        }
        if self.samples.len() > TRACK_SAMPLES_MAX {
            return refuse(CODE_RANGE, "parameter track exceeds the sample bound");
        }
        let mut previous: Option<f64> = None;
        for sample in &self.samples {
            if !sample.time.is_finite() || !sample.value.is_finite() {
                return refuse(CODE_MALFORMED, "track samples must be finite");
            }
            if let Some(before) = previous
                && sample.time <= before
            {
                return refuse(
                    CODE_MALFORMED,
                    "track sample times must be strictly increasing",
                );
            }
            previous = Some(sample.time);
        }
        Ok(())
    }
}

impl FrozenTrack {
    /// Validates a stored frozen track: id syntax, bounds,
    /// finite keyforms with strictly increasing times, and a
    /// finite non-negative measured deviation.
    pub fn validate(&self) -> GencondResult<()> {
        crate::schema::check_source_id(&self.parameter)?;
        if self.keyforms.len() < 2 {
            return refuse(CODE_MALFORMED, "a frozen track needs at least two keyforms");
        }
        if self.keyforms.len() > KEYFORMS_MAX {
            return refuse(CODE_RANGE, "frozen track exceeds the keyform bound");
        }
        let mut previous: Option<f64> = None;
        for key in &self.keyforms {
            if !key.time.is_finite()
                || !key.value.is_finite()
                || !key.tangent_in.is_finite()
                || !key.tangent_out.is_finite()
            {
                return refuse(CODE_MALFORMED, "keyform values and tangents must be finite");
            }
            if let Some(before) = previous
                && key.time <= before
            {
                return refuse(CODE_MALFORMED, "keyform times must be strictly increasing");
            }
            previous = Some(key.time);
        }
        if !self.max_deviation.is_finite() || self.max_deviation < 0.0 {
            return refuse(
                CODE_RANGE,
                "a frozen track's max_deviation must be finite and non-negative",
            );
        }
        Ok(())
    }
}

/// The epsilon a track's own samples suggest: the median
/// absolute second difference of the values — the scale at
/// which the samples themselves wiggle sample-to-sample.
/// A smooth track's second differences carry its curvature;
/// a noisy track's carry its noise; either way this is the
/// track's own roughness scale, measured, not chosen. A
/// perfectly straight track derives 0.0 (freeze exactly).
/// Defined for validated tracks; shorter or non-finite input
/// yields 0.0.
pub fn track_noise_epsilon(track: &ParameterTrack) -> f64 {
    if track.samples.len() < 3 {
        return 0.0;
    }
    let mut magnitudes: Vec<f64> = track
        .samples
        .windows(3)
        .map(|w| (w[0].value - 2.0 * w[1].value + w[2].value).abs())
        .filter(|value| value.is_finite())
        .collect();
    if magnitudes.is_empty() {
        return 0.0;
    }
    magnitudes.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    // Lower median: a pure selection, no averaging, so the
    // derived value is one of the track's own measurements.
    magnitudes[(magnitudes.len() - 1) / 2]
}

/// Freezes a dense track into keyforms under `epsilon`.
///
/// Key selection is deviation-driven: keys start at the two
/// end samples; tangents are fitted (centred differences over
/// key neighbours, one-sided at the ends); the worst-deviating
/// sample becomes a key; repeat until every sample is within
/// `epsilon` of the fitted Hermite curve. Deterministic by
/// construction — ties in worst deviation resolve to the
/// earliest sample. Refuses with `TG-GENCOND-FREEZE-DEVIATION`
/// when the keyform bound is reached with deviation still
/// above the epsilon: the track does not meet its contract
/// and no curve is emitted.
pub fn freeze_track(track: &ParameterTrack, epsilon: f64) -> GencondResult<FrozenTrack> {
    track.validate()?;
    if !epsilon.is_finite() || epsilon < 0.0 {
        return refuse(CODE_RANGE, "freeze epsilon must be finite and non-negative");
    }
    let samples = &track.samples;
    let count = samples.len();
    let mut keys: Vec<usize> = vec![0, count - 1];
    loop {
        let keyforms = fit_keyforms(samples, &keys);
        let (worst_index, worst_deviation) = worst_deviation(samples, &keyforms);
        if worst_deviation <= epsilon {
            return Ok(FrozenTrack {
                parameter: track.parameter.clone(),
                keyforms,
                max_deviation: worst_deviation,
            });
        }
        if keys.len() >= KEYFORMS_MAX || keys.len() >= count {
            return refuse(
                CODE_FREEZE_DEVIATION,
                format!(
                    "track '{}' cannot meet epsilon {epsilon} within the keyform bound: \
                     worst deviation {worst_deviation} at time {}",
                    track.parameter, samples[worst_index].time
                ),
            );
        }
        keys.push(worst_index);
        keys.sort_unstable();
    }
}

/// Fits one tangent per key: centred differences over key
/// neighbours in (time, value) space, one-sided at the ends.
fn fit_keyforms(samples: &[TrackSample], keys: &[usize]) -> Vec<Keyform> {
    let mut keyforms = Vec::with_capacity(keys.len());
    for (position, &index) in keys.iter().enumerate() {
        let sample = samples[index];
        let tangent = if position == 0 {
            let next = samples[keys[1]];
            (next.value - sample.value) / (next.time - sample.time)
        } else if position == keys.len() - 1 {
            let prev = samples[keys[position - 1]];
            (sample.value - prev.value) / (sample.time - prev.time)
        } else {
            let prev = samples[keys[position - 1]];
            let next = samples[keys[position + 1]];
            (next.value - prev.value) / (next.time - prev.time)
        };
        keyforms.push(Keyform {
            time: sample.time,
            value: sample.value,
            tangent_in: tangent,
            tangent_out: tangent,
        });
    }
    keyforms
}

/// Evaluates the Hermite curve through `keyforms` at `time`.
/// Outside the key range the end value holds (tracks are
/// authored over their span; extrapolation is not invented).
fn evaluate(keyforms: &[Keyform], time: f64) -> f64 {
    if time <= keyforms[0].time {
        return keyforms[0].value;
    }
    let last = keyforms[keyforms.len() - 1];
    if time >= last.time {
        return last.value;
    }
    let segment = keyforms
        .windows(2)
        .position(|pair| time >= pair[0].time && time <= pair[1].time)
        .unwrap_or(keyforms.len() - 2);
    let (a, b) = (keyforms[segment], keyforms[segment + 1]);
    let span = b.time - a.time;
    let s = (time - a.time) / span;
    let s2 = s * s;
    let s3 = s2 * s;
    let h00 = 2.0 * s3 - 3.0 * s2 + 1.0;
    let h10 = s3 - 2.0 * s2 + s;
    let h01 = -2.0 * s3 + 3.0 * s2;
    let h11 = s3 - s2;
    h00 * a.value + h10 * span * a.tangent_out + h01 * b.value + h11 * span * b.tangent_in
}

/// The worst absolute deviation of the fitted curve from the
/// samples, and the index of the sample where it occurs
/// (earliest on ties). Key samples deviate by exactly zero —
/// the Hermite basis is interpolatory — so the worst is
/// always a non-key sample until every sample is a key.
fn worst_deviation(samples: &[TrackSample], keyforms: &[Keyform]) -> (usize, f64) {
    let mut worst_index = 0;
    let mut worst = 0.0_f64;
    for (index, sample) in samples.iter().enumerate() {
        let deviation = (evaluate(keyforms, sample.time) - sample.value).abs();
        if deviation > worst {
            worst = deviation;
            worst_index = index;
        }
    }
    (worst_index, worst)
}

/// One snapped colour: the colour as authored and the locked
/// palette colour it snapped to.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PalettePair {
    /// The colour as authored, canonical `#rrggbb`.
    pub source: String,
    /// The palette colour it snapped to, canonical `#rrggbb`.
    pub snapped: String,
}

/// A colour set after snapping onto a locked palette.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PaletteSnap {
    /// SHA-256 of the locked palette's canonical form (the
    /// canonical colour list as a JSON array).
    pub palette_sha256: String,
    /// The tolerance the snap ran under (Chebyshev distance
    /// in sRGB, 0..=255).
    pub tolerance: u32,
    /// The snapped pairs, sorted by source colour.
    pub pairs: Vec<PalettePair>,
}

impl PaletteSnap {
    /// Validates a stored snap: hash shape, tolerance range,
    /// canonical colour forms, one pair per source colour.
    pub fn validate(&self) -> GencondResult<()> {
        crate::schema::check_sha256(&self.palette_sha256, "palette sha256")?;
        if self.tolerance > 255 {
            return refuse(CODE_RANGE, "palette tolerance must lie in [0, 255]");
        }
        if self.pairs.len() > PALETTE_COLORS_MAX {
            return refuse(CODE_RANGE, "palette snap exceeds the colour bound");
        }
        let mut sources: Vec<&str> = Vec::new();
        for pair in &self.pairs {
            parse_colour(&pair.source)?;
            parse_colour(&pair.snapped)?;
            if pair.source != canonical_colour(&pair.source)
                || pair.snapped != canonical_colour(&pair.snapped)
            {
                return refuse(
                    CODE_MALFORMED,
                    "palette snap colours must be in canonical #rrggbb form",
                );
            }
            if sources.contains(&pair.source.as_str()) {
                return refuse(
                    CODE_CONFLICT,
                    format!("palette snap carries colour '{}' twice", pair.source),
                );
            }
            sources.push(pair.source.as_str());
        }
        Ok(())
    }
}

/// Parses a `#rrggbb` colour (either hex case) into its sRGB
/// channels. Anything else is malformed, named in the refusal.
pub fn parse_colour(text: &str) -> GencondResult<(u8, u8, u8)> {
    let digits = text.strip_prefix('#').unwrap_or("");
    let valid = digits.len() == 6 && digits.bytes().all(|b| b.is_ascii_hexdigit());
    if !valid {
        return refuse(
            CODE_MALFORMED,
            format!("colour '{text}' is not a #rrggbb colour"),
        );
    }
    let channel = |at: usize| u8::from_str_radix(&digits[at..at + 2], 16).unwrap_or(0);
    Ok((channel(0), channel(2), channel(4)))
}

/// The canonical spelling of a colour: lowercase `#rrggbb`.
/// Callers validate with [`parse_colour`] first; a malformed
/// input is returned unchanged so validation can refuse it.
pub fn canonical_colour(text: &str) -> String {
    match parse_colour(text) {
        Ok((r, g, b)) => format!("#{r:02x}{g:02x}{b:02x}"),
        Err(_) => text.to_string(),
    }
}

/// Chebyshev distance between two sRGB colours: the largest
/// per-channel absolute difference, 0..=255.
fn colour_distance(a: (u8, u8, u8), b: (u8, u8, u8)) -> u32 {
    let channel = |x: u8, y: u8| (i32::from(x) - i32::from(y)).unsigned_abs();
    channel(a.0, b.0)
        .max(channel(a.1, b.1))
        .max(channel(a.2, b.2))
}

/// Snaps `colors` onto the locked `palette` under
/// `tolerance`. Every colour must have a palette entry within
/// tolerance (nearest wins; ties resolve to the earliest
/// palette entry — palette order is data). Any colour beyond
/// tolerance refuses the whole snap with
/// `TG-GENCOND-PALETTE-UNMATCHED`, naming each offending
/// colour and its nearest distance; nothing is applied.
pub fn freeze_palette(
    colors: &[String],
    palette: &[String],
    tolerance: u32,
) -> GencondResult<PaletteSnap> {
    if tolerance > 255 {
        return refuse(CODE_RANGE, "palette tolerance must lie in [0, 255]");
    }
    if palette.is_empty() {
        return refuse(CODE_MALFORMED, "a locked palette needs at least one colour");
    }
    if palette.len() > PALETTE_COLORS_MAX || colors.len() > PALETTE_COLORS_MAX {
        return refuse(CODE_RANGE, "palette snap exceeds the colour bound");
    }
    let mut palette_colours: Vec<String> = Vec::with_capacity(palette.len());
    for entry in palette {
        parse_colour(entry)?;
        let canonical = canonical_colour(entry);
        if palette_colours.contains(&canonical) {
            return refuse(
                CODE_CONFLICT,
                format!("locked palette carries colour '{canonical}' twice"),
            );
        }
        palette_colours.push(canonical);
    }
    let palette_channels: Vec<(u8, u8, u8)> = palette_colours
        .iter()
        .map(|entry| parse_colour(entry))
        .collect::<GencondResult<Vec<_>>>()?;
    let mut pairs: Vec<PalettePair> = Vec::new();
    let mut unmatched: Vec<String> = Vec::new();
    for colour in colors {
        let channels = parse_colour(colour)?;
        let canonical = canonical_colour(colour);
        if pairs.iter().any(|pair| pair.source == canonical) {
            continue;
        }
        let mut nearest: Option<(u32, usize)> = None;
        for (index, candidate) in palette_channels.iter().enumerate() {
            let distance = colour_distance(channels, *candidate);
            if nearest.is_none_or(|(best, _)| distance < best) {
                nearest = Some((distance, index));
            }
        }
        let (distance, index) = nearest.unwrap_or((0, 0));
        if distance > tolerance {
            unmatched.push(format!(
                "{canonical} (nearest {} at distance {distance})",
                palette_colours[index]
            ));
        } else {
            pairs.push(PalettePair {
                source: canonical,
                snapped: palette_colours[index].clone(),
            });
        }
    }
    if !unmatched.is_empty() {
        return refuse(
            CODE_PALETTE_UNMATCHED,
            format!(
                "colour(s) beyond tolerance {tolerance} of the locked palette: {}; \
                 nothing snapped",
                unmatched.join(", ")
            ),
        );
    }
    pairs.sort_by(|a, b| a.source.cmp(&b.source));
    let palette_json = serde_json::to_string(&palette_colours).unwrap_or_else(|_| "[]".to_string());
    Ok(PaletteSnap {
        palette_sha256: sha256_hex(palette_json.as_bytes()),
        tolerance,
        pairs,
    })
}

/// The canonical hash of a freeze input or output: SHA-256 of
/// its canonical JSON (the workspace's sorted-key object
/// model, compact separators — the graph identity rule, S-1
/// §3, applied to freeze data).
pub fn canonical_sha256<T: Serialize>(value: &T) -> GencondResult<String> {
    let text = serde_json::to_string(value).map_err(|error| {
        crate::error::GencondError::new(
            CODE_MALFORMED,
            format!("value cannot be canonically serialized: {error}"),
        )
    })?;
    Ok(sha256_hex(text.as_bytes()))
}
