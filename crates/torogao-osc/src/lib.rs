//! torogao-osc: the VRChat-dialect OSC parameter
//! surface for Torogao (ruling D-752).
//!
//! The avatar ecosystem already speaks one live
//! parameter protocol: VRChat's OSC dialect. Trackers,
//! face-tracking rigs, TouchOSC panels, and bridge tools
//! all emit and consume `/avatar/parameters/<name>`
//! values, `/avatar/change` notices, and `/input/*`
//! controller messages over localhost UDP. This crate
//! gives Torogao that surface — an external control seam
//! for demos, testing, and every tool that already
//! speaks OSC — instead of a private protocol. It
//! completes the interop set: VRM carries the models
//! (torogao-vrm), funscript/TCode carries the devices,
//! OSC carries live parameters.
//!
//! # The contract
//!
//! The dialect is VRChat's published documentation, not
//! reverse engineering: "OSC Overview", "OSC Avatar
//! Parameters", and "OSC as Input Controller" at
//! docs.vrchat.com (pages updated 2025-11-13, read
//! 2026-11-03), over the OSC 1.0 wire format. See
//! [`dialect`] for the shapes and their citations.
//!
//! A [`Surface`] declares the parameter and input names
//! once, each with its type and direction. Inbound
//! datagrams decode through the bounded [`codec`],
//! route against the declarations, and reach the
//! consumer as typed [`SurfaceEvent`]s through an
//! [`OscSink`]. Outbound publications are checked
//! against the same declarations before encoding. The
//! declarations map onto Torogao's cast parameter model
//! (torogao-format scalar parameters: `id` is the name,
//! the value travels as a float) — see [`surface`].
//!
//! # Failure posture
//!
//! Wire data is untrusted. Decoding is bounded by named
//! constants and refuses malformed packets; routing
//! rejects undeclared names, wrong types, wrong argument
//! counts, and wrong-direction writes. Every rejection
//! is counted under its [`RejectKind`] in
//! [`SurfaceStats`] — loud as a number an operator can
//! watch, never a panic, never a silent drop. The one
//! deliberate divergence from VRChat itself: VRChat
//! coerces mistyped values on arrival; this surface
//! rejects them, because against a declared surface a
//! mistyped sender is a bug to surface, not absorb.
//!
//! # Transport choice
//!
//! The codec is in-house, following the estate's
//! existing bounded OSC codec pattern (torogao-vmc).
//! The `rosc` crate was evaluated first (2026-11-03):
//! its licence is dual MIT / Apache-2.0, which is fine,
//! but it fails three stated requirements of this
//! surface — (1) named decode bounds enforced before
//! allocation (rosc 0.11.4 enforces no packet, argument,
//! address, or string caps), (2) adversarial input can
//! never crash the process (rosc's bundle decoding
//! recurses with no depth bound, so one crafted
//! datagram can exhaust the stack), and (3) strict
//! one-message-per-datagram semantics (rosc returns
//! trailing bytes as a successful remainder). The full
//! record is in this crate's README.

#![forbid(unsafe_code)]

pub mod codec;
pub mod dialect;
pub mod net;
pub mod stats;
pub mod surface;

pub use dialect::{DialectAddress, InputKind};
pub use net::{Listener, OscConfig, PublishError, Publisher, TransportError};
pub use stats::{StatsSnapshot, SurfaceStats};
pub use surface::{
    Direction, InputValue, OscSink, ParamType, ParamValue, ParameterDecl, RejectKind, Surface,
    SurfaceBuilder, SurfaceError, SurfaceEvent,
};
