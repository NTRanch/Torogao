//! The declared parameter surface: every name the OSC
//! endpoint will speak for, declared once with its type
//! and direction, and the router that turns a decoded
//! message into a typed event or a counted rejection.
//!
//! The surface maps onto Torogao's cast parameter model
//! (torogao-format): a model's scalar parameters are the
//! declaration source — the parameter's `id` is the
//! declaration name, and its value travels as a `Float`.
//! Parameters a character exposes as switches or steps
//! declare as `Bool` / `Int`. This crate takes plain
//! declarations and owns no format types; the app crate
//! walks its models and declares.
//!
//! Routing is strict, deliberately stricter than VRChat
//! itself: VRChat coerces value types on arrival (an int
//! 1 sets a bool parameter true), but a mistyped sender
//! against a declared surface is a bug an operator should
//! see counted, not silently absorbed. A wrong-typed,
//! wrong-count, undeclared, or wrong-direction message is
//! rejected with a [`RejectKind`] and counted in
//! [`crate::SurfaceStats`]; nothing here panics on wire
//! data.

use std::collections::BTreeMap;
use std::fmt;

use crate::codec::{OscArg, OscMessage};
use crate::dialect::{self, DialectAddress, InputKind};

/// Largest parameter or input name, in bytes. Names ride
/// inside the codec's address bound; this tighter bound
/// keeps declaration tables reviewable.
pub const NAME_BYTES_MAX: usize = 128;

/// A declared parameter's value type — the dialect's
/// three, per "OSC Avatar Parameters".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParamType {
    /// OSC `T` / `F`.
    Bool,
    /// OSC `f`.
    Float,
    /// OSC `i`.
    Int,
}

/// A parameter value, typed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ParamValue {
    /// A `Bool` parameter's value.
    Bool(bool),
    /// A `Float` parameter's value.
    Float(f32),
    /// An `Int` parameter's value.
    Int(i32),
}

impl ParamValue {
    /// The declared type this value belongs to.
    pub fn param_type(&self) -> ParamType {
        match self {
            ParamValue::Bool(_) => ParamType::Bool,
            ParamValue::Float(_) => ParamType::Float,
            ParamValue::Int(_) => ParamType::Int,
        }
    }
}

/// Which way a declared parameter may travel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// External tools set it; the surface never sends it.
    In,
    /// Both directions.
    InOut,
    /// The surface sends it; external writes are rejected.
    /// VRChat's reserved parameters live here.
    Out,
}

/// One declared parameter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParameterDecl {
    direction: Direction,
    name: String,
    param_type: ParamType,
}

impl ParameterDecl {
    /// The direction the parameter may travel.
    pub fn direction(&self) -> Direction {
        self.direction
    }

    /// The declared name (the address's final segment).
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The declared value type.
    pub fn param_type(&self) -> ParamType {
        self.param_type
    }
}

/// A value on the `/input/*` family, typed by the
/// input's documented kind.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum InputValue {
    /// An axis position, -1 to 1 by the dialect's rule
    /// (range is the consumer's policy, not the wire's).
    Axis(f32),
    /// A button state: pressed or released.
    Button(bool),
}

/// One routed dialect event: the unit the sink consumes.
#[derive(Debug, Clone, PartialEq)]
pub enum SurfaceEvent {
    /// `/avatar/change`: a new avatar was loaded.
    AvatarChanged {
        /// The new avatar's ID.
        avatar_id: String,
    },
    /// `/chatbox/input`: text for the chatbox.
    ChatboxInput {
        /// Whether the notification sound plays.
        notify: bool,
        /// True sends the text at once; false opens the
        /// keyboard with the text populated.
        send_immediately: bool,
        /// The text (the dialect caps it at 144 chars;
        /// the codec's string bound is the wire bound).
        text: String,
    },
    /// `/chatbox/typing`: the typing indicator toggled.
    ChatboxTyping {
        /// The indicator's new state.
        typing: bool,
    },
    /// `/input/<name>`: a client input moved.
    Input {
        /// The input's name.
        name: String,
        /// The typed value.
        value: InputValue,
    },
    /// `/avatar/parameters/<name>`: a parameter was set.
    Parameter {
        /// The parameter's name.
        name: String,
        /// The typed value.
        value: ParamValue,
    },
}

/// The consumer of routed events. The transport drains
/// its bounded queue into a sink; the sink owns what the
/// values mean (applying them to the cast is the app's
/// work, exactly as torogao-vmc leaves application to
/// the app crate).
pub trait OscSink {
    /// Consumes one routed event, in arrival order.
    fn on_event(&mut self, event: SurfaceEvent);
}

/// Why a message was rejected. Every rejection is
/// counted under its kind in [`crate::SurfaceStats`];
/// rejection is the loud failure mode — a counter an
/// operator can watch — never a panic and never a
/// silent drop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RejectKind {
    /// The datagram was not a well-formed OSC message.
    MalformedPacket,
    /// The event queue was full; the event was dropped.
    QueueFull,
    /// An `/input/*` name the surface does not declare.
    UndeclaredInput,
    /// A parameter name the surface does not declare.
    UndeclaredParameter,
    /// No dialect family claims the address.
    UnknownAddress,
    /// The argument count does not match the address's
    /// documented shape.
    WrongArgCount,
    /// The parameter's declared direction forbids this
    /// travel direction.
    WrongDirection,
    /// The argument type does not match the declared
    /// type (including a button int other than 0 or 1).
    WrongType,
}

impl fmt::Display for RejectKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = match self {
            RejectKind::MalformedPacket => "datagram is not a well-formed OSC message",
            RejectKind::QueueFull => "event queue is full",
            RejectKind::UndeclaredInput => "input is not declared on this surface",
            RejectKind::UndeclaredParameter => "parameter is not declared on this surface",
            RejectKind::UnknownAddress => "address belongs to no dialect family",
            RejectKind::WrongArgCount => "argument count does not match the address shape",
            RejectKind::WrongDirection => "declared direction forbids this direction of travel",
            RejectKind::WrongType => "argument type does not match the declared type",
        };
        write!(f, "{text}")
    }
}

/// Why a surface declaration set refused to build.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SurfaceError {
    /// The same input name was declared twice.
    DuplicateInput(String),
    /// The same parameter name was declared twice.
    DuplicateParameter(String),
    /// An input name is empty, over-long, or carries a
    /// `/` or NUL (it could never be addressed).
    InvalidInputName(String),
    /// A parameter name is empty, over-long, or carries
    /// a `/` or NUL (it could never be addressed).
    InvalidParameterName(String),
}

impl fmt::Display for SurfaceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SurfaceError::DuplicateInput(name) => write!(f, "input {name:?} is declared twice"),
            SurfaceError::DuplicateParameter(name) => {
                write!(f, "parameter {name:?} is declared twice")
            }
            SurfaceError::InvalidInputName(name) => write!(f, "input name {name:?} is not usable"),
            SurfaceError::InvalidParameterName(name) => {
                write!(f, "parameter name {name:?} is not usable")
            }
        }
    }
}

impl std::error::Error for SurfaceError {}

/// A name is usable if it can appear as an address's
/// final segment: non-empty, within the name bound, and
/// free of `/` and NUL.
fn name_is_usable(name: &str) -> bool {
    !name.is_empty() && name.len() <= NAME_BYTES_MAX && !name.contains('/') && !name.contains('\0')
}

/// Builds a [`Surface`] declaration by declaration.
#[derive(Debug, Default)]
pub struct SurfaceBuilder {
    inputs: Vec<(String, InputKind)>,
    parameters: Vec<ParameterDecl>,
}

impl SurfaceBuilder {
    /// An empty builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Declares the full documented `/input/*` family
    /// (every axis and button VRChat publishes).
    pub fn documented_inputs(mut self) -> Self {
        for name in dialect::INPUT_AXES {
            self.inputs.push(((*name).to_string(), InputKind::Axis));
        }
        for name in dialect::INPUT_BUTTONS {
            self.inputs.push(((*name).to_string(), InputKind::Button));
        }
        self
    }

    /// Declares one input with an explicit kind (for
    /// inputs beyond the documented family).
    pub fn input(mut self, name: &str, kind: InputKind) -> Self {
        self.inputs.push((name.to_string(), kind));
        self
    }

    /// Declares one parameter.
    pub fn parameter(mut self, name: &str, param_type: ParamType, direction: Direction) -> Self {
        self.parameters.push(ParameterDecl {
            direction,
            name: name.to_string(),
            param_type,
        });
        self
    }

    /// Validates the declaration set and freezes it.
    /// Duplicate or unusable names refuse the build.
    pub fn build(self) -> Result<Surface, SurfaceError> {
        let mut inputs: BTreeMap<String, InputKind> = BTreeMap::new();
        for (name, kind) in self.inputs {
            if !name_is_usable(&name) {
                return Err(SurfaceError::InvalidInputName(name));
            }
            if inputs.insert(name.clone(), kind).is_some() {
                return Err(SurfaceError::DuplicateInput(name));
            }
        }
        let mut parameters: BTreeMap<String, ParameterDecl> = BTreeMap::new();
        for decl in self.parameters {
            if !name_is_usable(&decl.name) {
                return Err(SurfaceError::InvalidParameterName(decl.name));
            }
            if parameters.insert(decl.name.clone(), decl.clone()).is_some() {
                return Err(SurfaceError::DuplicateParameter(decl.name));
            }
        }
        Ok(Surface { inputs, parameters })
    }
}

/// The frozen declaration set plus the router. Cheap to
/// clone; the listener and the publisher each hold one.
#[derive(Debug, Clone)]
pub struct Surface {
    inputs: BTreeMap<String, InputKind>,
    parameters: BTreeMap<String, ParameterDecl>,
}

impl Surface {
    /// Starts a declaration builder.
    pub fn builder() -> SurfaceBuilder {
        SurfaceBuilder::new()
    }

    /// The declared input's kind, if declared.
    pub fn input_kind(&self, name: &str) -> Option<InputKind> {
        self.inputs.get(name).copied()
    }

    /// The declared parameter, if declared.
    pub fn parameter(&self, name: &str) -> Option<&ParameterDecl> {
        self.parameters.get(name)
    }

    /// Routes one decoded message to its event, or
    /// rejects it with the counted kind. Pure: the same
    /// message against the same surface always routes
    /// the same way.
    pub fn route(&self, message: &OscMessage) -> Result<SurfaceEvent, RejectKind> {
        match dialect::classify(&message.address) {
            DialectAddress::AvatarChange => route_avatar_change(message),
            DialectAddress::ChatboxInput => route_chatbox_input(message),
            DialectAddress::ChatboxTyping => route_chatbox_typing(message),
            DialectAddress::Input(name) => self.route_input(name, message),
            DialectAddress::Parameter(name) => self.route_parameter(name, message),
            DialectAddress::Unknown => Err(RejectKind::UnknownAddress),
        }
    }

    /// Validates an outbound parameter publication
    /// against the declarations: declared, direction
    /// permits sending, type matches.
    pub fn check_publish(
        &self,
        name: &str,
        value: ParamValue,
    ) -> Result<&ParameterDecl, RejectKind> {
        let Some(decl) = self.parameters.get(name) else {
            return Err(RejectKind::UndeclaredParameter);
        };
        if decl.direction == Direction::In {
            return Err(RejectKind::WrongDirection);
        }
        if decl.param_type != value.param_type() {
            return Err(RejectKind::WrongType);
        }
        Ok(decl)
    }

    /// Validates an outbound input publication against
    /// the declarations: declared, and the value's shape
    /// matches the input's kind.
    pub fn check_publish_input(&self, name: &str, value: InputValue) -> Result<(), RejectKind> {
        let Some(kind) = self.inputs.get(name) else {
            return Err(RejectKind::UndeclaredInput);
        };
        let matches = matches!(
            (kind, value),
            (InputKind::Axis, InputValue::Axis(_)) | (InputKind::Button, InputValue::Button(_))
        );
        if matches {
            Ok(())
        } else {
            Err(RejectKind::WrongType)
        }
    }

    fn route_input(&self, name: &str, message: &OscMessage) -> Result<SurfaceEvent, RejectKind> {
        let Some(kind) = self.inputs.get(name) else {
            return Err(RejectKind::UndeclaredInput);
        };
        let arg = single_arg(message)?;
        let value = match (kind, arg) {
            (InputKind::Axis, OscArg::Float(position)) => InputValue::Axis(*position),
            (InputKind::Button, OscArg::Bool(pressed)) => InputValue::Button(*pressed),
            (InputKind::Button, OscArg::Int(0)) => InputValue::Button(false),
            (InputKind::Button, OscArg::Int(1)) => InputValue::Button(true),
            _ => return Err(RejectKind::WrongType),
        };
        Ok(SurfaceEvent::Input {
            name: name.to_string(),
            value,
        })
    }

    fn route_parameter(
        &self,
        name: &str,
        message: &OscMessage,
    ) -> Result<SurfaceEvent, RejectKind> {
        let Some(decl) = self.parameters.get(name) else {
            return Err(RejectKind::UndeclaredParameter);
        };
        if decl.direction == Direction::Out {
            return Err(RejectKind::WrongDirection);
        }
        let arg = single_arg(message)?;
        let value = match (decl.param_type, arg) {
            (ParamType::Bool, OscArg::Bool(value)) => ParamValue::Bool(*value),
            (ParamType::Float, OscArg::Float(value)) => ParamValue::Float(*value),
            (ParamType::Int, OscArg::Int(value)) => ParamValue::Int(*value),
            _ => return Err(RejectKind::WrongType),
        };
        Ok(SurfaceEvent::Parameter {
            name: name.to_string(),
            value,
        })
    }
}

/// The message's only argument, or the count rejection.
fn single_arg(message: &OscMessage) -> Result<&OscArg, RejectKind> {
    match message.args.as_slice() {
        [arg] => Ok(arg),
        _ => Err(RejectKind::WrongArgCount),
    }
}

fn route_avatar_change(message: &OscMessage) -> Result<SurfaceEvent, RejectKind> {
    match single_arg(message)? {
        OscArg::Str(avatar_id) => Ok(SurfaceEvent::AvatarChanged {
            avatar_id: avatar_id.clone(),
        }),
        _ => Err(RejectKind::WrongType),
    }
}

fn route_chatbox_input(message: &OscMessage) -> Result<SurfaceEvent, RejectKind> {
    let (text, send_immediately, notify) = match message.args.as_slice() {
        [OscArg::Str(text), OscArg::Bool(send)] => (text, *send, true),
        [OscArg::Str(text), OscArg::Bool(send), OscArg::Bool(notify)] => (text, *send, *notify),
        [_, _] | [_, _, _] => return Err(RejectKind::WrongType),
        _ => return Err(RejectKind::WrongArgCount),
    };
    Ok(SurfaceEvent::ChatboxInput {
        notify,
        send_immediately,
        text: text.clone(),
    })
}

fn route_chatbox_typing(message: &OscMessage) -> Result<SurfaceEvent, RejectKind> {
    match single_arg(message)? {
        OscArg::Bool(typing) => Ok(SurfaceEvent::ChatboxTyping { typing: *typing }),
        _ => Err(RejectKind::WrongType),
    }
}

#[cfg(test)]
mod tests {
    use super::{Direction, InputValue, ParamType, ParamValue, RejectKind, Surface, SurfaceEvent};
    use crate::codec::{OscArg, OscMessage};
    use crate::dialect::InputKind;

    fn surface() -> Surface {
        Surface::builder()
            .documented_inputs()
            .parameter("AFK", ParamType::Bool, Direction::InOut)
            .parameter("Face", ParamType::Int, Direction::In)
            .parameter("Smile", ParamType::Float, Direction::InOut)
            .parameter("VelocityX", ParamType::Float, Direction::Out)
            .build()
            .expect("surface builds")
    }

    fn message(address: &str, args: Vec<OscArg>) -> OscMessage {
        OscMessage {
            address: address.to_string(),
            args,
        }
    }

    #[test]
    fn builder_refuses_duplicates_and_unusable_names() {
        let duplicate = Surface::builder()
            .parameter("Face", ParamType::Int, Direction::In)
            .parameter("Face", ParamType::Int, Direction::In)
            .build();
        assert!(duplicate.is_err());
        let slashed = Surface::builder()
            .parameter("A/B", ParamType::Int, Direction::In)
            .build();
        assert!(slashed.is_err());
        let empty = Surface::builder()
            .parameter("", ParamType::Int, Direction::In)
            .build();
        assert!(empty.is_err());
        let duplicate_input = Surface::builder()
            .documented_inputs()
            .input("Jump", InputKind::Button)
            .build();
        assert!(duplicate_input.is_err());
    }

    #[test]
    fn routes_each_parameter_type() {
        let surface = surface();
        assert_eq!(
            surface.route(&message("/avatar/parameters/AFK", vec![OscArg::Bool(true)])),
            Ok(SurfaceEvent::Parameter {
                name: "AFK".to_string(),
                value: ParamValue::Bool(true),
            })
        );
        assert_eq!(
            surface.route(&message("/avatar/parameters/Face", vec![OscArg::Int(3)])),
            Ok(SurfaceEvent::Parameter {
                name: "Face".to_string(),
                value: ParamValue::Int(3),
            })
        );
        assert_eq!(
            surface.route(&message(
                "/avatar/parameters/Smile",
                vec![OscArg::Float(0.5)]
            )),
            Ok(SurfaceEvent::Parameter {
                name: "Smile".to_string(),
                value: ParamValue::Float(0.5),
            })
        );
    }

    #[test]
    fn routes_avatar_change_chatbox_and_inputs() {
        let surface = surface();
        assert_eq!(
            surface.route(&message(
                "/avatar/change",
                vec![OscArg::Str("avtr_x".to_string())]
            )),
            Ok(SurfaceEvent::AvatarChanged {
                avatar_id: "avtr_x".to_string(),
            })
        );
        assert_eq!(
            surface.route(&message(
                "/chatbox/input",
                vec![OscArg::Str("hi".to_string()), OscArg::Bool(true)]
            )),
            Ok(SurfaceEvent::ChatboxInput {
                notify: true,
                send_immediately: true,
                text: "hi".to_string(),
            })
        );
        assert_eq!(
            surface.route(&message("/chatbox/typing", vec![OscArg::Bool(false)])),
            Ok(SurfaceEvent::ChatboxTyping { typing: false })
        );
        assert_eq!(
            surface.route(&message("/input/Vertical", vec![OscArg::Float(-1.0)])),
            Ok(SurfaceEvent::Input {
                name: "Vertical".to_string(),
                value: InputValue::Axis(-1.0),
            })
        );
        assert_eq!(
            surface.route(&message("/input/Jump", vec![OscArg::Int(1)])),
            Ok(SurfaceEvent::Input {
                name: "Jump".to_string(),
                value: InputValue::Button(true),
            })
        );
        assert_eq!(
            surface.route(&message("/input/Jump", vec![OscArg::Bool(false)])),
            Ok(SurfaceEvent::Input {
                name: "Jump".to_string(),
                value: InputValue::Button(false),
            })
        );
    }

    #[test]
    fn rejects_every_bad_route_by_kind() {
        let surface = surface();
        assert_eq!(
            surface.route(&message("/avatar/parameters/Ghost", vec![OscArg::Int(1)])),
            Err(RejectKind::UndeclaredParameter)
        );
        assert_eq!(
            surface.route(&message(
                "/avatar/parameters/VelocityX",
                vec![OscArg::Float(1.0)]
            )),
            Err(RejectKind::WrongDirection)
        );
        assert_eq!(
            surface.route(&message(
                "/avatar/parameters/Face",
                vec![OscArg::Float(1.0)]
            )),
            Err(RejectKind::WrongType)
        );
        assert_eq!(
            surface.route(&message(
                "/avatar/parameters/Face",
                vec![OscArg::Int(1), OscArg::Int(2)]
            )),
            Err(RejectKind::WrongArgCount)
        );
        assert_eq!(
            surface.route(&message("/input/Dance", vec![OscArg::Int(1)])),
            Err(RejectKind::UndeclaredInput)
        );
        assert_eq!(
            surface.route(&message("/input/Jump", vec![OscArg::Int(2)])),
            Err(RejectKind::WrongType)
        );
        assert_eq!(
            surface.route(&message("/avatar/change", vec![OscArg::Int(7)])),
            Err(RejectKind::WrongType)
        );
        assert_eq!(
            surface.route(&message("/tracking/trackers", vec![OscArg::Int(7)])),
            Err(RejectKind::UnknownAddress)
        );
    }

    #[test]
    fn publish_checks_mirror_the_route_rules() {
        let surface = surface();
        assert!(
            surface
                .check_publish("Smile", ParamValue::Float(0.25))
                .is_ok()
        );
        assert_eq!(
            surface.check_publish("Face", ParamValue::Int(1)),
            Err(RejectKind::WrongDirection)
        );
        assert_eq!(
            surface.check_publish("Smile", ParamValue::Int(1)),
            Err(RejectKind::WrongType)
        );
        assert_eq!(
            surface.check_publish("Ghost", ParamValue::Int(1)),
            Err(RejectKind::UndeclaredParameter)
        );
        assert!(
            surface
                .check_publish_input("Jump", InputValue::Button(true))
                .is_ok()
        );
        assert_eq!(
            surface.check_publish_input("Jump", InputValue::Axis(1.0)),
            Err(RejectKind::WrongType)
        );
        assert_eq!(
            surface.check_publish_input("Dance", InputValue::Button(true)),
            Err(RejectKind::UndeclaredInput)
        );
    }
}
