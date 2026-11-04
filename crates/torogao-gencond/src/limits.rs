//! Named bounds for the conditioning seam. Every limit carries
//! its unit; a value outside its bound is a typed refusal, never
//! a truncation or a silent clamp (Tiger Style: bounded work).

/// Largest conditioning record the compiler accepts, in bytes of
/// canonical RON. Records live inside `project.ron`, whose own
/// bound is the author's; this is the section's share.
pub const RECORD_BYTES_MAX: usize = 1_048_576;

/// Text blocks in one positive slot list.
pub const TEXT_BLOCKS_MAX: usize = 16;

/// Bytes in one text block (identity, wardrobe, expression,
/// scene, or the negative block).
pub const TEXT_BLOCK_BYTES_MAX: usize = 16_384;

/// LoRA entries in one record's stack.
pub const LORA_ENTRIES_MAX: usize = 32;

/// Conditioning sources in one record.
pub const SOURCES_MAX: usize = 32;

/// Bytes in a source id (`[a-z0-9-]`, enforced by validation).
pub const SOURCE_ID_BYTES_MAX: usize = 64;

/// Bytes in an S-7 model enumeration name.
pub const MODEL_NAME_BYTES_MAX: usize = 256;

/// Bytes in an asset path inside a record.
pub const ASSET_PATH_BYTES_MAX: usize = 1_024;

/// Largest scalar weight or strength the schema admits. The
/// observed production band is 0.0–0.85; the bound is a sanity
/// ceiling against corrupt records, not a tuning claim.
pub const WEIGHT_MAX: f64 = 16.0;

/// Bytes in one asset blob (reference image or pose map) handed
/// to the compiler.
pub const ASSET_BYTES_MAX: usize = 67_108_864;

/// Pixels in one pose map the blank-map check will decode.
pub const MAP_PIXELS_MAX: u64 = 16_777_216;

/// Nodes in one compiled graph. The production shape is ~15;
/// the ceiling leaves room for wide source lists while keeping
/// every compile trivially bounded.
pub const GRAPH_NODES_MAX: usize = 512;

// ---- The LIFT layer (D-838): scene record, custody, freeze ----

/// Custody entries in one scene record's map.
pub const CUSTODY_ENTRIES_MAX: usize = 512;

/// Bytes in one custody/node path (`/`-joined segments).
pub const NODE_ID_BYTES_MAX: usize = 256;

/// Bytes in one scene-record free-text field (backend context,
/// custody reason, re-render reason).
pub const SCENE_TEXT_BYTES_MAX: usize = 1_024;

/// Freeze provenance entries in one scene record (append-only
/// history; the bound keeps a scene's history reviewable).
pub const PROVENANCE_ENTRIES_MAX: usize = 256;

/// Frozen tracks in one scene record.
pub const FROZEN_TRACKS_MAX: usize = 64;

/// Samples in one dense parameter track handed to
/// `freeze_track`.
pub const TRACK_SAMPLES_MAX: usize = 4_096;

/// Keyforms in one frozen track. The bound is what makes the
/// epsilon contract refusable: a track needing more keys
/// than this to meet its epsilon refuses by name instead of
/// growing without limit.
pub const KEYFORMS_MAX: usize = 256;

/// Colours on either side of one palette freeze (source set
/// or locked palette).
pub const PALETTE_COLORS_MAX: usize = 256;

/// Palette snaps in one scene record.
pub const PALETTE_SNAPS_MAX: usize = 64;
