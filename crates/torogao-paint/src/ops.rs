//! The op log's vocabulary (SPEC SS3.3), Phase 1 subset.
//!
//! An op is a verb-shaped record: fully parameterized,
//! self-contained, and replayable without any session
//! state. Phase 2 adds the mask ops and the SS5 verbs as
//! an additive minor extension (SS3.3 versioning); the
//! kinds here never change meaning.

use serde::{Deserialize, Serialize};

use crate::color::Rgba;
use crate::layer::{BlendMode, LayerId};
use crate::stroke::{GrainSpec, Stamp, StampMode};

/// An axis-aligned rectangle in canvas coordinates. `x`/`y`
/// may be negative (a rect may hang off the canvas edge);
/// application intersects with the canvas and refuses a
/// rect with no intersection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rect {
    /// Left edge, canvas px.
    pub x: i32,
    /// Top edge, canvas px.
    pub y: i32,
    /// Width in px, 1..=MAX_DIM.
    pub width: u32,
    /// Height in px, 1..=MAX_DIM.
    pub height: u32,
}

/// A property change for `Op::LayerProps`: `None` leaves
/// the property as it is.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LayerPropsChange {
    /// New display name.
    pub name: Option<String>,
    /// New opacity, 0-255.
    pub opacity: Option<u8>,
    /// New blend mode.
    pub blend: Option<BlendMode>,
    /// New visibility.
    pub visible: Option<bool>,
    /// New lock-alpha flag.
    pub lock_alpha: Option<bool>,
}

impl LayerPropsChange {
    /// True when the change sets no property at all (an op
    /// the document refuses as meaningless, so the log
    /// never carries no-ops).
    pub fn is_empty(&self) -> bool {
        self.name.is_none()
            && self.opacity.is_none()
            && self.blend.is_none()
            && self.visible.is_none()
            && self.lock_alpha.is_none()
    }
}

/// One colour stop of a gradient: `color` placed at
/// `at / 255` along the gradient's parameter. Stops are
/// recorded in strictly increasing `at` order (the
/// document validates this); the parameter clamps to the
/// end stops outside their range.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GradientStop {
    /// Position along the gradient, 0..=255.
    pub at: u8,
    /// The stop's colour; alpha interpolates like the
    /// colour channels.
    pub color: Rgba,
}

/// The shape of a gradient's parameter field. In both
/// shapes the parameter runs 0 at `from` to 1 at `to`:
/// linear projects onto the from-to axis, radial measures
/// distance from `from` against the from-to distance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GradientShape {
    /// Projection onto the from-to axis.
    Linear,
    /// Distance from `from`, scaled by the from-to
    /// distance as the radius.
    Radial,
}

/// A multi-stop gradient field in canvas space. The
/// field is a pure function of canvas position, so a
/// shape built from many rect fills under one spec still
/// shades continuously.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GradientSpec {
    /// Linear or radial parameterisation.
    pub shape: GradientShape,
    /// Parameter 0 point, canvas px.
    pub from: [i32; 2],
    /// Parameter 1 point, canvas px (the radial rim point
    /// in the radial shape). Must differ from `from`.
    pub to: [i32; 2],
    /// Colour stops, 2..=8, strictly increasing in `at`.
    pub stops: Vec<GradientStop>,
}

/// One op-log entry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op")]
pub enum Op {
    /// A brush or eraser stroke, as its recorded stamp
    /// placements (SS3.2.4) plus the deposit parameters.
    StampRun {
        /// The layer the stroke lands on.
        layer: LayerId,
        /// Paint or Erase (SS4.2).
        mode: StampMode,
        /// Deposit colour; alpha multiplies stamp alpha.
        color: Rgba,
        /// Stroke opacity ceiling.
        opacity: u8,
        /// Per-stamp deposit cap.
        flow: u8,
        /// The stamp placements, in stroke order.
        stamps: Vec<Stamp>,
    },
    /// A flat source-over fill of a rectangle (lock-alpha
    /// preserving, like stamp deposit).
    FillRect {
        /// The layer filled on.
        layer: LayerId,
        /// The rectangle, canvas coordinates.
        rect: Rect,
        /// The fill colour.
        color: Rgba,
    },
    /// A gradient fill of a rectangle: `FillRect`'s
    /// per-pixel deposit rule with the flat colour
    /// replaced by the gradient field's colour at each
    /// pixel (Phase 2 additive extension, SPEC SS3.3).
    GradientFill {
        /// The layer filled on.
        layer: LayerId,
        /// The rectangle, canvas coordinates.
        rect: Rect,
        /// The gradient field.
        gradient: GradientSpec,
    },
    /// A stamp run whose per-pixel deposit alpha is
    /// modulated by a deterministic grain field in canvas
    /// space (the textured brush: deposit density varies
    /// coherently across stamps, as a tip texture over
    /// paper grain does). Otherwise identical to
    /// `StampRun` (Phase 2 additive extension, SS3.3).
    TexturedStampRun {
        /// The layer the stroke lands on.
        layer: LayerId,
        /// Paint or Erase (SS4.2).
        mode: StampMode,
        /// Deposit colour; alpha multiplies stamp alpha.
        color: Rgba,
        /// Stroke opacity ceiling.
        opacity: u8,
        /// Per-stamp deposit cap.
        flow: u8,
        /// The stamp placements, in stroke order.
        stamps: Vec<Stamp>,
        /// The grain field modulating deposit alpha.
        grain: GrainSpec,
    },
    /// Add a layer. `after: None` puts it on top; otherwise
    /// it is inserted directly above the named layer.
    LayerAdd {
        /// The new layer's id (issued by the document when
        /// the op is first applied; replay reuses it).
        id: LayerId,
        /// The new layer's name.
        name: String,
        /// Insert position anchor.
        after: Option<LayerId>,
    },
    /// Remove a layer and its tiles.
    LayerRemove {
        /// The layer to remove.
        id: LayerId,
    },
    /// Move a layer to a new stack index (0 = bottom).
    LayerReorder {
        /// The layer to move.
        id: LayerId,
        /// The destination index in the stack.
        to_index: u32,
    },
    /// Change a layer's recorded properties.
    LayerProps {
        /// The layer to change.
        id: LayerId,
        /// The properties to set.
        change: LayerPropsChange,
    },
}

impl Op {
    /// The layer an op acts on, where it acts on exactly
    /// one existing layer (accounting and validation share
    /// this).
    pub fn target_layer(&self) -> Option<LayerId> {
        match self {
            Op::StampRun { layer, .. } => Some(*layer),
            Op::FillRect { layer, .. } => Some(*layer),
            Op::GradientFill { layer, .. } => Some(*layer),
            Op::TexturedStampRun { layer, .. } => Some(*layer),
            Op::LayerAdd { .. } => None,
            Op::LayerRemove { id } => Some(*id),
            Op::LayerReorder { id, .. } => Some(*id),
            Op::LayerProps { id, .. } => Some(*id),
        }
    }
}
