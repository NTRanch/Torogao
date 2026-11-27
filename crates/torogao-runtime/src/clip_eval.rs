//! Clip evaluation: the DESIGN Appendix C curve mathematics
//! (hand-animation programme Rung H1), the evaluator-bearing
//! Tier 2 witness the runtime plan's R3 rung calls for.
//!
//! This module is a Tier 2 module in the sense of
//! [`crate::tier1`]: it reads floats (the authored curve data
//! of a frozen `torogao-motion-clip/2` document) and produces
//! Tier 2 presentation gauges; it never feeds a Tier 1
//! simulation. The Tier 1 source lint exempts it by name, on
//! the same argument that admitted `tier2.rs` at Rung R2.
//!
//! The mathematics is DESIGN.md Appendix C, implemented
//! exactly as written there:
//!
//! - outside the key range, evaluation clamps to the end
//!   keys;
//! - `step` holds the segment's opening value;
//! - `linear` interpolates componentwise, except rotations,
//!   which take the quaternion slerp (shortest arc: the
//!   second key is sign-corrected when the dot product is
//!   negative; near-parallel keys fall back to a normalised
//!   lerp; the result is normalised);
//! - `cubic` is the cubic Hermite spline over the segment,
//!   with the keys' tangents read as derivatives with respect
//!   to frames and scaled by the segment length in frames;
//!   rotation results are normalised after the spline.
//!
//! Everything here is hand-rolled on `f64` (the estate's
//! minimal-stack ruling: no new crates for clip math). The
//! module consumes only documents the `torogao-motion-clip/2`
//! validator has accepted — [`Clip::from_text`] runs the
//! validator first and refuses whatever it refuses.

use serde_json::Value;
use sha2::{Digest, Sha256};
use torogao_author::{Refusal, RefusalClass};

/// Which channel property a clip channel drives; decides the
/// value's shape and whether rotation mathematics applies.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClipProperty {
    /// A joint's translation (vec3).
    Translation,
    /// A joint's rotation (quaternion `[x, y, z, w]`); the
    /// only property evaluated with slerp / normalisation.
    Rotation,
    /// A joint's scale (vec3).
    Scale,
    /// A morph target's weight (scalar).
    Weight,
    /// A named parameter's value (scalar).
    Value,
}

/// A channel's segment interpolation (glTF's three modes).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClipInterp {
    /// Hold the opening key's value across the segment.
    Step,
    /// Componentwise linear (slerp for rotations).
    Linear,
    /// Cubic Hermite over the keys' explicit tangents.
    Cubic,
}

/// One key of an evaluated channel: its frame, value, the
/// interpolation of the segment it opens, and the segment's
/// Hermite tangents where the format obliges them.
#[derive(Clone, Debug, PartialEq)]
pub struct ClipKey {
    /// The key's integer frame.
    pub frame: i64,
    /// The key's value, in the property's shape.
    pub value: Vec<f64>,
    /// The interpolation of the segment this key opens.
    pub interp: ClipInterp,
    /// Derivative entering this key (present exactly when
    /// the previous segment is cubic).
    pub in_tangent: Option<Vec<f64>>,
    /// Derivative leaving this key (present exactly when
    /// this key's segment is cubic).
    pub out_tangent: Option<Vec<f64>>,
}

/// One channel of an evaluated clip.
#[derive(Clone, Debug, PartialEq)]
pub struct ClipChannel {
    /// The channel's gauge identity: `kind:name:property`.
    pub target_id: String,
    /// The property the channel drives.
    pub property: ClipProperty,
    /// The channel's keys, at strictly increasing frames.
    pub keys: Vec<ClipKey>,
}

/// A validated `torogao-motion-clip/2` document, parsed into
/// evaluation shape. Construction runs the format validator;
/// a document the validator refuses never becomes a `Clip`.
#[derive(Clone, Debug, PartialEq)]
pub struct Clip {
    /// The clip's name.
    pub name: String,
    /// The clip's frame range `(start, end)`.
    pub range: (i64, i64),
    /// The loop sub-range, when the clip loops.
    pub loop_range: Option<(i64, i64)>,
    /// The clip's channels, in document order.
    pub channels: Vec<ClipChannel>,
    /// The freeze hash, when the document was frozen.
    pub freeze_sha256: Option<String>,
}

fn malformed(message: String) -> Refusal {
    Refusal::new(RefusalClass::Malformed, message)
}

fn field<'v>(object: &'v Value, name: &str) -> Result<&'v Value, Refusal> {
    object
        .get(name)
        .ok_or_else(|| malformed(format!("clip document is missing '{name}'")))
}

fn as_f64(value: &Value) -> Result<f64, Refusal> {
    value
        .as_f64()
        .ok_or_else(|| malformed("clip value component is not a number".to_string()))
}

fn number_vec(value: &Value) -> Result<Vec<f64>, Refusal> {
    // A value (or tangent) is an array for vec3/quaternion
    // properties and a bare number for the scalar properties
    // (morph weight, parameter value) — the format's two
    // shapes, normalised here to a component vector.
    if let Some(scalar) = value.as_f64() {
        return Ok(vec![scalar]);
    }
    let items = value
        .as_array()
        .ok_or_else(|| malformed("clip value is neither a number nor an array".to_string()))?;
    items.iter().map(as_f64).collect()
}

fn optional_vec(object: &Value, name: &str) -> Result<Option<Vec<f64>>, Refusal> {
    match object.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => Ok(Some(number_vec(value)?)),
    }
}

impl Clip {
    /// Parses and validates a clip document (the format
    /// validator runs first; its refusals are this
    /// constructor's refusals), then builds the evaluation
    /// shape.
    ///
    /// # Errors
    ///
    /// Returns a typed refusal when the text does not parse
    /// or the document fails `torogao-motion-clip/2`
    /// validation.
    pub fn from_text(text: &str) -> Result<Self, Refusal> {
        let document: Value = serde_json::from_str(text)
            .map_err(|error| malformed(format!("clip document does not parse: {error}")))?;
        let report = torogao_format::motion_clip::validate_clip(text.as_bytes())
            .map_err(|error| malformed(format!("clip document refused: {error}")))?;
        let range_object = field(&document, "range")?;
        let range = (
            field(range_object, "start")?
                .as_i64()
                .ok_or_else(|| malformed("clip range start is not an integer".to_string()))?,
            field(range_object, "end")?
                .as_i64()
                .ok_or_else(|| malformed("clip range end is not an integer".to_string()))?,
        );
        let loop_range = match document.get("loop") {
            None | Some(Value::Null) => None,
            Some(object) => Some((
                field(object, "start")?
                    .as_i64()
                    .ok_or_else(|| malformed("clip loop start is not an integer".to_string()))?,
                field(object, "end")?
                    .as_i64()
                    .ok_or_else(|| malformed("clip loop end is not an integer".to_string()))?,
            )),
        };
        let mut channels = Vec::new();
        for raw in field(&document, "channels")?
            .as_array()
            .ok_or_else(|| malformed("clip channels are not an array".to_string()))?
        {
            channels.push(parse_channel(raw)?);
        }
        Ok(Self {
            name: report.name,
            range,
            loop_range,
            channels,
            freeze_sha256: report.freeze_sha256,
        })
    }

    /// The frame a witness tick maps to: inside a looping
    /// clip, ticks wrap the loop range; outside one, the
    /// frame advances from the range start and clamps at the
    /// range end.
    #[must_use]
    pub fn frame_at(&self, tick: u64) -> i64 {
        let tick = i128::from(tick);
        if let Some((start, end)) = self.loop_range {
            let span = i128::from(end) - i128::from(start) + 1;
            let offset = tick % span;
            return i64::try_from(i128::from(start) + offset).unwrap_or(start);
        }
        let (start, end) = self.range;
        let frame = i128::from(start) + tick;
        if frame > i128::from(end) {
            end
        } else {
            i64::try_from(frame).unwrap_or(end)
        }
    }

    /// Every channel's value at one frame, in document order,
    /// as `(target_id, value)` pairs.
    #[must_use]
    pub fn evaluate_all(&self, frame: i64) -> Vec<(String, Vec<f64>)> {
        self.channels
            .iter()
            .map(|channel| (channel.target_id.clone(), evaluate_channel(channel, frame)))
            .collect()
    }

    /// The loop-closure check (DESIGN.md section 3): for a
    /// looping clip, each channel's value at the loop start
    /// and at the loop end, compared after rotation sign
    /// alignment; the per-channel maximum absolute component
    /// difference. A closed loop reports all zeros. `None`
    /// for a clip that does not loop.
    #[must_use]
    #[allow(clippy::float_arithmetic)]
    pub fn loop_closure(&self) -> Option<Vec<(String, f64)>> {
        let (start, end) = self.loop_range?;
        let mut report = Vec::new();
        for channel in &self.channels {
            let a = evaluate_channel(channel, start);
            let mut b = evaluate_channel(channel, end);
            if channel.property == ClipProperty::Rotation && dot(&a, &b) < 0.0 {
                for component in &mut b {
                    *component = -*component;
                }
            }
            let mut worst = 0.0_f64;
            for (x, y) in a.iter().zip(b.iter()) {
                let diff = (x - y).abs();
                if diff > worst {
                    worst = diff;
                }
            }
            report.push((channel.target_id.clone(), worst));
        }
        Some(report)
    }

    /// The Tier 2 evaluation digest: SHA-256 over the
    /// per-tick gauge bit patterns of `ticks` consecutive
    /// witness ticks starting at `from_tick` (frames resolved
    /// through [`Clip::frame_at`]), in the
    /// [`crate::tier2::presentation_digest`] discipline. Two
    /// evaluators that disagree anywhere in the window digest
    /// differently; the golden tests pin this value, so a
    /// planted change to the evaluation order or the curve
    /// mathematics fails the digest even when every individual
    /// value still looks plausible.
    #[must_use]
    pub fn digest(&self, from_tick: u64, ticks: u64) -> String {
        let mut hasher = Sha256::new();
        for offset in 0..ticks {
            let frame = self.frame_at(from_tick + offset);
            for (_, value) in self.evaluate_all(frame) {
                for component in value {
                    hasher.update(component.to_bits().to_le_bytes());
                }
            }
        }
        let digest = hasher.finalize();
        let mut hex = String::with_capacity(64);
        for byte in digest {
            hex.push_str(&format!("{byte:02x}"));
        }
        hex
    }
}

fn parse_channel(raw: &Value) -> Result<ClipChannel, Refusal> {
    let target = field(raw, "target")?;
    let kind = field(target, "kind")?
        .as_str()
        .ok_or_else(|| malformed("channel target kind is not a string".to_string()))?;
    let name = field(target, "name")?
        .as_str()
        .ok_or_else(|| malformed("channel target name is not a string".to_string()))?;
    let property_name = field(target, "property")?
        .as_str()
        .ok_or_else(|| malformed("channel target property is not a string".to_string()))?;
    let property = match (kind, property_name) {
        ("joint", "translation") => ClipProperty::Translation,
        ("joint", "rotation") => ClipProperty::Rotation,
        ("joint", "scale") => ClipProperty::Scale,
        ("morph", "weight") => ClipProperty::Weight,
        ("parameter", "value") => ClipProperty::Value,
        _ => {
            return Err(malformed(format!(
                "channel target {kind}:{property_name} is not a clip property"
            )));
        }
    };
    let mut keys = Vec::new();
    for raw_key in field(raw, "keys")?
        .as_array()
        .ok_or_else(|| malformed("channel keys are not an array".to_string()))?
    {
        let interp = match field(raw_key, "interp")?.as_str() {
            Some("step") => ClipInterp::Step,
            Some("linear") => ClipInterp::Linear,
            Some("cubic") => ClipInterp::Cubic,
            _ => return Err(malformed("key interp is not a clip mode".to_string())),
        };
        keys.push(ClipKey {
            frame: field(raw_key, "frame")?
                .as_i64()
                .ok_or_else(|| malformed("key frame is not an integer".to_string()))?,
            value: number_vec(field(raw_key, "value")?)?,
            interp,
            in_tangent: optional_vec(raw_key, "in_tangent")?,
            out_tangent: optional_vec(raw_key, "out_tangent")?,
        });
    }
    Ok(ClipChannel {
        target_id: format!("{kind}:{name}:{property_name}"),
        property,
        keys,
    })
}

/// Evaluates one channel at one frame (DESIGN Appendix C).
/// Frames outside the key range clamp to the end keys; a
/// frame exactly on a key returns that key's value.
#[must_use]
#[allow(clippy::float_arithmetic)]
pub fn evaluate_channel(channel: &ClipChannel, frame: i64) -> Vec<f64> {
    let keys = &channel.keys;
    let first = &keys[0];
    if frame <= first.frame {
        return first.value.clone();
    }
    let last = &keys[keys.len() - 1];
    if frame >= last.frame {
        return last.value.clone();
    }
    let segment = keys.partition_point(|key| key.frame <= frame) - 1;
    let k0 = &keys[segment];
    if frame == k0.frame {
        // A frame exactly on a key returns the key's value
        // verbatim — keyframe exactness is by construction,
        // not by the spline happening to land on it (an
        // authored quaternion key is unit only to authoring
        // precision, and normalising it would move it).
        return k0.value.clone();
    }
    let k1 = &keys[segment + 1];
    let delta = (k1.frame - k0.frame) as f64;
    let t = (frame - k0.frame) as f64 / delta;
    match k0.interp {
        ClipInterp::Step => k0.value.clone(),
        ClipInterp::Linear => {
            if channel.property == ClipProperty::Rotation {
                slerp(&k0.value, &k1.value, t)
            } else {
                lerp(&k0.value, &k1.value, t)
            }
        }
        ClipInterp::Cubic => {
            let m0 = k0.out_tangent.as_deref().unwrap_or(&[]);
            let m1 = k1.in_tangent.as_deref().unwrap_or(&[]);
            let value = hermite(&k0.value, m0, &k1.value, m1, delta, t);
            if channel.property == ClipProperty::Rotation {
                normalize(&value)
            } else {
                value
            }
        }
    }
}

/// Componentwise linear interpolation.
#[allow(clippy::float_arithmetic)]
fn lerp(a: &[f64], b: &[f64], t: f64) -> Vec<f64> {
    a.iter()
        .zip(b.iter())
        .map(|(x, y)| x + (y - x) * t)
        .collect()
}

/// The dot product of two same-shaped vectors.
#[allow(clippy::float_arithmetic)]
fn dot(a: &[f64], b: &[f64]) -> f64 {
    a.iter().zip(b.iter()).map(|(x, y)| x * y).sum()
}

/// A vector scaled to unit length (the zero vector passes
/// through unchanged; validated rotation keys are never
/// zero, and spline outputs reach zero only degenerately).
#[allow(clippy::float_arithmetic)]
fn normalize(value: &[f64]) -> Vec<f64> {
    let length = dot(value, value).sqrt();
    if length == 0.0 {
        return value.to_vec();
    }
    value.iter().map(|component| component / length).collect()
}

/// Quaternion slerp, shortest arc (DESIGN Appendix C): the
/// second key is sign-corrected when the dot product is
/// negative; near-parallel keys take a normalised lerp; the
/// result is normalised.
#[allow(clippy::float_arithmetic)]
fn slerp(q0: &[f64], q1: &[f64], t: f64) -> Vec<f64> {
    let mut b = q1.to_vec();
    let mut cosine = dot(q0, &b);
    if cosine < 0.0 {
        for component in &mut b {
            *component = -*component;
        }
        cosine = -cosine;
    }
    let blended = if cosine > 0.9995 {
        lerp(q0, &b, t)
    } else {
        let theta = cosine.acos();
        let sine = theta.sin();
        let a_weight = ((1.0 - t) * theta).sin() / sine;
        let b_weight = (t * theta).sin() / sine;
        q0.iter()
            .zip(b.iter())
            .map(|(x, y)| a_weight * x + b_weight * y)
            .collect()
    };
    normalize(&blended)
}

/// The cubic Hermite spline over one segment: endpoint values
/// `p0`/`p1`, endpoint derivatives `m0`/`m1` (with respect to
/// frames), the segment length in frames `delta`, and the
/// segment parameter `t` in `0..=1`.
#[allow(clippy::float_arithmetic)]
fn hermite(p0: &[f64], m0: &[f64], p1: &[f64], m1: &[f64], delta: f64, t: f64) -> Vec<f64> {
    let t2 = t * t;
    let t3 = t2 * t;
    let h00 = 2.0 * t3 - 3.0 * t2 + 1.0;
    let h10 = t3 - 2.0 * t2 + t;
    let h01 = -2.0 * t3 + 3.0 * t2;
    let h11 = t3 - t2;
    p0.iter()
        .enumerate()
        .map(|(index, x)| {
            h00 * x + h10 * delta * m0[index] + h01 * p1[index] + h11 * delta * m1[index]
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rotation_channel(interp: ClipInterp, q1: [f64; 4]) -> ClipChannel {
        ClipChannel {
            target_id: "joint:Spine:rotation".to_string(),
            property: ClipProperty::Rotation,
            keys: vec![
                ClipKey {
                    frame: 0,
                    value: vec![0.0, 0.0, 0.0, 1.0],
                    interp,
                    in_tangent: None,
                    out_tangent: None,
                },
                ClipKey {
                    frame: 10,
                    value: q1.to_vec(),
                    interp: ClipInterp::Step,
                    in_tangent: None,
                    out_tangent: None,
                },
            ],
        }
    }

    #[test]
    fn slerp_midpoint_of_a_quarter_turn() {
        let channel = rotation_channel(ClipInterp::Linear, [0.0, 1.0, 0.0, 0.0]);
        let mid = evaluate_channel(&channel, 5);
        let half = std::f64::consts::FRAC_1_SQRT_2;
        assert!((mid[1] - half).abs() < 1e-12, "y: {}", mid[1]);
        assert!((mid[3] - half).abs() < 1e-12, "w: {}", mid[3]);
        assert!(mid[0].abs() < 1e-12 && mid[2].abs() < 1e-12);
    }

    #[test]
    fn slerp_sign_corrects_a_negated_key() {
        // The same rotation written with the opposite sign
        // must evaluate identically (shortest arc): the
        // negated key's dot product with the identity is
        // negative, so the correction fires. (A half-turn
        // target would be the degenerate case — both arcs
        // equally short — and is not this test.)
        let half = std::f64::consts::FRAC_1_SQRT_2;
        let plain = rotation_channel(ClipInterp::Linear, [0.0, half, 0.0, half]);
        let negated = rotation_channel(ClipInterp::Linear, [0.0, -half, 0.0, -half]);
        // Interior frames: identical after the correction.
        // (The final frame returns the authored key verbatim
        // in both channels — the same rotation under the
        // opposite sign, by the keyframe-exactness rule.)
        for frame in 0..10 {
            assert_eq!(
                evaluate_channel(&plain, frame),
                evaluate_channel(&negated, frame),
                "frame {frame}"
            );
        }
    }

    #[test]
    fn hermite_midpoint_matches_the_hand_computation() {
        // p0 = p1 = 0, m0 = 1, m1 = -1, delta = 10:
        // at t = 0.5, h10 = 0.125, h11 = -0.125, so the
        // value is 0.125*10*1 + (-0.125)*10*(-1) = 2.5.
        let value = hermite(&[0.0], &[1.0], &[0.0], &[-1.0], 10.0, 0.5);
        assert!((value[0] - 2.5).abs() < 1e-12, "got {}", value[0]);
    }
}
