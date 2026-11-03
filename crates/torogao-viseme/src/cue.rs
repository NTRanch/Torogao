//! Rhubarb cue files: the import contract of this crate.
//!
//! Rhubarb Lip Sync (the authoring-time oracle, see the
//! crate README) emits timed mouth-shape cues in JSON or
//! TSV form. This module parses both into one validated
//! `Cue` list. Validation is total: a cue list that parses
//! is sorted by start time, non-overlapping, finite, and
//! non-empty — or the parse fails with a typed error. An
//! unknown shape token is a loud failure
//! (`TV-CUE-UNMAPPED-SHAPE`), never a silent substitution:
//! the mapping table in `map` is the only authority on what
//! a shape means.

use crate::error::{VisemeError, VisemeResult};
use serde::Deserialize;

/// Largest cue file accepted, in bytes. A cue file is a
/// text artefact of timed shapes; 16 MiB is far beyond any
/// real recording's cue sheet and bounds parse memory.
pub const INPUT_BYTES_MAX: usize = 16 * 1024 * 1024;

/// Most cues accepted in one file. At Rhubarb's minimum
/// cue granularity this still covers hours of speech.
pub const CUES_MAX: usize = 100_000;

/// Longest cue timeline accepted, in seconds (ten hours).
/// Bounds the baked frame count downstream.
pub const TIMELINE_SECONDS_MAX: f64 = 36_000.0;

/// One Rhubarb mouth shape: the basic set A–F, the
/// extended set G and H, and the rest shape X. The
/// semantics are Rhubarb's own (its README's mouth-shape
/// table); `map` assigns each shape its Fac_ weights.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Shape {
    /// Closed mouth, slight lip pressure (P, B, M).
    A,
    /// Slightly open, clenched teeth (K, S, T; EE).
    B,
    /// Open mouth (EH, AE); in-between toward D.
    C,
    /// Wide open mouth (AA).
    D,
    /// Slightly rounded mouth (AO, ER); in-between to F.
    E,
    /// Puckered lips (UW, OW, W).
    F,
    /// Upper teeth touching the lower lip (F, V).
    G,
    /// Tongue raised behind the upper teeth (L).
    H,
    /// Idle position: lips closed and relaxed (pauses).
    X,
}

impl Shape {
    /// Every shape, in token order. The mapping table must
    /// carry a row for each of these.
    pub const ALL: [Shape; 9] = [
        Shape::A,
        Shape::B,
        Shape::C,
        Shape::D,
        Shape::E,
        Shape::F,
        Shape::G,
        Shape::H,
        Shape::X,
    ];

    /// Parses one shape token. Anything outside A–H and X
    /// is rejected: an unmapped input is a loud error, not
    /// a default.
    pub fn from_token(token: &str) -> VisemeResult<Shape> {
        match token {
            "A" => Ok(Shape::A),
            "B" => Ok(Shape::B),
            "C" => Ok(Shape::C),
            "D" => Ok(Shape::D),
            "E" => Ok(Shape::E),
            "F" => Ok(Shape::F),
            "G" => Ok(Shape::G),
            "H" => Ok(Shape::H),
            "X" => Ok(Shape::X),
            other => Err(VisemeError::new(
                "TV-CUE-UNMAPPED-SHAPE",
                format!("mouth shape {other:?} is not one of A-H or X"),
            )),
        }
    }

    /// The shape's token, as Rhubarb writes it.
    pub fn token(self) -> &'static str {
        match self {
            Shape::A => "A",
            Shape::B => "B",
            Shape::C => "C",
            Shape::D => "D",
            Shape::E => "E",
            Shape::F => "F",
            Shape::G => "G",
            Shape::H => "H",
            Shape::X => "X",
        }
    }
}

/// One timed mouth-shape cue, in seconds. Invariant
/// (enforced by `validate_cues`): `0 <= start < end`,
/// both finite.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Cue {
    /// Cue start, in seconds from the recording's start.
    pub start_seconds: f64,
    /// Cue end, in seconds; the cue covers
    /// `[start_seconds, end_seconds)`.
    pub end_seconds: f64,
    /// The mouth shape held over the cue.
    pub shape: Shape,
}

#[derive(Deserialize)]
struct RhubarbJson {
    #[serde(rename = "mouthCues")]
    mouth_cues: Vec<RhubarbCue>,
}

#[derive(Deserialize)]
struct RhubarbCue {
    start: f64,
    end: f64,
    value: String,
}

fn check_input_bounds(bytes: &[u8]) -> VisemeResult<()> {
    if bytes.len() > INPUT_BYTES_MAX {
        return Err(VisemeError::new(
            "TV-INPUT-TOO-LARGE",
            format!(
                "cue file is {} bytes; the limit is {INPUT_BYTES_MAX}",
                bytes.len()
            ),
        ));
    }
    Ok(())
}

/// Parses a Rhubarb JSON cue file (`mouthCues` with
/// `start`/`end`/`value` entries, as written by
/// `rhubarb -f json`) and validates the cue list.
pub fn parse_cues_json(bytes: &[u8]) -> VisemeResult<Vec<Cue>> {
    check_input_bounds(bytes)?;
    let parsed: RhubarbJson = serde_json::from_slice(bytes).map_err(|err| {
        VisemeError::new(
            "TV-CUE-MALFORMED",
            format!("not a Rhubarb JSON cue file: {err}"),
        )
    })?;
    let mut cues = Vec::with_capacity(parsed.mouth_cues.len());
    for raw in parsed.mouth_cues {
        cues.push(Cue {
            start_seconds: raw.start,
            end_seconds: raw.end,
            shape: Shape::from_token(&raw.value)?,
        });
    }
    validate_cues(&cues)?;
    Ok(cues)
}

/// Parses a Rhubarb TSV cue file. Each line is
/// `<start-seconds>\t<shape>`; a cue runs from its own
/// timestamp to the next line's. The final line is
/// Rhubarb's end marker (its timestamp is the recording's
/// end and its span is empty), so it closes the last real
/// cue and contributes no segment of its own.
pub fn parse_cues_tsv(bytes: &[u8]) -> VisemeResult<Vec<Cue>> {
    check_input_bounds(bytes)?;
    let text = std::str::from_utf8(bytes).map_err(|err| {
        VisemeError::new("TV-CUE-MALFORMED", format!("cue file is not UTF-8: {err}"))
    })?;
    let mut marks: Vec<(f64, Shape)> = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let line = line.trim_end_matches('\r');
        if line.is_empty() {
            continue;
        }
        let (stamp, token) = line.split_once('\t').ok_or_else(|| {
            VisemeError::new(
                "TV-CUE-MALFORMED",
                format!("TSV line {} has no tab separator", index + 1),
            )
        })?;
        let start: f64 = stamp.trim().parse().map_err(|_| {
            VisemeError::new(
                "TV-CUE-MALFORMED",
                format!(
                    "TSV line {} has an unparseable timestamp {stamp:?}",
                    index + 1
                ),
            )
        })?;
        marks.push((start, Shape::from_token(token.trim())?));
    }
    if marks.len() < 2 {
        return Err(VisemeError::new(
            "TV-CUE-EMPTY",
            "TSV cue file needs at least one cue line and its end marker",
        ));
    }
    let mut cues = Vec::with_capacity(marks.len() - 1);
    for pair in marks.windows(2) {
        cues.push(Cue {
            start_seconds: pair[0].0,
            end_seconds: pair[1].0,
            shape: pair[0].1,
        });
    }
    validate_cues(&cues)?;
    Ok(cues)
}

/// Validates a parsed cue list: non-empty, within count
/// and timeline bounds, every cue finite with
/// `0 <= start < end`, sorted by start, and
/// non-overlapping (gaps between cues are legal — the
/// bake's rest law covers them with zero weights).
pub fn validate_cues(cues: &[Cue]) -> VisemeResult<()> {
    if cues.is_empty() {
        return Err(VisemeError::new("TV-CUE-EMPTY", "cue list is empty"));
    }
    if cues.len() > CUES_MAX {
        return Err(VisemeError::new(
            "TV-CUE-TOO-MANY",
            format!("{} cues; the limit is {CUES_MAX}", cues.len()),
        ));
    }
    let mut previous: Option<&Cue> = None;
    for cue in cues {
        if !cue.start_seconds.is_finite() || !cue.end_seconds.is_finite() {
            return Err(VisemeError::new(
                "TV-CUE-MALFORMED",
                format!(
                    "cue {:?} has a non-finite time ({}..{})",
                    cue.shape.token(),
                    cue.start_seconds,
                    cue.end_seconds
                ),
            ));
        }
        if cue.start_seconds < 0.0 || cue.end_seconds <= cue.start_seconds {
            return Err(VisemeError::new(
                "TV-CUE-MALFORMED",
                format!(
                    "cue {:?} must satisfy 0 <= start < end, got {}..{}",
                    cue.shape.token(),
                    cue.start_seconds,
                    cue.end_seconds
                ),
            ));
        }
        if cue.end_seconds > TIMELINE_SECONDS_MAX {
            return Err(VisemeError::new(
                "TV-CUE-MALFORMED",
                format!(
                    "cue ends at {}s; the timeline limit is {TIMELINE_SECONDS_MAX}s",
                    cue.end_seconds
                ),
            ));
        }
        if let Some(prev) = previous {
            if cue.start_seconds < prev.start_seconds {
                return Err(VisemeError::new(
                    "TV-CUE-MALFORMED",
                    "cues are not sorted by start time",
                ));
            }
            if cue.start_seconds < prev.end_seconds {
                return Err(VisemeError::new(
                    "TV-CUE-OVERLAP",
                    format!(
                        "cue {:?} starts at {}s before the previous cue ends at {}s",
                        cue.shape.token(),
                        cue.start_seconds,
                        prev.end_seconds
                    ),
                ));
            }
        }
        previous = Some(cue);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{Cue, Shape, parse_cues_json, parse_cues_tsv, validate_cues};

    fn cue(start: f64, end: f64, shape: Shape) -> Cue {
        Cue {
            start_seconds: start,
            end_seconds: end,
            shape,
        }
    }

    #[test]
    fn json_parse_reads_rhubarb_shape() {
        let bytes = br#"{"metadata":{"soundFile":"x.wav","duration":1.0},
            "mouthCues":[{"start":0.0,"end":0.5,"value":"X"},
                         {"start":0.5,"end":1.0,"value":"D"}]}"#;
        let cues = parse_cues_json(bytes).expect("valid cue file");
        assert_eq!(cues.len(), 2);
        assert_eq!(cues[1].shape, Shape::D);
        assert_eq!(cues[1].start_seconds, 0.5);
    }

    #[test]
    fn tsv_parse_closes_last_cue_at_end_marker() {
        let cues = parse_cues_tsv(b"0.00\tX\n0.50\tD\n1.00\tX\n").expect("valid cue file");
        assert_eq!(cues.len(), 2);
        assert_eq!(cues[1], cue(0.5, 1.0, Shape::D));
    }

    #[test]
    fn unknown_shape_is_a_loud_error() {
        let bytes = br#"{"mouthCues":[{"start":0.0,"end":0.5,"value":"Q"}]}"#;
        let err = parse_cues_json(bytes).expect_err("Q is not a shape");
        assert_eq!(err.code(), "TV-CUE-UNMAPPED-SHAPE");
    }

    #[test]
    fn overlapping_cues_are_rejected() {
        let cues = [cue(0.0, 0.6, Shape::X), cue(0.5, 1.0, Shape::D)];
        let err = validate_cues(&cues).expect_err("overlap must fail");
        assert_eq!(err.code(), "TV-CUE-OVERLAP");
    }

    #[test]
    fn nan_tsv_timestamp_is_rejected() {
        let err = parse_cues_tsv(b"0.00\tX\nNaN\tD\n1.00\tX\n").expect_err("NaN must fail");
        assert_eq!(err.code(), "TV-CUE-MALFORMED");
    }

    #[test]
    fn empty_and_degenerate_cues_are_rejected() {
        assert_eq!(
            validate_cues(&[]).expect_err("empty").code(),
            "TV-CUE-EMPTY"
        );
        assert_eq!(
            validate_cues(&[cue(0.5, 0.5, Shape::A)])
                .expect_err("zero-length")
                .code(),
            "TV-CUE-MALFORMED"
        );
        assert_eq!(
            validate_cues(&[cue(-0.1, 0.5, Shape::A)])
                .expect_err("negative")
                .code(),
            "TV-CUE-MALFORMED"
        );
    }
}
