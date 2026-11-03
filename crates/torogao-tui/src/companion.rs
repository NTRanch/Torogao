//! The companion pane (D-639): Prima, Torogao's verified
//! desktop companion, present on the Projects front door and
//! the Media screen as a baked frame loop.
//!
//! The frames are renders of Prima's own Daydream
//! performance, captured through `torogao-companion`'s
//! `--frames/--screenshot` evidence route and normalised to
//! 464×960 masters (bake provenance: the D-639 build
//! report). The TUI adds no motion of its own — posture
//! sway, pose drift and the blink are the performance's;
//! there is no synthetic blink overlay and no procedural
//! animation. This module holds the frame table, the
//! advance state, and the pure decisions (pane threshold,
//! pane fit) plus the direct-graphics blast construction
//! reusing [`crate::media`]'s machinery. Rendering the pane
//! into the ratatui buffer (half-block mode) lives in the
//! crate's view module.

use crate::app::Screen;
use crate::media::{self, GraphicsMode};
use image::RgbaImage;

/// Baked frames in the loop.
pub const FRAME_COUNT: usize = 40;
/// Master frame width in pixels (bake normalisation).
pub const FRAME_WIDTH: u32 = 464;
/// Master frame height in pixels (bake normalisation).
pub const FRAME_HEIGHT: u32 = 960;
/// The companion pane's width in terminal columns.
pub const PANE_COLS: u16 = 28;
/// Projects shows the pane at or above this terminal width.
pub const PROJECTS_MIN_COLS: u16 = 100;
/// Projects shows the pane at or above this terminal height.
pub const PROJECTS_MIN_ROWS: u16 = 30;
/// Media shows the pane at or above this terminal width
/// (the preview keeps its room first).
pub const MEDIA_MIN_COLS: u16 = 120;
/// Kitty image id for the companion placement. The Media
/// screen's ids are a serial starting at 1; this fixed
/// high id is the companion's dedicated id space, so the
/// two placements never collide.
pub const KITTY_ID: u32 = 0xFF00_0001;

/// The baked Daydream frames, embedded at build time (no
/// path resolution, no runtime file dependency — the TUI
/// runs from many directories).
static FRAMES: [&[u8]; FRAME_COUNT] = [
    include_bytes!("../assets/companion/prima-000.png"),
    include_bytes!("../assets/companion/prima-001.png"),
    include_bytes!("../assets/companion/prima-002.png"),
    include_bytes!("../assets/companion/prima-003.png"),
    include_bytes!("../assets/companion/prima-004.png"),
    include_bytes!("../assets/companion/prima-005.png"),
    include_bytes!("../assets/companion/prima-006.png"),
    include_bytes!("../assets/companion/prima-007.png"),
    include_bytes!("../assets/companion/prima-008.png"),
    include_bytes!("../assets/companion/prima-009.png"),
    include_bytes!("../assets/companion/prima-010.png"),
    include_bytes!("../assets/companion/prima-011.png"),
    include_bytes!("../assets/companion/prima-012.png"),
    include_bytes!("../assets/companion/prima-013.png"),
    include_bytes!("../assets/companion/prima-014.png"),
    include_bytes!("../assets/companion/prima-015.png"),
    include_bytes!("../assets/companion/prima-016.png"),
    include_bytes!("../assets/companion/prima-017.png"),
    include_bytes!("../assets/companion/prima-018.png"),
    include_bytes!("../assets/companion/prima-019.png"),
    include_bytes!("../assets/companion/prima-020.png"),
    include_bytes!("../assets/companion/prima-021.png"),
    include_bytes!("../assets/companion/prima-022.png"),
    include_bytes!("../assets/companion/prima-023.png"),
    include_bytes!("../assets/companion/prima-024.png"),
    include_bytes!("../assets/companion/prima-025.png"),
    include_bytes!("../assets/companion/prima-026.png"),
    include_bytes!("../assets/companion/prima-027.png"),
    include_bytes!("../assets/companion/prima-028.png"),
    include_bytes!("../assets/companion/prima-029.png"),
    include_bytes!("../assets/companion/prima-030.png"),
    include_bytes!("../assets/companion/prima-031.png"),
    include_bytes!("../assets/companion/prima-032.png"),
    include_bytes!("../assets/companion/prima-033.png"),
    include_bytes!("../assets/companion/prima-034.png"),
    include_bytes!("../assets/companion/prima-035.png"),
    include_bytes!("../assets/companion/prima-036.png"),
    include_bytes!("../assets/companion/prima-037.png"),
    include_bytes!("../assets/companion/prima-038.png"),
    include_bytes!("../assets/companion/prima-039.png"),
];

/// The total embedded byte count of the frame table (the
/// build report measures it against the spec's 2.5 MiB
/// re-bake rule).
pub fn embedded_bytes() -> usize {
    FRAMES.iter().map(|frame| frame.len()).sum()
}

/// Whether the companion pane exists on `screen` at the
/// given terminal size. A pure function of the frame size
/// so the threshold is unit-testable without a terminal.
/// Below threshold the layouts are the pre-slice layouts
/// exactly; the pane hides, it never crushes the work area.
pub fn pane_visible(screen: Screen, cols: u16, rows: u16) -> bool {
    match screen {
        Screen::Projects => cols >= PROJECTS_MIN_COLS && rows >= PROJECTS_MIN_ROWS,
        Screen::Media => cols >= MEDIA_MIN_COLS,
        _ => false,
    }
}

/// The sub-rect of `pane` the sprite occupies: the master's
/// aspect fitted inside the pane without stretching,
/// centred. Cell geometry: a terminal cell is ~1:2
/// (width:height), so the master's 464:960 pixel aspect is
/// a `cols : 2·rows` aspect in cell space. `pane` and the
/// result are `(x, y, width, height)` in cells.
pub fn fit_rect(pane: (u16, u16, u16, u16)) -> (u16, u16, u16, u16) {
    let (x, y, w, h) = pane;
    if w == 0 || h == 0 {
        return (x, y, 0, 0);
    }
    // cols/rows ratio the master needs: (464/960) * 2.
    let want = f64::from(FRAME_WIDTH) * 2.0 / f64::from(FRAME_HEIGHT);
    let cols = ((f64::from(h) * want).floor() as u16).min(w);
    let rows = if cols >= w {
        ((f64::from(w) / want).floor() as u16).min(h)
    } else {
        h
    };
    let cols = cols.max(1);
    let rows = rows.max(1);
    (x + (w - cols) / 2, y + (h - rows) / 2, cols, rows)
}

/// The companion's animation state: the current frame, its
/// decoded raster (decoded once per frame, on advance), and
/// the direct-graphics blast cache.
pub struct CompanionState {
    /// Current frame index, `0..FRAME_COUNT`.
    pub frame: usize,
    /// The decoded current frame, when decoding succeeded.
    pub raster: Option<RgbaImage>,
    /// Which frame `raster` holds, when it holds one.
    pub raster_frame: Option<usize>,
    /// The graphics blast cache: (signature, escape text)
    /// the shell writes after the frame draw.
    pub blast: Option<(u64, String)>,
}

impl Default for CompanionState {
    fn default() -> CompanionState {
        CompanionState {
            frame: 0,
            raster: None,
            raster_frame: None,
            blast: None,
        }
    }
}

impl CompanionState {
    /// The current frame's embedded PNG bytes.
    pub fn current_png(&self) -> &'static [u8] {
        FRAMES[self.frame % FRAME_COUNT]
    }

    /// Advances one frame, wrapping at the loop end. The
    /// raster cache is dropped; the next decode happens in
    /// [`CompanionState::ensure_raster`].
    pub fn advance(&mut self) {
        self.frame = (self.frame + 1) % FRAME_COUNT;
        self.raster = None;
        self.raster_frame = None;
    }

    /// Decodes the current frame if the cache does not hold
    /// it. A decode failure leaves the cache empty (the pane
    /// draws its named empty state); the embedded assets are
    /// build-time PNGs, so this is a cannot-happen guard,
    /// not an expected path.
    pub fn ensure_raster(&mut self) {
        if self.raster_frame == Some(self.frame) && self.raster.is_some() {
            return;
        }
        self.raster = image::load_from_memory(self.current_png())
            .ok()
            .map(|image| image.to_rgba8());
        self.raster_frame = if self.raster.is_some() {
            Some(self.frame)
        } else {
            None
        };
    }

    /// Builds (or reuses) the direct-graphics blast for the
    /// current frame in `pane` — the pane's inner rect as
    /// last drawn, `(x, y, width, height)` in cells. Kitty
    /// and sixel only; half-block renders in-buffer and
    /// never blasts. The signature covers frame, mode and
    /// placement, so an unchanged frame is never re-sent.
    pub fn build_blast(
        &mut self,
        mode: GraphicsMode,
        pane: (u16, u16, u16, u16),
    ) -> Option<(u64, String)> {
        if mode == GraphicsMode::HalfBlock || pane.2 == 0 || pane.3 == 0 {
            self.blast = None;
            return None;
        }
        let (fx, fy, fw, fh) = fit_rect(pane);
        let signature = blast_signature(mode, self.frame, (fx, fy, fw, fh));
        if self.blast.as_ref().map(|(sig, _)| *sig) == Some(signature) {
            return self.blast.clone();
        }
        self.ensure_raster();
        let raster = self.raster.as_ref()?;
        let text = match mode {
            GraphicsMode::Kitty => {
                let fitted = media::fit_edge(raster, media::KITTY_EDGE_MAX);
                let png = media::encode_png(&fitted).ok()?;
                media::kitty_blast(&png, KITTY_ID, fx + 1, fy + 1, fw, fh)
            }
            GraphicsMode::Sixel => {
                let fitted = media::fit_edge(raster, media::SIXEL_EDGE_MAX);
                format!("\x1b[{};{}H{}", fy + 1, fx + 1, media::sixel_blast(&fitted))
            }
            GraphicsMode::HalfBlock => unreachable!("half-block returned above"),
        };
        self.blast = Some((signature, text));
        self.blast.clone()
    }
}

/// The blast signature: FNV-1a over the mode, frame index
/// and fitted placement, so any change re-sends exactly
/// once.
fn blast_signature(mode: GraphicsMode, frame: usize, fit: (u16, u16, u16, u16)) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    let mut mix = |value: u64| {
        hash ^= value;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    };
    mix(match mode {
        GraphicsMode::Kitty => 1,
        GraphicsMode::Sixel => 2,
        GraphicsMode::HalfBlock => 3,
    });
    mix(frame as u64);
    mix(u64::from(fit.0));
    mix(u64::from(fit.1));
    mix(u64::from(fit.2));
    mix(u64::from(fit.3));
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pane_threshold_projects() {
        assert!(pane_visible(Screen::Projects, 100, 30));
        assert!(pane_visible(Screen::Projects, 120, 36));
        assert!(!pane_visible(Screen::Projects, 99, 30));
        assert!(!pane_visible(Screen::Projects, 100, 29));
        assert!(!pane_visible(Screen::Projects, 99, 29));
    }

    #[test]
    fn pane_threshold_media_and_other_screens() {
        assert!(pane_visible(Screen::Media, 120, 36));
        assert!(pane_visible(Screen::Media, 200, 40));
        assert!(!pane_visible(Screen::Media, 119, 36));
        for screen in [
            Screen::Browse,
            Screen::Detail,
            Screen::Console,
            Screen::Validate,
            Screen::Replay,
            Screen::Preview,
            Screen::Files,
            Screen::Editor,
            Screen::Settings,
            Screen::Help,
        ] {
            assert!(!pane_visible(screen, 200, 60), "{screen:?}");
        }
    }

    #[test]
    fn advance_wraps_the_loop() {
        let mut state = CompanionState::default();
        assert_eq!(state.frame, 0);
        for _ in 0..FRAME_COUNT {
            state.advance();
        }
        assert_eq!(state.frame, 0);
        state.advance();
        assert_eq!(state.frame, 1);
    }

    #[test]
    fn frames_decode_at_master_size() {
        let mut state = CompanionState::default();
        state.ensure_raster();
        let raster = state.raster.expect("embedded frame decodes");
        assert_eq!(
            (raster.width(), raster.height()),
            (FRAME_WIDTH, FRAME_HEIGHT)
        );
    }

    #[test]
    fn embedded_total_is_reported_and_bounded() {
        // The spec's re-bake rule fires above 2.5 MiB; the
        // bake at 464x960 must land under it.
        assert!(embedded_bytes() > 0);
        assert!(
            embedded_bytes() <= 2_621_440,
            "embedded total {} exceeds the 2.5 MiB rule",
            embedded_bytes()
        );
    }

    #[test]
    fn fit_rect_centres_without_stretch() {
        // The spec's reference pane interior: 26x29 cells.
        // Width binds: 26 cols at the master's cell aspect
        // (cols:rows = 464*2:960) needs 26 rows, centred.
        let (x, y, w, h) = fit_rect((10, 5, 26, 29));
        assert_eq!((w, h), (26, 26));
        assert_eq!((x, y), (10, 6));
        // A wide, short pane is height-bound.
        let (x, y, w, h) = fit_rect((0, 0, 40, 10));
        assert_eq!((w, h), (9, 10));
        assert_eq!((x, y), (15, 0));
        // A degenerate pane stays degenerate, never panics.
        assert_eq!(fit_rect((3, 3, 0, 0)), (3, 3, 0, 0));
    }

    #[test]
    fn halfblock_pane_sampling_shape() {
        let mut state = CompanionState::default();
        state.ensure_raster();
        let raster = state.raster.as_ref().expect("frame decoded");
        // The pane interior budget at reference size.
        let grid = media::halfblock_cells(raster, 26, 28);
        assert!(!grid.is_empty());
        assert!(grid.len() <= 28);
        for row in &grid {
            assert!(row.len() <= 26);
        }
    }

    #[test]
    fn blast_signature_tracks_frame_and_placement() {
        let pane = (0, 0, 26, 29);
        let a = blast_signature(GraphicsMode::Kitty, 0, pane);
        assert_ne!(a, blast_signature(GraphicsMode::Kitty, 1, pane));
        assert_ne!(a, blast_signature(GraphicsMode::Sixel, 0, pane));
        assert_ne!(a, blast_signature(GraphicsMode::Kitty, 0, (1, 0, 26, 29)));
        assert_eq!(a, blast_signature(GraphicsMode::Kitty, 0, pane));
    }
}
