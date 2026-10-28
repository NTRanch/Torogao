//! Presentation frame-time recording for the performance
//! smoke (BRIEF-A1 §Scope 6 / gate B8).
//!
//! The shell pushes one sample per presented frame
//! (milliseconds, measured around its own redraw: advance,
//! viewport refresh, egui frame, surface present
//! submission). The buffer is bounded; the summary is
//! computed over what it holds. Readings are *smoke-class*
//! evidence wherever they were taken (emulator or device):
//! the run target is named in the record the shell writes,
//! never implied by the numbers.

use std::collections::VecDeque;

/// The sample buffer's capacity (frames). At 60 fps this
/// holds ~68 seconds of presentation; the smoke's summary
/// window is the most recent stretch, which is what a
/// sustained-reading wants.
pub const SAMPLES_MAX: usize = 4096;

/// A bounded ring of frame-time samples (milliseconds).
#[derive(Debug, Default)]
pub struct FrameTimes {
    /// The samples, oldest first.
    samples: VecDeque<f32>,
}

impl FrameTimes {
    /// An empty recorder.
    pub fn new() -> FrameTimes {
        FrameTimes {
            samples: VecDeque::new(),
        }
    }

    /// Records one frame's presentation time. Non-finite
    /// or negative samples are refused (a broken clock
    /// reading must not poison the summary).
    pub fn push(&mut self, frame_ms: f32) {
        if !frame_ms.is_finite() || frame_ms < 0.0 {
            return;
        }
        if self.samples.len() >= SAMPLES_MAX {
            self.samples.pop_front();
        }
        self.samples.push_back(frame_ms);
    }

    /// The number of samples held.
    pub fn len(&self) -> usize {
        self.samples.len()
    }

    /// Whether no sample has been recorded.
    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    /// The summary over the held samples; `None` when
    /// empty (an empty smoke is a missing smoke, not a
    /// zero reading).
    pub fn summary(&self) -> Option<PerfSummary> {
        if self.samples.is_empty() {
            return None;
        }
        let mut sorted: Vec<f32> = self.samples.iter().copied().collect();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let count = sorted.len();
        let mean = sorted.iter().sum::<f32>() / count as f32;
        let p95 = sorted[((count as f32 * 0.95).ceil() as usize).clamp(1, count) - 1];
        let max = sorted[count - 1];
        Some(PerfSummary {
            frames: count,
            mean_ms: mean,
            p95_ms: p95,
            max_ms: max,
        })
    }
}

/// One performance summary (milliseconds).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PerfSummary {
    /// Frames in the summary window.
    pub frames: usize,
    /// Mean frame time.
    pub mean_ms: f32,
    /// 95th-percentile frame time.
    pub p95_ms: f32,
    /// Worst frame time.
    pub max_ms: f32,
}

impl PerfSummary {
    /// The summary as a JSON record, with the run target
    /// and animation named by the caller (evidence honesty:
    /// the numbers never travel without their context).
    pub fn to_json(&self, run_target: &str, animation: &str) -> serde_json::Value {
        serde_json::json!({
            "kind": "torogao-android-perf-smoke/1",
            "run_target": run_target,
            "animation": animation,
            "frames": self.frames,
            "mean_ms": self.mean_ms,
            "p95_ms": self.p95_ms,
            "max_ms": self.max_ms,
            "budget_ms": 16.7,
            "evidence_class": "smoke — emulator readings do not close A-G6",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{FrameTimes, SAMPLES_MAX};

    /// The summary's statistics are the samples' own.
    #[test]
    fn summary_matches_the_samples() {
        let mut times = FrameTimes::new();
        assert!(times.summary().is_none());
        for sample in [10.0, 20.0, 30.0, 40.0] {
            times.push(sample);
        }
        let summary = times.summary().expect("summary exists");
        assert_eq!(summary.frames, 4);
        assert_eq!(summary.mean_ms, 25.0);
        assert_eq!(summary.max_ms, 40.0);
        assert_eq!(summary.p95_ms, 40.0);
    }

    /// The buffer is bounded and refuses broken readings.
    #[test]
    fn buffer_is_bounded_and_sane() {
        let mut times = FrameTimes::new();
        times.push(f32::NAN);
        times.push(-1.0);
        times.push(f32::INFINITY);
        assert!(times.is_empty());
        for _ in 0..(SAMPLES_MAX + 10) {
            times.push(16.0);
        }
        assert_eq!(times.len(), SAMPLES_MAX);
    }

    /// Adversarial (cure round): a NaN storm — broken
    /// clock readings interleaved with valid frames, in
    /// volume — can neither poison the summary nor evict
    /// valid samples: refused samples never enter the
    /// buffer, so they hold no capacity.
    #[test]
    fn nan_storm_cannot_poison_or_evict() {
        let mut times = FrameTimes::new();
        for _ in 0..10_000 {
            times.push(f32::NAN);
            times.push(f32::INFINITY);
            times.push(f32::NEG_INFINITY);
            times.push(-0.5);
        }
        assert!(times.is_empty());
        assert!(times.summary().is_none());
        for _ in 0..100 {
            times.push(20.0);
            times.push(f32::NAN);
        }
        let summary = times.summary().expect("summary over the valid frames");
        assert_eq!(summary.frames, 100);
        assert_eq!(summary.mean_ms, 20.0);
        assert!(summary.mean_ms.is_finite() && summary.max_ms.is_finite());
    }
}
