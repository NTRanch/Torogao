//! The unified command vocabulary.
//!
//! One typed envelope for every input a hosted core can
//! receive. Pointer, key, gamepad, and system input are
//! translated by the host into these commands at tick
//! boundaries; the core sees one vocabulary regardless of
//! whether the player used touch, mouse, pad, or keyboard
//! (the P2-10/D-465 seam, generalized: continuous input is
//! reduced to aggregate events before it reaches the core —
//! a drag arrives as one command carrying milli deltas, a
//! hold as one command carrying a duration bucket).
//!
//! Commands are the ONLY input to [`crate::GameCore::step`];
//! wall-clock time is never an argument anywhere in the
//! runtime. Every payload is an integer, a bool, or a string:
//! the vocabulary is Tier 1 by construction.

use serde::{Deserialize, Serialize};

/// A keyboard key, reduced to the runtime's vocabulary. Hosts
/// translate platform key codes into these; `Named` carries
/// any further key by its stable name.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Key {
    /// Confirm / accept (Enter, Space, tap-equivalent).
    Confirm,
    /// Cancel / back (Escape).
    Cancel,
    /// Directional up.
    Up,
    /// Directional down.
    Down,
    /// Directional left.
    Left,
    /// Directional right.
    Right,
    /// Menu / pause key.
    Menu,
    /// Any other key, by stable name (e.g. `"f1"`, `"q"`).
    Named(String),
}

/// A gamepad button, reduced to the runtime's vocabulary
/// (south/east/north/west face positions, so the name does not
/// depend on a vendor's lettering).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PadButton {
    /// South face button (cross / A).
    South,
    /// East face button (circle / B).
    East,
    /// North face button (triangle / Y).
    North,
    /// West face button (square / X).
    West,
    /// Start / options.
    Start,
    /// Select / back / share.
    Select,
    /// Any other button, by stable name.
    Named(String),
}

/// One command at a tick boundary. A step receives a slice of
/// these; the core reports one [`crate::Verdict`] per command.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum Command {
    /// Advance one tick with no game input.
    Advance,
    /// A tap on a named target (dialogue surface, node, UI
    /// element — the name is the game's own).
    PointerTap {
        /// The tapped target's name.
        target: String,
    },
    /// A hold on a named target, reduced to a duration bucket
    /// (the host's bucketing is a host contract; the core sees
    /// the bucket index, never a duration).
    PointerHold {
        /// The held target's name.
        target: String,
        /// Duration bucket index.
        bucket: u32,
    },
    /// A drag on a named target, reduced to one aggregate
    /// event with milli-unit deltas.
    PointerDrag {
        /// The dragged target's name.
        target: String,
        /// Horizontal delta, milli-units.
        dx_milli: i64,
        /// Vertical delta, milli-units.
        dy_milli: i64,
    },
    /// A key press.
    KeyPress {
        /// The key.
        key: Key,
    },
    /// A gamepad button press.
    PadPress {
        /// The button.
        button: PadButton,
    },
    /// A gamepad axis reduced to a bucketed integer at the
    /// tick boundary (never a raw analog float).
    PadAxis {
        /// The axis name (`"left_x"`, `"right_trigger"`, ...).
        axis: String,
        /// The bucketed value, milli-units in [-1000, 1000].
        bucket_milli: i64,
    },
    /// Lifecycle: the host asks the session to pause. A core
    /// that has no pause state acknowledges it as ignored;
    /// pausing the presentation clock is the host's business.
    Pause,
    /// Lifecycle: resume after [`Command::Pause`].
    Resume,
    /// Lifecycle: the platform is suspending the process
    /// (Android `onPause` class). The session survives by
    /// construction — state lives in the core and the save
    /// service — so cores acknowledge and continue.
    Suspend,
    /// A named game event (the AUTH-G `event` input; own-core
    /// games map it onto their event tables).
    Event {
        /// The event id.
        id: String,
    },
    /// A choice at a named choice point (the AUTH-G `choice`
    /// input).
    Choice {
        /// The choice id.
        id: String,
        /// The chosen option id, when the input names one.
        option: Option<String>,
    },
    /// An interaction with a named grid/cell (the AUTH-G
    /// `interact` input).
    Interact {
        /// The interaction grid id.
        id: String,
        /// The `<tool>:<node>` pair, when the input names one.
        option: Option<String>,
    },
    /// An offer decision (the AUTH-G `offer` input).
    Offer {
        /// The offer (trigger) id.
        id: String,
        /// True to accept, false to decline.
        accept: bool,
    },
    /// The game-extended payload: a core's own verb with
    /// integer arguments. This is the one open hatch in the
    /// vocabulary — the runtime owns the envelope, the game
    /// owns the verb set (Loop Service: `action`, `chase`,
    /// `buy_item`, ...). Verbs are named, arguments are
    /// integers only, so the op log stays Tier 1.
    Invoke {
        /// The core's verb name.
        verb: String,
        /// Integer arguments, in the verb's own order.
        args: Vec<i64>,
    },
}
