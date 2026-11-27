//! The clip witness core (hand-animation programme Rung
//! H1): the first evaluator-bearing Tier 2 witness. It hosts
//! one frozen `torogao-motion-clip/2` document — the H0
//! golden `loop-hold`, committed in this crate at
//! `corpus/clip-witness.clip.json` and pinned byte-identical
//! to the format crate's golden by test — and presents its
//! evaluated channels as Tier 2 gauges.
//!
//! The witness's Tier 1 state is its tick and nothing else:
//! an integer. Its hash chain, saves, and replay therefore
//! stay Tier 1 byte-identical on every target, while its
//! presentation carries the clip evaluation — slerp and
//! Hermite splines over authored curve data — into the twin
//! gate's Tier 2 band comparison. Commands mean nothing to a
//! clip being witnessed; every command is ignored, one
//! verdict per command, and the tick advances.

use crate::clip_eval::Clip;
use crate::command::Command;
use crate::core::{GameCore, SaveBlob, StepReport, Verdict};
use crate::hash::Hash;
use crate::present::{GaugeView, MeterView, PresentationState};
use serde_json::{Value, json};
use torogao_author::{Refusal, RefusalClass};

/// The witness's core id (the corpus document of the same
/// name binds it).
pub const CLIP_WITNESS_CORE_ID: &str = "clip-witness:loop-hold";

/// The hosted clip document: the H0 golden, byte for byte.
const GOLDEN_CLIP_TEXT: &str = include_str!("../../corpus/clip-witness.clip.json");

/// A [`GameCore`] hosting one frozen clip for evaluation.
pub struct ClipWitnessCore {
    clip: Clip,
    tick: u64,
}

impl ClipWitnessCore {
    /// The witness over the committed golden clip.
    ///
    /// # Errors
    ///
    /// Returns a typed refusal when the committed document
    /// fails validation (a gate failure, by construction).
    pub fn golden() -> Result<Self, Refusal> {
        Self::from_clip_text(GOLDEN_CLIP_TEXT)
    }

    /// A witness over any clip document text.
    ///
    /// # Errors
    ///
    /// Returns the evaluator's refusal for an invalid
    /// document.
    pub fn from_clip_text(text: &str) -> Result<Self, Refusal> {
        Ok(Self {
            clip: Clip::from_text(text)?,
            tick: 0,
        })
    }

    /// The hosted clip.
    #[must_use]
    pub fn clip(&self) -> &Clip {
        &self.clip
    }

    fn state_document(&self) -> Value {
        json!({
            "clip": self.clip.freeze_sha256,
            "tick": self.tick,
        })
    }
}

impl GameCore for ClipWitnessCore {
    fn core_id(&self) -> &str {
        CLIP_WITNESS_CORE_ID
    }

    fn tick(&self) -> u64 {
        self.tick
    }

    fn step(&mut self, commands: &[Command]) -> StepReport {
        self.tick += 1;
        StepReport {
            tick: self.tick,
            verdicts: commands
                .iter()
                .map(|_| Verdict::Ignored {
                    reason: "the clip witness consumes no commands".to_string(),
                })
                .collect(),
            events: Vec::new(),
        }
    }

    fn state_hash(&self) -> Hash {
        let text = torogao_format::jsonfmt::to_d_json(&self.state_document());
        Hash::sha256(text.as_bytes())
    }

    fn save(&self) -> Result<SaveBlob, Refusal> {
        Ok(SaveBlob(torogao_format::jsonfmt::to_d_json(
            &self.state_document(),
        )))
    }

    fn load(&mut self, blob: &SaveBlob) -> Result<(), Refusal> {
        let document: Value = serde_json::from_str(blob.as_str()).map_err(|error| {
            Refusal::new(
                RefusalClass::Malformed,
                format!("clip witness save does not parse: {error}"),
            )
        })?;
        let saved_clip = document.get("clip").and_then(Value::as_str);
        if saved_clip != self.clip.freeze_sha256.as_deref() {
            return Err(Refusal::new(
                RefusalClass::Conflict,
                "the save binds a different clip than this witness hosts".to_string(),
            ));
        }
        let tick = document
            .get("tick")
            .and_then(Value::as_u64)
            .ok_or_else(|| {
                Refusal::new(
                    RefusalClass::Malformed,
                    "clip witness save carries no tick".to_string(),
                )
            })?;
        self.tick = tick;
        Ok(())
    }

    fn present(&self) -> PresentationState {
        let frame = self.clip.frame_at(self.tick);
        let mut gauges = Vec::new();
        for (target_id, value) in self.clip.evaluate_all(frame) {
            for (index, component) in value.into_iter().enumerate() {
                gauges.push(GaugeView {
                    id: format!("{target_id}.{index}"),
                    fraction: component,
                });
            }
        }
        PresentationState {
            tick: self.tick,
            headline: self.clip.name.clone(),
            detail: Some(format!(
                "frame {} of {}..={}",
                frame, self.clip.range.0, self.clip.range.1
            )),
            meters: vec![
                MeterView {
                    id: "tick".to_string(),
                    value: i64::try_from(self.tick).unwrap_or(i64::MAX),
                },
                MeterView {
                    id: "frame".to_string(),
                    value: frame,
                },
            ],
            gauges,
        }
    }
}
