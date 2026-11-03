//! Counters: the surface's loud failure mode. Every
//! datagram the transport touches is accounted —
//! delivered, sent, malformed, or rejected under a named
//! [`RejectKind`] — so an operator (or a test) can watch
//! misbehaving peers and full queues as numbers instead
//! of discovering them as silence. Counters are atomic:
//! the receive thread records without locking, and any
//! thread may take a [`StatsSnapshot`].

use std::sync::atomic::{AtomicU64, Ordering};

use crate::surface::RejectKind;

/// A plain-data copy of the counters at one instant.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StatsSnapshot {
    /// Datagrams the receive loop read off the socket.
    pub datagrams_received: u64,
    /// Events successfully placed on the bounded queue.
    pub events_delivered: u64,
    /// Events dropped because the queue was full
    /// ([`RejectKind::QueueFull`]).
    pub events_queue_dropped: u64,
    /// Datagrams the publisher sent successfully.
    pub messages_sent: u64,
    /// Datagrams that refused OSC decoding
    /// ([`RejectKind::MalformedPacket`]).
    pub packets_malformed: u64,
    /// Messages rejected as [`RejectKind::UndeclaredInput`].
    pub rejected_undeclared_input: u64,
    /// Messages rejected as [`RejectKind::UndeclaredParameter`].
    pub rejected_undeclared_parameter: u64,
    /// Messages rejected as [`RejectKind::UnknownAddress`].
    pub rejected_unknown_address: u64,
    /// Messages rejected as [`RejectKind::WrongArgCount`].
    pub rejected_wrong_arg_count: u64,
    /// Messages rejected as [`RejectKind::WrongDirection`].
    pub rejected_wrong_direction: u64,
    /// Messages rejected as [`RejectKind::WrongType`].
    pub rejected_wrong_type: u64,
    /// Sends that failed at the socket.
    pub send_failures: u64,
}

/// The live counters, shared between the receive thread
/// and the handle the app holds.
#[derive(Debug, Default)]
pub struct SurfaceStats {
    datagrams_received: AtomicU64,
    events_delivered: AtomicU64,
    events_queue_dropped: AtomicU64,
    messages_sent: AtomicU64,
    packets_malformed: AtomicU64,
    rejected_undeclared_input: AtomicU64,
    rejected_undeclared_parameter: AtomicU64,
    rejected_unknown_address: AtomicU64,
    rejected_wrong_arg_count: AtomicU64,
    rejected_wrong_direction: AtomicU64,
    rejected_wrong_type: AtomicU64,
    send_failures: AtomicU64,
}

impl SurfaceStats {
    /// All counters at zero.
    pub fn new() -> Self {
        Self::default()
    }

    /// Copies the counters into a plain snapshot.
    pub fn snapshot(&self) -> StatsSnapshot {
        StatsSnapshot {
            datagrams_received: self.datagrams_received.load(Ordering::Relaxed),
            events_delivered: self.events_delivered.load(Ordering::Relaxed),
            events_queue_dropped: self.events_queue_dropped.load(Ordering::Relaxed),
            messages_sent: self.messages_sent.load(Ordering::Relaxed),
            packets_malformed: self.packets_malformed.load(Ordering::Relaxed),
            rejected_undeclared_input: self.rejected_undeclared_input.load(Ordering::Relaxed),
            rejected_undeclared_parameter: self
                .rejected_undeclared_parameter
                .load(Ordering::Relaxed),
            rejected_unknown_address: self.rejected_unknown_address.load(Ordering::Relaxed),
            rejected_wrong_arg_count: self.rejected_wrong_arg_count.load(Ordering::Relaxed),
            rejected_wrong_direction: self.rejected_wrong_direction.load(Ordering::Relaxed),
            rejected_wrong_type: self.rejected_wrong_type.load(Ordering::Relaxed),
            send_failures: self.send_failures.load(Ordering::Relaxed),
        }
    }

    pub(crate) fn record_datagram(&self) {
        self.datagrams_received.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_delivered(&self) {
        self.events_delivered.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_reject(&self, kind: RejectKind) {
        let counter = match kind {
            RejectKind::MalformedPacket => &self.packets_malformed,
            RejectKind::QueueFull => &self.events_queue_dropped,
            RejectKind::UndeclaredInput => &self.rejected_undeclared_input,
            RejectKind::UndeclaredParameter => &self.rejected_undeclared_parameter,
            RejectKind::UnknownAddress => &self.rejected_unknown_address,
            RejectKind::WrongArgCount => &self.rejected_wrong_arg_count,
            RejectKind::WrongDirection => &self.rejected_wrong_direction,
            RejectKind::WrongType => &self.rejected_wrong_type,
        };
        counter.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_send_failure(&self) {
        self.send_failures.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_sent(&self) {
        self.messages_sent.fetch_add(1, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::SurfaceStats;
    use crate::surface::RejectKind;

    #[test]
    fn counters_start_at_zero_and_record_by_kind() {
        let stats = SurfaceStats::new();
        assert_eq!(stats.snapshot().datagrams_received, 0);
        stats.record_datagram();
        stats.record_datagram();
        stats.record_delivered();
        stats.record_reject(RejectKind::MalformedPacket);
        stats.record_reject(RejectKind::QueueFull);
        stats.record_reject(RejectKind::WrongType);
        stats.record_reject(RejectKind::WrongType);
        stats.record_sent();
        stats.record_send_failure();
        let snapshot = stats.snapshot();
        assert_eq!(snapshot.datagrams_received, 2);
        assert_eq!(snapshot.events_delivered, 1);
        assert_eq!(snapshot.events_queue_dropped, 1);
        assert_eq!(snapshot.packets_malformed, 1);
        assert_eq!(snapshot.rejected_wrong_type, 2);
        assert_eq!(snapshot.messages_sent, 1);
        assert_eq!(snapshot.send_failures, 1);
    }
}
