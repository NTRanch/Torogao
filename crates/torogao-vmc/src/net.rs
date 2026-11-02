//! UDP transport for VMC/OSC: one datagram per message,
//! frames assembled on receive. Sockets are blocking with
//! an optional read timeout; a receive timeout is a
//! `Ok(None)`, never an error — a quiet feed is the normal
//! tracking-loss case, and the caller's hold policy (the
//! tracking driver) owns what happens next.

use std::fmt;
use std::net::{SocketAddr, UdpSocket};
use std::time::Duration;

use crate::arkit::GovernedValue;
use crate::osc::{self, OSC_PACKET_BYTES_MAX, OscMessage};
use crate::vmc::{FrameAssembler, VmcFrame};

/// Why a transport operation failed.
#[derive(Debug)]
pub enum TransportError {
    /// A socket operation failed.
    Io(String),
    /// A packet refused OSC decoding.
    Osc(osc::OscError),
    /// A message refused frame assembly.
    Vmc(crate::vmc::VmcError),
}

impl fmt::Display for TransportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TransportError::Io(message) => write!(f, "UDP transport failed: {message}"),
            TransportError::Osc(err) => write!(f, "OSC decode failed: {err}"),
            TransportError::Vmc(err) => write!(f, "VMC assembly failed: {err}"),
        }
    }
}

impl std::error::Error for TransportError {}

impl From<osc::OscError> for TransportError {
    fn from(err: osc::OscError) -> TransportError {
        TransportError::Osc(err)
    }
}

impl From<crate::vmc::VmcError> for TransportError {
    fn from(err: crate::vmc::VmcError) -> TransportError {
        TransportError::Vmc(err)
    }
}

fn io_error(err: std::io::Error) -> TransportError {
    TransportError::Io(err.to_string())
}

fn is_timeout(err: &std::io::Error) -> bool {
    matches!(
        err.kind(),
        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
    )
}

/// The sending half: messages and frames to one target.
#[derive(Debug)]
pub struct OscSender {
    socket: UdpSocket,
    target: SocketAddr,
}

impl OscSender {
    /// Binds an ephemeral local port and targets `target`.
    pub fn to(target: SocketAddr) -> Result<OscSender, TransportError> {
        let socket = UdpSocket::bind("0.0.0.0:0").map_err(io_error)?;
        Ok(OscSender { socket, target })
    }

    /// Sends one message as one datagram.
    pub fn send_message(&self, message: &OscMessage) -> Result<(), TransportError> {
        let packet = osc::encode(message)?;
        self.socket
            .send_to(&packet, self.target)
            .map_err(io_error)?;
        Ok(())
    }

    /// Sends one frame (its messages in VMC order,
    /// Apply last).
    pub fn send_frame(&self, frame: &VmcFrame) -> Result<(), TransportError> {
        for message in frame.messages() {
            self.send_message(&message)?;
        }
        Ok(())
    }

    /// Sends a governed state (Torogao performing onward
    /// to an OBS-side consumer).
    pub fn send_governed(
        &self,
        values: &std::collections::BTreeMap<String, GovernedValue>,
    ) -> Result<(), TransportError> {
        for message in crate::arkit::governed_messages(values) {
            self.send_message(&message)?;
        }
        Ok(())
    }
}

/// The receiving half: datagrams decoded and assembled
/// into frames.
#[derive(Debug)]
pub struct OscReceiver {
    socket: UdpSocket,
    assembler: FrameAssembler,
}

impl OscReceiver {
    /// Binds `addr` (e.g. `127.0.0.1:39539`, or port 0 for
    /// an ephemeral port).
    pub fn bind(addr: &str) -> Result<OscReceiver, TransportError> {
        let socket = UdpSocket::bind(addr).map_err(io_error)?;
        Ok(OscReceiver {
            socket,
            assembler: FrameAssembler::new(),
        })
    }

    /// The bound local address.
    pub fn local_addr(&self) -> Result<SocketAddr, TransportError> {
        self.socket.local_addr().map_err(io_error)
    }

    /// Sets the read timeout (`None` blocks indefinitely).
    pub fn set_timeout(&self, timeout: Option<Duration>) -> Result<(), TransportError> {
        self.socket.set_read_timeout(timeout).map_err(io_error)
    }

    /// Receives the next complete frame: reads datagrams
    /// until an Apply completes one. A read timeout
    /// returns `Ok(None)` (see the module note); malformed
    /// packets and mis-shaped messages refuse typed.
    pub fn recv_frame(&mut self) -> Result<Option<VmcFrame>, TransportError> {
        let mut buffer = vec![0_u8; OSC_PACKET_BYTES_MAX];
        loop {
            let (len, _) = match self.socket.recv_from(&mut buffer) {
                Ok(received) => received,
                Err(err) if is_timeout(&err) => return Ok(None),
                Err(err) => return Err(io_error(err)),
            };
            let message = osc::decode(&buffer[..len])?;
            if let Some(frame) = self.assembler.push(&message)? {
                return Ok(Some(frame));
            }
        }
    }
}
