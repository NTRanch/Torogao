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
