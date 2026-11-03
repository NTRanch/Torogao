//! UDP transport: a listener thread and a publisher,
//! both localhost-first.
//!
//! Network discipline: the bind address is explicit
//! configuration whose default is `127.0.0.1` (the
//! dialect is a localhost protocol — VRChat's own OSC
//! never leaves the machine). Binding a wildcard or LAN
//! address is possible only by writing that address into
//! the configuration deliberately; there is no boolean
//! that widens the bind behind the caller's back. Ports
//! default to the VRChat convention — receive on 9000,
//! send to 9001 — and are configurable.
//!
//! Robustness: the receive loop reads into a fixed
//! buffer sized [`codec::OSC_PACKET_BYTES_MAX`], decodes
//! strictly, routes, and offers the event to a bounded
//! queue ([`EVENT_QUEUE_CAPACITY`]). A malformed or
//! oversized datagram is dropped and counted; a full
//! queue drops and counts; neither blocks the loop, and
//! no wire content can panic it. Socket errors other
//! than the poll timeout back off for one poll interval
//! and the loop continues until shutdown is signalled.

use std::fmt;
use std::io;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::thread::JoinHandle;
use std::time::Duration;

use crate::codec::{self, OscArg, OscMessage};
use crate::dialect;
use crate::stats::{StatsSnapshot, SurfaceStats};
use crate::surface::{InputValue, OscSink, ParamValue, RejectKind, Surface, SurfaceEvent};

/// Capacity of the bounded queue between the receive
/// loop and the sink. When it fills, new events drop and
/// count ([`RejectKind::QueueFull`]) — the loop never
/// blocks behind a slow consumer.
pub const EVENT_QUEUE_CAPACITY: usize = 256;

/// The default receive-poll interval: how often the
/// receive loop wakes to check the shutdown flag.
pub const RECEIVE_POLL_DEFAULT: Duration = Duration::from_millis(50);

/// The default receive port — the VRChat convention.
pub const RECEIVE_PORT_DEFAULT: u16 = 9000;

/// The default send port — the VRChat convention.
pub const SEND_PORT_DEFAULT: u16 = 9001;

/// Transport configuration: explicit addresses, always.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OscConfig {
    /// The address the listener binds. Default
    /// `127.0.0.1:9000`. Writing a non-loopback address
    /// here is the deliberate act that widens the bind.
    pub bind_addr: SocketAddr,
    /// The address the publisher sends to. Default
    /// `127.0.0.1:9001`.
    pub peer_addr: SocketAddr,
    /// The receive-poll interval (shutdown latency is
    /// bounded by it).
    pub receive_poll: Duration,
}

impl Default for OscConfig {
    fn default() -> Self {
        OscConfig {
            bind_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), RECEIVE_PORT_DEFAULT),
            peer_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), SEND_PORT_DEFAULT),
            receive_poll: RECEIVE_POLL_DEFAULT,
        }
    }
}

impl OscConfig {
    /// Loopback configuration with explicit ports (a
    /// port of 0 asks the OS for an ephemeral bind).
    pub fn loopback(receive_port: u16, send_port: u16) -> Self {
        OscConfig {
            bind_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), receive_port),
            peer_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), send_port),
            receive_poll: RECEIVE_POLL_DEFAULT,
        }
    }
}

/// Why a transport operation failed.
#[derive(Debug)]
pub enum TransportError {
    /// A socket operation failed.
    Io(String),
}

impl fmt::Display for TransportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TransportError::Io(message) => write!(f, "UDP transport failed: {message}"),
        }
    }
}

impl std::error::Error for TransportError {}

impl From<io::Error> for TransportError {
    fn from(err: io::Error) -> TransportError {
        TransportError::Io(err.to_string())
    }
}

/// Why a publication refused or failed.
#[derive(Debug)]
pub enum PublishError {
    /// The message refused codec encoding.
    Codec(codec::OscError),
    /// The socket send failed.
    Io(String),
    /// The surface rejected the publication.
    Rejected(RejectKind),
}

impl fmt::Display for PublishError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PublishError::Codec(err) => write!(f, "OSC encode failed: {err}"),
            PublishError::Io(message) => write!(f, "UDP send failed: {message}"),
            PublishError::Rejected(kind) => write!(f, "publication rejected: {kind}"),
        }
    }
}

impl std::error::Error for PublishError {}

/// The listening half: owns the receive thread, the
/// bounded event queue, and the counters.
pub struct Listener {
    events: mpsc::Receiver<SurfaceEvent>,
    local_addr: SocketAddr,
    stats: Arc<SurfaceStats>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Listener {
    /// Binds the configured address and starts the
    /// receive thread. Fails with the socket error if
    /// the bind fails (address in use is the common
    /// case); no thread exists until this returns Ok.
    pub fn spawn(surface: Surface, config: OscConfig) -> Result<Listener, TransportError> {
        let socket = UdpSocket::bind(config.bind_addr)?;
        socket.set_read_timeout(Some(config.receive_poll))?;
        let local_addr = socket.local_addr()?;
        let stats = Arc::new(SurfaceStats::new());
        let stop = Arc::new(AtomicBool::new(false));
        let (sender, events) = mpsc::sync_channel(EVENT_QUEUE_CAPACITY);
        let thread = {
            let stats = Arc::clone(&stats);
            let stop = Arc::clone(&stop);
            std::thread::spawn(move || {
                receive_loop(
                    &socket,
                    &surface,
                    &sender,
                    &stats,
                    &stop,
                    config.receive_poll,
                );
            })
        };
        Ok(Listener {
            events,
            local_addr,
            stats,
            stop,
            thread: Some(thread),
        })
    }

    /// The address the socket actually bound (with an
    /// ephemeral configured port, the OS-chosen one).
    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    /// Drains up to `event_count_max` queued events into
    /// the sink, in arrival order; returns how many were
    /// delivered. Non-blocking: an empty queue returns 0.
    pub fn poll_events(&self, sink: &mut impl OscSink, event_count_max: usize) -> usize {
        let mut delivered = 0_usize;
        while delivered < event_count_max {
            match self.events.try_recv() {
                Ok(event) => {
                    sink.on_event(event);
                    delivered += 1;
                }
                Err(_) => break,
            }
        }
        delivered
    }

    /// The counters as of now.
    pub fn stats(&self) -> StatsSnapshot {
        self.stats.snapshot()
    }

    /// Signals shutdown and joins the receive thread.
    /// The thread exits within one poll interval.
    pub fn shutdown(mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        // The loop checks the flag every poll interval,
        // so a forgotten shutdown still ends the thread.
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// The receive loop: read, count, decode, route, offer.
/// Runs until the stop flag is set or the queue's
/// receiving half is gone.
fn receive_loop(
    socket: &UdpSocket,
    surface: &Surface,
    sender: &mpsc::SyncSender<SurfaceEvent>,
    stats: &SurfaceStats,
    stop: &AtomicBool,
    receive_poll: Duration,
) {
    let mut buffer = vec![0_u8; codec::OSC_PACKET_BYTES_MAX];
    while !stop.load(Ordering::Relaxed) {
        match socket.recv(&mut buffer) {
            Ok(len) => {
                stats.record_datagram();
                let outcome = codec::decode(&buffer[..len])
                    .map_err(|_| RejectKind::MalformedPacket)
                    .and_then(|message| surface.route(&message));
                match outcome {
                    Ok(event) => match sender.try_send(event) {
                        Ok(()) => stats.record_delivered(),
                        Err(mpsc::TrySendError::Full(_)) => {
                            stats.record_reject(RejectKind::QueueFull);
                        }
                        Err(mpsc::TrySendError::Disconnected(_)) => return,
                    },
                    Err(kind) => stats.record_reject(kind),
                }
            }
            Err(err)
                if err.kind() == io::ErrorKind::WouldBlock
                    || err.kind() == io::ErrorKind::TimedOut =>
            {
                // The poll interval elapsed quietly: the
                // normal case, loop back to the stop flag.
            }
            Err(_) => {
                // A persistent socket error must not spin:
                // back off one poll interval and continue;
                // shutdown still ends the loop.
                std::thread::sleep(receive_poll);
            }
        }
    }
}

/// The publishing half: owns a send socket and validates
/// every publication against the surface before it
/// reaches the wire.
pub struct Publisher {
    peer_addr: SocketAddr,
    socket: UdpSocket,
    stats: Arc<SurfaceStats>,
    surface: Surface,
}

impl Publisher {
    /// Prepares a publisher. The send socket binds an
    /// ephemeral loopback port (peers in this dialect do
    /// not authenticate by source port); publications go
    /// to the configured peer address.
    pub fn bind(surface: Surface, config: OscConfig) -> Result<Publisher, TransportError> {
        let bind = SocketAddr::new(config.bind_addr.ip(), 0);
        let socket = UdpSocket::bind(bind)?;
        Ok(Publisher {
            peer_addr: config.peer_addr,
            socket,
            stats: Arc::new(SurfaceStats::new()),
            surface,
        })
    }

    /// Sends `/avatar/change` with the avatar's ID.
    pub fn publish_avatar_change(&self, avatar_id: &str) -> Result<(), PublishError> {
        self.send(&OscMessage {
            address: dialect::AVATAR_CHANGE_ADDRESS.to_string(),
            args: vec![OscArg::Str(avatar_id.to_string())],
        })
    }

    /// Sends one input value: axes as a float argument,
    /// buttons as the dialect's int 1/0.
    pub fn publish_input(&self, name: &str, value: InputValue) -> Result<(), PublishError> {
        self.surface
            .check_publish_input(name, value)
            .map_err(PublishError::Rejected)?;
        let arg = match value {
            InputValue::Axis(position) => OscArg::Float(position),
            InputValue::Button(pressed) => OscArg::Int(i32::from(pressed)),
        };
        self.send(&OscMessage {
            address: dialect::input_address(name),
            args: vec![arg],
        })
    }

    /// Sends one parameter value under its dialect
    /// address, after the surface's publish checks.
    pub fn publish_parameter(&self, name: &str, value: ParamValue) -> Result<(), PublishError> {
        self.surface
            .check_publish(name, value)
            .map_err(PublishError::Rejected)?;
        let arg = match value {
            ParamValue::Bool(value) => OscArg::Bool(value),
            ParamValue::Float(value) => OscArg::Float(value),
            ParamValue::Int(value) => OscArg::Int(value),
        };
        self.send(&OscMessage {
            address: dialect::parameter_address(name),
            args: vec![arg],
        })
    }

    /// The counters as of now.
    pub fn stats(&self) -> StatsSnapshot {
        self.stats.snapshot()
    }

    fn send(&self, message: &OscMessage) -> Result<(), PublishError> {
        let packet = codec::encode(message).map_err(PublishError::Codec)?;
        match self.socket.send_to(&packet, self.peer_addr) {
            Ok(_) => {
                self.stats.record_sent();
                Ok(())
            }
            Err(err) => {
                self.stats.record_send_failure();
                Err(PublishError::Io(err.to_string()))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{OscConfig, RECEIVE_PORT_DEFAULT, SEND_PORT_DEFAULT};
    use std::net::{IpAddr, Ipv4Addr};

    #[test]
    fn default_config_is_the_vrchat_seat_on_loopback() {
        let config = OscConfig::default();
        assert_eq!(config.bind_addr.ip(), IpAddr::V4(Ipv4Addr::LOCALHOST));
        assert_eq!(config.bind_addr.port(), RECEIVE_PORT_DEFAULT);
        assert_eq!(config.peer_addr.ip(), IpAddr::V4(Ipv4Addr::LOCALHOST));
        assert_eq!(config.peer_addr.port(), SEND_PORT_DEFAULT);
    }
}
