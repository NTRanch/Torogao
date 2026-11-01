//! The portrait layout model for the phone Persona frame
//! (AMENDMENT-ANDROID §3.3, rules 1–5 + 7).
//!
//! Pure geometry over a viewport measured in density-
//! independent points (egui points on Android are dp: the
//! platform scale factor converts). The shell draws exactly
//! what this model computes, and the phone audit (plus the
//! host tests) verify the §3.3 rules against the model:
//!
//! 1. Portrait-primary: the model is defined for any
//!    viewport, and portrait viewports get the full frame
//!    (landscape is an enhancement, never the only home of
//!    a capability).
//! 2. One panel column: the content region is the full
//!    viewport width; desktop multi-panel layouts stack.
//! 3. Touch target floor: every interactive element the
//!    model sizes is ≥ `TOUCH_TARGET_MIN_DP` on its
//!    smallest dimension.
//! 4. No hover / right-click / chord as sole paths: the
//!    frame carries visible controls only (a drawing rule,
//!    enforced by the shell using buttons everywhere).
//! 5. Standing controls never scroll away: the standing
//!    bar is pinned to the viewport's bottom edge and the
//!    content region *excludes* it — structurally outside
//!    the scroll area, not merely usually visible.
//!
//! The stage (her viewport) pins at the top as a collapsible
//! header per rule 2; its collapsed height persists per
//! shell session here and as Addition S phone state in the
//! settings store's keeping (A-1 keeps the collapse in the
//! shell; per-area persisted collapse is A-2's census work).

/// Android's accessibility floor for touch targets (§3.3
/// rule 3), in dp.
pub const TOUCH_TARGET_MIN_DP: f32 = 48.0;
/// The standing bar's height: one touch row plus its
/// status line, always ≥ the floor.
pub const STANDING_BAR_HEIGHT_DP: f32 = 64.0;
/// The area rail's height (one row of area chips, each a
/// touch target).
pub const RAIL_HEIGHT_DP: f32 = 56.0;
/// One area chip's minimum width (its height is the rail's).
pub const RAIL_CHIP_MIN_WIDTH_DP: f32 = 88.0;
/// A command row's height in the content column.
pub const COMMAND_ROW_HEIGHT_DP: f32 = 56.0;
/// The collapsed stage header's height (title + transport
/// row only).
pub const STAGE_COLLAPSED_HEIGHT_DP: f32 = 64.0;
/// The expanded stage's height as a fraction of the
/// viewport height (capped by `STAGE_HEIGHT_MAX_DP`).
pub const STAGE_HEIGHT_FRACTION: f32 = 0.36;
/// The expanded stage's height cap, in dp.
pub const STAGE_HEIGHT_MAX_DP: f32 = 340.0;
/// The expanded stage's height floor, in dp (below this
/// the face leaves the readability class — PROGRAM §3.2's
/// floor is a render concern; this is the header's).
pub const STAGE_HEIGHT_MIN_DP: f32 = 180.0;

/// A viewport in dp.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ViewportDp {
    /// Width in dp.
    pub width: f32,
    /// Height in dp.
    pub height: f32,
}

impl ViewportDp {
    /// Whether this viewport is portrait (the primary
    /// orientation, §3.3 rule 1).
    pub fn is_portrait(&self) -> bool {
        self.height >= self.width
    }
}

/// An axis-aligned rectangle in dp, origin top-left.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RectDp {
    /// Left edge.
    pub x: f32,
    /// Top edge.
    pub y: f32,
    /// Width.
    pub width: f32,
    /// Height.
    pub height: f32,
}

impl RectDp {
    /// The bottom edge.
    pub fn bottom(&self) -> f32 {
        self.y + self.height
    }

    /// The right edge.
    pub fn right(&self) -> f32 {
        self.x + self.width
    }

    /// Whether the rectangle lies fully inside `outer`.
    pub fn inside(&self, outer: &RectDp) -> bool {
        self.x >= outer.x
            && self.y >= outer.y
            && self.right() <= outer.right()
            && self.bottom() <= outer.bottom()
    }

    /// Whether two rectangles overlap (shared area, not
    /// just an edge).
    pub fn overlaps(&self, other: &RectDp) -> bool {
        self.x < other.right()
            && other.x < self.right()
            && self.y < other.bottom()
            && other.y < self.bottom()
    }
}

/// The Persona frame's regions for one viewport.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FrameLayout {
    /// The stage header (pinned top; her viewport + title).
    pub stage: RectDp,
    /// The scrolling content column (one panel column).
    pub content: RectDp,
    /// The area rail (pinned; never scrolls away with the
    /// content — it is the frame's navigation).
    pub rail: RectDp,
    /// The standing-controls bar (pinned to the bottom
    /// edge; §3.3 rule 5).
    pub standing_bar: RectDp,
}

/// Rule 3's floor, as a compile-time contract: the
/// interactive heights in this module never dip under
/// Android's 48 dp minimum, by construction.
const _: () = {
    assert!(TOUCH_TARGET_MIN_DP >= 48.0);
    assert!(STANDING_BAR_HEIGHT_DP >= TOUCH_TARGET_MIN_DP);
    assert!(RAIL_HEIGHT_DP >= TOUCH_TARGET_MIN_DP);
    assert!(RAIL_CHIP_MIN_WIDTH_DP >= TOUCH_TARGET_MIN_DP);
    assert!(COMMAND_ROW_HEIGHT_DP >= TOUCH_TARGET_MIN_DP);
};

/// Computes the frame layout for a viewport.
///
/// Degenerate viewports (zero or negative extents — a
/// surface mid-teardown) clamp every region to zero height
/// rather than producing negative geometry: the shell
/// treats a zero-height content region as "draw nothing",
/// the surface-loss adversarial case.
pub fn frame_layout(viewport: ViewportDp, stage_collapsed: bool) -> FrameLayout {
    let width = viewport.width.max(0.0);
    let height = viewport.height.max(0.0);
    let full = RectDp {
        x: 0.0,
        y: 0.0,
        width,
        height,
    };
    let standing_bar = RectDp {
        x: 0.0,
        y: (height - STANDING_BAR_HEIGHT_DP).max(0.0),
        width,
        height: STANDING_BAR_HEIGHT_DP.min(height),
    };
    let rail = RectDp {
        x: 0.0,
        y: (standing_bar.y - RAIL_HEIGHT_DP).max(0.0),
        width,
        height: RAIL_HEIGHT_DP.min(standing_bar.y.max(0.0)),
    };
    let stage_height = if stage_collapsed {
        STAGE_COLLAPSED_HEIGHT_DP
    } else {
        (height * STAGE_HEIGHT_FRACTION).clamp(STAGE_HEIGHT_MIN_DP, STAGE_HEIGHT_MAX_DP)
    };
    let stage = RectDp {
        x: 0.0,
        y: 0.0,
        width,
        height: stage_height.min(rail.y.max(0.0)),
    };
    let content = RectDp {
        x: 0.0,
        y: stage.bottom(),
        width,
        height: (rail.y - stage.bottom()).max(0.0),
    };
    let layout = FrameLayout {
        stage,
        content,
        rail,
        standing_bar,
    };
    debug_assert!(layout.stage.inside(&full));
    debug_assert!(layout.rail.inside(&full));
    debug_assert!(layout.standing_bar.inside(&full));
    layout
}

/// Reference phone viewports the audit and tests verify
/// the rules against: a compact Android phone, a large
/// one, and a small tablet-class portrait.
pub const REFERENCE_VIEWPORTS: [ViewportDp; 3] = [
    ViewportDp {
        width: 360.0,
        height: 640.0,
    },
    ViewportDp {
        width: 411.0,
        height: 891.0,
    },
    ViewportDp {
        width: 600.0,
        height: 960.0,
    },
];

#[cfg(test)]
mod tests {
    use super::{
        REFERENCE_VIEWPORTS, STANDING_BAR_HEIGHT_DP, TOUCH_TARGET_MIN_DP, ViewportDp, frame_layout,
    };

    /// Rule 5: the standing bar is pinned to the bottom
    /// edge and the content region excludes it, on every
    /// reference viewport, collapsed or not.
    #[test]
    fn standing_bar_is_pinned_and_content_excludes_it() {
        for viewport in REFERENCE_VIEWPORTS {
            for collapsed in [false, true] {
                let layout = frame_layout(viewport, collapsed);
                assert_eq!(layout.standing_bar.bottom(), viewport.height);
                assert_eq!(layout.standing_bar.height, STANDING_BAR_HEIGHT_DP);
                assert!(layout.content.bottom() <= layout.rail.y);
                assert!(layout.rail.bottom() <= layout.standing_bar.y);
                assert!(!layout.content.overlaps(&layout.standing_bar));
                assert!(!layout.content.overlaps(&layout.rail));
            }
        }
    }

    /// Rules 1–2: portrait viewports get a full-width
    /// single content column under a top-pinned stage,
    /// with positive content height.
    #[test]
    fn portrait_frame_is_one_column_under_a_top_stage() {
        for viewport in REFERENCE_VIEWPORTS {
            assert!(viewport.is_portrait());
            let layout = frame_layout(viewport, false);
            assert_eq!(layout.stage.y, 0.0);
            assert_eq!(layout.content.x, 0.0);
            assert_eq!(layout.content.width, viewport.width);
            assert!(layout.content.height > 0.0);
        }
    }

    /// Rule 3: every interactive height the model sizes
    /// meets the 48 dp floor (the constants themselves are
    /// compile-time asserted below the model; this checks
    /// the laid-out heights the shell actually draws).
    #[test]
    fn touch_targets_meet_the_floor() {
        for viewport in REFERENCE_VIEWPORTS {
            let layout = frame_layout(viewport, false);
            assert!(layout.standing_bar.height >= TOUCH_TARGET_MIN_DP);
            assert!(layout.rail.height >= TOUCH_TARGET_MIN_DP);
        }
    }

    /// Adversarial: a zero viewport (surface mid-teardown)
    /// yields clamped, non-negative, non-overlapping
    /// regions — draw-nothing geometry, never a panic.
    #[test]
    fn zero_viewport_clamps_to_nothing() {
        let layout = frame_layout(
            ViewportDp {
                width: 0.0,
                height: 0.0,
            },
            false,
        );
        assert_eq!(layout.content.height, 0.0);
        assert_eq!(layout.stage.height, 0.0);
        assert!(!layout.content.overlaps(&layout.standing_bar));
    }

    /// Adversarial: a viewport shorter than the frame's
    /// pinned furniture still keeps the standing bar at
    /// the bottom edge with no negative heights anywhere.
    #[test]
    fn tiny_viewport_keeps_the_standing_bar() {
        let layout = frame_layout(
            ViewportDp {
                width: 320.0,
                height: 90.0,
            },
            false,
        );
        assert_eq!(layout.standing_bar.bottom(), 90.0);
        assert!(layout.stage.height >= 0.0);
        assert!(layout.content.height >= 0.0);
        assert!(layout.rail.height >= 0.0);
    }
}
