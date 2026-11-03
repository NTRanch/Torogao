//! The VRChat OSC dialect: the address shapes and value
//! rules VRChat publishes, and nothing else. The contract
//! sources are VRChat's own documentation pages —
//! "OSC Overview", "OSC Avatar Parameters", and "OSC as
//! Input Controller" at docs.vrchat.com (pages updated
//! 2025-11-13, read 2026-11-03) — plus the OSC 1.0
//! specification for the wire form itself.
//!
//! The shapes, as published:
//!
//! - `/avatar/parameters/<name>` carries one value of the
//!   matching avatar parameter. The only types are `Int`,
//!   `Bool`, and `Float` ("type: string of "Int", "Bool",
//!   or "Float" *only*"). Custom parameters are read-write;
//!   VRChat's reserved parameters (VelocityX and kin) are
//!   read-only from outside, which this crate models as a
//!   declaration direction, not a special case.
//! - `/avatar/change` carries one string: the ID of the
//!   avatar that was just loaded.
//! - `/input/<name>` is write-only client input. Axes
//!   expect a float from -1 to 1 and must be reset to 0;
//!   buttons expect an int of 1 for pressed and 0 for
//!   released (booleans are also accepted for buttons).
//! - `/chatbox/input` carries a string, a bool (send
//!   immediately), and an optional bool (notification
//!   sound, defaulting to true). `/chatbox/typing`
//!   carries one bool.
//! - Ports: VRChat receives UDP on 9000 and sends to UDP
//!   9001, on localhost. This crate's defaults take the
//!   same seats from the ecosystem's side: bind 9000,
//!   send to 9001 (see [`crate::net`]).

/// The avatar-change address: one string argument, the
/// newly loaded avatar's ID.
pub const AVATAR_CHANGE_ADDRESS: &str = "/avatar/change";
/// The avatar-parameter address prefix; the parameter
/// name follows it.
pub const AVATAR_PARAMETERS_PREFIX: &str = "/avatar/parameters/";
/// The chatbox text address: string, bool, optional bool.
pub const CHATBOX_INPUT_ADDRESS: &str = "/chatbox/input";
/// The chatbox typing-indicator address: one bool.
pub const CHATBOX_TYPING_ADDRESS: &str = "/chatbox/typing";
/// The client-input address prefix; the input name
/// follows it.
pub const INPUT_PREFIX: &str = "/input/";

/// The documented axis inputs (float, -1 to 1), from
/// "OSC as Input Controller". Alphabetical.
pub const INPUT_AXES: &[&str] = &[
    "GrabAxisRight",
    "Horizontal",
    "LookHorizontal",
    "MoveHoldFB",
    "SpinHoldCwCcw",
    "SpinHoldLR",
    "SpinHoldUD",
    "UseAxisRight",
    "Vertical",
];

/// The documented button inputs (int 1 pressed / 0
/// released; bool also accepted), from "OSC as Input
/// Controller". Alphabetical.
pub const INPUT_BUTTONS: &[&str] = &[
    "ComfortLeft",
    "ComfortRight",
    "DropLeft",
    "DropRight",
    "GrabLeft",
    "GrabRight",
    "Jump",
    "LookLeft",
    "LookRight",
    "MoveBackward",
    "MoveForward",
    "MoveLeft",
    "MoveRight",
    "PanicButton",
    "QuickMenuToggleLeft",
    "QuickMenuToggleRight",
    "Run",
    "UseLeft",
    "UseRight",
    "Voice",
];

/// Whether a documented input is an axis or a button.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputKind {
    /// Float from -1 to 1; reset to 0 when not in use.
    Axis,
    /// Pressed/released; int 1/0 or bool.
    Button,
}

/// The documented kind of an input name, if the name is
/// one VRChat publishes.
pub fn documented_input_kind(name: &str) -> Option<InputKind> {
    if INPUT_AXES.contains(&name) {
        Some(InputKind::Axis)
    } else if INPUT_BUTTONS.contains(&name) {
        Some(InputKind::Button)
    } else {
        None
    }
}

/// Which dialect family an address belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DialectAddress<'a> {
    /// `/avatar/change` exactly.
    AvatarChange,
    /// `/chatbox/input` exactly.
    ChatboxInput,
    /// `/chatbox/typing` exactly.
    ChatboxTyping,
    /// `/input/<name>`; the name follows the prefix.
    Input(&'a str),
    /// `/avatar/parameters/<name>`; the name follows the
    /// prefix.
    Parameter(&'a str),
    /// No dialect family claims this address.
    Unknown,
}

/// Classifies an address into its dialect family. Pure
/// prefix/exact matching — the surface layer decides
/// whether the name is declared and the value is legal.
pub fn classify(address: &str) -> DialectAddress<'_> {
    if address == AVATAR_CHANGE_ADDRESS {
        DialectAddress::AvatarChange
    } else if address == CHATBOX_INPUT_ADDRESS {
        DialectAddress::ChatboxInput
    } else if address == CHATBOX_TYPING_ADDRESS {
        DialectAddress::ChatboxTyping
    } else if let Some(name) = address.strip_prefix(AVATAR_PARAMETERS_PREFIX) {
        DialectAddress::Parameter(name)
    } else if let Some(name) = address.strip_prefix(INPUT_PREFIX) {
        DialectAddress::Input(name)
    } else {
        DialectAddress::Unknown
    }
}

/// The full address a parameter value travels on.
pub fn parameter_address(name: &str) -> String {
    format!("{AVATAR_PARAMETERS_PREFIX}{name}")
}

/// The full address an input value travels on.
pub fn input_address(name: &str) -> String {
    format!("{INPUT_PREFIX}{name}")
}

#[cfg(test)]
mod tests {
    use super::{
        DialectAddress, classify, documented_input_kind, input_address, parameter_address,
    };
    use crate::dialect::InputKind;

    #[test]
    fn classify_reads_every_dialect_family() {
        assert_eq!(classify("/avatar/change"), DialectAddress::AvatarChange);
        assert_eq!(classify("/chatbox/input"), DialectAddress::ChatboxInput);
        assert_eq!(classify("/chatbox/typing"), DialectAddress::ChatboxTyping);
        assert_eq!(
            classify("/avatar/parameters/VRCEmote"),
            DialectAddress::Parameter("VRCEmote")
        );
        assert_eq!(classify("/input/Jump"), DialectAddress::Input("Jump"));
    }

    #[test]
    fn classify_refuses_near_misses() {
        // No trailing slash: not the parameter prefix.
        assert_eq!(classify("/avatar/parameters"), DialectAddress::Unknown);
        // A longer path under a fixed address is not it.
        assert_eq!(classify("/avatar/change/extra"), DialectAddress::Unknown);
        assert_eq!(classify("/tracking/trackers"), DialectAddress::Unknown);
        assert_eq!(classify("avatar/change"), DialectAddress::Unknown);
    }

    #[test]
    fn documented_inputs_split_axes_from_buttons() {
        assert_eq!(documented_input_kind("Vertical"), Some(InputKind::Axis));
        assert_eq!(
            documented_input_kind("SpinHoldCwCcw"),
            Some(InputKind::Axis)
        );
        assert_eq!(documented_input_kind("Jump"), Some(InputKind::Button));
        assert_eq!(documented_input_kind("Voice"), Some(InputKind::Button));
        assert_eq!(documented_input_kind("LookVertical"), None);
        assert_eq!(documented_input_kind("Dance"), None);
    }

    #[test]
    fn address_builders_match_the_classified_shapes() {
        assert_eq!(parameter_address("Face"), "/avatar/parameters/Face");
        assert_eq!(input_address("Jump"), "/input/Jump");
        assert_eq!(
            classify(&parameter_address("Face")),
            DialectAddress::Parameter("Face")
        );
        assert_eq!(
            classify(&input_address("Jump")),
            DialectAddress::Input("Jump")
        );
    }
}
