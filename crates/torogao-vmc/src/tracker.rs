//! The tracking driver: a bounded queue of received
//! frames, applied one per 60 fps tick, over a governed
//! state that only ever moves by governed values.
//!
//! Loss policy (the Stage 3.3 gate's rule): when the feed
//! stops, `tick` applies nothing and the governed state
//! holds exactly — tracking loss degrades to the last
//! governed state, never to an ungoverned pose, a reset,
//! or a default. When the feed outruns the tick, the queue
//! drops the *oldest* frame (replace-obsolete: a stale
//! pose is worthless to a live rig) and counts the drop.

use std::collections::{BTreeMap, VecDeque};

use crate::arkit::{GovernedValue, governed_values};
use crate::vmc::VmcFrame;

/// Largest number of unapplied frames the driver queues.
pub const TRACKING_QUEUE_CAPACITY: usize = 8;

/// One frame the driver applied, with its latency
/// accounting.
#[derive(Debug, Clone, PartialEq)]
pub struct AppliedFrame {
    /// The frame's stamp (the tick its sender produced it
    /// for, on the shared 60 fps clock).
    pub stamp: u64,
    /// `applied_tick - stamp`, in frames.
    pub latency_frames: u64,
    /// The governed values the frame carried.
    pub values: BTreeMap<String, GovernedValue>,
}

/// The tracking driver state.
#[derive(Debug, Default)]
pub struct TrackingDriver {
    queue: VecDeque<(u64, VmcFrame)>,
    state: BTreeMap<String, GovernedValue>,
    last_applied_tick: Option<u64>,
    dropped_frames: u64,
}

impl TrackingDriver {
    /// A fresh driver: empty queue, no governed state yet
    /// (the rig stands at its authored pose until the
    /// first frame lands).
    pub fn new() -> TrackingDriver {
        TrackingDriver::default()
    }

    /// Queues one received frame under its stamp. When the
    /// queue is full, the oldest queued frame is dropped
    /// and counted (see the module note).
    pub fn ingest(&mut self, stamp: u64, frame: VmcFrame) {
        if self.queue.len() >= TRACKING_QUEUE_CAPACITY {
            self.queue.pop_front();
            self.dropped_frames += 1;
        }
        self.queue.push_back((stamp, frame));
    }

    /// Applies the oldest queued frame, if any: converts
    /// it to governed values and merges them into the
    /// governed state. Returns `None` when the queue is
    /// empty — the state then holds, untouched.
    pub fn tick(&mut self, now: u64) -> Option<AppliedFrame> {
        let (stamp, frame) = self.queue.pop_front()?;
        let values = governed_values(&frame);
        for (name, value) in &values {
            self.state.entry(name.clone()).or_default().merge(*value);
        }
        self.last_applied_tick = Some(now);
        Some(AppliedFrame {
            stamp,
            latency_frames: now.saturating_sub(stamp),
            values,
        })
    }

    /// The current governed state (every parameter tracking
    /// has governed so far, at its last governed value).
    pub fn state(&self) -> &BTreeMap<String, GovernedValue> {
        &self.state
    }

    /// Whether the driver is holding: the queue is empty
    /// and no frame has been applied for at least
    /// `hold_after_frames` ticks (or ever).
    pub fn holding(&self, now: u64, hold_after_frames: u64) -> bool {
        if !self.queue.is_empty() {
            return false;
        }
        match self.last_applied_tick {
            Some(tick) => now.saturating_sub(tick) >= hold_after_frames,
            None => true,
        }
    }

    /// Frames dropped by the replace-obsolete queue policy.
    pub fn dropped_frames(&self) -> u64 {
        self.dropped_frames
    }

    /// Frames currently queued.
    pub fn queued(&self) -> usize {
        self.queue.len()
    }
}

#[cfg(test)]
mod tests {
    use super::TrackingDriver;
    use crate::vmc::VmcFrame;

    fn blink_frame(value: f32) -> VmcFrame {
        let mut frame = VmcFrame::default();
        frame.blends.push(("eyeBlinkLeft".to_string(), value));
        frame
    }

    #[test]
    fn state_holds_exactly_when_the_feed_stops() {
        let mut driver = TrackingDriver::new();
        driver.ingest(0, blink_frame(0.3));
        let applied = driver.tick(0).expect("frame applies");
        assert_eq!(applied.latency_frames, 0);
        let held = driver.state().clone();
        for now in 1..90 {
            assert!(driver.tick(now).is_none(), "nothing applies without a feed");
        }
        assert_eq!(driver.state(), &held, "the governed state never drifted");
        assert!(driver.holding(90, 30));
    }

    #[test]
    fn queue_drops_oldest_and_counts() {
        let mut driver = TrackingDriver::new();
        for stamp in 0..12 {
            driver.ingest(stamp, blink_frame(stamp as f32 / 100.0));
        }
        assert_eq!(driver.dropped_frames(), 4);
        assert_eq!(driver.queued(), super::TRACKING_QUEUE_CAPACITY);
        let first = driver.tick(12).expect("a frame applies");
        assert_eq!(first.stamp, 4, "the oldest surviving frame applies first");
    }

    #[test]
    fn latency_is_measured_against_the_stamp() {
        let mut driver = TrackingDriver::new();
        driver.ingest(10, blink_frame(0.5));
        let applied = driver.tick(12).expect("frame applies");
        assert_eq!(applied.latency_frames, 2);
        assert_eq!(
            applied.values["Eye:: Left:: Blink"].x,
            Some(0.5),
            "the applied values ride along"
        );
    }
}
