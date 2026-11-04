//! Normative bounds, copied from the format specification
//! (SPEC.md section 12). Every constant names the spec table
//! row it comes from; validation never uses an unnamed number.

/// `CLIP_BYTES_MAX`: the clip file, 16 MiB.
pub const CLIP_BYTES_MAX: usize = 16_777_216;
/// `ID_BYTES_MAX`: every id, in bytes.
pub const ID_BYTES_MAX: usize = 64;
/// `NAME_BYTES_MAX`: layer `name` and free-text fields, in bytes.
pub const NAME_BYTES_MAX: usize = 128;
/// `FRAMES_MAX`: frames per clip (600 s at 60 Hz).
pub const FRAMES_MAX: u32 = 36_000;
/// Maximum channel groups in one clip (one per kind, SPEC §3.3).
pub const GROUPS_MAX: usize = 4;
/// `TRACKS_PER_GROUP_MAX`: tracks in one channel group.
pub const TRACKS_PER_GROUP_MAX: usize = 1_024;
/// `TRACKS_PER_CLIP_MAX`: base tracks in one clip.
pub const TRACKS_PER_CLIP_MAX: usize = 4_096;
/// `KEYS_PER_TRACK_MAX`: keys in one component key list.
pub const KEYS_PER_TRACK_MAX: usize = 36_000;
/// `LAYERS_MAX`: layers in the stack.
pub const LAYERS_MAX: usize = 64;
/// `TRACKS_PER_LAYER_MAX`: tracks in one layer.
pub const TRACKS_PER_LAYER_MAX: usize = 1_024;
/// `SCOPE_ENTRIES_MAX`: entries in one scope list.
pub const SCOPE_ENTRIES_MAX: usize = 512;
/// `COORD_MAGNITUDE_MAX`: absolute value of any translation
/// component, in metres.
pub const COORD_MAGNITUDE_MAX: f64 = 1e6;
/// `SCALAR_MAGNITUDE_MAX`: absolute value of any scalar channel
/// value or delta.
pub const SCALAR_MAGNITUDE_MAX: f64 = 1e6;
/// `SCALE_FACTOR_MAX`: layer scale factor upper bound (the
/// lower bound is exclusive zero).
pub const SCALE_FACTOR_MAX: f64 = 1_000.0;
/// `QUAT_UNIT_TOLERANCE`: permitted `|‖q‖ − 1|` for a stored
/// quaternion key.
pub const QUAT_UNIT_TOLERANCE: f64 = 1e-6;
/// `SOURCE_FRAMES_MAX`: `source_frame_count` in a frame map.
pub const SOURCE_FRAMES_MAX: u32 = 1_000_000;
