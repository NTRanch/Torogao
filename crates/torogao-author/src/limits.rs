//! Authoring bounds. Every limit the crate enforces lives here
//! with its unit in the name or the doc comment. Model-shaped
//! limits reuse `torogao_format::limits` so an authored project
//! never admits a value the model format would refuse.

pub use torogao_format::limits::{
    COORD_MAGNITUDE_MAX, DEFORMERS_MAX, ID_BYTES_MAX, INDICES_PER_MESH_MAX,
    KEYFORMS_PER_OBJECT_MAX, KEYS_PER_AXIS_MAX, NAME_BYTES_MAX, PARAMETERS_MAX, TREE_DEPTH_MAX,
    VERTICES_PER_MESH_MAX,
};

/// Maximum assets per project (one per separated layer at most).
pub const ASSETS_MAX: usize = 1024;
/// Maximum draw-order band number (inclusive); bands are 0..=99.
pub const DRAW_BAND_MAX: u32 = 99;
/// Maximum index within a draw-order band (inclusive).
pub const DRAW_INDEX_MAX: u32 = 99;
/// Maximum expressions per project.
pub const EXPRESSIONS_MAX: usize = 256;
/// Maximum rows (parameter or visibility) in one expression.
pub const EXPRESSION_ROWS_MAX: usize = 512;
/// Maximum glue records per project.
pub const GLUES_MAX: usize = 1024;
/// Maximum vertex pairs in one glue record.
pub const GLUE_PAIRS_MAX: usize = 4096;
/// Maximum PNG source size accepted by intake, in bytes (128 MiB).
pub const IMAGE_SOURCE_BYTES_MAX: u64 = 134_217_728;
/// Maximum image side accepted by intake, in pixels (the 8K target).
pub const IMAGE_SIDE_MAX: u32 = 8192;
/// Maximum size of one op-input JSON document, in bytes (64 MiB).
pub const INPUT_BYTES_MAX: u64 = 67_108_864;
/// Maximum rig-model file size accepted by import, in bytes. The
/// format's own input bound.
pub const MODEL_SOURCE_BYTES_MAX: u64 = torogao_format::limits::INPUT_BYTES_MAX as u64;
/// Maximum op-log size read by replay, in bytes (256 MiB).
pub const OPS_LOG_BYTES_MAX: u64 = 268_435_456;
/// Maximum op-log records read by replay.
pub const OPS_LOG_RECORDS_MAX: usize = 1_000_000;
/// Maximum parts per project (the model's mesh bound: every
/// drawable part exports as one mesh).
pub const PARTS_MAX: usize = torogao_format::limits::MESHES_MAX;
/// Maximum free-text bytes in a provenance or stamp field.
pub const PROVENANCE_TEXT_BYTES_MAX: usize = 512;
/// Maximum project file size, in bytes (64 MiB).
pub const PROJECT_BYTES_MAX: u64 = 67_108_864;
/// Maximum PSD source size accepted by intake, in bytes (512 MiB).
pub const PSD_SOURCE_BYTES_MAX: u64 = 536_870_912;
/// Maximum layer records in one PSD.
pub const PSD_LAYERS_MAX: usize = 1024;
/// Maximum nesting depth of a RON document.
pub const RON_DEPTH_MAX: usize = 32;
/// Maximum ops in one script run.
pub const SCRIPT_OPS_MAX: usize = 10_000;
/// Maximum variant sets per project.
pub const VARIANT_SETS_MAX: usize = 256;

// ---- AUTH-G (game layer) bounds ----------------------------------

/// Maximum characters in one game.
pub const GAME_CHARACTERS_MAX: usize = 256;
/// Maximum appearance slots on one character's doll.
pub const GAME_APPEARANCE_SLOTS_MAX: usize = 64;
/// Maximum conditions in one game.
pub const GAME_CONDITIONS_MAX: usize = 1024;
/// Maximum size of `content.ron`, in bytes (64 MiB).
pub const GAME_CONTENT_BYTES_MAX: u64 = 67_108_864;
/// Maximum prose entries in `content.ron`.
pub const GAME_CONTENT_ENTRIES_MAX: usize = 16_384;
/// Maximum bytes of prose in one `content.ron` entry.
pub const GAME_CONTENT_TEXT_BYTES_MAX: usize = 16_384;
/// Maximum dialogue lines in one game.
pub const GAME_DIALOGUE_LINES_MAX: usize = 16_384;
/// Maximum effect sets in one game.
pub const GAME_EFFECT_SETS_MAX: usize = 1024;
/// Maximum atoms in one effect set.
pub const GAME_EFFECT_ATOMS_MAX: usize = 256;
/// Maximum expression depth (nodes on the longest root-to-leaf
/// path). The project document's own depth bound leaves room for
/// the records wrapping an expression.
pub const GAME_EXPR_DEPTH_MAX: usize = 24;
/// Maximum expression nodes in one expression tree.
pub const GAME_EXPR_NODES_MAX: usize = 4096;
/// Maximum flags in one game.
pub const GAME_FLAGS_MAX: usize = 1024;
/// Maximum lifetime window on a stat, in days.
pub const GAME_LIFETIME_DAYS_MAX: u32 = 36_500;
/// Maximum offer expiry window on a trigger, in days.
pub const GAME_OFFER_DAYS_MAX: u32 = 36_500;
/// Maximum RNG streams in one game.
pub const GAME_RNG_STREAMS_MAX: usize = 256;
/// Maximum scenes in one game.
pub const GAME_SCENES_MAX: usize = 1024;
/// Maximum slots in one clock (periods per day).
pub const GAME_SLOTS_MAX: usize = 64;
/// Maximum stats in one game.
pub const GAME_STATS_MAX: usize = 1024;
/// Maximum weighted tables in one game.
pub const GAME_TABLES_MAX: usize = 1024;
/// Maximum outcomes in one weighted table.
pub const GAME_TABLE_OUTCOMES_MAX: usize = 256;
/// Maximum table-draw nesting inside one effect transaction
/// (a drawn outcome's set may itself roll, bounded).
pub const GAME_TABLE_NESTING_MAX: u32 = 8;
/// Maximum thresholds in one tier definition.
pub const GAME_TIER_THRESHOLDS_MAX: usize = 64;
/// Maximum triggers in one game.
pub const GAME_TRIGGERS_MAX: usize = 1024;
/// Maximum visual-state rows in one game.
pub const GAME_VISUAL_STATES_MAX: usize = 4096;
/// Maximum named values in one game.
pub const GAME_VALUES_MAX: usize = 1024;
/// Maximum ticks in one `game_simulate` run.
pub const GAME_SIM_TICKS_MAX: u64 = 10_000;
/// Maximum choice-log entries in one `game_simulate` run.
pub const GAME_SIM_LOG_MAX: usize = 10_000;
/// Twice the triangle area below which a triangle is degenerate,
/// in squared model units. Model coordinates are pixels at the
/// art's resolution, so this is far below any painted feature.
pub const TRIANGLE_AREA2_MIN: f64 = 1e-9;
