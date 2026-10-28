//! The Persona presentation (Addition Y): the whole-UI
//! stage that is Torogao's default front-end, with the
//! traditional panel UI surviving as "Nerd mode" (a user
//! toggle, default OFF — the Persona presentation is the
//! default). One functional skeleton, two presentations:
//! both drive the same commands, state, and records, and
//! every registry capability reachable in Nerd mode is
//! reachable here (the parity audit below is the gate).
//!
//! This module is the presentation *model* — areas, host
//! slots, signature accents, the command skeleton, the
//! fixed-step motion state, and the twelve readability
//! rules of the Persona study (§4.7) expressed as checks
//! over the model. The windowed shell renders this model;
//! `--persona-audit` prints the checks headlessly. The
//! rules are gate criteria, not aspirations: a rule that
//! fails here fails the phase gate however the stage
//! looks. The look itself is the Owner's eye gate, queued
//! per area, never self-approved.
//!
//! Grounding: `BUILD-OUT-AMENDMENT.md` Addition Y (whole-
//! UI ruling 2026-10-26 10:22; signature-accent ruling
//! 10:11) and `persona5-menu-research.md` §4 (synthesis)
//! with §4.7 (the twelve rules). Spirit, not copy: what
//! transfers is the feedback system (one moving element,
//! one screen-unique event color, physical confirmation)
//! and the host-per-area structure — never Atlus art,
//! assets, compositions, typefaces, or the red/black/
//! white palette as a set. Torogao's identity stays
//! Addition U's black ground + pink tier family.
//!
//! Two standing constraints shape this build:
//! - AUDIO SILENCE (program rule): the study's sound
//!   half of physical confirmation is not implemented —
//!   nothing in this program plays audio. Confirmation
//!   is motion + proportion + palette only, and refusals
//!   read as refusals visually (rule 11's typed, visible
//!   half). Recorded as a deviation with cause, not
//!   smoothed over.
//! - Casting and accents are Owner rulings at this
//!   item's gate (Addition Y: "Casting actual characters
//!   into slots is an Owner/Pax decision at build time";
//!   accents are ruled per area when its area is built).
//!   The build delivers slots, registers, and the accent
//!   *mechanism* with a candidate accent per area
//!   (mutually distinct by the measured margin the audit
//!   enforces); the candidates stand for the Owner's
//!   ruling, they are not the ruling.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};

// ------------------------------------------------------------------
// Palette — Addition U tiers (the resting identity) plus the
// emergency register. Full-strength accent is the EVENT color: it
// appears only in event roles (rule 3).

/// Addition U full-strength accent (the house pink).
pub const HOUSE_ACCENT: [u8; 3] = [0xFF, 0x2E, 0x88];
/// Addition U body text tier.
pub const TEXT_RESTING: [u8; 3] = [0xFF, 0x9E, 0xC4];
/// Addition U heading tier.
pub const TEXT_HEADING: [u8; 3] = [0xFF, 0x5C, 0xA8];
/// Addition U disabled / mauve tier (also the frozen tier).
pub const TEXT_DISABLED: [u8; 3] = [0x7A, 0x4A, 0x61];
/// Addition U frame background (deep pink-black).
pub const FRAME_BG: [u8; 3] = [0x1C, 0x0B, 0x14];
/// Addition U deep rose (scrollbar grab; the background
/// pattern field's colour).
pub const PATTERN_ROSE: [u8; 3] = [0x57, 0x1E, 0x38];
/// The emergency register's ground (§4.3: accent + black
/// + this, nothing else, when a mistake is expensive).
pub const EMERGENCY_BG: [u8; 3] = [0x14, 0x0A, 0x10];
/// Black (focused-row text on the accent fill).
pub const BLACK: [u8; 3] = [0x00, 0x00, 0x00];
/// The physics debug overlay's line colour — a DATA
/// colour (G-10). Signature accents keep a measured hue
/// distance from it so a data mark can never read as a
/// selection state (Addition Y colour carve-out).
pub const PHYSICS_DATA_AMBER: [u8; 3] = [255, 206, 63];

/// What a palette colour is *for*. The event roles are
/// the only roles that may carry a full-strength accent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// Body text at rest.
    RestingText,
    /// Headings at rest.
    Heading,
    /// Borders at rest (~0.5 alpha accent in the theme).
    Border,
    /// Panel / frame background.
    FrameBg,
    /// Background pattern field.
    Pattern,
    /// Disabled and frozen (desaturated) content.
    Disabled,
    /// The focused element's fill (event).
    FocusFill,
    /// The active state of the control being pressed
    /// (event).
    ActiveEvent,
    /// The area ribbon (event).
    Ribbon,
    /// The emergency register's ground.
    EmergencyBg,
    /// Handwritten-register annotations (host paperwork
    /// values) — a resting tier, never an event.
    Annotation,
}

impl Role {
    /// Every role, for the rule-3 sweep.
    pub const ALL: [Role; 11] = [
        Role::RestingText,
        Role::Heading,
        Role::Border,
        Role::FrameBg,
        Role::Pattern,
        Role::Disabled,
        Role::FocusFill,
        Role::ActiveEvent,
        Role::Ribbon,
        Role::EmergencyBg,
        Role::Annotation,
    ];

    /// Whether this role may carry the full-strength
    /// event colour.
    pub fn is_event(self) -> bool {
        matches!(self, Role::FocusFill | Role::ActiveEvent | Role::Ribbon)
    }
}

/// The colour for a role in an area whose signature
/// accent is `accent` (the house accent when the area
/// carries none of its own — the Settings meta register).
/// Event roles return the accent at full strength; every
/// other role returns a resting tier, never the accent.
pub fn color_for(role: Role, accent: [u8; 3]) -> [u8; 3] {
    match role {
        Role::RestingText => TEXT_RESTING,
        Role::Heading => TEXT_HEADING,
        Role::Border => PATTERN_ROSE,
        Role::FrameBg => FRAME_BG,
        Role::Pattern => PATTERN_ROSE,
        Role::Disabled => TEXT_DISABLED,
        Role::FocusFill | Role::ActiveEvent | Role::Ribbon => accent,
        Role::EmergencyBg => EMERGENCY_BG,
        Role::Annotation => TEXT_RESTING,
    }
}

/// HSV hue of an RGB colour, in degrees 0..360 (0 for
/// greys). Used by the accent-distinctness audit.
pub fn hue_degrees(rgb: [u8; 3]) -> f32 {
    let (r, g, b) = (
        f32::from(rgb[0]) / 255.0,
        f32::from(rgb[1]) / 255.0,
        f32::from(rgb[2]) / 255.0,
    );
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let delta = max - min;
    if delta <= f32::EPSILON {
        return 0.0;
    }
    let hue = if (max - r).abs() <= f32::EPSILON {
        60.0 * (((g - b) / delta) % 6.0)
    } else if (max - g).abs() <= f32::EPSILON {
        60.0 * ((b - r) / delta + 2.0)
    } else {
        60.0 * ((r - g) / delta + 4.0)
    };
    if hue < 0.0 { hue + 360.0 } else { hue }
}

/// Circular distance between two hues, in degrees.
pub fn hue_distance(a: f32, b: f32) -> f32 {
    let d = (a - b).abs() % 360.0;
    if d > 180.0 { 360.0 - d } else { d }
}

/// The minimum hue distance signature accents keep from
/// each other and from the physics data colour.
pub const ACCENT_HUE_MARGIN_MIN: f32 = 18.0;

// ------------------------------------------------------------------
// Areas, hosts, registers, accents.

/// How much choreography an area's surface carries
/// (§4.4: the motion budget is inverse to reading load).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadingLoad {
    /// Authoring-class surface: full choreography
    /// (15-step brushstroke wipe between areas).
    Choreographed,
    /// Reading surface (ledger / audit / meta): ambient-
    /// only motion, numbers nailed still (rule 12); area
    /// switches are instant, no wipe.
    AmbientOnly,
}

/// The wipe length for choreographed areas, in fixed
/// steps at the shell's 60 steps/second clock (~15
/// frames per the study's measurement).
pub const WIPE_STEPS_CHOREOGRAPHED: u32 = 15;
/// Host pull-forward length in fixed steps: on focus the
/// host is pulled forward once over this many steps,
/// then still.
pub const HOST_PULL_STEPS: u32 = 6;
/// The host's idle-motion salience budget (rule 2/10:
/// capped below the cursor's salience).
pub const HOST_IDLE_SALIENCE: f32 = 0.35;
/// The focus's salience (the one moving element of
/// consequence moves at full strength).
pub const FOCUS_SALIENCE: f32 = 1.0;

/// Stage-space layout rectangles [x, y, w, h] in a
/// 100x100 stage: the command list's region and the
/// host's fixed edge region (rule 10: fixed edge
/// placement, disjoint from the options).
pub const LIST_REGION: [f32; 4] = [0.0, 0.0, 76.0, 100.0];
/// The host cut-in's region: the right edge.
pub const HOST_REGION: [f32; 4] = [78.0, 0.0, 22.0, 100.0];

/// One feature area of the Persona stage, in rail order
/// (the order BRIEF-P6 Item 10 names them).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Area {
    /// The Playroom — the live puppet stage.
    Playroom,
    /// The Character Creator.
    Creator,
    /// Scenes.
    Scenes,
    /// Effects.
    Effects,
    /// Audio.
    Audio,
    /// The Writer.
    Writer,
    /// Video.
    Video,
    /// Tracking / Interop.
    Tracking,
    /// The Toolbox.
    Toolbox,
    /// Convert / Conversions.
    Convert,
    /// Tutorials.
    Tutorials,
    /// Settings / System — the hostless meta register.
    Settings,
}

impl Area {
    /// Every area, in rail order.
    pub const ALL: [Area; 12] = [
        Area::Playroom,
        Area::Creator,
        Area::Scenes,
        Area::Effects,
        Area::Audio,
        Area::Writer,
        Area::Video,
        Area::Tracking,
        Area::Toolbox,
        Area::Convert,
        Area::Tutorials,
        Area::Settings,
    ];

    /// The area's record: goal label, host slot, register,
    /// candidate accent, reading load.
    pub fn info(self) -> &'static AreaInfo {
        &AREAS[self as usize]
    }

    /// The area's stable id (persistence, audit output).
    pub fn id(self) -> &'static str {
        self.info().id
    }

    /// Area lookup by stable id.
    pub fn from_id(id: &str) -> Option<Area> {
        Area::ALL.iter().copied().find(|area| area.id() == id)
    }
}

/// One area's fixed record. All Persona strings live in
/// this table and its siblings — the E12 precondition
/// (one auditable string table) is structural here.
#[derive(Debug, Clone, Copy)]
pub struct AreaInfo {
    /// Stable id.
    pub id: &'static str,
    /// The goal label the menu is known by (rule 7).
    pub title: &'static str,
    /// The host slot ("HOST-01 · THE COUTURIER"), or
    /// None for the deliberately hostless meta register.
    pub host_slot: Option<&'static str>,
    /// The host's trade-paperwork register.
    pub register: &'static str,
    /// The candidate signature accent (Owner rules per
    /// area at the eye gate; these candidates satisfy the
    /// distinctness audit). None = the area carries no
    /// accent of its own and rests in the house tiers
    /// (the meta register's deliberate break).
    pub accent: Option<[u8; 3]>,
    /// The accent's provenance note for the eye gate.
    pub accent_note: &'static str,
    /// Choreography class (rule 12).
    pub reading_load: ReadingLoad,
}

/// The twelve areas with their slots, registers, and
/// candidate accents. Accent hues were spread around the
/// wheel to satisfy ACCENT_HUE_MARGIN_MIN pairwise and
/// against the physics data amber; the eye gate rules
/// them per area.
pub const AREAS: [AreaInfo; 12] = [
    AreaInfo {
        id: "playroom",
        title: "Playroom",
        host_slot: Some("THE PLAYMATE · user-chosen (Addition B)"),
        register: "the staged room",
        accent: Some(HOUSE_ACCENT),
        accent_note: "the house accent itself — the Playroom is the identity's home stage, and no other area shares it",
        reading_load: ReadingLoad::Choreographed,
    },
    AreaInfo {
        id: "creator",
        title: "Creator",
        host_slot: Some("HOST-01 · THE COUTURIER"),
        register: "fitting room — measurement cards, tailor's tape, named fitting cards",
        accent: Some([0x9B, 0x5D, 0xE5]),
        accent_note: "candidate: violet",
        reading_load: ReadingLoad::Choreographed,
    },
    AreaInfo {
        id: "scenes",
        title: "Scenes",
        host_slot: Some("HOST-02 · THE DIRECTOR"),
        register: "playbill — act cards, the lab chart as sub-register",
        accent: Some([0x2E, 0xC4, 0xB6]),
        accent_note: "candidate: teal (stage gel)",
        reading_load: ReadingLoad::Choreographed,
    },
    AreaInfo {
        id: "effects",
        title: "Effects",
        host_slot: Some("HOST-03 · THE PROJECTIONIST"),
        register: "darkroom — film-leader strip, contact-sheet frames",
        accent: Some([0x4C, 0xC9, 0xF0]),
        accent_note: "candidate: cyan",
        reading_load: ReadingLoad::Choreographed,
    },
    AreaInfo {
        id: "audio",
        title: "Audio",
        host_slot: Some("HOST-04 · THE ENGINEER"),
        register: "studio track sheet — tape labels, loudness notes in the annotation register",
        accent: Some([0x3E, 0xD9, 0x7E]),
        accent_note: "candidate: green",
        reading_load: ReadingLoad::Choreographed,
    },
    AreaInfo {
        id: "writer",
        title: "Writer",
        host_slot: Some("HOST-05 · THE AUTHOR"),
        register: "manuscript — page stack, canon gaps as margin notes (dressed, still loud)",
        accent: Some([0xBC, 0x47, 0x49]),
        accent_note: "candidate: brick — flagged for the eye gate: nearest candidate to a red; re-rule freely",
        reading_load: ReadingLoad::Choreographed,
    },
    AreaInfo {
        id: "video",
        title: "Video",
        host_slot: Some("HOST-06 · THE EDITOR"),
        register: "cutting room — film-strip frames on the light-table ground",
        accent: Some([0x43, 0x61, 0xEE]),
        accent_note: "candidate: royal blue",
        reading_load: ReadingLoad::Choreographed,
    },
    AreaInfo {
        id: "tracking",
        title: "Tracking",
        host_slot: Some("HOST-07 · THE OPERATOR"),
        register: "broadcast — signal scopes, the patch-bay list; her freeze signals tracking loss",
        accent: Some([0xBE, 0xDF, 0x21]),
        accent_note: "candidate: signal lime (hue ~71 deg: >= 18 deg clear of the physics data amber at ~45 deg, per the audit)",
        reading_load: ReadingLoad::Choreographed,
    },
    AreaInfo {
        id: "toolbox",
        title: "Toolbox",
        host_slot: Some("HOST-08 · THE QUARTERMASTER"),
        register: "warehouse ledger — manifests as ledger pages, counts in the annotation register",
        accent: Some([0x7F, 0xB0, 0x69]),
        accent_note: "candidate: sage green (darkened from the first candidate so its hue clears Tracking's lime by the audit margin)",
        reading_load: ReadingLoad::AmbientOnly,
    },
    AreaInfo {
        id: "convert",
        title: "Convert",
        host_slot: Some("HOST-09 · THE COURIER"),
        register: "shipping manifest — forms, not lists; the byte-stability seal stamped on the manifest",
        accent: Some([0xFF, 0x7B, 0x33]),
        accent_note: "candidate: courier orange (HOST-08 holds the asset-side manifest register inside this area until Addition X lands its own tool)",
        reading_load: ReadingLoad::Choreographed,
    },
    AreaInfo {
        id: "tutorials",
        title: "Tutorials",
        host_slot: Some("HOST-10 · THE TUTOR"),
        register: "classroom — syllabus board, ink-stamp completions, the report card",
        accent: Some([0xD3, 0x36, 0xD3]),
        accent_note: "candidate: magenta",
        reading_load: ReadingLoad::Choreographed,
    },
    AreaInfo {
        id: "settings",
        title: "Settings",
        host_slot: None,
        register: "the meta register — deep tiers, the mechanical motif; leaving the fiction is the signal",
        accent: None,
        accent_note: "hostless and accentless by design (Addition Y); HOST-11 · THE ARCHIVIST is the recorded alternative if the Owner wants total coverage",
        reading_load: ReadingLoad::AmbientOnly,
    },
];

/// The event colour in force for an area: its signature
/// accent, or the house accent for the accentless meta
/// register (at its deep tiers — the fallback is the
/// identity, never another area's colour).
pub fn event_accent(area: Area) -> [u8; 3] {
    area.info().accent.unwrap_or(HOUSE_ACCENT)
}

// ------------------------------------------------------------------
// The command skeleton: registry tools homed by functional class,
// plus the shell's native commands. One skeleton under both
// presentations; the Nerd-mode Tools panel lists the registry
// directly, so parity = this table covering the registry exactly.

/// The shell's native (non-registry) commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeCommand {
    /// Toggle the physics debug overlay (Nerd mode: the
    /// Model panel's checkbox).
    TogglePhysicsOverlay,
    /// Toggle Nerd mode (Settings meta register + hotkey;
    /// persisted as Addition S state).
    ToggleNerdMode,
    /// Save the session state (Nerd mode: Save state).
    SaveSessionState,
    /// Load a session state (Nerd mode: Load state).
    LoadSessionState,
}

/// What a command runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandKind {
    /// A registry tool, by name.
    Tool(&'static str),
    /// A shell-native command.
    Native(NativeCommand),
}

/// A command's layout family. Opposite operations never
/// share layout + palette (rule 6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    /// A row in the area's single-axis list.
    ListRow,
    /// A form anchored to the left of the stage.
    FormLeft,
    /// A form anchored to the right of the stage.
    FormRight,
}

/// A command's palette register.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Register {
    /// The resting tiers.
    Resting,
    /// Outbound (writes state out — save).
    Outbound,
    /// Inbound (reads state in — load).
    Inbound,
    /// Accent + black + emergency ground only.
    Emergency,
}

impl Register {
    /// The register's signature colour in an area.
    pub fn color(self, area: Area) -> [u8; 3] {
        match self {
            Register::Resting => TEXT_HEADING,
            Register::Outbound => event_accent(area),
            Register::Inbound => TEXT_RESTING,
            Register::Emergency => event_accent(area),
        }
    }
}

/// One command on an area's stage.
#[derive(Debug, Clone)]
pub struct Command {
    /// Stable id (tool name or native id).
    pub id: String,
    /// The goal label (the registry title, or the
    /// native command's label).
    pub label: String,
    /// The glanceable summary shown at rest (rule 4:
    /// non-focus shows summary only).
    pub summary: String,
    /// The full detail shown on focus (rule 4).
    pub detail: String,
    /// The printed activation affordance (rule 9: every
    /// control prints how it activates).
    pub affordance: String,
    /// What the command runs.
    pub kind: CommandKind,
    /// Layout family (rule 6).
    pub layout: Layout,
    /// Palette register (rule 6).
    pub register: Register,
}

/// The tool → (area, functional class) home table. Each
/// registry tool appears exactly once (the parity
/// audit). Classes are the RP4 inventory's F1–F14
/// regrouping; F10 splits by its own name — scene tools
/// to Scenes, effects/fluid tools to Effects — because
/// Addition Y gives scenes and effects separate stages.
pub const TOOL_HOMES: [(&str, Area, &str); 57] = [
    // Playroom — F9 playroom & session lifecycle + F2
    // state/preview/physics evaluation (the live stage).
    ("playroom-create", Area::Playroom, "F9"),
    ("playroom-open", Area::Playroom, "F9"),
    ("playroom-set", Area::Playroom, "F9"),
    ("playroom-compose", Area::Playroom, "F9"),
    ("session-validate", Area::Playroom, "F9"),
    ("apply-state", Area::Playroom, "F2"),
    ("physics-preview", Area::Playroom, "F2"),
    ("list-states", Area::Playroom, "F2"),
    // Creator — F3 new-file authoring + F8 packs /
    // templates / preference data (the fitting cards) +
    // F11 consumer-layer reads (the counterparts of
    // F3's authors).
    ("create-character", Area::Creator, "F3"),
    ("hitareas-author", Area::Creator, "F3"),
    ("events-author", Area::Creator, "F3"),
    ("instantiate-template", Area::Creator, "F3"),
    ("pack-info", Area::Creator, "F8"),
    ("list-templates", Area::Creator, "F8"),
    ("prefset-list", Area::Creator, "F8"),
    ("prefset-save", Area::Creator, "F8"),
    ("prefset-apply", Area::Creator, "F8"),
    ("events-list", Area::Creator, "F11"),
    ("hitareas-list", Area::Creator, "F11"),
    // Scenes — F10, the scene half.
    ("scene-info", Area::Scenes, "F10"),
    ("scene-validate", Area::Scenes, "F10"),
    // Effects — F10, the effects/fluids half.
    ("effects-validate", Area::Effects, "F10"),
    ("effects-compile", Area::Effects, "F10"),
    ("fluid-sim", Area::Effects, "F10"),
    // Audio — F6.
    ("audio-bake", Area::Audio, "F6"),
    ("audio-render", Area::Audio, "F6"),
    ("audio-validate", Area::Audio, "F6"),
    ("sfx-list", Area::Audio, "F6"),
    ("session-build", Area::Audio, "F6"),
    ("lipsync-analyze", Area::Audio, "F6"),
    // Writer — F13.
    ("writer-scaffold", Area::Writer, "F13"),
    ("writer-draft", Area::Writer, "F13"),
    // Video — F14.
    ("video-build", Area::Video, "F14"),
    ("video-validate", Area::Video, "F14"),
    ("video-tempo", Area::Video, "F14"),
    ("video-export", Area::Video, "F14"),
    // Tracking — F7 viseme & tracking.
    ("viseme-author", Area::Tracking, "F7"),
    ("viseme-bake", Area::Tracking, "F7"),
    ("viseme-validate", Area::Tracking, "F7"),
    ("tracking-bake", Area::Tracking, "F7"),
    ("tracking-validate", Area::Tracking, "F7"),
    // Toolbox — F1 census/validation/governance (the
    // ledger) + F4 the disabled mutation (its gate state
    // is reported here, where governance lives).
    ("inspect", Area::Toolbox, "F1"),
    ("project-info", Area::Toolbox, "F1"),
    ("validate", Area::Toolbox, "F1"),
    ("expression-check", Area::Toolbox, "F1"),
    ("rating-check", Area::Toolbox, "F1"),
    ("tracking-check", Area::Toolbox, "F1"),
    ("viseme-check", Area::Toolbox, "F1"),
    ("voice-check", Area::Toolbox, "F1"),
    ("apply-fixes", Area::Toolbox, "F4"),
    // Convert — F5 rig export & 3D interchange.
    ("export-rig", Area::Convert, "F5"),
    ("glb-info", Area::Convert, "F5"),
    ("glb-expression-check", Area::Convert, "F5"),
    ("glb-resolve-expression", Area::Convert, "F5"),
    // Tutorials — F12.
    ("tutorial-list", Area::Tutorials, "F12"),
    ("tutorial-status", Area::Tutorials, "F12"),
    ("tutorial-check", Area::Tutorials, "F12"),
];

/// The area a registry tool is homed in, when it is a
/// registry tool at all.
pub fn area_of_tool(name: &str) -> Option<Area> {
    TOOL_HOMES
        .iter()
        .find(|(tool, _, _)| *tool == name)
        .map(|(_, area, _)| *area)
}

/// The class → home-area consistency map the audit
/// enforces (F10 is the one documented split).
fn class_home(class: &str, tool: &str) -> Area {
    match class {
        "F1" | "F4" => Area::Toolbox,
        "F2" | "F9" => Area::Playroom,
        "F3" | "F8" | "F11" => Area::Creator,
        "F5" => Area::Convert,
        "F6" => Area::Audio,
        "F7" => Area::Tracking,
        "F10" => {
            if tool.starts_with("scene-") {
                Area::Scenes
            } else {
                Area::Effects
            }
        }
        "F12" => Area::Tutorials,
        "F13" => Area::Writer,
        "F14" => Area::Video,
        _ => Area::Settings,
    }
}

/// The native commands per area.
pub fn native_commands(area: Area) -> Vec<Command> {
    match area {
        Area::Playroom => vec![Command {
            id: "native:toggle-physics-overlay".to_string(),
            label: "Physics debug overlay".to_string(),
            summary: "Physics debug overlay — toggle".to_string(),
            detail: "Draws the engine's physics debug segments (anchor-to-bob) over the viewport. A data overlay: its amber is a data colour, never a selection state.".to_string(),
            affordance: "Enter or click — toggle".to_string(),
            kind: CommandKind::Native(NativeCommand::TogglePhysicsOverlay),
            layout: Layout::ListRow,
            register: Register::Resting,
        }],
        Area::Settings => vec![
            Command {
                id: "native:toggle-nerd-mode".to_string(),
                label: "Nerd mode".to_string(),
                summary: "Nerd mode — the traditional panel presentation".to_string(),
                detail: "Swaps the whole UI between the Persona stage (default) and Nerd mode (the traditional toolbar/panel presentation). One functional skeleton under both; the choice persists as Addition S state.".to_string(),
                affordance: "F9 or click — toggle".to_string(),
                kind: CommandKind::Native(NativeCommand::ToggleNerdMode),
                layout: Layout::ListRow,
                register: Register::Resting,
            },
            Command {
                id: "native:save-session-state".to_string(),
                label: "Save state".to_string(),
                summary: "Save state — write the session to a file".to_string(),
                detail: "Captures the session (genesis + admitted op log + play state) through the AUTH-D2 decide() write gate. Refuses the loaded model's own path and refuses to overwrite without force — the shell never forces.".to_string(),
                affordance: "Enter or click — save to the path in the form".to_string(),
                kind: CommandKind::Native(NativeCommand::SaveSessionState),
                layout: Layout::FormLeft,
                register: Register::Outbound,
            },
            Command {
                id: "native:load-session-state".to_string(),
                label: "Load state".to_string(),
                summary: "Load state — restore a session from a file".to_string(),
                detail: "Restores a saved session: the op log replays through a fresh session and the play state restores frame-exactly by its recorded trajectory. Opposite of Save state in layout and palette, by rule.".to_string(),
                affordance: "Enter or click — load from the path in the form".to_string(),
                kind: CommandKind::Native(NativeCommand::LoadSessionState),
                layout: Layout::FormRight,
                register: Register::Inbound,
            },
        ],
        _ => Vec::new(),
    }
}

/// An area's full command list: its registry tools (in
/// registry order) followed by its native commands.
/// Labels and details come from the registry itself —
/// the Persona layer invents no tool wording (rule 7:
/// menus are labeled by the user's goal; the registry
/// titles are those labels).
pub fn commands(area: Area) -> Vec<Command> {
    let mut out: Vec<Command> = Vec::new();
    for meta in crate::tools::tools() {
        if area_of_tool(meta.name) != Some(area) {
            continue;
        }
        let mut detail = meta.description.to_string();
        if !meta.gate.is_empty() {
            detail.push_str(&format!(" Gate: {}", meta.gate));
        }
        detail.push_str(&format!(" Posture: {}.", meta.posture));
        out.push(Command {
            id: meta.name.to_string(),
            label: meta.title.to_string(),
            summary: format!("{} — {}", meta.title, meta.posture),
            detail,
            affordance: "Enter or double-click — edit arguments, then Run".to_string(),
            kind: CommandKind::Tool(meta.name),
            layout: Layout::ListRow,
            register: if meta.name == "apply-fixes" {
                Register::Emergency
            } else {
                Register::Resting
            },
        });
    }
    out.extend(native_commands(area));
    out
}

/// Opposite-operation pairs the rule-6 audit checks:
/// (an outbound command, its inbound opposite).
pub const OPPOSITE_PAIRS: [(&str, &str); 1] =
    [("native:save-session-state", "native:load-session-state")];

// ------------------------------------------------------------------
// Pinned context (rule 5): the comparison context that stays on
// screen in every area — the puppet being edited and the values
// that keep a user from holding state in memory across menus.

/// The pinned comparison context: present verbatim in
/// every area's stage view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StageContext {
    /// The loaded model's display name ("no model loaded"
    /// when the stage is empty).
    pub model_label: String,
    /// Node count of the loaded model (0 when empty).
    pub node_count: usize,
    /// Parameter count of the loaded model (0 when empty).
    pub param_count: usize,
    /// The selected node index, when one is selected.
    pub selected_node: Option<usize>,
    /// The transport state line ("playing Greeting ·
    /// frame 61/120", "stopped", …).
    pub play_state: String,
}

impl Default for StageContext {
    fn default() -> StageContext {
        StageContext {
            model_label: "no model loaded".to_string(),
            node_count: 0,
            param_count: 0,
            selected_node: None,
            play_state: "stopped".to_string(),
        }
    }
}

/// One area's stage view: the area record, its commands,
/// and the pinned context (identical in every view).
#[derive(Debug, Clone)]
pub struct StageView {
    /// The area being staged.
    pub area: Area,
    /// The area's command list (single axis).
    pub commands: Vec<Command>,
    /// The pinned comparison context.
    pub pinned: StageContext,
}

/// Builds the stage view for an area under a context.
pub fn stage_view(area: Area, context: &StageContext) -> StageView {
    StageView {
        area,
        commands: commands(area),
        pinned: context.clone(),
    }
}

// ------------------------------------------------------------------
// Display treatment (§4.2): a procedural per-letter treatment for
// area titles and the focused row only — generated from the string
// at runtime, so no per-menu title art exists. Body text never
// passes through here.

/// One letter's treatment: rotation in degrees and a
/// vertical offset in points, both deterministic in the
/// title string (a stable hash, never a clock or an RNG
/// — the determinism contract covers menu motion too).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LetterTreatment {
    /// The letter.
    pub ch: char,
    /// Rotation, degrees, within ±6.
    pub rotation_deg: f32,
    /// Vertical offset, points, within ±1.5.
    pub y_offset: f32,
}

/// The display treatment for a title string.
pub fn display_treatment(title: &str) -> Vec<LetterTreatment> {
    title
        .chars()
        .enumerate()
        .map(|(index, ch)| {
            // FNV-1a over (title, index): stable across runs
            // and machines, like the state hashes.
            let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
            for byte in title.bytes().chain([index as u8]) {
                hash ^= u64::from(byte);
                hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
            }
            let unit = (hash % 2001) as f32 / 1000.0 - 1.0; // -1..=1
            let unit2 = ((hash >> 16) % 2001) as f32 / 1000.0 - 1.0;
            LetterTreatment {
                ch,
                rotation_deg: unit * 6.0,
                y_offset: unit2 * 1.5,
            }
        })
        .collect()
}

// ------------------------------------------------------------------
// Motion + navigation state. Everything here advances on the
// shell's fixed-step clock (STEP_SECONDS): menu motion is stepped,
// never wall-clock — `tick()` is called once per fixed step.

/// The one moving element of consequence (rule 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MovingElement {
    /// The focus (steady state).
    Focus,
    /// The brushstroke wipe (area transition in flight;
    /// the wipe is the focus's carrier).
    Wipe,
}

/// An in-flight area transition.
#[derive(Debug, Clone, Copy)]
pub struct Transition {
    /// The area being left (frozen on arrival).
    pub from: Area,
    /// The area being entered.
    pub to: Area,
    /// Steps elapsed.
    pub step: u32,
    /// Steps total (0 for reading surfaces: instant).
    pub total: u32,
}

/// The Persona presentation's state.
#[derive(Debug, Clone)]
pub struct PersonaState {
    /// The area on stage.
    pub current: Area,
    /// Areas visited this session (their focus is kept —
    /// freeze, don't erase).
    visited: BTreeSet<Area>,
    /// Per-area focus index, kept across switches (the
    /// frozen drill-down state).
    focus: BTreeMap<Area, usize>,
    /// The in-flight transition, when one runs.
    transition: Option<Transition>,
    /// Host pull-forward step, 0..=HOST_PULL_STEPS.
    pull_step: u32,
    /// Whether Nerd mode is on (the persisted preference;
    /// false = the Persona stage, the default).
    pub nerd_mode: bool,
}

impl Default for PersonaState {
    fn default() -> PersonaState {
        PersonaState::new()
    }
}

impl PersonaState {
    /// A fresh Persona state: the Playroom on stage (the
    /// staged room is the product's front door), Persona
    /// presentation (Nerd mode off).
    pub fn new() -> PersonaState {
        let mut visited = BTreeSet::new();
        visited.insert(Area::Playroom);
        PersonaState {
            current: Area::Playroom,
            visited,
            focus: BTreeMap::new(),
            transition: None,
            pull_step: HOST_PULL_STEPS,
            nerd_mode: false,
        }
    }

    /// The focus index within an area's command list
    /// (0 when never focused; always in range).
    pub fn focus_index(&self, area: Area) -> usize {
        let count = commands(area).len();
        if count == 0 {
            return 0;
        }
        self.focus.get(&area).copied().unwrap_or(0).min(count - 1)
    }

    /// The focused command of the current area, when the
    /// area has commands.
    pub fn focused_command(&self) -> Option<Command> {
        let list = commands(self.current);
        list.get(self.focus_index(self.current)).cloned()
    }

    /// Moves the focus one step along the area's single
    /// axis (wrapping; rule 1's only movers are next /
    /// prev / jump). Restarts the host pull.
    pub fn focus_next(&mut self) {
        self.move_focus(1);
    }

    /// Moves the focus one step back along the axis.
    pub fn focus_prev(&mut self) {
        self.move_focus(-1);
    }

    fn move_focus(&mut self, delta: i32) {
        let count = commands(self.current).len();
        if count == 0 {
            return;
        }
        let current = self.focus_index(self.current) as i32;
        let next = (current + delta).rem_euclid(count as i32);
        self.focus.insert(self.current, next as usize);
        self.pull_step = 0;
    }

    /// Sets the focus directly (a click on a row).
    pub fn set_focus(&mut self, index: usize) {
        let count = commands(self.current).len();
        if count == 0 {
            return;
        }
        self.focus.insert(self.current, index.min(count - 1));
        self.pull_step = 0;
    }

    /// Jump navigation (rule 8): moves the focus to the
    /// first command whose label starts with `letter`
    /// (case-insensitive), searching from just after the
    /// current focus and wrapping. Returns whether a
    /// match exists.
    pub fn jump_to(&mut self, letter: char) -> bool {
        let list = commands(self.current);
        if list.is_empty() {
            return false;
        }
        let start = self.focus_index(self.current);
        for offset in 1..=list.len() {
            let index = (start + offset) % list.len();
            let first = list[index].label.chars().find(|c| c.is_alphabetic());
            if let Some(c) = first {
                if c.eq_ignore_ascii_case(&letter) {
                    self.focus.insert(self.current, index);
                    self.pull_step = 0;
                    return true;
                }
            }
        }
        false
    }

    /// Switches the stage to another area. The area left
    /// behind keeps its focus — frozen, desaturated,
    /// resumable (freeze, don't erase). Choreographed
    /// areas transition over the wipe; reading surfaces
    /// switch instantly (rule 12).
    pub fn switch_area(&mut self, to: Area) {
        if to == self.current {
            return;
        }
        let from = self.current;
        self.visited.insert(to);
        self.current = to;
        self.pull_step = 0;
        let total = match to.info().reading_load {
            ReadingLoad::Choreographed => WIPE_STEPS_CHOREOGRAPHED,
            ReadingLoad::AmbientOnly => 0,
        };
        self.transition = if total == 0 {
            None
        } else {
            Some(Transition {
                from,
                to,
                step: 0,
                total,
            })
        };
    }

    /// Whether an area is frozen: visited, not on stage —
    /// its drill-down state stands where it was left,
    /// rendered desaturated toward the mauve tier.
    pub fn is_frozen(&self, area: Area) -> bool {
        area != self.current && self.visited.contains(&area)
    }

    /// The in-flight transition, when one runs.
    pub fn transition(&self) -> Option<Transition> {
        self.transition
    }

    /// Wipe progress 0..=1 (1 when no transition runs).
    pub fn wipe_progress(&self) -> f32 {
        match self.transition {
            Some(t) if t.total > 0 => t.step as f32 / t.total as f32,
            _ => 1.0,
        }
    }

    /// The one moving element of consequence right now
    /// (rule 2 — exactly one, always).
    pub fn moving_element(&self) -> MovingElement {
        if self.transition.is_some() {
            MovingElement::Wipe
        } else {
            MovingElement::Focus
        }
    }

    /// The host's pull-forward amount, 0..=1: on focus
    /// the host is pulled forward once, then still.
    pub fn host_pull(&self) -> f32 {
        self.pull_step as f32 / HOST_PULL_STEPS as f32
    }

    /// Advances the motion state by one fixed step.
    pub fn tick(&mut self) {
        if let Some(mut t) = self.transition {
            t.step += 1;
            if t.step >= t.total {
                self.transition = None;
            } else {
                self.transition = Some(t);
            }
        }
        if self.pull_step < HOST_PULL_STEPS {
            self.pull_step += 1;
        }
    }

    /// The wipe length an area switch into `area` uses.
    pub fn wipe_steps(area: Area) -> u32 {
        match area.info().reading_load {
            ReadingLoad::Choreographed => WIPE_STEPS_CHOREOGRAPHED,
            ReadingLoad::AmbientOnly => 0,
        }
    }
}

// ------------------------------------------------------------------
// Rule 11 presentation: a result's register. A refusal is typed,
// visible, and visually distinct from a confirmation — the sound
// half of the study's rule is not implemented under the AUDIO
// SILENCE program rule (see the module note).

/// How a command result presents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResultRegister {
    /// A confirmation: resting tiers.
    Confirmation,
    /// A refusal: the emergency register (accent +
    /// black + emergency ground) and the REFUSED prefix.
    Refusal,
}

/// The presentation for a tool result: errors present
/// as Refusal, payloads as Confirmation.
pub fn result_register(is_error: bool) -> ResultRegister {
    if is_error {
        ResultRegister::Refusal
    } else {
        ResultRegister::Confirmation
    }
}

// ------------------------------------------------------------------
// The persisted preference (Addition S state): the Nerd-mode
// choice, beside the tutorial progress file under the same state
// directory. Written only when the toggle changes — Phase F adds
// exactly this one write class to the app's C2 audit list.

/// The persisted presentation preference.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct PreferenceState {
    /// Whether Nerd mode is on. Default false: the
    /// Persona stage is the default UI.
    pub nerd_mode: bool,
}

/// The preference file's path under the Addition S
/// state directory, when one resolves.
pub fn preference_path() -> Option<PathBuf> {
    torogao_cli::paths::base_dir(torogao_cli::paths::PathKind::State)
        .map(|dir| PathBuf::from(dir).join("persona-preference.json"))
}

/// Loads the preference from a path: a missing or
/// unreadable file yields the default (Persona), never
/// an error — a preference must not brick the shell.
pub fn load_preference(path: &std::path::Path) -> PreferenceState {
    match std::fs::read_to_string(path) {
        Ok(text) => serde_json::from_str(&text).unwrap_or_default(),
        Err(_) => PreferenceState::default(),
    }
}

/// Saves the preference (temp file + rename, the save
/// module's discipline: a crash never leaves a torn
/// preference).
pub fn save_preference(path: &std::path::Path, pref: &PreferenceState) -> AppResult<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| AppError::Io {
            context: "create preference directory",
            message: err.to_string(),
        })?;
    }
    let text = serde_json::to_string(pref).map_err(|err| AppError::Format(err.to_string()))?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, text).map_err(|err| AppError::Io {
        context: "write preference",
        message: err.to_string(),
    })?;
    std::fs::rename(&tmp, path).map_err(|err| AppError::Io {
        context: "rename preference",
        message: err.to_string(),
    })?;
    Ok(())
}

// ------------------------------------------------------------------
// The audit: the twelve rules + parity + accent distinctness,
// evaluated over the model. This is the mechanically checkable
// half of the RP5 gate; the look is the Owner's eye gate.

/// One audit line.
#[derive(Debug, Clone, Serialize)]
pub struct AuditLine {
    /// What was checked ("rule 3", "parity", …).
    pub check: String,
    /// Whether it holds.
    pub pass: bool,
    /// The measured evidence.
    pub evidence: String,
}

/// Runs the full audit. Every line must pass for the
/// gate; the audit is data (the CLI mode prints it as
/// JSON, the tests assert on it directly).
pub fn audit() -> Vec<AuditLine> {
    let mut lines: Vec<AuditLine> = Vec::new();
    let registry = crate::tools::tools();
    let registry_names: BTreeSet<&str> = registry.iter().map(|meta| meta.name).collect();

    // ---- parity: Persona coverage == registry == Nerd
    // coverage (the Nerd Tools panel lists the registry
    // itself, so Nerd coverage is the registry by
    // construction; the check pins that the Persona
    // table neither drops nor duplicates a tool).
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    let mut duplicates: Vec<&str> = Vec::new();
    for (name, _, _) in TOOL_HOMES {
        if !seen.insert(name) {
            duplicates.push(name);
        }
    }
    let missing: Vec<&str> = registry_names.difference(&seen).copied().collect();
    let unknown: Vec<&str> = seen.difference(&registry_names).copied().collect();
    lines.push(AuditLine {
        check: "parity".to_string(),
        pass: missing.is_empty() && unknown.is_empty() && duplicates.is_empty(),
        evidence: format!(
            "registry {} tools; Persona homes {}; missing {:?}; unknown {:?}; duplicates {:?}; Nerd surface = the registry itself ({} listed)",
            registry.len(),
            seen.len(),
            missing,
            unknown,
            duplicates,
            registry.len()
        ),
    });

    // ---- rule 1: single-axis lists — next walks every
    // index exactly once and returns; prev inverts next.
    let mut r1_fail: Vec<String> = Vec::new();
    for area in Area::ALL {
        let count = commands(area).len();
        if count == 0 {
            continue;
        }
        let mut state = PersonaState::new();
        state.current = area;
        let mut walked: Vec<usize> = vec![state.focus_index(area)];
        for _ in 0..count {
            state.focus_next();
            walked.push(state.focus_index(area));
        }
        let unique: BTreeSet<usize> = walked.iter().copied().collect();
        if walked.last() != walked.first() || unique.len() != count {
            r1_fail.push(format!("{} walk {:?}", area.id(), walked));
        }
        state.focus_prev();
        if state.focus_index(area) != walked[walked.len() - 2] {
            r1_fail.push(format!("{} prev does not invert next", area.id()));
        }
    }
    lines.push(AuditLine {
        check: "rule 1 (single-axis lists)".to_string(),
        pass: r1_fail.is_empty(),
        evidence: if r1_fail.is_empty() {
            "every area's focus walk visits each command exactly once and wraps; prev inverts next"
                .to_string()
        } else {
            r1_fail.join("; ")
        },
    });

    // ---- rule 2: exactly one moving element; the host's
    // idle budget is capped below the focus's salience.
    let mut state = PersonaState::new();
    let steady = state.moving_element();
    state.switch_area(Area::Creator);
    let in_flight = state.moving_element();
    lines.push(AuditLine {
        check: "rule 2 (one moving element)".to_string(),
        pass: steady == MovingElement::Focus
            && in_flight == MovingElement::Wipe
            && HOST_IDLE_SALIENCE < FOCUS_SALIENCE,
        evidence: format!(
            "steady state moves {:?} only; a transition moves {:?} only; host idle salience {} < focus salience {}",
            steady, in_flight, HOST_IDLE_SALIENCE, FOCUS_SALIENCE
        ),
    });

    // ---- rule 3: full-strength accent = event roles only.
    let mut r3_fail: Vec<String> = Vec::new();
    for area in Area::ALL {
        let accent = event_accent(area);
        for role in Role::ALL {
            let color = color_for(role, accent);
            if role.is_event() && color != accent {
                r3_fail.push(format!("{} {:?} is not the accent", area.id(), role));
            }
            if !role.is_event() && color == accent {
                r3_fail.push(format!("{} {:?} leaks the full accent", area.id(), role));
            }
        }
        // The resting tiers themselves never equal any
        // area's accent.
        for tier in [
            TEXT_RESTING,
            TEXT_HEADING,
            TEXT_DISABLED,
            FRAME_BG,
            PATTERN_ROSE,
        ] {
            if tier == accent && area.info().accent.is_some() && area != Area::Playroom {
                r3_fail.push(format!("{} accent collides with a resting tier", area.id()));
            }
        }
    }
    lines.push(AuditLine {
        check: "rule 3 (accent = event color only)".to_string(),
        pass: r3_fail.is_empty(),
        evidence: if r3_fail.is_empty() {
            "in all 12 areas the full-strength accent appears in exactly the FocusFill / ActiveEvent / Ribbon roles".to_string()
        } else {
            r3_fail.join("; ")
        },
    });

    // ---- rule 4: focus shows full detail; non-focus
    // shows glanceable summary.
    let mut r4_fail: Vec<String> = Vec::new();
    let mut command_total = 0_usize;
    for area in Area::ALL {
        for command in commands(area) {
            command_total += 1;
            if command.detail.is_empty() || command.detail == command.summary {
                r4_fail.push(format!("{}: {}", area.id(), command.id));
            }
            if command.summary.is_empty() {
                r4_fail.push(format!("{}: {} has no summary", area.id(), command.id));
            }
        }
    }
    lines.push(AuditLine {
        check: "rule 4 (focus detail / rest summary)".to_string(),
        pass: r4_fail.is_empty(),
        evidence: format!(
            "{} commands carry a summary distinct from their full detail; failures: {:?}",
            command_total, r4_fail
        ),
    });

    // ---- rule 5: comparison context pinned in every
    // area's view.
    let context = StageContext {
        model_label: "Prima (argenta.inx)".to_string(),
        node_count: 39,
        param_count: 25,
        selected_node: Some(3),
        play_state: "playing Greeting · frame 61/120".to_string(),
    };
    let mut r5_fail: Vec<String> = Vec::new();
    for area in Area::ALL {
        if stage_view(area, &context).pinned != context {
            r5_fail.push(area.id().to_string());
        }
    }
    lines.push(AuditLine {
        check: "rule 5 (pinned comparison context)".to_string(),
        pass: r5_fail.is_empty(),
        evidence: format!(
            "the pinned context (model, counts, selection, play state) is identical in all 12 stage views; failures: {:?}",
            r5_fail
        ),
    });

    // ---- rule 6: opposite operations never share
    // layout + palette.
    let mut r6_fail: Vec<String> = Vec::new();
    for (a_id, b_id) in OPPOSITE_PAIRS {
        let find = |id: &str| {
            Area::ALL
                .iter()
                .flat_map(|area| commands(*area))
                .find(|command| command.id == id)
        };
        match (find(a_id), find(b_id)) {
            (Some(a), Some(b)) => {
                let same_layout = a.layout == b.layout;
                let same_palette = a.register == b.register;
                if same_layout && same_palette {
                    r6_fail.push(format!("{a_id} / {b_id} share layout and palette"));
                }
            }
            _ => r6_fail.push(format!("pair {a_id} / {b_id} not found")),
        }
    }
    lines.push(AuditLine {
        check: "rule 6 (opposites never look-alike)".to_string(),
        pass: r6_fail.is_empty(),
        evidence: format!(
            "save (FormLeft + Outbound) vs load (FormRight + Inbound) differ in layout and register; failures: {:?}",
            r6_fail
        ),
    });

    // ---- rule 7: menus labeled by the user's goal; no
    // function hides under an unrelated area — every
    // tool's home is its functional class's home.
    let mut r7_fail: Vec<String> = Vec::new();
    for meta in &registry {
        if meta.title.is_empty() {
            r7_fail.push(format!("{} has no goal label", meta.name));
        }
    }
    for (name, area, class) in TOOL_HOMES {
        if class_home(class, name) != area {
            r7_fail.push(format!(
                "{name} ({class}) homed in {}, class home is {}",
                area.id(),
                class_home(class, name).id()
            ));
        }
    }
    lines.push(AuditLine {
        check: "rule 7 (goal labels, true homes)".to_string(),
        pass: r7_fail.is_empty(),
        evidence: format!(
            "all {} tools carry registry goal labels and sit in their F-class home area (F10 split by name: scene-* to Scenes, effects/fluid to Effects); failures: {:?}",
            registry.len(),
            r7_fail
        ),
    });

    // ---- rule 8: jump navigation on long lists.
    let mut r8_fail: Vec<String> = Vec::new();
    for area in Area::ALL {
        let list = commands(area);
        if list.is_empty() {
            continue;
        }
        let mut state = PersonaState::new();
        state.current = area;
        // Jump to the first letter of the last command:
        // the search must find a match from any focus.
        let target_letter = list[list.len() - 1]
            .label
            .chars()
            .find(|c| c.is_alphabetic())
            .unwrap_or('a');
        if !state.jump_to(target_letter) {
            r8_fail.push(format!(
                "{}: no jump match for '{}'",
                area.id(),
                target_letter
            ));
            continue;
        }
        let landed = &list[state.focus_index(area)];
        let landed_letter = landed
            .label
            .chars()
            .find(|c| c.is_alphabetic())
            .unwrap_or(' ');
        if !landed_letter.eq_ignore_ascii_case(&target_letter) {
            r8_fail.push(format!("{}: jump landed on '{}'", area.id(), landed.label));
        }
    }
    lines.push(AuditLine {
        check: "rule 8 (jump navigation)".to_string(),
        pass: r8_fail.is_empty(),
        evidence: format!(
            "letter jump lands on a matching command in every non-empty area; failures: {:?}",
            r8_fail
        ),
    });

    // ---- rule 9: every control prints its activation
    // affordance.
    let mut r9_fail: Vec<String> = Vec::new();
    for area in Area::ALL {
        for command in commands(area) {
            if command.affordance.is_empty() {
                r9_fail.push(format!("{}: {}", area.id(), command.id));
            }
        }
    }
    lines.push(AuditLine {
        check: "rule 9 (printed affordances)".to_string(),
        pass: r9_fail.is_empty(),
        evidence: format!(
            "every command in every area carries a printed affordance; failures: {:?}",
            r9_fail
        ),
    });

    // ---- rule 10: hosts never out-compete their
    // options: fixed edge placement disjoint from the
    // list, idle budget below the focus, and a host in
    // every area except the deliberately hostless meta
    // register.
    let list_right = LIST_REGION[0] + LIST_REGION[2];
    let host_right = HOST_REGION[0] + HOST_REGION[2];
    let disjoint = HOST_REGION[0] >= list_right;
    let at_edge = (host_right - 100.0).abs() <= f32::EPSILON;
    let mut r10_fail: Vec<String> = Vec::new();
    for area in Area::ALL {
        let has_host = area.info().host_slot.is_some();
        if area == Area::Settings && has_host {
            r10_fail.push("settings must be hostless (meta register)".to_string());
        }
        if area != Area::Settings && !has_host {
            r10_fail.push(format!("{} has no host slot", area.id()));
        }
    }
    lines.push(AuditLine {
        check: "rule 10 (hosts never out-compete)".to_string(),
        pass: disjoint && at_edge && HOST_IDLE_SALIENCE < FOCUS_SALIENCE && r10_fail.is_empty(),
        evidence: format!(
            "host region {:?} sits at the right edge, disjoint from the list region {:?}; idle salience {} < focus {}; slot failures: {:?}",
            HOST_REGION, LIST_REGION, HOST_IDLE_SALIENCE, FOCUS_SALIENCE, r10_fail
        ),
    });

    // ---- rule 11: refusals present distinctly from
    // confirmations (typed + visible; the sound half is
    // not implemented under the AUDIO SILENCE rule).
    let refusal = result_register(true);
    let confirmation = result_register(false);
    lines.push(AuditLine {
        check: "rule 11 (refusals read as refusals)".to_string(),
        pass: refusal == ResultRegister::Refusal && confirmation == ResultRegister::Confirmation && refusal != confirmation,
        evidence: "error results present in the emergency register with the REFUSED code prefix; successes in the resting register; sound is not implemented (AUDIO SILENCE program rule) — the visual half carries the rule".to_string(),
    });

    // ---- rule 12: reading surfaces are exempt from
    // choreography.
    let mut r12_fail: Vec<String> = Vec::new();
    for area in Area::ALL {
        let steps = PersonaState::wipe_steps(area);
        match area.info().reading_load {
            ReadingLoad::AmbientOnly if steps != 0 => {
                r12_fail.push(format!(
                    "{} is a reading surface but wipes in {} steps",
                    area.id(),
                    steps
                ));
            }
            ReadingLoad::Choreographed if steps != WIPE_STEPS_CHOREOGRAPHED => {
                r12_fail.push(format!(
                    "{} wipe is {} steps, not {}",
                    area.id(),
                    steps,
                    WIPE_STEPS_CHOREOGRAPHED
                ));
            }
            _ => {}
        }
    }
    lines.push(AuditLine {
        check: "rule 12 (reading surfaces exempt)".to_string(),
        pass: r12_fail.is_empty(),
        evidence: format!(
            "toolbox + settings (ledger / meta) switch instantly with ambient-only motion; choreographed areas wipe in {} fixed steps; failures: {:?}",
            WIPE_STEPS_CHOREOGRAPHED, r12_fail
        ),
    });

    // ---- accent distinctness (Addition Y colour
    // ruling): per-area, never shared, measured margin
    // from each other and from the physics data colour.
    let amber_hue = hue_degrees(PHYSICS_DATA_AMBER);
    let mut accents: Vec<(Area, f32)> = Vec::new();
    for area in Area::ALL {
        if let Some(accent) = area.info().accent {
            accents.push((area, hue_degrees(accent)));
        }
    }
    let mut accent_fail: Vec<String> = Vec::new();
    for (i, (area_a, hue_a)) in accents.iter().enumerate() {
        if hue_distance(*hue_a, amber_hue) < ACCENT_HUE_MARGIN_MIN {
            accent_fail.push(format!(
                "{} is within margin of the physics data amber",
                area_a.id()
            ));
        }
        for (area_b, hue_b) in &accents[i + 1..] {
            if hue_distance(*hue_a, *hue_b) < ACCENT_HUE_MARGIN_MIN {
                accent_fail.push(format!(
                    "{} / {} hues {:.0} / {:.0} are closer than {}",
                    area_a.id(),
                    area_b.id(),
                    hue_a,
                    hue_b,
                    ACCENT_HUE_MARGIN_MIN
                ));
            }
        }
    }
    lines.push(AuditLine {
        check: "accents (signature, never shared)".to_string(),
        pass: accent_fail.is_empty(),
        evidence: format!(
            "{} candidate accents, pairwise hue distance >= {} deg and >= that from the physics data amber ({:.0} deg); failures: {:?}",
            accents.len(),
            ACCENT_HUE_MARGIN_MIN,
            amber_hue,
            accent_fail
        ),
    });

    // ---- display treatment determinism (§4.2 +
    // determinism contract): same string, same letters,
    // bounded rotations; generated from the string only.
    let a = display_treatment("Playroom");
    let b = display_treatment("Playroom");
    let bounded = a
        .iter()
        .all(|letter| letter.rotation_deg.abs() <= 6.0 && letter.y_offset.abs() <= 1.5);
    lines.push(AuditLine {
        check: "display treatment (procedural, deterministic)".to_string(),
        pass: a == b && bounded && a.len() == "Playroom".chars().count(),
        evidence: "per-letter rotation within +/-6 deg and offset within +/-1.5 pt, a pure function of the title string; body text never passes through it".to_string(),
    });

    lines
}

/// The audit as one JSON document (the `--persona-audit`
/// gate surface).
pub fn audit_json() -> serde_json::Value {
    let lines = audit();
    let areas: Vec<serde_json::Value> = Area::ALL
        .iter()
        .map(|area| {
            let info = area.info();
            serde_json::json!({
                "accent": info.accent.map(|rgb| format!("#{:02X}{:02X}{:02X}", rgb[0], rgb[1], rgb[2])),
                "accent_note": info.accent_note,
                "command_count": commands(*area).len(),
                "host_slot": info.host_slot,
                "id": info.id,
                "reading_load": format!("{:?}", info.reading_load),
                "register": info.register,
                "title": info.title,
                "tool_count": TOOL_HOMES.iter().filter(|(_, home, _)| home == area).count(),
            })
        })
        .collect();
    let passed = lines.iter().filter(|line| line.pass).count();
    serde_json::json!({
        "areas": areas,
        "checks": lines.iter().map(|line| serde_json::json!({
            "check": line.check,
            "evidence": line.evidence,
            "pass": line.pass,
        })).collect::<Vec<_>>(),
        "checks_passed": passed,
        "checks_total": lines.len(),
        "nerd_mode_default": false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_audit_passes_in_full() {
        let lines = audit();
        let failures: Vec<&AuditLine> = lines.iter().filter(|line| !line.pass).collect();
        assert!(
            failures.is_empty(),
            "persona audit failures: {:?}",
            failures
                .iter()
                .map(|line| format!("{}: {}", line.check, line.evidence))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn freeze_keeps_the_drill_down_state() {
        let mut state = PersonaState::new();
        state.set_focus(2);
        state.switch_area(Area::Creator);
        assert!(state.is_frozen(Area::Playroom));
        assert_eq!(state.focus_index(Area::Playroom), 2);
        state.switch_area(Area::Playroom);
        assert!(!state.is_frozen(Area::Playroom));
        assert_eq!(state.focus_index(Area::Playroom), 2);
    }

    #[test]
    fn transitions_complete_on_the_fixed_step_clock() {
        let mut state = PersonaState::new();
        state.switch_area(Area::Video);
        assert_eq!(state.moving_element(), MovingElement::Wipe);
        for _ in 0..WIPE_STEPS_CHOREOGRAPHED {
            state.tick();
        }
        assert!(state.transition().is_none());
        assert_eq!(state.moving_element(), MovingElement::Focus);
        assert_eq!(state.wipe_progress(), 1.0);
    }

    #[test]
    fn reading_surfaces_switch_without_a_wipe() {
        let mut state = PersonaState::new();
        state.switch_area(Area::Settings);
        assert!(state.transition().is_none());
        assert_eq!(state.current, Area::Settings);
    }

    #[test]
    fn preference_round_trips_and_defaults_to_persona() {
        let dir = std::env::temp_dir().join(format!("torogao-persona-test-{}", std::process::id()));
        let path = dir.join("persona-preference.json");
        assert_eq!(load_preference(&path), PreferenceState { nerd_mode: false });
        save_preference(&path, &PreferenceState { nerd_mode: true }).expect("save");
        assert_eq!(load_preference(&path), PreferenceState { nerd_mode: true });
        std::fs::remove_dir_all(&dir).ok();
    }
}
